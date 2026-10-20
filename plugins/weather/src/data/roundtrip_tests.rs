//! The weather's data beyond the plain cases: the kept file round trips
//! any weather (property test) and refuses or clamps what is broken in
//! it, Open-Meteo's answers with fields missing, out of range or of the
//! wrong type, arbitrary bytes, places capped and checked, numbers in
//! tenths against plain arithmetic, dates against known days, and unit
//! conversions at the limits of what can come in.

extern crate std;

use super::*;
use alloc::vec;
use proptest::prelude::*;

fn day() -> impl Strategy<Value = Day> {
    (
        -100_000i32..100_000,
        any::<u8>(),
        -1000i32..1000,
        -1000i32..1000,
        proptest::option::of(0u8..=100),
        proptest::option::of(0u16..1440),
        proptest::option::of(0u16..1440),
    )
        .prop_map(|(date, code, high, low, rain, sunrise, sunset)| Day {
            date,
            code,
            high,
            low,
            rain,
            sunrise,
            sunset,
        })
}

fn current() -> impl Strategy<Value = Current> {
    (
        -1000i32..1000,
        proptest::option::of(-1000i32..1000),
        proptest::option::of(0u8..=100),
        any::<u8>(),
        proptest::option::of(0i32..5000),
        any::<bool>(),
    )
        .prop_map(|(temperature, feels, humidity, code, wind, day)| Current {
            temperature,
            feels,
            humidity,
            code,
            wind,
            day,
        })
}

fn weather() -> impl Strategy<Value = Weather> {
    (
        0i64..4_000_000_000,
        (-90i32..=90, -180i32..=180),
        proptest::option::of(current()),
        proptest::collection::vec(day(), 0..=DAYS),
    )
        .prop_map(|(fetched, (lat, lon), current, days)| Weather {
            fetched,
            coordinates: format!("{lat}.5,{lon}.25"),
            current,
            days,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// The kept file reads back what was written.
    #[test]
    fn the_kept_weather_round_trips(weather in weather()) {
        prop_assert_eq!(Weather::from_text(&weather.to_text()), Some(weather));
    }

    /// No bytes make the readers panic.
    #[test]
    fn arbitrary_answers_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = parse_forecast(&bytes, "1,2", 0);
        prop_assert!(parse_places(&bytes).len() <= PLACES);
        let _ = Weather::from_text(&String::from_utf8_lossy(&bytes));
    }

    /// Decimals in tenths, rounded half up, as integer arithmetic has it.
    #[test]
    fn tenths_round_as_arithmetic_does(whole in 0i32..100_000, hundredths in 0i32..100, negative in any::<bool>()) {
        let text = format!("{}{whole}.{hundredths:02}", if negative { "-" } else { "" });
        let expected = (whole * 100 + hundredths + 5) / 10;
        let expected = if negative { -expected } else { expected };
        prop_assert_eq!(tenths(&text), Some(expected), "{}", text);
    }

    /// Each day is one after the one before; weeks have seven.
    #[test]
    fn days_follow_each_other(year in 1600i32..2400, month in 1u8..=12, day in 1u8..=28) {
        let date = days_from_civil(year, month, day);
        if day < 28 {
            prop_assert_eq!(days_from_civil(year, month, day + 1), date + 1);
        }
        // The next month starts 1 to 4 days after its 28th.
        let first = if month < 12 {
            days_from_civil(year, month + 1, 1)
        } else {
            days_from_civil(year + 1, 1, 1)
        };
        let after = first - days_from_civil(year, month, 28);
        prop_assert!((1..=4).contains(&after), "{}", after);
        prop_assert_eq!(weekday((date + 7) as i32), weekday(date as i32));
        prop_assert_eq!(
            parse_date(&format!("{year:04}-{month:02}-{day:02}")),
            Some(date as i32)
        );
    }

    /// Coordinates accepted are numbers in range, nothing else.
    #[test]
    fn coordinates_accepted_are_in_range(text in r"[-0-9., &=a-z]{0,20}") {
        if let Some((lat, lon)) = split_coordinates(&text) {
            let lat = tenths(lat).unwrap();
            let lon = tenths(lon).unwrap();
            prop_assert!((-900..=900).contains(&lat) && (-1800..=1800).contains(&lon));
            prop_assert!(forecast_url(&text).is_some());
        }
    }
}

#[test]
fn days_of_known_dates() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    assert_eq!(days_from_civil(1969, 12, 31), -1);
    assert_eq!(
        days_from_civil(2024, 2, 29) + 1,
        days_from_civil(2024, 3, 1)
    );
    assert_eq!(
        days_from_civil(2100, 2, 28) + 1,
        days_from_civil(2100, 3, 1)
    );
    // 1970-01-01 was a Thursday.
    assert_eq!(weekday(0), 4);
    assert_eq!(weekday(-1), 3);
    assert_eq!(
        parse_date("2026-02-30"),
        Some(days_from_civil(2026, 3, 2) as i32)
    );
    assert_eq!(parse_date("2026-13-01"), None);
    assert_eq!(parse_date("2026-00-10"), None);
    assert_eq!(parse_date("2026-1-1"), None);
    assert_eq!(parse_date("ÄÖÜ-10-14"), None);
    assert_eq!(parse_minutes("2026-10-14T24:00"), None);
    assert_eq!(parse_minutes("2026-10-14T07:6"), None);
    assert_eq!(parse_minutes("2026-10-14T07:59:59"), Some(7 * 60 + 59));
    assert_eq!(parse_minutes("T1é:00"), None);
}

