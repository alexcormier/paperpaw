//! The X3's real-time clock: a DS3231 on I²C (address 0x68), which keeps
//! the time while the device is off, as the X4 Pro's BM8563 does. Its
//! registers from 0x00 hold seconds, minutes, hours (24-hour, as written
//! here), the weekday, the day, the month (its top bit the century) and
//! the year of the century, in BCD. The top bit of its status at 0x0F
//! says its oscillator stopped since it was last set: its time is not to
//! be trusted then; setting it clears the bit.

use embedded_hal::i2c::I2c;

pub use crate::bm8563::Time;

pub const ADDRESS: u8 = 0x68;

const SECONDS: u8 = 0x00;
const STATUS: u8 = 0x0F;
/// In [`STATUS`]: the oscillator stopped.
const STOPPED: u8 = 0x80;

fn from_bcd(b: u8) -> u8 {
    (b >> 4) * 10 + (b & 0x0F)
}

fn to_bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

/// The clock's time, `None` if it stopped or does not answer.
pub fn read<I: I2c>(i2c: &mut I) -> Option<Time> {
    let mut status = [0u8; 1];
    i2c.write_read(ADDRESS, &[STATUS], &mut status).ok()?;
    if status[0] & STOPPED != 0 {
        return None;
    }
    let mut raw = [0u8; 7];
    i2c.write_read(ADDRESS, &[SECONDS], &mut raw).ok()?;
    parse(raw)
}

fn parse(raw: [u8; 7]) -> Option<Time> {
    // A clock set to 12 hours (bit 6) is not one PaperPaw set.
    if raw[2] & 0x40 != 0 {
        return None;
    }
    let time = Time {
        second: from_bcd(raw[0] & 0x7F),
        minute: from_bcd(raw[1] & 0x7F),
        hour: from_bcd(raw[2] & 0x3F),
        day: from_bcd(raw[4] & 0x3F),
        month: from_bcd(raw[5] & 0x1F),
        // The century bit set means 2100s: PaperPaw only writes 2000s.
        year: 2000 + u16::from(from_bcd(raw[6])),
    };
    let valid = (1..=12).contains(&time.month)
        && (1..=31).contains(&time.day)
        && time.hour < 24
        && time.minute < 60
        && time.second < 60;
    valid.then_some(time)
}

/// Sets the clock, and clears the flag that it had stopped.
pub fn write<I: I2c>(i2c: &mut I, time: &Time) -> Option<()> {
    let year = time.year.saturating_sub(2000).min(99) as u8;
    let bytes = [
        SECONDS,
        to_bcd(time.second),
        to_bcd(time.minute),
        to_bcd(time.hour),
        // The weekday, which PaperPaw does not use.
        1,
        to_bcd(time.day),
        to_bcd(time.month),
        to_bcd(year),
    ];
    i2c.write(ADDRESS, &bytes).ok()?;
    let mut status = [0u8; 1];
    i2c.write_read(ADDRESS, &[STATUS], &mut status).ok()?;
    i2c.write(ADDRESS, &[STATUS, status[0] & !STOPPED]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_registers_read() {
        // 24 October 2026, 21:05:09 (a Saturday, weekday 7).
        let t = parse([0x09, 0x05, 0x21, 0x07, 0x24, 0x10, 0x26]).unwrap();
        assert_eq!(
            t,
            Time {
                year: 2026,
                month: 10,
                day: 24,
                hour: 21,
                minute: 5,
                second: 9
            }
        );
        // Twelve hours: not ours.
        assert!(parse([0x09, 0x05, 0x49, 0x07, 0x24, 0x10, 0x26]).is_none());
        // No month 13.
        assert!(parse([0, 0, 0, 1, 1, 0x13, 0x26]).is_none());
    }

    #[test]
    fn bcd_goes_both_ways() {
        for v in [0u8, 9, 10, 45, 59, 99] {
            assert_eq!(from_bcd(to_bcd(v)), v);
        }
    }
}
