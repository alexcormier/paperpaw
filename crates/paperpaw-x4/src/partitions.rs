//! The flash's partition table, as far as updates need it: where the OTA
//! data is and the two app partitions it chooses between ([`Slots`]).
//! The firmware reads the table with this at every start, and the release
//! checks each image's table with it (`paperpaw-sign check-image`), so
//! an image whose table leaves updates nowhere to go is never published.
//!
//! The table is ESP-IDF's: 32-byte entries from [`TABLE_AT`], each with
//! the magic bytes `AA 50`, its type and subtype, offset, size, label and
//! flags, ended by an entry of `EB EB` (its MD5) or by erased flash.

/// Where the partition table is in the flash.
pub const TABLE_AT: u32 = 0x8000;
/// The most the table takes (ESP-IDF's `ESP_PARTITION_TABLE_MAX_LEN`).
pub const TABLE_LEN: usize = 0xC00;
/// An entry's length.
const ENTRY: usize = 32;
/// An entry's first two bytes.
const MAGIC: [u8; 2] = [0xAA, 0x50];
/// App partitions (type 0) and their subtypes.
const APP: u8 = 0x00;
const FACTORY: u8 = 0x00;
const OTA_0: u8 = 0x10;
const OTA_1: u8 = 0x11;
const OTA_LAST: u8 = 0x1F;
/// Data partitions (type 1): the OTA data.
const DATA: u8 = 0x01;
const OTA_DATA: u8 = 0x00;
/// App partitions start on a 64 KB boundary (the MMU's page).
const APP_ALIGN: u32 = 0x1_0000;
/// The OTA data's two entries, a sector each.
const OTA_DATA_LEN: u32 = 0x2000;

/// One entry of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Partition {
    pub kind: u8,
    pub subtype: u8,
    pub offset: u32,
    pub size: u32,
}

/// Why a table leaves updates nowhere to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableError {
    /// No entry at all where the table should be.
    Empty,
    /// No OTA data partition, or one too small for its two entries.
    NoOtaData,
    /// Not both of `ota_0` and `ota_1`.
    NoSecondSlot,
    /// More app partitions than the two the OTA data chooses between
    /// (`ota_2` and on, or a factory app the bootloader would start).
    MoreApps,
    /// An app partition not on a 64 KB boundary, or two that overlap.
    BadPlace,
}

/// The entries of the table in `table` (read from [`TABLE_AT`]), up to
/// the first that is none.
pub fn entries(table: &[u8]) -> impl Iterator<Item = Partition> + '_ {
    table
        .as_chunks::<ENTRY>()
        .0
        .iter()
        .take_while(|entry| entry[..2] == MAGIC)
        .map(|entry| {
            let word = |at: usize| {
                u32::from_le_bytes([entry[at], entry[at + 1], entry[at + 2], entry[at + 3]])
            };
            Partition {
                kind: entry[2],
                subtype: entry[3],
                offset: word(4),
                size: word(8),
            }
        })
}

/// Where updates go: the OTA data and the two app partitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    /// Where the OTA data partition is.
    pub data: u32,
    /// The app partitions `ota_0` and `ota_1`: where each is, and its size.
    pub apps: [(u32, u32); 2],
}

impl Slots {
    /// The slots of the table in `table`, or why it has none fit for
    /// updates.
    pub fn find(table: &[u8]) -> Result<Self, TableError> {
        let mut any = false;
        let mut data = None;
        let mut apps = [None; 2];
        for partition in entries(table) {
            any = true;
            match (partition.kind, partition.subtype) {
                (DATA, OTA_DATA) if partition.size >= OTA_DATA_LEN => {
                    data = Some(partition.offset);
                }
                (APP, OTA_0) => apps[0] = Some((partition.offset, partition.size)),
                (APP, OTA_1) => apps[1] = Some((partition.offset, partition.size)),
                (APP, FACTORY | 0x12..=OTA_LAST) => return Err(TableError::MoreApps),
                _ => {}
            }
        }
        if !any {
            return Err(TableError::Empty);
        }
        let data = data.ok_or(TableError::NoOtaData)?;
        let (Some(first), Some(second)) = (apps[0], apps[1]) else {
            return Err(TableError::NoSecondSlot);
        };
        let overlap = first.0 < second.0.saturating_add(second.1)
            && second.0 < first.0.saturating_add(first.1);
        let misplaced = [first, second]
            .iter()
            .any(|&(at, size)| !at.is_multiple_of(APP_ALIGN) || size == 0);
        if overlap || misplaced {
            return Err(TableError::BadPlace);
        }
        Ok(Self {
            data,
            apps: [first, second],
        })
    }

