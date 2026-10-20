//! The date and time logic of the time widgets against known dates and
//! as properties over whole years: calendar months starting on each
//! weekday (weeks from Monday and from Sunday, leap Februaries), the
//! year so far on its first and last days and the leap day, ISO weeks,
//! countdowns to dates passed, today and to come, the world clock's
//! places across summer time switches (EU, US, the southern hemisphere)
//! and with half hours and negative offsets, and the moon's phases
//! against published new and full moons.

extern crate std;

use alloc::format;
use alloc::vec::Vec;

use paperpaw_sdk::time::{LocalTime, TIME_ZONES};
use proptest::prelude::*;

use super::countdown::{self, Target};
use super::dates::{self, MonthGrid};
use super::moon;
use super::world;
use super::year;

/// Seconds since 1970 (UTC) of a date and time.
fn unix(year: i32, month: u8, day: u8, hour: i64, minute: i64) -> i64 {
    dates::days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60
}

fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> LocalTime {
    LocalTime {
        year,
        month,
        day,
        hour,
        minute,
        second: 0,
        weekday: dates::weekday(year, month, day),
    }
}

// ---- Calendar ------------------------------------------------------------

/// Months of 2026 (and a leap February of 2032) that start on each
/// weekday, Sunday first.
const FIRST_ON: [(i32, u8, u8); 7] = [
    (2026, 2, 0), // Sunday
    (2026, 6, 1), // Monday
    (2026, 9, 2), // Tuesday
    (2026, 4, 3), // Wednesday
    (2026, 1, 4), // Thursday
    (2026, 5, 5), // Friday
    (2026, 8, 6), // Saturday
];

#[test]
fn months_starting_on_each_weekday_are_laid_out_in_their_column() {
    for (year, month, weekday) in FIRST_ON {
        assert_eq!(dates::weekday(year, month, 1), weekday, "{year}-{month}");
        // From Sunday the first lies in the weekday's column; from
        // Monday one further left, Sunday last.
        let sunday = MonthGrid::new(year, month, 0);
        assert_eq!(sunday.offset, weekday, "{year}-{month} from Sunday");
        let monday = MonthGrid::new(year, month, 1);
        assert_eq!(
            monday.offset,
            (weekday + 6) % 7,
            "{year}-{month} from Monday"
        );
        for grid in [sunday, monday] {
            assert_eq!(grid.day_at(0, grid.offset), 0);
            assert_eq!(grid.weekday_of(grid.offset), weekday);
        }
    }
}

#[test]
fn februaries_take_four_five_or_six_weeks() {
    // February 2026 starts on a Sunday: four full weeks from Sunday.
    assert_eq!(MonthGrid::new(2026, 2, 0).weeks, 4);
    assert_eq!(MonthGrid::new(2026, 2, 1).weeks, 5);
    // February 2027 starts on a Monday: four full weeks from Monday.
    assert_eq!(MonthGrid::new(2027, 2, 1).weeks, 4);
    assert_eq!(MonthGrid::new(2027, 2, 0).weeks, 5);
    // A leap February starting on a Sunday has its 29th in a fifth week.
    let leap = MonthGrid::new(2032, 2, 0);
    assert_eq!((leap.offset, leap.days, leap.weeks), (0, 29, 5));
    assert_eq!(leap.day_at(4, 0), 28);
    // 2100 is no leap year, 2000 was.
    assert_eq!(MonthGrid::new(2100, 2, 1).days, 28);
    assert_eq!(MonthGrid::new(2000, 2, 1).days, 29);
    // August 2026 starts on a Saturday: six weeks either way.
    assert_eq!(MonthGrid::new(2026, 8, 0).weeks, 6);
    assert_eq!(MonthGrid::new(2026, 8, 1).weeks, 6);
}

