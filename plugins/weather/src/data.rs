//! The weather as Open-Meteo answers it, read with little memory: the
//! conditions now and a few days ahead ([`Weather`]), places found by name
//! ([`Place`]); the weather kept in a small text file between starts
//! ([`Weather::to_text`]); weather codes as conditions and symbols; units.
//!
//! Temperatures are kept in tenths of a degree Celsius and wind speeds in
//! tenths of km/h, as Open-Meteo sends them; they are converted for the
//! units chosen only when shown, so changing the units needs no fetch.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::web::encode;

/// Days of the forecast asked for and kept.
pub(crate) const DAYS: usize = 8;
/// Places found by name, at most.
pub(crate) const PLACES: usize = 10;

/// The weather now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Current {
    /// Tenths of a degree Celsius.
    pub temperature: i32,
    /// How warm it feels, tenths of a degree Celsius.
    pub feels: Option<i32>,
    /// Relative humidity in percent.
    pub humidity: Option<u8>,
    /// The WMO weather code.
    pub code: u8,
    /// Tenths of km/h.
    pub wind: Option<i32>,
    /// Whether the sun is up.
    pub day: bool,
}

/// The weather of a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Day {
    /// Days since 1970-01-01.
    pub date: i32,
    pub code: u8,
    /// Highest and lowest temperature, tenths of a degree Celsius.
    pub high: i32,
    pub low: i32,
    /// The chance of rain (or snow) in percent, at its highest.
    pub rain: Option<u8>,
    /// Sunrise and sunset, minutes after midnight (the place's time).
    pub sunrise: Option<u16>,
    pub sunset: Option<u16>,
}

/// The weather of a place: now and the next days.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Weather {
    /// When it was fetched, seconds since 1970 (UTC); 0 if not known.
    pub fetched: i64,
    /// The coordinates it is for, as asked (`52.52,13.41`).
    pub coordinates: String,
    pub current: Option<Current>,
    pub days: Vec<Day>,
}

/// A place found by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Place {
    pub name: String,
    /// The state or region, and the country.
    pub region: String,
    pub country: String,
    /// As sent, shortened to two decimals (about a kilometre).
    pub latitude: String,
    pub longitude: String,
}

impl Place {
    /// `latitude,longitude`, as kept in the settings.
    pub fn coordinates(&self) -> String {
        format!("{},{}", self.latitude, self.longitude)
    }

    /// Where it is: "Land Berlin, Germany".
    pub fn whereabouts(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if !self.region.is_empty() && self.region != self.name {
            parts.push(&self.region);
        }
        if !self.country.is_empty() {
            parts.push(&self.country);
        }
        parts.join(", ")
    }
}

/// The address of the forecast for `coordinates` (`lat,lon`): only the
/// values shown, in Celsius and km/h, the days in the place's time.
pub(crate) fn forecast_url(coordinates: &str) -> Option<String> {
    let (latitude, longitude) = split_coordinates(coordinates)?;
    Some(format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,is_day,weather_code,wind_speed_10m\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset\
         &timezone=auto&forecast_days={DAYS}"
    ))
}

/// The address of a search for places called `name`, their names in
/// `language` where known.
pub(crate) fn search_url(name: &str, language: &str) -> String {
    let language: String = language
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_lowercase();
    let language = if language.is_empty() {
        String::from("en")
    } else {
        language
    };
    format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={PLACES}&language={language}&format=json",
        encode(name.trim())
    )
}

/// The latitude and longitude of `coordinates` (`lat,lon`), if both are
/// numbers in range.
pub(crate) fn split_coordinates(coordinates: &str) -> Option<(&str, &str)> {
    let (latitude, longitude) = coordinates.split_once(',')?;
    let (latitude, longitude) = (latitude.trim(), longitude.trim());
    let in_range = |text: &str, max: i32| {
        tenths(text).is_some_and(|v| (-max * 10..=max * 10).contains(&v))
            && text
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    };
    (in_range(latitude, 90) && in_range(longitude, 180)).then_some((latitude, longitude))
}

/// A coordinate as sent (`52.52437`) to two decimals (`52.52`).
pub(crate) fn short_coordinate(text: &str) -> String {
    let text = text.trim();
    match text.split_once('.') {
        Some((whole, part)) => {
            let part: String = part.chars().take(2).collect();
            let part = part.trim_end_matches('0');
            if part.is_empty() {
                whole.to_string()
            } else {
                format!("{whole}.{part}")
            }
        }
        None => text.to_string(),
    }
}