#[test]
fn numbers_that_are_none() {
    for text in ["-", ".", "-.", "1.2.3", "12a", "1234567.0", "+5", "١٢", " "] {
        assert_eq!(tenths(text), None, "{text}");
    }
    assert_eq!(tenths("999999.99"), Some(10_000_000));
    assert_eq!(tenths("  -0.05 "), Some(-1));
}

#[test]
fn answers_with_only_now_or_only_days() {
    let now = br#"{"current": {"temperature_2m": -3.46, "weather_code": 71}}"#;
    let weather = parse_forecast(now, "1,2", 5).unwrap();
    let current = weather.current.unwrap();
    assert_eq!((current.temperature, current.code), (-35, 71));
    assert_eq!(
        (current.feels, current.humidity, current.wind),
        (None, None, None)
    );
    // Without "is_day" it is day.
    assert!(current.day);
    assert!(weather.days.is_empty());

    let days = br#"{"daily": {"time": ["2026-10-14", "2026-10-15"],
        "weather_code": [3, 61], "temperature_2m_max": [10.0, 9.5],
        "temperature_2m_min": [2.0, null]}}"#;
    let weather = parse_forecast(days, "1,2", 5).unwrap();
    assert!(weather.current.is_none());
    // A day without its lowest is left out.
    assert_eq!(weather.days.len(), 1);
    assert_eq!(weather.days[0].rain, None);
    assert_eq!(weather.days[0].sunrise, None);

    // A temperature without its code is no weather now.
    assert!(parse_forecast(br#"{"current": {"temperature_2m": 1}}"#, "1,2", 0).is_none());
}

#[test]
fn values_of_the_wrong_kind_or_range_are_not_taken_as_they_are() {
    let body = br#"{"current": {"temperature_2m": "warm", "weather_code": 2,
        "relative_humidity_2m": 250, "is_day": 0, "wind_speed_10m": true},
        "daily": {"time": ["2026-10-14"], "weather_code": [100000],
        "temperature_2m_max": [{"x": 1}], "temperature_2m_min": [1],
        "precipitation_probability_max": [-20], "sunrise": ["2026-10-14T99:99"]}}"#;
    // The temperature is no number: no weather now; the day's highest
    // neither: no day.
    assert!(parse_forecast(body, "1,2", 0).is_none());
    let body = br#"{"current": {"temperature_2m": 5, "weather_code": 2,
        "relative_humidity_2m": 250, "is_day": 0},
        "daily": {"time": ["2026-10-14"], "weather_code": [100000],
        "temperature_2m_max": [3], "temperature_2m_min": [1],
        "precipitation_probability_max": [-20], "sunrise": ["2026-10-14T99:99"]}}"#;
    let weather = parse_forecast(body, "1,2", 0).unwrap();
    let current = weather.current.unwrap();
    assert_eq!(current.humidity, Some(100));
    assert!(!current.day);
    let day = weather.days[0];
    assert_eq!(day.rain, Some(0));
    assert_eq!(day.sunrise, None);
    // A code beyond any known is an unknown condition, not a panic.
    let _ = condition(day.code);
}

