//! The battery log read back when it is damaged or hostile: lines cut
//! short, numbers too large, invalid UTF-8, logs larger than a read, more
//! times awake than are kept; and lines written then read back for any
//! reading (property based).

extern crate std;

use alloc::string::String;

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use super::*;

const MINUTE: u64 = 60_000;

fn reading(ms: u64, percent: u8, pages: u32) -> Reading {
    Reading {
        ms,
        millivolts: Some(4000),
        percent: Some(percent),
        pages,
        ..Reading::default()
    }
}

/// A time awake on battery: `minutes` long, `used` percent from 90.
fn session(log: &mut String, minutes: u64, used: u8, pages: u32) {
    log.push_str(&line(Kind::Start, &reading(0, 90, 0)));
    log.push_str(&line(
        Kind::Sample,
        &reading(minutes * MINUTE, 90 - used, pages),
    ));
    log.push_str(&line(
        Kind::End,
        &reading(minutes * MINUTE, 90 - used, pages),
    ));
}

#[test]
fn lines_that_are_not_readings_are_skipped() {
    for text in [
        "",
        ",",
        "start",
        "start,",
        "sample,abc,4000,90,0,0,0,0,0,0,0,0,",
        "sample,-5,4000,90,0,0,0,0,0,0,0,0,",
        "sample,10,4000,90,0,0,0,0",
        "Sample,10,4000,90,0,0,0,0,0,0,0,0,",
        "sample;10;4000;90;0;0;0;0;0;0;0;0;",
        "sample,10,4000,90,0,0,0,x,0,0,0,0,",
        "sample,10,4000,90,0,0,0,0,0,0,0,99999999999,",
        "\u{feff}start,0,4100,95,0,0,0,0,0,0,0,0,",
        HEADER,
        PLUGINS_HEADER,
    ] {
        assert_eq!(parse(text), None, "{text:?}");
    }
}

#[test]
fn readings_with_odd_parts_are_read_as_far_as_they_go() {
    // No voltage, a percent beyond 255, charging as anything but 1, no
    // time of day, extra fields, a line end of Windows.
    let (kind, r) = parse("sample,10,,300,yes,1,2,3,4,5,6,7\r\n").unwrap();
    assert_eq!(kind, Kind::Sample);
    assert_eq!(r.millivolts, None);
    assert_eq!(r.percent, None);
    assert!(!r.charging);
    assert_eq!(r.usage.refreshes, [1, 2, 3]);
    assert_eq!(r.usage.pixels, 4000);
    assert_eq!(r.pages, 7);
    assert_eq!(r.unix, None);
    let (_, r) = parse("end,10,70000,90,1,0,0,0,0,0,0,0,1,extra,fields").unwrap();
    assert_eq!(r.millivolts, None);
    assert!(r.charging);
    assert_eq!(r.unix, Some(1));
    let (_, r) = parse("start,0,4000,90,0,0,0,0,0,0,0,0,-86400").unwrap();
    assert_eq!(r.unix, Some(-86400));
}

/// Kilopixels are read and multiplied by 1000 without a check: a number
/// that fits a `u64` but not a thousand times over panics in debug
/// builds (and wraps in release ones).
#[test]
#[ignore = "bug: huge kilopixels in a log line overflow when read (panic in debug builds)"]
fn huge_kilopixels_do_not_overflow() {
    let text = "sample,10,4000,90,0,0,0,0,18446744073709551615,0,0,0,";
    let _ = parse(text);
}

/// The totals add the times awake without a check: two long ones of a
/// hostile log overflow.
#[test]
#[ignore = "bug: totals of times awake overflow on a hostile log (panic in debug builds)"]
fn totals_of_huge_times_awake_do_not_overflow() {
    let mut log = String::new();
    for _ in 0..2 {
        log.push_str("start,0,4000,90,0,0,0,0,0,0,0,0,\n");
        log.push_str(&alloc::format!(
            "end,{},4000,80,0,0,0,0,0,0,0,{},\n",
            u64::MAX,
            u32::MAX
        ));
    }
    let mut sessions = Sessions::default();
    sessions.read_text(&log);
    let _ = sessions.totals();
}

