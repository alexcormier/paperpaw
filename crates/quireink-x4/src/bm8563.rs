//! The X4 Pro's real-time clock: a BM8563 on I²C (address 0x51), which
//! keeps the time while the device is off. Its registers from 0x02 hold
//! seconds, minutes, hours, the day, the weekday, the month and the year
//! (of the century) in BCD; the top bit of the seconds says the clock
//! stopped (its time is not to be trusted).

use embedded_hal::i2c::I2c;

pub const ADDRESS: u8 = 0x51;

const SECONDS: u8 = 0x02;

/// A date and time as the clock keeps it (UTC, as QuireInk sets it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn from_bcd(b: u8) -> u8 {
    (b >> 4) * 10 + (b & 0x0F)
}

fn to_bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

/// The clock's time, `None` if it stopped or does not answer.
pub fn read<I: I2c>(i2c: &mut I) -> Option<Time> {
    let mut raw = [0u8; 7];
    i2c.write_read(ADDRESS, &[SECONDS], &mut raw).ok()?;
    parse(raw)
}

fn parse(raw: [u8; 7]) -> Option<Time> {
    if raw[0] & 0x80 != 0 {
        return None;
    }
    let time = Time {
        second: from_bcd(raw[0] & 0x7F),
        minute: from_bcd(raw[1] & 0x7F),
        hour: from_bcd(raw[2] & 0x3F),
        day: from_bcd(raw[3] & 0x3F),
        month: from_bcd(raw[5] & 0x1F),
        // The century bit set means 1900s: QuireInk only writes 2000s.
        year: 2000 + u16::from(from_bcd(raw[6])),
    };
    let valid = (1..=12).contains(&time.month)
        && (1..=31).contains(&time.day)
        && time.hour < 24
        && time.minute < 60
        && time.second < 60;
    valid.then_some(time)
}

impl Time {
    /// Milliseconds since 1970 (UTC).
    pub fn unix_ms(&self) -> i64 {
        // Days from 1970 by the civil calendar (proleptic Gregorian).
        let (y, m) = if self.month <= 2 {
            (i64::from(self.year) - 1, i64::from(self.month) + 9)
        } else {
            (i64::from(self.year), i64::from(self.month) - 3)
        };
        let era = y.div_euclid(400);
        let year_of_era = y - era * 400;
        let day_of_year = (153 * m + 2) / 5 + i64::from(self.day) - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = era * 146_097 + day_of_era - 719_468;
        let seconds = days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second);
        seconds * 1000
    }

    /// The time at `unix_ms` (milliseconds since 1970, UTC).
    pub fn from_unix_ms(unix_ms: i64) -> Self {
        let seconds = unix_ms.div_euclid(1000);
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year: year as u16,
            month: month as u8,
            day: day as u8,
            hour: (rest / 3600) as u8,
            minute: (rest / 60 % 60) as u8,
            second: (rest % 60) as u8,
        }
    }
}

/// Sets the clock (and starts it if it had stopped).
pub fn write<I: I2c>(i2c: &mut I, time: &Time) -> Option<()> {
    let year = time.year.saturating_sub(2000).min(99) as u8;
    let bytes = [
        SECONDS,
        to_bcd(time.second),
        to_bcd(time.minute),
        to_bcd(time.hour),
        to_bcd(time.day),
        0,
        to_bcd(time.month),
        to_bcd(year),
    ];
    i2c.write(ADDRESS, &bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_registers_read() {
        // As read from an X4 Pro: 19 October 2026, 13:46:33.
        let t = parse([0x33, 0x46, 0x13, 0x19, 0x01, 0x10, 0x26]).unwrap();
        assert_eq!(
            t,
            Time {
                year: 2026,
                month: 10,
                day: 19,
                hour: 13,
                minute: 46,
                second: 33
            }
        );
        // Stopped: not trusted.
        assert!(parse([0x80, 0, 0, 1, 0, 1, 0]).is_none());
    }

    #[test]
    fn unix_time_goes_both_ways() {
        let t = Time {
            year: 2026,
            month: 10,
            day: 19,
            hour: 13,
            minute: 46,
            second: 33,
        };
        // 2026-10-19T13:46:33Z.
        assert_eq!(t.unix_ms(), 1_792_417_593_000);
        assert_eq!(Time::from_unix_ms(t.unix_ms()), t);
        let leap = Time {
            year: 2028,
            month: 2,
            day: 29,
            hour: 0,
            minute: 0,
            second: 0,
        };
        assert_eq!(Time::from_unix_ms(leap.unix_ms()), leap);
    }

    #[test]
    fn bcd_goes_both_ways() {
        for v in [0u8, 9, 10, 45, 59, 99] {
            assert_eq!(from_bcd(to_bcd(v)), v);
        }
    }
}