#[test]
#[ignore = "bug: a day's date that cannot be read shifts the days after it onto the values of the day before"]
fn days_keep_their_own_values_when_a_date_is_unreadable() {
    let body = br#"{"daily": {"time": ["2026-10-14", "not a date", "2026-10-16"],
        "weather_code": [1, 2, 3], "temperature_2m_max": [10, 20, 30],
        "temperature_2m_min": [0, 0, 0]}}"#;
    let weather = parse_forecast(body, "1,2", 0).unwrap();
    let last = weather
        .days
        .iter()
        .find(|d| d.date == parse_date("2026-10-16").unwrap())
        .unwrap();
    assert_eq!((last.code, last.high), (3, 300));
}

/// An answer with `count` days, the dates first as Open-Meteo sends
/// them.
fn days_answer(count: i32) -> String {
    let dates: Vec<String> = (0..count)
        .map(|i| {
            format!(
                "\"{}\"",
                format_date(days_from_civil(2026, 1, 1) as i32 + i)
            )
        })
        .collect();
    let numbers = vec!["1"; count as usize].join(",");
    format!(
        r#"{{"daily": {{"time": [{}], "weather_code": [{numbers}],
            "temperature_2m_max": [{numbers}], "temperature_2m_min": [{numbers}]}}}}"#,
        dates.join(",")
    )
}

#[test]
fn the_days_asked_for_are_all_read() {
    let weather = parse_forecast(days_answer(DAYS as i32).as_bytes(), "1,2", 0).unwrap();
    assert_eq!(weather.days.len(), DAYS);
    assert_eq!(
        weather.days[DAYS - 1].date,
        days_from_civil(2026, 1, 1) as i32 + DAYS as i32 - 1
    );
}

#[test]
#[ignore = "bug: an answer with more than 16 days loses all of them (the cap on dates stops the other columns)"]
fn more_days_than_asked_for_are_cut_not_lost() {
    let weather = parse_forecast(days_answer(40).as_bytes(), "1,2", 0);
    assert_eq!(weather.map(|w| w.days.len()), Some(DAYS));
}

#[test]
fn places_are_capped() {
    let place = r#"{"name": "X", "latitude": 1.234, "longitude": 5.678, "country": "Y"}"#;
    let body = format!(r#"{{"results": [{}]}}"#, vec![place; 30].join(","));
    assert_eq!(parse_places(body.as_bytes()).len(), PLACES);
}

/// `YYYY-MM-DD` of days since 1970, the inverse of `days_from_civil`.
fn format_date(days: i32) -> String {
    let z = i64::from(days) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[test]
fn places_without_a_name_or_usable_coordinates_are_left_out() {
    let body = br#"{"results": [
        {"name": "", "latitude": 1, "longitude": 2},
        {"latitude": 1, "longitude": 2},
        {"name": "Far north", "latitude": 95.0, "longitude": 2},
        {"name": "Nowhere", "latitude": null, "longitude": 2},
        {"name": "Text", "latitude": "1&x=2", "longitude": 2},
        {"name": "  Kept  ", "latitude": -33.8688, "longitude": 151.2093,
         "admin1": "Kept", "country": "Australia"}
    ]}"#;
    let places = parse_places(body);
    assert_eq!(places.len(), 1, "{places:?}");
    assert_eq!(places[0].name, "Kept");
    assert_eq!(places[0].coordinates(), "-33.86,151.2");
    // The region is not repeated when it is the place's name.
    assert_eq!(places[0].whereabouts(), "Australia");
}

