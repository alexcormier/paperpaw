//! Reads integers from an ESP-IDF NVS partition, the factory's: Xteink's
//! stock firmware keeps there what the factory measured, among it which
//! panel a device has (`hw_calib`, `screenType`). Only read, never
//! written; anything that does not check out is no value.
//!
//! The layout (ESP-IDF's `nvs_flash`, Apache-2.0, its documented format):
//! pages of 4096 bytes, each a 32-byte header (its state first), a bitmap
//! of its entries' states (2 bits each), then 126 entries of 32 bytes: the
//! namespace's index, the type, the entries it spans, a chunk index, a
//! CRC-32, the key (16 bytes, zero-ended) and 8 bytes of data. A
//! namespace is an entry of namespace 0 whose `u8` is its index.

/// The bytes of a page.
pub const PAGE: usize = 4096;

/// Pages whose entries count: in use, full, or being freed (their
/// entries still there).
const ACTIVE: u32 = 0xFFFF_FFFE;
const FULL: u32 = 0xFFFF_FFFC;
const FREEING: u32 = 0xFFFF_FFF8;

const ENTRIES: usize = 126;
const BITMAP_AT: usize = 32;
const ENTRIES_AT: usize = 64;
const ENTRY: usize = 32;
/// An entry's state in the bitmap: written (not empty, not erased).
const WRITTEN: u8 = 0b10;
/// The type of an unsigned byte, which namespaces have.
const U8: u8 = 0x01;

/// The index of namespace `name` in `page`, if the page names it.
pub fn namespace(page: &[u8], name: &str) -> Option<u8> {
    entries(page)
        .find(|e| e[0] == 0 && e[1] == U8 && key(e) == name.as_bytes())
        .map(|e| e[24])
}

/// The integer `name` of namespace `index` in `page`, whichever integer
/// type it was written as.
pub fn integer(page: &[u8], index: u8, name: &str) -> Option<i64> {
    entries(page)
        .filter(|e| e[0] == index && index != 0 && key(e) == name.as_bytes())
        .find_map(|e| {
            let bytes = usize::from(e[1] & 0x0F);
            let signed = e[1] >> 4 == 1;
            if !matches!(bytes, 1 | 2 | 4 | 8) || e[1] >> 4 > 1 {
                return None;
            }
            let mut raw = [0u8; 8];
            raw[..bytes].copy_from_slice(&e[24..24 + bytes]);
            let value = u64::from_le_bytes(raw);
            Some(if signed {
                let shift = 64 - 8 * bytes as u32;
                ((value << shift) as i64) >> shift
            } else {
                value as i64
            })
        })
}

/// The entries of `page` that were written and check out (an entry that
/// spans more is passed over with its data).
fn entries(page: &[u8]) -> impl Iterator<Item = &[u8]> {
    let state = page
        .get(..4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let usable = page.len() >= PAGE && matches!(state, Some(ACTIVE | FULL | FREEING));
    let mut at = 0;
    core::iter::from_fn(move || {
        while usable && at < ENTRIES {
            let i = at;
            let state = page[BITMAP_AT + i / 4] >> (i % 4 * 2) & 0b11;
            let entry = &page[ENTRIES_AT + i * ENTRY..][..ENTRY];
            if state != WRITTEN
                || crc(entry) != u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]])
            {
                at += 1;
                continue;
            }
            at += usize::from(entry[2]).max(1);
            return Some(entry);
        }
        None
    })
}

/// An entry's key, up to its zero.
fn key(entry: &[u8]) -> &[u8] {
    let key = &entry[8..24];
    &key[..key.iter().position(|&b| b == 0).unwrap_or(key.len())]
}