/// A number as text (`-3.46`) in tenths (`-35`), rounded; `None` for
/// what is no number (`null`).
pub(crate) fn tenths(text: &str) -> Option<i32> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, part) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && part.is_empty() {
        return None;
    }
    if !whole
        .chars()
        .chain(part.chars())
        .all(|c| c.is_ascii_digit())
        || whole.len() > 6
    {
        return None;
    }
    let whole: i32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut part = part.bytes().map(|b| i32::from(b - b'0'));
    let first = part.next().unwrap_or(0);
    let round = i32::from(part.next().unwrap_or(0) >= 5);
    let value = whole * 10 + first + round;
    Some(if negative { -value } else { value })
}

/// A date (`2026-10-14`, also the start of `2026-10-14T07:23`) as days
/// since 1970-01-01.
pub(crate) fn parse_date(text: &str) -> Option<i32> {
    let mut parts = text.get(..10)?.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u8 = parts.next()?.parse().ok()?;
    let day: u8 = parts.next()?.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day))
        .then(|| days_from_civil(year, month, day) as i32)
}

/// The time of day of `2026-10-14T07:23` in minutes after midnight.
pub(crate) fn parse_minutes(text: &str) -> Option<u16> {
    let (_, time) = text.split_once('T')?;
    let (hour, minute) = time.split_once(':')?;
    let hour: u16 = hour.parse().ok()?;
    let minute: u16 = minute.get(..2)?.parse().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// Days since 1970-01-01 of a date.
pub(crate) fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The weekday of a day since 1970, 0 (Sunday) to 6.
pub(crate) fn weekday(date: i32) -> u8 {
    (date + 4).rem_euclid(7) as u8
}

/// What a forecast's arrays hold, gathered before they are put together.
#[derive(Default)]
struct Columns {
    /// A date each, `None` where it could not be read: the columns stay
    /// in step, that day is left out.
    dates: Vec<Option<i32>>,
    codes: Vec<u8>,
    highs: Vec<Option<i32>>,
    lows: Vec<Option<i32>>,
    rain: Vec<Option<u8>>,
    sunrise: Vec<Option<u16>>,
    sunset: Vec<Option<u16>>,
}

fn percent(text: &str) -> Option<u8> {
    tenths(text).map(|v| ((v + 5) / 10).clamp(0, 100) as u8)
}

/// The weather in Open-Meteo's answer (`body`) for `coordinates`;
/// `None` if it holds none.
pub(crate) fn parse_forecast(body: &[u8], coordinates: &str, fetched: i64) -> Option<Weather> {
    let mut temperature = None;
    let mut feels = None;
    let mut humidity = None;
    let mut code = None;
    let mut wind = None;
    let mut day = true;
    let mut columns = Columns::default();
    let mut parser = Parser::default();
    parser.feed(body, &mut |event| {
        let Event::Value(path, text) = event else {
            return;
        };
        let keys: Vec<&str> = path.iter().map(String::as_str).collect();
        match keys.as_slice() {
            ["current", "temperature_2m"] => temperature = tenths(text),
            ["current", "apparent_temperature"] => feels = tenths(text),
            ["current", "relative_humidity_2m"] => humidity = percent(text),
            ["current", "weather_code"] => code = tenths(text).map(|c| (c / 10) as u8),
            ["current", "wind_speed_10m"] => wind = tenths(text),
            ["current", "is_day"] => day = text.trim() != "0",
            // Each column cut on its own at twice the days kept, so more
            // days than asked for leave the first ones.
            ["daily", key, "[]"] => {
                fn add<T>(column: &mut Vec<T>, value: T) {
                    if column.len() < DAYS * 2 {
                        column.push(value);
                    }
                }
                match *key {
                    "time" => add(&mut columns.dates, parse_date(text)),
                    "weather_code" => add(
                        &mut columns.codes,
                        tenths(text).map_or(255, |c| (c / 10) as u8),
                    ),
                    "temperature_2m_max" => add(&mut columns.highs, tenths(text)),
                    "temperature_2m_min" => add(&mut columns.lows, tenths(text)),
                    "precipitation_probability_max" => add(&mut columns.rain, percent(text)),
                    "sunrise" => add(&mut columns.sunrise, parse_minutes(text)),
                    "sunset" => add(&mut columns.sunset, parse_minutes(text)),
                    _ => {}
                }
            }
            _ => {}
        }
    });
    let current = match (temperature, code) {
        (Some(temperature), Some(code)) => Some(Current {
            temperature,
            feels,
            humidity,
            code,
            wind,
            day,
        }),
        _ => None,
    };
    let days: Vec<Day> = columns
        .dates
        .iter()
        .enumerate()
        .filter_map(|(i, &date)| {
            Some(Day {
                date: date?,
                code: *columns.codes.get(i)?,
                high: (*columns.highs.get(i)?)?,
                low: (*columns.lows.get(i)?)?,
                rain: columns.rain.get(i).copied().flatten(),
                sunrise: columns.sunrise.get(i).copied().flatten(),
                sunset: columns.sunset.get(i).copied().flatten(),
            })
        })
        .take(DAYS)
        .collect();
    if current.is_none() && days.is_empty() {
        return None;
    }
    Some(Weather {
        fetched,
        coordinates: coordinates.to_string(),
        current,
        days,
    })
}

/// The places in Open-Meteo's answer to a search (`body`), at most
/// [`PLACES`].
pub(crate) fn parse_places(body: &[u8]) -> Vec<Place> {
    let mut places = Vec::new();
    let mut place = Place::default();
    let mut parser = Parser::default();
    parser.feed(body, &mut |event| match event {
        Event::Value(path, text) if path.len() == 3 && path[0] == "results" => {
            let text = text.trim();
            match path[2].as_str() {
                "name" => place.name = text.to_string(),
                "admin1" => place.region = text.to_string(),
                "country" => place.country = text.to_string(),
                "latitude" => place.latitude = short_coordinate(text),
                "longitude" => place.longitude = short_coordinate(text),
                _ => {}
            }
        }
        Event::Close(path) if path.len() == 2 && path[0] == "results" => {
            let done = core::mem::take(&mut place);
            if !done.name.is_empty()
                && split_coordinates(&done.coordinates()).is_some()
                && places.len() < PLACES
            {
                places.push(done);
            }
        }
        _ => {}
    });
    places
}

/// The first line of the kept weather; a later version of the file
/// starts with another.
const HEADER: &str = "weather 1";

fn optional(value: Option<impl ToString>) -> String {
    value.map_or_else(|| String::from("-"), |v| v.to_string())
}

impl Weather {
    /// As kept in its file: a line for now and one a day, numbers only.
    pub fn to_text(&self) -> String {
        let mut text = format!(
            "{HEADER}\nfetched {}\ncoordinates {}\n",
            self.fetched, self.coordinates
        );
        if let Some(c) = self.current {
            text.push_str(&format!(
                "now {} {} {} {} {} {}\n",
                c.temperature,
                optional(c.feels),
                optional(c.humidity),
                c.code,
                optional(c.wind),
                u8::from(c.day)
            ));
        }
        for d in &self.days {
            text.push_str(&format!(
                "day {} {} {} {} {} {} {}\n",
                d.date,
                d.code,
                d.high,
                d.low,
                optional(d.rain),
                optional(d.sunrise),
                optional(d.sunset)
            ));
        }
        text
    }

    /// The weather kept in `text` ([`Weather::to_text`]); `None` if it is
    /// not such a file.
    pub fn from_text(text: &str) -> Option<Weather> {
        let mut lines = text.lines();
        if lines.next()?.trim() != HEADER {
            return None;
        }
        let mut weather = Weather::default();
        for line in lines {
            let mut words = line.split_whitespace();
            let kind = words.next().unwrap_or("");
            let values: Vec<&str> = words.collect();
            let number = |i: usize| values.get(i).and_then(|v| v.parse::<i32>().ok());
            match kind {
                "fetched" => weather.fetched = values.first().and_then(|v| v.parse().ok())?,
                "coordinates" => weather.coordinates = values.first()?.to_string(),
                "now" => {
                    weather.current = Some(Current {
                        temperature: number(0)?,
                        feels: number(1),
                        humidity: number(2).map(|v| v.clamp(0, 100) as u8),
                        code: number(3)?.clamp(0, 255) as u8,
                        wind: number(4),
                        day: number(5) != Some(0),
                    });
                }
                "day" if weather.days.len() < DAYS => {
                    let Some((date, code, high, low)) = number(0)
                        .zip(number(1))
                        .zip(number(2).zip(number(3)))
                        .map(|((d, c), (h, l))| (d, c, h, l))
                    else {
                        continue;
                    };
                    weather.days.push(Day {
                        date,
                        code: code.clamp(0, 255) as u8,
                        high,
                        low,
                        rain: number(4).map(|v| v.clamp(0, 100) as u8),
                        sunrise: number(5).map(|v| v.clamp(0, 1439) as u16),
                        sunset: number(6).map(|v| v.clamp(0, 1439) as u16),
                    });
                }
                _ => {}
            }
        }
        Some(weather)
    }

    /// The days from `today` on (all if today is not known): a forecast
    /// kept a while ago still shows what is ahead.
    pub fn days_from(&self, today: Option<i32>) -> &[Day] {
        let Some(today) = today else {
            return &self.days;
        };
        let start = self
            .days
            .iter()
            .position(|d| d.date >= today)
            .unwrap_or(self.days.len());
        &self.days[start..]
    }
}

// ---- Conditions ---------------------------------------------------------

/// What a symbol shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Symbol {
    Clear,
    /// Mostly clear: a little cloud before the sun.
    FewClouds,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    HeavyRain,
    /// Rain and snow, or freezing rain.
    Sleet,
    Snow,
    /// Rain from a cloud before the sun.
    Showers,
    SnowShowers,
    Thunder,
    Hail,
}