#[test]
fn every_month_grid_holds_its_days_and_no_empty_week() {
    for year in 1899..=2101 {
        for month in 1..=12u8 {
            for start in [0u8, 1] {
                let grid = MonthGrid::new(year, month, start);
                assert!(grid.offset < 7);
                assert_eq!(grid.days, dates::days_in_month(year, month));
                let cells = u16::from(grid.weeks) * 7;
                let used = u16::from(grid.offset) + u16::from(grid.days);
                assert!(cells >= used && cells - used < 7, "{year}-{month}/{start}");
                assert!((4..=6).contains(&grid.weeks), "{year}-{month}/{start}");
                // The last day is in the last week.
                let last = i32::from(grid.days) - 1;
                assert_eq!(
                    (last + i32::from(grid.offset)) / 7,
                    i32::from(grid.weeks) - 1
                );
                // Each column has the weekday of the days in it.
                for column in 0..7 {
                    let day = grid.day_at(1, column);
                    let date = dates::days_from_civil(year, month, 1) + i64::from(day);
                    let (y, m, d) = dates::civil_from_days(date);
                    assert_eq!(grid.weekday_of(column), dates::weekday(y, m, d));
                }
                // The next month follows on.
                let next = grid.next();
                let (y, m) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };
                assert_eq!((next.year, next.month, next.week_start), (y, m, start));
            }
        }
    }
}

#[test]
fn week_numbers_of_a_grid_count_on_by_one() {
    for year in 2020..=2032 {
        for month in 1..=12u8 {
            let grid = MonthGrid::new(year, month, 1);
            for week in 1..grid.weeks {
                let (before, now) = (grid.week_number(week - 1), grid.week_number(week));
                assert!(
                    now == before + 1 || (now == 1 && before >= 52),
                    "{year}-{month} week {week}: {before} then {now}"
                );
            }
        }
    }
}

#[test]
fn iso_weeks_change_on_mondays_and_week_one_holds_the_fourth_of_january() {
    let first = dates::days_from_civil(1999, 1, 1);
    let last = dates::days_from_civil(2041, 1, 1);
    let mut before = None;
    for day in first..last {
        let (y, m, d) = dates::civil_from_days(day);
        let week = dates::iso_week(y, m, d);
        assert!((1..=53).contains(&week), "{y}-{m}-{d}: {week}");
        if let Some(before) = before {
            let monday = dates::weekday(y, m, d) == 1;
            assert_eq!(week != before, monday, "{y}-{m}-{d}: {before} then {week}");
        }
        if (m, d) == (1, 4) {
            assert_eq!(week, 1, "{y}-01-04");
        }
        if (m, d) == (12, 28) {
            // The 28th of December is always in the year's last week.
            assert!(week >= 52, "{y}-12-28: {week}");
        }
        before = Some(week);
    }
}

// ---- The year so far ------------------------------------------------------

#[test]
fn the_year_on_its_first_and_last_minute() {
    for (year, days) in [(2026, 365), (2028, 366), (2100, 365), (2000, 366)] {
        let first = year::progress(&at(year, 1, 1, 0, 0));
        assert_eq!(
            (first.day, first.days, first.permille, first.left),
            (1, days, 0, days - 1),
            "{year}"
        );
        let last = year::progress(&at(year, 12, 31, 23, 59));
        assert_eq!((last.day, last.days, last.left), (days, days, 0), "{year}");
        assert_eq!(last.permille, 999, "{year}: never 100 % before it ends");
    }
}

#[test]
fn the_leap_day_counts() {
    let leap = year::progress(&at(2028, 2, 29, 12, 0));
    assert_eq!(
        (leap.day, leap.days, leap.left, leap.week),
        (60, 366, 306, 9)
    );
    // The first of March is a day later in a leap year.
    assert_eq!(year::progress(&at(2028, 3, 1, 0, 0)).day, 61);
    assert_eq!(year::progress(&at(2027, 3, 1, 0, 0)).day, 60);
}

