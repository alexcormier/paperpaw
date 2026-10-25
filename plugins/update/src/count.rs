//! Counting devices without knowing them: the update question says what
//! started it (`trigger`, `auto` for the weekly look, `manual` where the
//! reader asked) and of which periods it is this device's first check
//! (`first`: any of `day`, `week` and `month`, in UTC, the week as ISO
//! 8601 counts). The server counts devices by them: no id, no
//! fingerprint, no request of its own. The device keeps only the date of
//! its last check (`last-check.txt`), never sent. Without the time (the
//! clock not set yet) `first` is left out and the date kept as it was.
//! A check counts when it is asked, whatever its answer.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time::{civil_from_days, days_from_civil, iso_week};
use paperpaw_sdk::{Env, Storage};

use crate::{PLUGIN, file};

/// The date of the last check, as `YYYY-MM-DD` (UTC).
const LAST: &str = "last-check.txt";
const DAY: i64 = 24 * 3600;

/// What started a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// The weekly look, or a check due in the background.
    Auto,
    /// The reader asked.
    Manual,
}

impl Trigger {
    fn name(self) -> &'static str {
        match self {
            Trigger::Auto => "auto",
            Trigger::Manual => "manual",
        }
    }
}

/// The ISO week of the day `days` after 1970-01-01, with its year (which
/// is the next or the last near the turn of the year).
fn week(days: i64) -> (i32, u8) {
    let (year, month, day) = civil_from_days(days);
    let week = iso_week(year, month, day);
    let year = match (month, week) {
        (1, 52..) => year - 1,
        (12, 1) => year + 1,
        _ => year,
    };
    (year, week)
}

fn month(days: i64) -> (i32, u8) {
    let (year, month, _) = civil_from_days(days);
    (year, month)
}

/// The periods of which a check on `today` is the first after one on
/// `last` (days since 1970). A day before `last` (the clock went back)
/// is the first of none.
pub(crate) fn first(last: Option<i64>, today: i64) -> Vec<&'static str> {
    let Some(last) = last else {
        return alloc::vec!["day", "week", "month"];
    };
    let mut periods = Vec::new();
    if today > last {
        periods.push("day");
    }
    if week(today) > week(last) {
        periods.push("week");
    }
    if month(today) > month(last) {
        periods.push("month");
    }
    periods
}

fn read_last(storage: &dyn Storage) -> Option<i64> {
    let text = paperpaw_sdk::read_file(storage, &file(LAST), 64).ok()?;
    let text = core::str::from_utf8(&text).ok()?.trim();
    let mut parts = text.splitn(3, '-').map(str::parse::<u32>);
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) =
        (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let (month, day) = (u8::try_from(month).ok()?, u8::try_from(day).ok()?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(i32::try_from(year).ok()?, month, day))
}

fn write_last(storage: &dyn Storage, days: i64) {
    let (year, month, day) = civil_from_days(days);
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(
        &file(LAST),
        format!("{year:04}-{month:02}-{day:02}\n").as_bytes(),
    );
}

/// The question's members for counting (`,"trigger":…,"first":[…]`), at
/// `unix` seconds since 1970 where the time is known; the check is kept
/// as the last.
pub(crate) fn members(storage: &dyn Storage, trigger: Trigger, unix: Option<i64>) -> String {
    let mut members = format!(",\"trigger\":\"{}\"", trigger.name());
    if let Some(unix) = unix {
        let today = unix.div_euclid(DAY);
        let last = read_last(storage);
        let periods: Vec<String> = first(last, today)
            .into_iter()
            .map(|p| format!("\"{p}\""))
            .collect();
        members.push_str(&format!(",\"first\":[{}]", periods.join(",")));
        if last.is_none_or(|last| today > last) {
            write_last(storage, today);
        }
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn day(year: i32, month: u8, day: u8) -> i64 {
        days_from_civil(year, month, day)
    }

    #[test]
    fn the_first_check_of_a_device_is_the_first_of_everything() {
        assert_eq!(first(None, day(2026, 10, 25)), ["day", "week", "month"]);
    }

    #[test]
    fn a_second_check_the_same_day_is_the_first_of_nothing() {
        let today = day(2026, 10, 21);
        assert!(first(Some(today), today).is_empty());
    }

    #[test]
    fn weeks_begin_on_monday() {
        // Sunday 25 October 2026, then Monday 26.
        assert_eq!(first(Some(day(2026, 10, 24)), day(2026, 10, 25)), ["day"]);
        assert_eq!(
            first(Some(day(2026, 10, 25)), day(2026, 10, 26)),
            ["day", "week"]
        );
    }

    #[test]
    fn months_begin_on_the_first() {
        // Saturday 31 October 2026, Sunday 1 November: the same week.
        assert_eq!(
            first(Some(day(2026, 10, 31)), day(2026, 11, 1)),
            ["day", "month"]
        );
        // A month later, the same day of the month.
        assert_eq!(
            first(Some(day(2026, 10, 14)), day(2026, 11, 14)),
            ["day", "week", "month"]
        );
    }

    #[test]
    fn weeks_across_the_turn_of_the_year_are_iso_weeks() {
        // Monday 29 December 2025 starts week 1 of 2026: Thursday 1
        // January is in it, a new month but not a new week.
        assert_eq!(
            first(Some(day(2025, 12, 29)), day(2026, 1, 1)),
            ["day", "month"]
        );
        // Week 53 of 2026 runs into January 2027; week 1 of 2027 begins
        // on Monday 4 January.
        assert_eq!(
            first(Some(day(2026, 12, 31)), day(2027, 1, 3)),
            ["day", "month"]
        );
        assert_eq!(
            first(Some(day(2027, 1, 3)), day(2027, 1, 4)),
            ["day", "week"]
        );
    }

    #[test]
    fn a_clock_gone_back_counts_nothing_again() {
        assert!(first(Some(day(2026, 10, 25)), day(2026, 9, 1)).is_empty());
    }

    #[test]
    fn without_the_time_no_first_is_sent_and_the_date_is_kept() {
        let storage = MemoryStorage::new();
        let noon = day(2026, 10, 25) * DAY + 12 * 3600;
        assert_eq!(
            members(&storage, Trigger::Manual, Some(noon)),
            ",\"trigger\":\"manual\",\"first\":[\"day\",\"week\",\"month\"]"
        );
        assert_eq!(
            members(&storage, Trigger::Auto, None),
            ",\"trigger\":\"auto\""
        );
        assert_eq!(read_last(&storage), Some(day(2026, 10, 25)));
        // Later the same day: nothing is the first.
        assert_eq!(
            members(&storage, Trigger::Auto, Some(noon + 3600)),
            ",\"trigger\":\"auto\",\"first\":[]"
        );
        // The next day, Monday.
        assert_eq!(
            members(&storage, Trigger::Auto, Some(noon + DAY)),
            ",\"trigger\":\"auto\",\"first\":[\"day\",\"week\"]"
        );
        assert_eq!(read_last(&storage), Some(day(2026, 10, 26)));
    }

    #[test]
    fn only_the_date_is_kept() {
        let storage = MemoryStorage::new();
        members(&storage, Trigger::Auto, Some(day(2026, 3, 9) * DAY + 5));
        let kept = paperpaw_sdk::read_file(&storage, &file(LAST), 64).unwrap();
        assert_eq!(kept, b"2026-03-09\n");
    }
}
