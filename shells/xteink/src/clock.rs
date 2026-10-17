//! The time of day on the X4, which has no clock that runs while it is
//! off: set over the network (SNTP, by `network` once the radio is online)
//! and then counted on with the processor's timer, which runs through
//! light sleep. It is gone when the X4 is switched off, until the network
//! sets it again.
//!
//! Also the SNTP messages: a request, and the time in the answer.

use quireink_platform::{Instant, WallClock};

/// The servers asked: many, near and spread by DNS.
pub const SERVER: &str = "pool.ntp.org";
pub const PORT: u16 = 123;
/// Bytes of an SNTP message.
pub const MESSAGE: usize = 48;
/// Seconds from 1900 (NTP's start) to 1970 (Unix's).
const NTP_TO_UNIX: i64 = 2_208_988_800;

/// Milliseconds since 1970 (UTC) minus the timer's milliseconds, once
/// set; and whether it was set since last asked.
static mut BASE: (Option<i64>, bool) = (None, false);

fn base() -> Option<i64> {
    // SAFETY: one core, used from the main loop only.
    unsafe { core::ptr::addr_of!(BASE).read().0 }
}

/// Sets the time: `unix_ms` at the timer's `now`.
pub fn set(now: Instant, unix_ms: i64) {
    // SAFETY: as in `base`.
    unsafe { core::ptr::addr_of_mut!(BASE).write((Some(unix_ms - now.0 as i64), true)) };
}

/// Whether the time was set since this was last asked (for the status
/// to be passed on at once).
pub fn take_changed() -> bool {
    // SAFETY: as in `base`.
    unsafe {
        let base = &mut *core::ptr::addr_of_mut!(BASE);
        core::mem::take(&mut base.1)
    }
}

/// Milliseconds since 1970 (UTC) at the timer's `now`, once known.
pub fn unix_ms(now: Instant) -> Option<i64> {
    base().map(|base| base + now.0 as i64)
}

/// The wall clock for the kernel's status (its time zone is the user's
/// setting: the X4 has none of its own).
pub fn wall(now: Instant) -> Option<WallClock> {
    unix_ms(now).map(|unix_millis| WallClock {
        at: now,
        unix_millis,
        utc_offset_minutes: None,
    })
}

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