/// The translation key of the condition of WMO weather `code` (as
/// Open-Meteo sends it), and its symbol.
pub(crate) fn condition(code: u8) -> (&'static str, Symbol) {
    match code {
        0 => ("weather.condition.clear", Symbol::Clear),
        1 => ("weather.condition.mainly-clear", Symbol::FewClouds),
        2 => ("weather.condition.partly-cloudy", Symbol::PartlyCloudy),
        3 => ("weather.condition.overcast", Symbol::Cloudy),
        45 | 48 => ("weather.condition.fog", Symbol::Fog),
        51 | 53 | 55 => ("weather.condition.drizzle", Symbol::Drizzle),
        56 | 57 => ("weather.condition.freezing-drizzle", Symbol::Sleet),
        61 => ("weather.condition.light-rain", Symbol::Rain),
        63 => ("weather.condition.rain", Symbol::Rain),
        65 => ("weather.condition.heavy-rain", Symbol::HeavyRain),
        66 | 67 => ("weather.condition.freezing-rain", Symbol::Sleet),
        71 => ("weather.condition.light-snow", Symbol::Snow),
        73 | 77 => ("weather.condition.snow", Symbol::Snow),
        75 => ("weather.condition.heavy-snow", Symbol::Snow),
        80 | 81 => ("weather.condition.showers", Symbol::Showers),
        82 => ("weather.condition.heavy-showers", Symbol::Showers),
        85 | 86 => ("weather.condition.snow-showers", Symbol::SnowShowers),
        95 => ("weather.condition.thunder", Symbol::Thunder),
        96 | 99 => ("weather.condition.hail", Symbol::Hail),
        _ => ("weather.condition.unknown", Symbol::Cloudy),
    }
}