#[test]
fn only_the_latest_times_awake_are_kept() {
    let mut log = String::from(HEADER);
    for i in 0..(KEPT as u32 + 10) {
        session(&mut log, 10, 1, i);
    }
    let mut sessions = Sessions::default();
    sessions.read_text(&log);
    assert_eq!(sessions.ended.len(), KEPT);
    // The oldest went.
    assert_eq!(sessions.ended[0].pages(), 10);
    assert_eq!(sessions.ended[KEPT - 1].pages(), KEPT as u32 + 9);
    assert!(sessions.current.is_none());
}

#[test]
fn a_log_larger_than_a_read_is_read_whole() {
    // Many lines, so they cross the 2 KiB pieces the log is read in.
    let mut log = String::from(HEADER);
    for i in 0..50 {
        session(&mut log, 10 + i, 1, 1);
    }
    assert!(log.len() > 2 * 2048);
    let storage = MemoryStorage::new().with("battery/log.csv", log.as_str());
    let read = Sessions::read(&storage, "battery");
    let mut whole = Sessions::default();
    whole.read_text(&log);
    assert_eq!(read.ended, whole.ended);
    assert_eq!(read.ended.len(), 50);
}

#[test]
fn the_old_log_comes_first_and_a_last_line_without_its_end_is_read() {
    let mut old = String::from(HEADER);
    session(&mut old, 30, 2, 5);
    let mut log = String::from(HEADER);
    log.push_str(&line(Kind::Start, &reading(0, 80, 0)));
    let last = line(Kind::Sample, &reading(20 * MINUTE, 78, 9));
    log.push_str(last.trim_end());
    let storage = MemoryStorage::new()
        .with("battery/log.old.csv", old.as_str())
        .with("battery/log.csv", log.as_str());
    let sessions = Sessions::read(&storage, "battery");
    assert_eq!(sessions.ended.len(), 1);
    assert_eq!(sessions.ended[0].pages(), 5);
    let current = sessions.current.unwrap();
    assert_eq!(current.pages(), 9);
    assert_eq!(current.used(), Some(2));
}

#[test]
fn no_log_and_empty_logs_have_no_times_awake() {
    for log in [None, Some(""), Some(HEADER), Some("\n\n\n")] {
        let storage = match log {
            Some(text) => MemoryStorage::new().with("battery/log.csv", text),
            None => MemoryStorage::new(),
        };
        let sessions = Sessions::read(&storage, "battery");
        assert!(sessions.ended.is_empty());
        assert!(sessions.current.is_none());
        assert_eq!(sessions.totals(), Totals::default());
        assert_eq!(sessions.totals().per_hour(), None);
        assert_eq!(sessions.totals().hours_left(50), None);
    }
}

#[test]
fn one_reading_alone_is_no_use_on_battery() {
    let mut sessions = Sessions::default();
    sessions.read_text(&line(Kind::Sample, &reading(MINUTE, 50, 3)));
    let current = sessions.current.unwrap();
    assert_eq!(current.millis(), 0);
    assert_eq!(current.used(), Some(0));
    assert!(!current.counts());
    assert_eq!(sessions.totals(), Totals::default());
}

#[test]
fn a_battery_that_gained_or_has_no_percent_does_not_count() {
    let mut log = String::new();
    // Gained without charging (a calibration): not counted.
    log.push_str(&line(Kind::Start, &reading(0, 50, 0)));
    log.push_str(&line(Kind::End, &reading(60 * MINUTE, 55, 0)));
    // No percent at all.
    let mut none = reading(0, 0, 0);
    none.percent = None;
    log.push_str(&line(Kind::Start, &none));
    none.ms = 60 * MINUTE;
    log.push_str(&line(Kind::End, &none));
    let mut sessions = Sessions::default();
    sessions.read_text(&log);
    assert_eq!(sessions.ended.len(), 2);
    assert_eq!(sessions.ended[0].used(), Some(-5));
    assert_eq!(sessions.ended[1].used(), None);
    assert_eq!(sessions.counting().count(), 0);
}