#[test]
fn the_year_grows_minute_by_minute_and_never_reaches_a_thousand() {
    for year in [2026, 2028] {
        let mut before = 0;
        let start = dates::days_from_civil(year, 1, 1);
        for day in start..start + i64::from(dates::days_in_year(year)) {
            let (y, m, d) = dates::civil_from_days(day);
            for (hour, minute) in [(0, 0), (11, 30), (23, 59)] {
                let p = year::progress(&at(y, m, d, hour, minute));
                assert!(p.permille >= before && p.permille < 1000, "{y}-{m}-{d}");
                assert_eq!(p.day + p.left, p.days);
                assert_eq!(i64::from(p.day), day - start + 1);
                before = p.permille;
            }
        }
    }
}

// ---- Countdown ------------------------------------------------------------

#[test]
fn countdowns_to_dates_passed_today_and_to_come() {
    let today = (2026, 10, 20);
    assert_eq!(
        countdown::days_until(today, Target::Once(2026, 10, 20)).0,
        0
    );
    assert_eq!(
        countdown::days_until(today, Target::Once(2026, 10, 21)).0,
        1
    );
    assert_eq!(
        countdown::days_until(today, Target::Once(2026, 10, 19)).0,
        -1
    );
    assert_eq!(
        countdown::days_until(today, Target::Once(2025, 10, 20)).0,
        -365
    );
    assert_eq!(
        countdown::days_until(today, Target::Once(2028, 10, 20)).0,
        731
    );
    // Far off in either direction: no overflow.
    assert_eq!(
        countdown::days_until(today, Target::Once(9999, 12, 31)).0,
        dates::days_from_civil(9999, 12, 31) - dates::days_from_civil(2026, 10, 20)
    );
    assert!(countdown::days_until(today, Target::Once(1, 1, 1)).0 < -700_000);
    // Yearly: today is today, never a year ahead.
    assert_eq!(
        countdown::days_until(today, Target::Yearly(10, 20)),
        (0, (2026, 10, 20))
    );
    // New Year's Eve to New Year's Day.
    assert_eq!(
        countdown::days_until((2026, 12, 31), Target::Yearly(1, 1)),
        (1, (2027, 1, 1))
    );
    // A leap day yearly: on the leap day itself it is today.
    assert_eq!(
        countdown::days_until((2028, 2, 29), Target::Yearly(2, 29)),
        (0, (2028, 2, 29))
    );
}

#[test]
fn countdown_dates_in_every_form_are_read() {
    assert_eq!(
        countdown::parse("2026-12-24"),
        Some(Target::Once(2026, 12, 24))
    );
    assert_eq!(
        countdown::parse("24.12.2026"),
        Some(Target::Once(2026, 12, 24))
    );
    assert_eq!(countdown::parse("1.1.2027"), Some(Target::Once(2027, 1, 1)));
    assert_eq!(countdown::parse("--12-24"), Some(Target::Yearly(12, 24)));
    assert_eq!(countdown::parse("31.12."), Some(Target::Yearly(12, 31)));
    assert_eq!(
        countdown::parse("9999-12-31"),
        Some(Target::Once(9999, 12, 31))
    );
}

#[test]
fn countdown_dates_that_are_no_dates_are_refused() {
    for text in [
        "",
        " ",
        "-",
        "--",
        ".",
        "..",
        "2026",
        "2026-",
        "2026-00-10",
        "2026-13-10",
        "2026-04-31",
        "2026-02-29",
        "2026-12-00",
        "0-12-24",
        "10000-01-01",
        "2026-12-24-01",
        "1.2.3.4",
        "32.01.",
        "31.04.",
        "30.02.",
        "a-b-c",
        "2026-12-2x",
        "2026-1٢-24",
        "4294967296-01-01",
        "\u{0}",
    ] {
        assert_eq!(countdown::parse(text), None, "{text:?}");
    }
}

