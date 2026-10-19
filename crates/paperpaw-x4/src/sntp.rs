//! Setting the clock over the network: SNTP's messages (RFC 4330), a
//! request and the time in its answer; and Unix time of a calendar date.

/// The servers asked: many, near and spread by DNS.
pub const SERVER: &str = "pool.ntp.org";
pub const PORT: u16 = 123;
/// Bytes of an SNTP message.
pub const MESSAGE: usize = 48;
/// Seconds from 1900 (NTP's start) to 1970 (Unix's).
const NTP_TO_UNIX: i64 = 2_208_988_800;

/// An SNTP request (version 4, from a client).
pub fn request() -> [u8; MESSAGE] {
    let mut message = [0u8; MESSAGE];
    message[0] = (4 << 3) | 3;
    message
}

/// The time an SNTP answer gives, in milliseconds since 1970 (UTC), if it
/// is a server's answer that tells one.
pub fn answer(message: &[u8]) -> Option<i64> {
    let message = message.get(..MESSAGE)?;
    let mode = message[0] & 7;
    let stratum = message[1];
    // From a server (4), synchronised (stratum 1 to 15; 0 is a refusal).
    if mode != 4 || !(1..=15).contains(&stratum) {
        return None;
    }
    // The transmit time: seconds since 1900 and a fraction of 2^32.
    let seconds = u32::from_be_bytes(message[40..44].try_into().ok()?);
    let fraction = u32::from_be_bytes(message[44..48].try_into().ok()?);
    if seconds == 0 {
        return None;
    }
    // NTP's seconds roll over in 2036: a time before 2020 (3_786_825_600
    // seconds after 1900) is one after the roll-over.
    let mut seconds = i64::from(seconds);
    if seconds < 3_786_825_600 {
        seconds += 1 << 32;
    }
    let ms = (i64::from(fraction) * 1000) >> 32;
    Some((seconds - NTP_TO_UNIX) * 1000 + ms)
}

/// Seconds since 1970 (UTC) of a calendar date and time (UTC), for the
/// dates of certificates.
pub fn unix_seconds(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    // Days from 1970-01-01 (Howard Hinnant's days_from_civil).
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer from a server: mode 4, stratum 2, the transmit time.
    fn answer_at(seconds: u32, fraction: u32) -> [u8; MESSAGE] {
        let mut message = [0u8; MESSAGE];
        message[0] = (4 << 3) | 4;
        message[1] = 2;
        message[40..44].copy_from_slice(&seconds.to_be_bytes());
        message[44..48].copy_from_slice(&fraction.to_be_bytes());
        message
    }

    #[test]
    fn asks_as_a_client() {
        assert_eq!(request()[0], 0x23);
    }

    #[test]
    fn reads_the_time_of_an_answer() {
        // 2026-01-01 00:00:00.5 UTC.
        let unix = unix_seconds(2026, 1, 1, 0, 0, 0);
        let ntp = (unix + NTP_TO_UNIX) as u32;
        assert_eq!(answer(&answer_at(ntp, 1 << 31)), Some(unix * 1000 + 500));
    }

    #[test]
    fn after_the_roll_over_of_2036() {
        // 2040-01-01: NTP's seconds started from 0 again in 2036.
        let unix = unix_seconds(2040, 1, 1, 0, 0, 0);
        let ntp = (unix + NTP_TO_UNIX - (1 << 32)) as u32;
        assert_eq!(answer(&answer_at(ntp, 0)), Some(unix * 1000));
    }

    #[test]
    fn refusals_and_clients_tell_no_time() {
        let mut refused = answer_at(4_000_000_000, 0);
        refused[1] = 0;
        assert_eq!(answer(&refused), None);
        let mut client = answer_at(4_000_000_000, 0);
        client[0] = (4 << 3) | 3;
        assert_eq!(answer(&client), None);
        assert_eq!(answer(&[0; 12]), None);
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(unix_seconds(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(unix_seconds(2000, 3, 1, 12, 30, 15), 951_913_815);
        assert_eq!(unix_seconds(2024, 2, 29, 0, 0, 0), 1_709_164_800);
    }
}