// ---- Units --------------------------------------------------------------

/// Units of temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Temperature {
    Celsius,
    Fahrenheit,
}

impl Temperature {
    /// `tenths` of a degree Celsius in this unit, whole degrees.
    pub fn degrees(self, tenths: i32) -> i32 {
        let tenths = i64::from(tenths);
        let tenths = match self {
            Temperature::Celsius => tenths,
            Temperature::Fahrenheit => tenths * 9 / 5 + 320,
        };
        round_tenths(tenths)
    }

    /// The unit's sign: `°C` or `°F`.
    pub fn sign(self) -> &'static str {
        match self {
            Temperature::Celsius => "°C",
            Temperature::Fahrenheit => "°F",
        }
    }
}

/// Units of wind speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wind {
    Kmh,
    Mph,
    Ms,
    Knots,
}

impl Wind {
    /// `tenths` of km/h in this unit, whole.
    pub fn speed(self, tenths: i32) -> i32 {
        let tenths = i64::from(tenths);
        let tenths = match self {
            Wind::Kmh => tenths,
            Wind::Mph => tenths * 1000 / 1609,
            Wind::Ms => tenths * 10 / 36,
            Wind::Knots => tenths * 1000 / 1852,
        };
        round_tenths(tenths)
    }

    /// The translation key of the unit's name with a number.
    pub fn key(self) -> &'static str {
        match self {
            Wind::Kmh => "weather.speed.kmh",
            Wind::Mph => "weather.speed.mph",
            Wind::Ms => "weather.speed.ms",
            Wind::Knots => "weather.speed.knots",
        }
    }
}