/// An entry's CRC-32, as ESP-IDF counts it: over its first 4 bytes and
/// those after the CRC, begun from 0xFFFFFFFF (zlib's `crc32(data,
/// 0xFFFFFFFF)`).
fn crc(entry: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &byte in entry[..4].iter().chain(&entry[8..ENTRY]) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                crc >> 1 ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// An entry as ESP-IDF writes one: unused data 0xFF.
    fn entry(index: u8, kind: u8, key: &str, data: &[u8]) -> [u8; ENTRY] {
        let mut e = [0u8; ENTRY];
        e[..4].copy_from_slice(&[index, kind, 1, 0xFF]);
        e[8..8 + key.len()].copy_from_slice(key.as_bytes());
        e[24..].fill(0xFF);
        e[24..24 + data.len()].copy_from_slice(data);
        let crc = crc(&e);
        e[4..8].copy_from_slice(&crc.to_le_bytes());
        e
    }

    /// A page of `state` holding `entries` from the first slot on, each
    /// written but those `erased`.
    fn page(state: u32, entries: &[[u8; ENTRY]], erased: &[usize]) -> Vec<u8> {
        let mut page = vec![0xFF; PAGE];
        page[..4].copy_from_slice(&state.to_le_bytes());
        for (i, e) in entries.iter().enumerate() {
            let bits = if erased.contains(&i) { 0b00 } else { WRITTEN };
            let at = BITMAP_AT + i / 4;
            page[at] = page[at] & !(0b11 << (i % 4 * 2)) | bits << (i % 4 * 2);
            page[ENTRIES_AT + i * ENTRY..][..ENTRY].copy_from_slice(e);
        }
        page
    }

    #[test]
    fn its_crc_is_esp_idfs() {
        // As Python's zlib.crc32 gives it (ESP-IDF's nvs_partition_gen).
        let crc_of = |e: [u8; ENTRY]| u32::from_le_bytes([e[4], e[5], e[6], e[7]]);
        assert_eq!(crc_of(entry(0, U8, "hw_calib", &[1])), 0x604C_1680);
        assert_eq!(crc_of(entry(1, U8, "screenType", &[2])), 0x215B_64B2);
    }

    #[test]
    fn the_factory_screen_type_is_found() {
        let page = page(
            ACTIVE,
            &[
                entry(0, U8, "wifi", &[3]),
                entry(0, U8, "hw_calib", &[1]),
                entry(3, U8, "screenType", &[9]),
                entry(1, 0x14, "adc", &(-5i32).to_le_bytes()),
                entry(1, U8, "screenType", &[2]),
            ],
            &[],
        );
        let index = namespace(&page, "hw_calib").unwrap();
        assert_eq!(index, 1);
        assert_eq!(integer(&page, index, "screenType"), Some(2));
        assert_eq!(integer(&page, index, "adc"), Some(-5));
        assert_eq!(integer(&page, index, "missing"), None);
        assert_eq!(namespace(&page, "hw"), None);
    }

    #[test]
    fn erased_broken_or_foreign_is_nothing() {
        let ns = entry(0, U8, "hw_calib", &[1]);
        let value = entry(1, U8, "screenType", &[2]);
        // Erased.
        let erased = page(FULL, &[ns, value], &[1]);
        assert_eq!(integer(&erased, 1, "screenType"), None);
        // A wrong CRC.
        let mut broken = value;
        broken[24] = 3;
        assert_eq!(
            integer(&page(FULL, &[ns, broken], &[]), 1, "screenType"),
            None
        );
        // A page not in use, erased flash, another layout.
        assert_eq!(namespace(&page(0xFFFF_FFFF, &[ns], &[]), "hw_calib"), None);
        assert_eq!(namespace(&[0xFF; PAGE], "hw_calib"), None);
        assert_eq!(namespace(&[0u8; 100], "hw_calib"), None);
        // A string is no integer.
        let text = page(ACTIVE, &[ns, entry(1, 0x21, "screenType", &[2])], &[]);
        assert_eq!(integer(&text, 1, "screenType"), None);
    }

    #[test]
    fn an_entry_spanning_more_is_passed_over_whole() {
        let ns = entry(0, U8, "hw_calib", &[1]);
        // A string over two slots, its second looking like an entry.
        let mut long = entry(1, 0x21, "name", &[]);
        long[2] = 2;
        let crc = crc(&long);
        long[4..8].copy_from_slice(&crc.to_le_bytes());
        let inside = entry(1, U8, "screenType", &[1]);
        let after = entry(1, U8, "screenType", &[2]);
        let page = page(ACTIVE, &[ns, long, inside, after], &[]);
        assert_eq!(integer(&page, 1, "screenType"), Some(2));
    }
}