/// A day of the month past 255 must not wrap round into a small one: the
/// day is read as a `u32` and cast to `u8` before it is checked.
#[test]
#[ignore = "bug: a day of 257 wraps to the 1st (countdown::parse casts before checking)"]
fn countdown_days_past_255_do_not_wrap() {
    assert_eq!(countdown::parse("2026-12-257"), None);
    assert_eq!(countdown::parse("257.12.2026"), None);
    assert_eq!(countdown::parse("12-258"), None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Whatever is typed as the date, reading it never panics, and what is
    /// read is a real date.
    #[test]
    fn countdown_parse_never_panics_and_gives_real_dates(text in "\\PC{0,24}") {
        match countdown::parse(&text) {
            Some(Target::Once(y, m, d)) => {
                prop_assert!((1..=9999).contains(&y));
                prop_assert!((1..=12).contains(&m));
                prop_assert!(d >= 1 && d <= dates::days_in_month(y, m));
            }
            Some(Target::Yearly(m, d)) => {
                prop_assert!((1..=12).contains(&m));
                prop_assert!(d >= 1 && d <= dates::days_in_month(2024, m));
            }
            None => {}
        }
    }

    /// Dates written in either form are read back as they were.
    #[test]
    fn countdown_dates_written_are_read_back(
        year in 1i32..=9999,
        month in 1u8..=12,
        day in 1u8..=31,
    ) {
        let day = day.min(dates::days_in_month(year, month));
        let iso = format!("{year:04}-{month:02}-{day:02}");
        let german = format!("{day}.{month}.{year}");
        prop_assert_eq!(countdown::parse(&iso), Some(Target::Once(year, month, day)));
        prop_assert_eq!(countdown::parse(&german), Some(Target::Once(year, month, day)));
    }

    /// A yearly date is never passed: it is today or within a year, on
    /// the day and month asked (the 28th for a leap day in other years).
    #[test]
    fn yearly_countdowns_are_within_a_year(
        days in -20_000i64..40_000,
        month in 1u8..=12,
        day in 1u8..=31,
    ) {
        let day = day.min(dates::days_in_month(2024, month));
        let (y, m, d) = dates::civil_from_days(days);
        let (left, (ty, tm, td)) = countdown::days_until((y, m, d), Target::Yearly(month, day));
        prop_assert!((0..=366).contains(&left));
        prop_assert!(ty == y || ty == y + 1);
        prop_assert_eq!(tm, month);
        if (month, day) == (2, 29) && !paperpaw_sdk::time::is_leap(ty) {
            prop_assert_eq!(td, 28);
        } else {
            prop_assert_eq!(td, day);
        }
        prop_assert_eq!(dates::days_from_civil(ty, tm, td) - days, left);
    }
}

// ---- World clock ------------------------------------------------------------

/// The rule of time zone `id`.
fn rule(id: &str) -> &'static str {
    TIME_ZONES
        .iter()
        .find(|(z, _)| *z == id)
        .map(|(_, rule)| *rule)
        .unwrap()
}

/// Minutes east of UTC of zone `id` at `unix`, as the world clock shows
/// it (here being UTC).
fn offset(id: &str, unix: i64) -> i64 {
    world::place(id, rule(id), unix, 0).unwrap().minutes
}

#[test]
fn every_place_offered_has_its_offsets_in_winter_and_summer() {
    let january = unix(2026, 1, 15, 12, 0);
    let july = unix(2026, 7, 15, 12, 0);
    for (id, winter, summer) in [
        ("core.tz.utc", 0, 0),
        ("core.tz.honolulu", -600, -600),
        ("core.tz.anchorage", -540, -480),
        ("core.tz.los-angeles", -480, -420),
        ("core.tz.phoenix", -420, -420),
        ("core.tz.denver", -420, -360),
        ("core.tz.chicago", -360, -300),
        ("core.tz.new-york", -300, -240),
        ("core.tz.sao-paulo", -180, -180),
        ("core.tz.london", 0, 60),
        ("core.tz.lisbon", 0, 60),
        ("core.tz.berlin", 60, 120),
        ("core.tz.helsinki", 120, 180),
        ("core.tz.istanbul", 180, 180),
        ("core.tz.moscow", 180, 180),
        ("core.tz.dubai", 240, 240),
        ("core.tz.india", 330, 330),
        ("core.tz.bangkok", 420, 420),
        ("core.tz.china", 480, 480),
        ("core.tz.tokyo", 540, 540),
        // The southern hemisphere: summer time in January.
        ("core.tz.sydney", 660, 600),
        ("core.tz.auckland", 780, 720),
    ] {
        assert_eq!(offset(id, january), winter, "{id} in January");
        assert_eq!(offset(id, july), summer, "{id} in July");
    }
    assert_eq!(TIME_ZONES.len(), 22, "a zone added: add it above");
}