    /// The slot the flash address `address` is in (where the running code
    /// was found mapped from).
    pub fn slot_at(&self, address: u32) -> Option<u32> {
        self.apps
            .iter()
            .position(|&(at, size)| address >= at && address - at < size)
            .map(|slot| slot as u32)
    }

    /// The slot an update goes to beside `running`: the other.
    pub fn target(&self, running: u32) -> u32 {
        (running + 1) % crate::ota::SLOTS
    }

    /// The room for an update beside `running`.
    pub fn room(&self, running: u32) -> u32 {
        self.apps[self.target(running) as usize].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn table(parts: &[(u8, u8, u32, u32)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for &(kind, subtype, offset, size) in parts {
            bytes.extend_from_slice(&MAGIC);
            bytes.extend_from_slice(&[kind, subtype]);
            bytes.extend_from_slice(&offset.to_le_bytes());
            bytes.extend_from_slice(&size.to_le_bytes());
            bytes.extend_from_slice(&[0; 20]);
        }
        // The entry with the table's MD5, after the last.
        bytes.extend_from_slice(&[0xEB, 0xEB]);
        bytes.resize(TABLE_LEN, 0xFF);
        bytes
    }

    #[test]
    fn the_two_slots_and_the_ota_data_are_found() {
        let slots = Slots::find(&table(&[
            (DATA, OTA_DATA, 0xd000, 0x2000),
            (APP, OTA_0, 0x10000, 0x7F0000),
            (APP, OTA_1, 0x800000, 0x7F0000),
        ]))
        .unwrap();
        assert_eq!(slots.data, 0xd000);
        assert_eq!(slots.slot_at(0x10020), Some(0));
        assert_eq!(slots.slot_at(0x800000), Some(1));
        assert_eq!(slots.slot_at(0x8000), None);
        assert_eq!(slots.target(0), 1);
        assert_eq!(slots.target(1), 0);
    }

    #[test]
    fn tables_without_room_for_updates_are_refused() {
        let ota_data = (DATA, OTA_DATA, 0xd000, 0x2000);
        let first = (APP, OTA_0, 0x10000, 0x7F0000);
        let second = (APP, OTA_1, 0x800000, 0x7F0000);
        let find = |parts: &[_]| Slots::find(&table(parts));
        assert_eq!(find(&[]), Err(TableError::Empty));
        assert_eq!(find(&[first, second]), Err(TableError::NoOtaData));
        assert_eq!(find(&[ota_data, first]), Err(TableError::NoSecondSlot));
        // A factory app is what the bootloader starts with no choice made.
        assert_eq!(
            find(&[ota_data, (APP, FACTORY, 0x10000, 0x100000), second]),
            Err(TableError::MoreApps)
        );
        assert_eq!(
            find(&[ota_data, first, second, (APP, 0x12, 0x900000, 0x1000)]),
            Err(TableError::MoreApps)
        );
        assert_eq!(
            find(&[ota_data, first, (APP, OTA_1, 0x700000, 0x7F0000)]),
            Err(TableError::BadPlace)
        );
        assert_eq!(
            find(&[ota_data, first, (APP, OTA_1, 0x800100, 0x7F0000)]),
            Err(TableError::BadPlace)
        );
        // Erased flash where the table should be.
        assert_eq!(Slots::find(&[0xFF; TABLE_LEN]), Err(TableError::Empty));
    }
}
