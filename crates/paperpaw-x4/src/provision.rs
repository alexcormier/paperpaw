//! Wi-Fi given with the firmware: the web flasher may write the networks
//! a reader types there into the flash's `nvs` partition (which the
//! firmware does not use otherwise), so that the first start joins one.
//! The record: [`MAGIC`], then lines as the card's `wifi.ini` has them
//! (`ssid = …`, `password = …`, a pair for each network), ended by a zero
//! byte or the erased flash's `0xFF`. At a start the firmware gives it to
//! the card as `wifi.ini` (which the `wifi` plugin keeps with the
//! passwords and removes) and erases the record.

/// What a record begins with.
pub const MAGIC: &[u8] = b"PPWIFI1\n";
/// The most a record holds, its magic too: one sector of the flash.
pub const MAX: usize = 4096;

/// The text of a record at the start of `flash` (as read from the
/// partition), if there is one: valid UTF-8, without the magic.
pub fn record(flash: &[u8]) -> Option<&str> {
    let rest = flash.get(..flash.len().min(MAX))?.strip_prefix(MAGIC)?;
    let end = rest
        .iter()
        .position(|&b| b == 0 || b == 0xFF)
        .unwrap_or(rest.len());
    core::str::from_utf8(&rest[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn a_record_gives_its_lines() {
        let mut flash: Vec<u8> = MAGIC.to_vec();
        flash.extend_from_slice("ssid = Home Net\npassword = pä ss\n".as_bytes());
        flash.resize(MAX, 0xFF);
        assert_eq!(record(&flash), Some("ssid = Home Net\npassword = pä ss\n"));
        // Ended by a zero byte too.
        let mut zero = MAGIC.to_vec();
        zero.extend_from_slice(b"ssid = Cafe\n\0garbage");
        assert_eq!(record(&zero), Some("ssid = Cafe\n"));
    }

    #[test]
    fn erased_or_foreign_flash_is_no_record() {
        assert_eq!(record(&[0xFF; MAX]), None);
        assert_eq!(record(b"NVS data of another firmware"), None);
        let mut broken = MAGIC.to_vec();
        broken.extend_from_slice(&[0xC3, 0x28, b'\n']);
        assert_eq!(record(&broken), None, "not UTF-8");
    }
}