#[test]
fn summer_time_switches_at_the_minute() {
    for (id, switch, before, after) in [
        // EU: 01:00 UTC on the last Sundays of March and October.
        ("core.tz.berlin", unix(2026, 3, 29, 1, 0), 60, 120),
        ("core.tz.berlin", unix(2026, 10, 25, 1, 0), 120, 60),
        ("core.tz.london", unix(2026, 3, 29, 1, 0), 0, 60),
        ("core.tz.london", unix(2026, 10, 25, 1, 0), 60, 0),
        ("core.tz.helsinki", unix(2026, 3, 29, 1, 0), 120, 180),
        ("core.tz.helsinki", unix(2026, 10, 25, 1, 0), 180, 120),
        // US: 02:00 local on the second Sunday of March and the first of
        // November.
        ("core.tz.new-york", unix(2026, 3, 8, 7, 0), -300, -240),
        ("core.tz.new-york", unix(2026, 11, 1, 6, 0), -240, -300),
        ("core.tz.los-angeles", unix(2026, 3, 8, 10, 0), -480, -420),
        ("core.tz.los-angeles", unix(2026, 11, 1, 9, 0), -420, -480),
        // Sydney: the first Sundays of October (02:00) and April (03:00).
        ("core.tz.sydney", unix(2026, 10, 3, 16, 0), 600, 660),
        ("core.tz.sydney", unix(2026, 4, 4, 16, 0), 660, 600),
        // Auckland: the last Sunday of September, the first of April.
        ("core.tz.auckland", unix(2026, 9, 26, 14, 0), 720, 780),
        ("core.tz.auckland", unix(2026, 4, 4, 14, 0), 780, 720),
    ] {
        assert_eq!(
            offset(id, switch - 60),
            before,
            "{id} a minute before {switch}"
        );
        assert_eq!(offset(id, switch), after, "{id} at {switch}");
    }
}

#[test]
fn local_times_across_the_switch_skip_and_repeat_an_hour() {
    let ny = rule("core.tz.new-york");
    let time = |unix: i64| {
        let t = world::place("New York", ny, unix, 0).unwrap().time;
        (t.hour, t.minute)
    };
    // In spring 01:59 is followed by 03:00.
    let spring = unix(2026, 3, 8, 7, 0);
    assert_eq!(time(spring - 60), (1, 59));
    assert_eq!(time(spring), (3, 0));
    // In autumn 01:59 is followed by 01:00 again.
    let autumn = unix(2026, 11, 1, 6, 0);
    assert_eq!(time(autumn - 60), (1, 59));
    assert_eq!(time(autumn), (1, 0));
}

#[test]
fn half_hours_and_negative_offsets_tell_the_day_there() {
    let india = rule("core.tz.india");
    // 18:29 UTC is 23:59 in India, 18:30 midnight of the next day.
    let before = world::place("India", india, unix(2026, 10, 20, 18, 29), 0).unwrap();
    assert_eq!(
        (before.time.hour, before.time.minute, before.days),
        (23, 59, 0)
    );
    let after = world::place("India", india, unix(2026, 10, 20, 18, 30), 0).unwrap();
    assert_eq!(
        (
            after.time.day,
            after.time.hour,
            after.time.minute,
            after.days
        ),
        (21, 0, 0, 1)
    );
    assert_eq!(after.minutes, 330);
    // Seen from New York (-4 h in October): India is 9:30 ahead.
    let ny = -4 * 3600;
    let seen = world::place("India", india, unix(2026, 10, 20, 12, 0), ny).unwrap();
    assert_eq!(seen.minutes, 570);
    // Honolulu is still in yesterday early in the UTC day.
    let honolulu = rule("core.tz.honolulu");
    let early = world::place("Honolulu", honolulu, unix(2026, 1, 1, 9, 59), 0).unwrap();
    assert_eq!(
        (
            early.time.year,
            early.time.month,
            early.time.day,
            early.days
        ),
        (2025, 12, 31, -1)
    );
    assert_eq!(early.minutes, -600);
    let later = world::place("Honolulu", honolulu, unix(2026, 1, 1, 10, 0), 0).unwrap();
    assert_eq!((later.time.year, later.days), (2026, 0));
    // São Paulo, three hours behind all year.
    let sao_paulo = rule("core.tz.sao-paulo");
    let place = world::place("São Paulo", sao_paulo, unix(2026, 7, 1, 1, 0), 0).unwrap();
    assert_eq!((place.time.day, place.time.hour, place.days), (30, 22, -1));
}

