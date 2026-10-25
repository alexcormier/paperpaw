//! `check-image`: whether a whole flash image (as the web flasher writes
//! it) can take updates over the air once flashed. The release runs it on
//! every device's image and fails where it cannot:
//!
//! - its partition table, read with the firmware's own code
//!   (`paperpaw_x4::partitions`), has the OTA data and two app slots;
//! - the app in the first slot is an image the bootloader starts, with
//!   one data segment (DROM): the bootloader maps only the last of
//!   several, which hides from the firmware the slot it runs in;
//! - the app fits either slot, and CrossPoint's (PaperPaw runs in
//!   CrossPoint's partitions when installed from its menu).

use paperpaw_x4::partitions::{Slots, TABLE_AT, TABLE_LEN};

/// The magic byte of an app image.
const MAGIC: u8 = 0xE9;
/// An image's header, with its extension: then its segments.
const HEADER: usize = 24;
/// Where data (DROM) is mapped, on the ESP32-C3 and the ESP32-S3.
const DROM: core::ops::Range<u32> = 0x3C00_0000..0x3E00_0000;
/// The room of an app slot in CrossPoint's partition table.
const CROSSPOINT_SLOT: u32 = 0x64_0000;

/// What an app image is made of.
#[derive(Debug, PartialEq, Eq)]
pub struct App {
    /// Its length, with its checksum and hash.
    pub len: u32,
    /// Its segments mapped as data.
    pub drom: usize,
}

/// The app image at the start of `bytes`, as ESP-IDF lays it out: the
/// header, the segments (each its address, length and bytes), padding to
/// 16 bytes with the checksum last, and the SHA-256 where the header says.
pub fn app(bytes: &[u8]) -> Result<App, String> {
    if bytes.len() < HEADER || bytes[0] != MAGIC {
        return Err(String::from("no app image (no magic byte)"));
    }
    let segments = bytes[1];
    let hashed = bytes[23] == 1;
    let mut at = HEADER;
    let mut drom = 0;
    for n in 0..segments {
        let head = bytes
            .get(at..at + 8)
            .ok_or_else(|| format!("segment {n} cut short"))?;
        let address = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
        if DROM.contains(&address) {
            drom += 1;
        }
        at += 8 + len;
        if at > bytes.len() {
            return Err(format!("segment {n} cut short"));
        }
    }
    let mut len = (at + 1).next_multiple_of(16);
    if hashed {
        len += 32;
    }
    if len > bytes.len() {
        return Err(String::from("the image's checksum or hash cut off"));
    }
    Ok(App {
        len: len as u32,
        drom,
    })
}

/// What keeps the flash image `bytes` from taking updates; its slots and
/// app where nothing does.
pub fn check(bytes: &[u8]) -> Result<(Slots, App), String> {
    let table = bytes
        .get(TABLE_AT as usize..TABLE_AT as usize + TABLE_LEN)
        .ok_or("no partition table: the image ends before it")?;
    let slots = Slots::find(table)
        .map_err(|why| format!("the partition table leaves updates no slot: {why:?}"))?;
    let (first, _) = slots.apps[0];
    let app = app(bytes.get(first as usize..).unwrap_or_default())
        .map_err(|why| format!("the app in the first slot: {why}"))?;
    if app.drom != 1 {
        return Err(format!(
            "the app has {} data segments: the bootloader maps only the last, and the \
             firmware cannot tell which slot it runs in",
            app.drom
        ));
    }
    let room = slots.apps[0].1.min(slots.apps[1].1).min(CROSSPOINT_SLOT);
    if app.len > room {
        return Err(format!(
            "the app ({} bytes) does not fit a slot of {room} bytes (its own or CrossPoint's)",
            app.len
        ));
    }
    Ok((slots, app))
}

/// `check-image <flash.bin>`.
pub fn check_file(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let (slots, app) = check(&bytes).map_err(|why| format!("{path}: {why}"))?;
    let [(a, a_len), (b, b_len)] = slots.apps;
    println!(
        "{path}: updates go between {a:#x} ({a_len} bytes) and {b:#x} ({b_len} bytes), \
         OTA data at {:#x}; the app takes {} bytes, one data segment",
        slots.data, app.len
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app image of `segments` (address, length), its checksum padded,
    /// with a hash.
    fn image(segments: &[(u32, usize)]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER];
        bytes[0] = MAGIC;
        bytes[1] = segments.len() as u8;
        bytes[23] = 1;
        for &(address, len) in segments {
            bytes.extend_from_slice(&address.to_le_bytes());
            bytes.extend_from_slice(&(len as u32).to_le_bytes());
            bytes.extend(std::iter::repeat_n(0x5A, len));
        }
        let padded = (bytes.len() + 1).next_multiple_of(16);
        bytes.resize(padded + 32, 0);
        bytes
    }

    /// A flash image: the X4's table at 0x8000, `app` at 0x10000.
    fn flash(app: &[u8], second_slot: bool) -> Vec<u8> {
        let mut bytes = vec![0xFF; 0x10000];
        let mut table = Vec::new();
        let mut entry = |kind: u8, subtype: u8, offset: u32, size: u32| {
            table.extend_from_slice(&[0xAA, 0x50, kind, subtype]);
            table.extend_from_slice(&offset.to_le_bytes());
            table.extend_from_slice(&size.to_le_bytes());
            table.extend_from_slice(&[0; 20]);
        };
        entry(1, 0x00, 0xd000, 0x2000);
        entry(0, 0x10, 0x10000, 0x7F_0000);
        if second_slot {
            entry(0, 0x11, 0x80_0000, 0x7F_0000);
        }
        table.extend_from_slice(&[0xEB, 0xEB]);
        bytes[0x8000..0x8000 + table.len()].copy_from_slice(&table);
        bytes.extend_from_slice(app);
        bytes
    }

    #[test]
    fn an_image_like_the_releases_passes() {
        let app = image(&[(0x3C00_0020, 1000), (0x3FC9_4440, 64), (0x4200_0020, 2000)]);
        let (slots, found) = check(&flash(&app, true)).unwrap();
        assert_eq!(slots.apps[1].0, 0x80_0000);
        assert_eq!(found.len as usize, app.len());
        assert_eq!(found.drom, 1);
    }

    #[test]
    fn an_image_with_two_data_segments_fails() {
        // As the builds with the feature remote were, on the ESP32-S3.
        let app = image(&[(0x3C00_0020, 0x464), (0x3FC9_4440, 64), (0x3C00_0488, 2000)]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("2 data segments"), "{why}");
    }

    #[test]
    fn a_table_without_a_second_slot_fails() {
        let app = image(&[(0x3C00_0020, 1000)]);
        let why = check(&flash(&app, false)).unwrap_err();
        assert!(why.contains("NoSecondSlot"), "{why}");
    }

    #[test]
    fn an_app_too_large_for_crosspoints_slot_fails() {
        let app = image(&[(0x3C00_0020, CROSSPOINT_SLOT as usize)]);
        let why = check(&flash(&app, true)).unwrap_err();
        assert!(why.contains("does not fit"), "{why}");
    }
}