/// One byte that is not UTF-8 in the log loses only its own line, not
/// the lines read with it.
#[test]
#[ignore = "bug: one invalid UTF-8 byte in the battery log drops every line read in the same 2 KiB piece"]
fn a_damaged_line_loses_only_itself() {
    let mut log = String::from(HEADER);
    session(&mut log, 30, 3, 10);
    let mut data = log.into_bytes();
    data.extend_from_slice(b"sample,\xff\xfe,garbage\n");
    let mut after = String::new();
    session(&mut after, 40, 4, 20);
    data.extend_from_slice(after.as_bytes());
    let storage = MemoryStorage::new().with("battery/log.csv", data);
    let sessions = Sessions::read(&storage, "battery");
    assert_eq!(sessions.ended.len(), 2, "{:?}", sessions.ended);
}

#[test]
fn plugin_lines_leave_out_the_idle_and_count_in_whole_units() {
    let plugins = [PluginUsage {
        plugin: paperpaw_sdk::Id::new_static("busy"),
        busy_us: 999,
        calls: 1,
        shown_ms: 0,
        refreshes: [0, 0, 0],
        pixels: 999,
        read: 1023,
        written: 1025,
    }];
    assert_eq!(plugin_lines(5, &plugins), "5,busy,0,1,0,0,0,0,0,0,1\n");
    assert_eq!(plugin_lines(5, &[]), "");
}

fn reading_strategy() -> impl Strategy<Value = Reading> {
    (
        any::<u64>(),
        any::<Option<u16>>(),
        any::<Option<u8>>(),
        any::<bool>(),
        any::<[u32; 3]>(),
        0..u64::MAX / 1000,
        any::<u32>(),
        any::<u64>(),
        any::<u32>(),
        any::<Option<i64>>(),
    )
        .prop_map(
            |(
                ms,
                millivolts,
                percent,
                charging,
                refreshes,
                kilo,
                inputs,
                panel_ms,
                pages,
                unix,
            )| {
                Reading {
                    ms,
                    millivolts,
                    percent,
                    charging,
                    usage: Counts {
                        refreshes,
                        pixels: kilo * 1000,
                        inputs,
                        panel_ms,
                    },
                    pages,
                    unix,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Every reading written is read back as it was (pixels in
    /// thousands, as written).
    #[test]
    fn lines_round_trip(r in reading_strategy(), kind in 0..3u8) {
        let kind = [Kind::Start, Kind::Sample, Kind::End][usize::from(kind)];
        let text = line(kind, &r);
        prop_assert!(text.ends_with('\n'));
        prop_assert_eq!(text.matches('\n').count(), 1);
        prop_assert_eq!(parse(&text), Some((kind, r)));
    }

    /// Any text as a line: read or skipped, never a panic (numbers kept
    /// short of the kilopixels overflow, which has its own test).
    #[test]
    fn any_line_is_read_or_skipped(text in "(start|sample|end|x)?(,[0-9]{0,12}|,[a-z-]{0,3}){0,15}") {
        let _ = parse(&text);
    }

    /// Any bytes as a log: read without a panic, at most the kept times.
    #[test]
    fn any_log_is_read(lines in proptest::collection::vec(
        "(start|sample|end),[0-9]{1,8},[0-9]{0,4},[0-9]{0,3},[01],1,2,3,4,5,6,[0-9]{1,4},",
        0..200,
    ), junk in proptest::collection::vec(any::<u8>(), 0..64)) {
        let mut data = lines.join("\n").into_bytes();
        data.extend_from_slice(&junk);
        let storage = MemoryStorage::new().with("battery/log.csv", data);
        let sessions = Sessions::read(&storage, "battery");
        prop_assert!(sessions.ended.len() <= KEPT);
        let totals = sessions.totals();
        if let Some(rate) = totals.per_hour() {
            prop_assert!(rate.is_finite() && rate > 0.0);
        }
        let _ = totals.per_100_pages();
        let _ = totals.hours_left(100);
    }
}