#[test]
fn broken_kept_files_are_refused_or_clamped() {
    assert_eq!(Weather::from_text(""), None);
    assert_eq!(Weather::from_text("weather 2\nfetched 1\n"), None);
    assert_eq!(Weather::from_text("weather 1\nfetched soon\n"), None);
    assert_eq!(Weather::from_text("weather 1\ncoordinates\n"), None);
    let weather = Weather::from_text(
        "weather 1\r\nfetched 5\r\ncoordinates 1,2\r\n\
         now 10 - 400 999 - 0\r\n\
         day 1 300 5 1 150 -5 5000\r\n\
         day x 1 2 3\r\n\
         unknown line\r\n",
    )
    .unwrap();
    assert_eq!(weather.fetched, 5);
    let current = weather.current.unwrap();
    assert_eq!(
        (current.humidity, current.code, current.day),
        (Some(100), 255, false)
    );
    assert_eq!(weather.days.len(), 1);
    let day = weather.days[0];
    assert_eq!(
        (day.code, day.rain, day.sunrise, day.sunset),
        (255, Some(100), Some(0), Some(1439))
    );
    // More days than kept are left out.
    let mut text = String::from("weather 1\nfetched 1\ncoordinates 1,2\n");
    for i in 0..20 {
        text.push_str(&format!("day {i} 1 2 3 - - -\n"));
    }
    assert_eq!(Weather::from_text(&text).unwrap().days.len(), DAYS);
}

/// The current writer writes version 1 as it was.
#[test]
fn the_writer_still_writes_version_1() {
    let v1 = include_str!("../../testdata/weather.txt.v1");
    let weather = Weather::from_text(v1).unwrap();
    assert_eq!(weather.to_text(), v1.replace("\r\n", "\n"));
}

#[test]
fn search_addresses_carry_any_name_safely() {
    let url = search_url(" a&b=c #d/é ", "de_DE");
    assert!(
        url.starts_with("https://geocoding-api.open-meteo.com/v1/search?name="),
        "{url}"
    );
    let query = url.split_once("?name=").unwrap().1;
    let name = query.split('&').next().unwrap();
    assert!(!name.contains(['&', '=', '#', ' ', '/']), "{name}");
    assert!(url.ends_with("&count=10&language=de&format=json"), "{url}");
    assert!(search_url("x", "").contains("&language=en&"));
    assert!(search_url("x", "-").contains("&language=en&"));
    assert!(search_url("x", "zh-Hant").contains("&language=zh&"));
}

#[test]
fn units_at_everyday_extremes() {
    assert_eq!(Temperature::Fahrenheit.degrees(-900), -130);
    assert_eq!(Temperature::Celsius.degrees(-5), -1);
    assert_eq!(Temperature::Celsius.degrees(5), 1);
    assert_eq!(Wind::Kmh.speed(0), 0);
    assert_eq!(Wind::Knots.speed(4000), 216);
}

#[test]
#[ignore = "bug: wind speeds from the server overflow i32 when converted to mph or knots"]
fn the_largest_wind_speed_an_answer_holds_converts() {
    // Six digits are what `tenths` takes from an answer.
    let tenths = tenths("999999.9").unwrap();
    let mph = (i64::from(tenths) * 1000 / 1609 + 5) / 10;
    assert_eq!(i64::from(Wind::Mph.speed(tenths)), mph);
    let knots = (i64::from(tenths) * 1000 / 1852 + 5) / 10;
    assert_eq!(i64::from(Wind::Knots.speed(tenths)), knots);
}

#[test]
#[ignore = "bug: temperatures and winds at the limits of a kept file overflow i32 when shown"]
fn the_largest_values_of_a_kept_file_convert() {
    let weather = Weather::from_text(&format!(
        "weather 1\nfetched 1\ncoordinates 1,2\nnow {} - - 2 {} 1\n",
        i32::MAX,
        i32::MAX
    ))
    .unwrap();
    let current = weather.current.unwrap();
    assert!(Temperature::Celsius.degrees(current.temperature) > 0);
    assert!(Temperature::Fahrenheit.degrees(current.temperature) > 0);
    assert!(Wind::Kmh.speed(current.wind.unwrap()) > 0);
}