/// Tenths to whole numbers, halves away from zero (worked out in i64,
/// so that values at the limits of a kept file or an answer convert).
fn round_tenths(tenths: i64) -> i32 {
    let whole = if tenths >= 0 {
        (tenths + 5) / 10
    } else {
        -((-tenths + 5) / 10)
    };
    whole.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// A temperature as shown: `12°`, `-3°`, with the unit's sign `12°C`.
pub(crate) fn degrees_text(unit: Temperature, tenths: i32, with_unit: bool) -> String {
    let degrees = unit.degrees(tenths);
    if with_unit {
        format!("{degrees}{}", unit.sign())
    } else {
        format!("{degrees}°")
    }
}

// ---- When to fetch --------------------------------------------------------

/// Whether the weather fetched `age` seconds ago (`None`: not known,
/// or none fetched) is to be fetched again, at most every `every`
/// seconds.
pub(crate) fn due(age: Option<i64>, every: i64) -> bool {
    age.is_none_or(|age| age >= every || age < 0)
}

/// How long until the weather `age` seconds old is due again, in
/// seconds, at least `least`.
pub(crate) fn until_due(age: Option<i64>, every: i64, least: i64) -> i64 {
    match age {
        Some(age) if (0..every).contains(&age) => (every - age).max(least),
        _ => least,
    }
}

/// Whether the weather `age` seconds old counts as old, so that its age is
/// shown: from three hours, or twice the time between fetches.
pub(crate) fn is_old(age: i64, every: i64) -> bool {
    age >= (3 * 3600).max(2 * every)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = include_str!("../testdata/forecast.json");
    const SEARCH: &str = include_str!("../testdata/search.json");

    #[test]
    fn numbers_in_tenths() {
        assert_eq!(tenths("12.3"), Some(123));
        assert_eq!(tenths("-3.46"), Some(-35));
        assert_eq!(tenths("-0.4"), Some(-4));
        assert_eq!(tenths("7"), Some(70));
        assert_eq!(tenths(".5"), Some(5));
        assert_eq!(tenths("null"), None);
        assert_eq!(tenths(""), None);
        assert_eq!(tenths("1e5"), None);
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(parse_date("1970-01-02"), Some(1));
        let date = parse_date("2026-10-14").unwrap();
        assert_eq!(weekday(date), 3);
        assert_eq!(parse_date("2026-10-14T07:23"), Some(date));
        assert_eq!(parse_minutes("2026-10-14T07:23"), Some(7 * 60 + 23));
        assert_eq!(parse_minutes("2026-10-14"), None);
        assert_eq!(parse_date("14.10.2026"), None);
    }

    #[test]
    fn coordinates() {
        assert_eq!(short_coordinate("52.52437"), "52.52");
        assert_eq!(short_coordinate("-0.1"), "-0.1");
        assert_eq!(short_coordinate("13.40"), "13.4");
        assert_eq!(short_coordinate("7"), "7");
        assert_eq!(split_coordinates("52.52, 13.41"), Some(("52.52", "13.41")));
        assert_eq!(split_coordinates("95,13"), None);
        assert_eq!(split_coordinates("52.52"), None);
        assert_eq!(split_coordinates("1&x=2,3"), None);
        let url = forecast_url("52.52,13.41").unwrap();
        assert!(
            url.starts_with(
                "https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&"
            )
        );
        assert!(url.contains("forecast_days=8") && !url.contains(' '));
        assert_eq!(forecast_url("nowhere"), None);
        assert_eq!(
            search_url("São Paulo", "pt-BR"),
            "https://geocoding-api.open-meteo.com/v1/search?name=S%C3%A3o+Paulo&count=10&language=pt&format=json"
        );
    }

    #[test]
    fn a_forecast_is_read() {
        let weather = parse_forecast(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
        let now = weather.current.unwrap();
        assert_eq!(now.temperature, 124);
        assert_eq!(now.feels, Some(98));
        assert_eq!(now.humidity, Some(71));
        assert_eq!(now.code, 2);
        assert_eq!(now.wind, Some(154));
        assert!(now.day);
        assert_eq!(weather.days.len(), 7);
        let first = weather.days[0];
        assert_eq!(first.date, parse_date("2026-10-14").unwrap());
        assert_eq!((first.code, first.high, first.low), (2, 145, 62));
        assert_eq!(first.rain, Some(10));
        assert_eq!(first.sunrise, Some(7 * 60 + 23));
        assert_eq!(first.sunset, Some(18 * 60 + 12));
        // A value missing (null) leaves the rest.
        assert_eq!(weather.days[3].rain, None);
        assert_eq!(weather.days[3].low, -12);
        assert!(parse_forecast(b"{\"error\":true,\"reason\":\"x\"}", "1,2", 0).is_none());
        assert!(parse_forecast(b"not json", "1,2", 0).is_none());
    }

    #[test]
    fn places_are_found() {
        let places = parse_places(SEARCH.as_bytes());
        assert_eq!(places.len(), 3);
        assert_eq!(places[0].name, "Berlin");
        assert_eq!(places[0].coordinates(), "52.52,13.41");
        assert_eq!(places[0].whereabouts(), "Land Berlin, Deutschland");
        assert_eq!(places[2].whereabouts(), "Vereinigte Staaten");
        assert!(parse_places(br#"{"generationtime_ms":0.4}"#).is_empty());
    }

    #[test]
    fn the_weather_is_kept() {
        let weather = parse_forecast(FORECAST.as_bytes(), "52.52,13.41", 1_760_000_000).unwrap();
        let text = weather.to_text();
        assert!(text.starts_with("weather 1\n"));
        assert_eq!(Weather::from_text(&text), Some(weather.clone()));
        assert_eq!(Weather::from_text("something else"), None);
        // Today on: the days before are left out.
        let today = weather.days[2].date;
        assert_eq!(weather.days_from(Some(today)).len(), 5);
        assert_eq!(weather.days_from(None).len(), 7);
        assert!(weather.days_from(Some(today + 30)).is_empty());
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn weather_of_version_1_is_read() {
        let weather = Weather::from_text(include_str!("../testdata/weather.txt.v1")).unwrap();
        assert_eq!(weather.coordinates, "52.52,13.41");
        assert_eq!(weather.current.unwrap().temperature, 124);
        assert_eq!(weather.days.len(), 7);
    }

    #[test]
    fn codes_have_conditions() {
        assert_eq!(condition(0), ("weather.condition.clear", Symbol::Clear));
        assert_eq!(condition(3).1, Symbol::Cloudy);
        assert_eq!(condition(48).1, Symbol::Fog);
        assert_eq!(condition(65).1, Symbol::HeavyRain);
        assert_eq!(condition(67).1, Symbol::Sleet);
        assert_eq!(condition(77).1, Symbol::Snow);
        assert_eq!(condition(81).1, Symbol::Showers);
        assert_eq!(condition(86).1, Symbol::SnowShowers);
        assert_eq!(condition(95).1, Symbol::Thunder);
        assert_eq!(condition(99).1, Symbol::Hail);
        assert_eq!(condition(42).0, "weather.condition.unknown");
    }

    #[test]
    fn units() {
        let c = Temperature::Celsius;
        let f = Temperature::Fahrenheit;
        assert_eq!(c.degrees(124), 12);
        assert_eq!(c.degrees(125), 13);
        assert_eq!(c.degrees(-25), -3);
        assert_eq!(c.degrees(-4), 0);
        assert_eq!(f.degrees(0), 32);
        assert_eq!(f.degrees(1000), 212);
        assert_eq!(f.degrees(-400), -40);
        assert_eq!(degrees_text(c, 124, false), "12°");
        assert_eq!(degrees_text(f, 124, true), "54°F");
        assert_eq!(Wind::Kmh.speed(154), 15);
        assert_eq!(Wind::Mph.speed(1609), 100);
        assert_eq!(Wind::Ms.speed(360), 10);
        assert_eq!(Wind::Knots.speed(1852), 100);
    }

    #[test]
    fn the_fetch_schedule() {
        let hour = 3600;
        // Nothing fetched yet, or not known when: now.
        assert!(due(None, hour));
        assert!(!due(Some(10 * 60), hour));
        assert!(due(Some(hour), hour));
        // A clock set back: fetched again.
        assert!(due(Some(-5), hour));
        // The next look when it is due, but not too soon.
        assert_eq!(until_due(Some(10 * 60), hour, 300), 50 * 60);
        assert_eq!(until_due(Some(hour - 10), hour, 300), 300);
        assert_eq!(until_due(None, hour, 300), 300);
        assert!(!is_old(2 * hour, hour));
        assert!(is_old(3 * hour, hour));
        assert!(!is_old(5 * hour, 3 * hour));
    }
}

#[cfg(test)]
mod roundtrip_tests;