#[test]
fn place_names_are_the_first_city_and_bad_rules_are_no_place() {
    let place = world::place(" Berlin, Vienna, Zurich ", "CET-1", 0, 0).unwrap();
    assert_eq!(place.name, "Berlin");
    assert_eq!(world::place("", "UTC0", 0, 0).unwrap().name, "");
    assert_eq!(world::place(",,,", "UTC0", 0, 0).unwrap().name, "");
    for bad in ["", "not a zone", "CET-1CEST,M13.5.0,M10.5.0", "<+05"] {
        assert_eq!(world::place("Bad", bad, 0, 0), None, "{bad:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// For every place offered and any time, the place's time is UTC plus
    /// its offset, and its day differs from here by at most one.
    #[test]
    fn places_agree_with_their_offset(
        zone in 0usize..22,
        unix in -2_000_000_000i64..4_000_000_000,
        here in -11i64 * 60..=12 * 60,
    ) {
        let (id, rule) = TIME_ZONES[zone];
        let place = world::place(id, rule, unix, here * 60).unwrap();
        let offset = place.minutes + here;
        prop_assert!((-11 * 60..=14 * 60).contains(&offset), "{id}: {offset}");
        prop_assert_eq!(place.time, LocalTime::from_seconds(unix + offset * 60));
        prop_assert!((-1..=1).contains(&place.days));
    }
}

// ---- Moon ---------------------------------------------------------------------

const HOUR_MS: i64 = 3_600_000;

/// Milliseconds since 1970 (UTC).
fn ms(year: i32, month: u8, day: u8, hour: i64, minute: i64) -> i64 {
    unix(year, month, day, hour, minute) * 1000
}

/// New moons, as published (UTC).
const NEW_MOONS: [(i32, u8, u8, i64, i64); 16] = [
    (1999, 8, 11, 11, 8),
    (2017, 8, 21, 18, 30),
    (2019, 7, 2, 19, 16),
    (2024, 1, 11, 11, 57),
    (2024, 4, 8, 18, 21),
    (2024, 10, 2, 18, 49),
    (2024, 12, 30, 22, 27),
    (2025, 1, 29, 12, 36),
    (2025, 3, 29, 10, 58),
    (2025, 5, 27, 3, 2),
    (2025, 7, 24, 19, 11),
    (2025, 9, 21, 19, 54),
    (2025, 10, 21, 12, 25),
    (2025, 11, 20, 6, 47),
    (2025, 12, 20, 1, 43),
    (2026, 8, 12, 17, 37),
];

/// Full moons, as published (UTC).
const FULL_MOONS: [(i32, u8, u8, i64, i64); 15] = [
    (1997, 9, 16, 18, 50),
    (2024, 1, 25, 17, 54),
    (2024, 3, 25, 7, 0),
    (2024, 6, 22, 1, 8),
    (2024, 9, 18, 2, 34),
    (2024, 12, 15, 9, 2),
    (2025, 1, 13, 22, 27),
    (2025, 2, 12, 13, 53),
    (2025, 4, 13, 0, 22),
    (2025, 6, 11, 7, 44),
    (2025, 8, 9, 7, 55),
    (2025, 9, 7, 18, 9),
    (2025, 10, 7, 3, 48),
    (2025, 12, 4, 23, 14),
    (2026, 3, 3, 11, 38),
];

#[test]
fn new_moons_begin_a_lunation_within_hours_of_the_tables() {
    for (y, mo, d, h, mi) in NEW_MOONS {
        let at = ms(y, mo, d, h, mi);
        let before = moon::moon(at - 3 * HOUR_MS);
        let after = moon::moon(at + 3 * HOUR_MS);
        assert!(
            before.age >= 28 && before.degrees > 340,
            "{y}-{mo}-{d}: {before:?}"
        );
        assert!(
            after.age == 0 && after.degrees < 20,
            "{y}-{mo}-{d}: {after:?}"
        );
        assert_eq!(moon::phase(after.degrees), 0, "{y}-{mo}-{d}");
        assert!(moon::lit(after.degrees) <= 2, "{y}-{mo}-{d}");
    }
}

#[test]
fn full_moons_fall_within_hours_of_the_tables() {
    for (y, mo, d, h, mi) in FULL_MOONS {
        let at = ms(y, mo, d, h, mi);
        // Half a day before, the next full moon is this one.
        let before = moon::moon(at - 12 * HOUR_MS);
        let off = (before.next_full - at).abs();
        assert!(off <= 3 * HOUR_MS, "{y}-{mo}-{d}: {} min off", off / 60_000);
        // At the time it is full, named so and lit all but entirely.
        let full = moon::moon(at);
        assert_eq!(moon::phase(full.degrees), 4, "{y}-{mo}-{d}");
        assert!(moon::lit(full.degrees) >= 97, "{y}-{mo}-{d}");
        // A week and a half later the next one is about a lunation on.
        let later = moon::moon(at + 10 * 24 * HOUR_MS);
        let lunation = later.next_full - before.next_full;
        assert!(
            (29 * 24 * HOUR_MS..30 * 24 * HOUR_MS).contains(&lunation),
            "{y}-{mo}-{d}"
        );
    }
}

#[test]
fn the_moon_far_from_the_epoch_and_before_1970_is_still_reckoned() {
    for at in [
        ms(1900, 1, 1, 0, 0),
        ms(1969, 12, 31, 23, 59),
        0,
        ms(2100, 6, 1, 0, 0),
        ms(2999, 12, 31, 0, 0),
    ] {
        let m = moon::moon(at);
        assert!((0..360).contains(&m.degrees), "{at}: {m:?}");
        assert!((0..30).contains(&m.age), "{at}: {m:?}");
        assert!(m.next_full > at - 24 * HOUR_MS && m.next_full < at + 31 * 24 * HOUR_MS);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// At any time the moon is round its lunation, younger than one, and
    /// its next full moon is at most a lunation away (or today).
    #[test]
    fn the_moon_is_always_within_its_lunation(
        at in -2_000_000_000_000i64..4_000_000_000_000,
    ) {
        let m = moon::moon(at);
        prop_assert!((0..360).contains(&m.degrees));
        prop_assert!((0..30).contains(&m.age));
        prop_assert!(moon::phase(m.degrees) < 8);
        prop_assert!(moon::lit(m.degrees) <= 100);
        prop_assert!(m.next_full > at - 24 * HOUR_MS);
        prop_assert!(m.next_full <= at + 30 * 24 * HOUR_MS);
        // An hour later it is further round (or a new lunation began).
        let later = moon::moon(at + HOUR_MS);
        prop_assert!(later.degrees >= m.degrees || later.degrees < 5);
    }
}

#[test]
fn lunations_follow_each_other_without_gaps() {
    // Each half lunation's time comes after the one before, 13 to 17
    // days apart (the true lunation varies by hours round its mean).
    let times: Vec<i64> = (-2_000..2_000).map(moon::phase_time).collect();
    for pair in times.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(
            (13 * 24 * HOUR_MS..17 * 24 * HOUR_MS).contains(&gap),
            "{}",
            gap / HOUR_MS
        );
    }
}
