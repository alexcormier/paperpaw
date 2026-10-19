//! The countdown: the days until a date the reader sets, once
//! (`2026-12-24`) or each year (`12-24`, e.g. a birthday), with a title
//! ("12 days until Holiday"). On the day it says so; a day once passed is
//! counted since. A tap opens its settings.

use alloc::format;
use alloc::string::String;

use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::time;
use quireink_sdk::widgets::{self, Align, Drawn, Frame, Size};
use quireink_sdk::{Arg, Canvas, Env, FontHandle, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::word_clock::{balanced, largest};
use super::{chosen, dates, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.countdown");
/// The date counted to: `YYYY-MM-DD` once, `MM-DD` each year (also
/// `DD.MM.YYYY` and `DD.MM.`).
const DATE: Setting<Text> = Setting::new("widgets.countdown.date");
/// What happens that day, e.g. "Holiday"; without it, the date.
const TITLE: Setting<Text> = Setting::new("widgets.countdown.title");
/// The values of the style setting: the number large, a sentence.
const STYLES: [&str; 2] = [
    "widgets.countdown.style.number",
    "widgets.countdown.style.sentence",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 9] = [
    widgets::align_spec("widgets.countdown.align", ID, Align::Left),
    widgets::frame_spec("widgets.countdown.frame", ID, Frame::None),
    SettingSpec::fixed_text(
        "widgets.countdown.title",
        "widgets.setting.countdown-title",
        "",
        ID,
    )
    .described("widgets.setting.countdown-title.help"),
    SettingSpec::fixed_text(
        "widgets.countdown.date",
        "widgets.setting.countdown-date",
        "12-24",
        ID,
    )
    .described("widgets.setting.countdown-date.help"),
    SettingSpec::fixed_choice(
        "widgets.countdown.style",
        "widgets.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    ),
    widgets::size_spec("widgets.countdown.size", ID, Size::Fit, true),
    widgets::part_spec(
        "widgets.countdown.show-date",
        ID,
        "widgets.setting.show-date",
        true,
    ),
    widgets::part_spec(
        "widgets.countdown.weeks",
        ID,
        "widgets.setting.weeks",
        false,
    ),
    super::date_format_spec("widgets.countdown.date-format", ID, super::DATE_FORMATS[1]),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add_settable(
        r,
        &ID,
        "widgets.countdown",
        Drawn::new(height, draw).shows(shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// A date to count to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Once(i32, u8, u8),
    /// Each year: month, day.
    Yearly(u8, u8),
}

/// The date in `text`: `YYYY-MM-DD`, `MM-DD`, `DD.MM.YYYY` or `DD.MM.`.
pub(super) fn parse(text: &str) -> Option<Target> {
    let text = text.trim().trim_start_matches("--");
    let numbers = |sep: char| -> Option<([u32; 3], usize)> {
        let mut out = [0u32; 3];
        let mut count = 0;
        for part in text.split(sep).filter(|p| !p.is_empty()) {
            if count == 3 {
                return None;
            }
            out[count] = part.trim().parse().ok()?;
            count += 1;
        }
        Some((out, count))
    };
    let (year, month, day) = if text.contains('.') {
        match numbers('.')? {
            ([d, m, y], 3) => (Some(y), m, d),
            ([d, m, _], 2) => (None, m, d),
            _ => return None,
        }
    } else {
        match numbers('-')? {
            ([y, m, d], 3) => (Some(y), m, d),
            ([m, d, _], 2) => (None, m, d),
            _ => return None,
        }
    };
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let (month, day) = (month as u8, day as u8);
    match year {
        Some(y) if (1..=9999).contains(&y) => {
            let y = y as i32;
            (day <= dates::days_in_month(y, month)).then_some(Target::Once(y, month, day))
        }
        Some(_) => None,
        // Each year: 29 February too (the 28th where there is none).
        None => (day <= dates::days_in_month(2024, month)).then_some(Target::Yearly(month, day)),
    }
}

/// The days from `today` to the target's next day (negative once passed)
/// and that day.
pub(super) fn days_until(today: (i32, u8, u8), target: Target) -> (i64, (i32, u8, u8)) {
    let from = dates::days_from_civil(today.0, today.1, today.2);
    let day = match target {
        Target::Once(y, m, d) => (y, m, d),
        Target::Yearly(m, d) => {
            let in_year = |y: i32| (y, m, d.min(dates::days_in_month(y, m)));
            let this = in_year(today.0);
            if dates::days_from_civil(this.0, this.1, this.2) >= from {
                this
            } else {
                in_year(today.0 + 1)
            }
        }
    };
    (dates::days_from_civil(day.0, day.1, day.2) - from, day)
}

fn shows(env: &Env<'_>) -> String {
    format!(
        "{}|{}|{}",
        super::today_shows(env),
        env.settings.read(&DATE),
        env.settings.read(&TITLE)
    )
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    widgets::line_height(env, style.title * 1.6)
        + widgets::line_height(env, style.small) * 2
        + frame_height(env, ID.as_str())
}

/// What it shows: the number (or "Today"), and what it counts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Shown {
    /// The number of days, or "Today", "Tomorrow", "Yesterday".
    pub big: String,
    /// "days until Holiday".
    pub note: String,
    /// The whole as a sentence: "12 days until Holiday".
    pub sentence: String,
}

/// The texts for `days` days to (or since) `title`.
pub(super) fn shown(i18n: &quireink_sdk::Translator, days: i64, title: &str) -> Shown {
    let args = [("title", Arg::Str(title))];
    let word = |key: &str| String::from(i18n.tr(key));
    match days {
        0 => Shown {
            big: word("widgets.countdown.today"),
            note: String::from(title),
            sentence: i18n.format("widgets.countdown.today-is", &args),
        },
        1 => Shown {
            big: word("widgets.countdown.tomorrow"),
            note: String::from(title),
            sentence: i18n.format("widgets.countdown.tomorrow-is", &args),
        },
        -1 => Shown {
            big: word("widgets.countdown.yesterday"),
            note: String::from(title),
            sentence: i18n.format("widgets.countdown.yesterday-was", &args),
        },
        2.. => Shown {
            big: format!("{days}"),
            note: i18n.plural("widgets.countdown.until", days, &args),
            sentence: i18n.plural("widgets.countdown.days-until", days, &args),
        },
        _ => Shown {
            big: format!("{}", -days),
            note: i18n.plural("widgets.countdown.since", -days, &args),
            sentence: i18n.plural("widgets.countdown.days-ago", -days, &args),
        },
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let (Some(font), Some(now)) = (font, time::local_time(env)) else {
        return;
    };
    let Some(target) = parse(env.settings.read(&DATE)) else {
        widgets::draw_message(canvas, env, &look, env.i18n.tr("widgets.countdown.no-date"));
        return;
    };
    let (days, day) = days_until((now.year, now.month, now.day), target);
    let date = super::date_text(
        env,
        ID.as_str(),
        day.0,
        day.1,
        day.2,
        matches!(target, Target::Once(..)) && day.0 != now.year,
    );
    let title = env.settings.read(&TITLE).trim();
    let titled = !title.is_empty();
    // Without a title, the date in short: "24 December".
    let short = if day.0 == now.year {
        env.i18n.format(
            "widgets.date.short",
            &[
                ("day", Arg::Int(i64::from(day.2))),
                ("month", Arg::Str(super::month_name(env, day.1))),
            ],
        )
    } else {
        date.clone()
    };
    let title = if titled { title } else { &short };
    let shown = shown(env.i18n, days, title);
    // Notes below: the date (where the title is not it already) and the
    // weeks.
    let mut notes = alloc::vec::Vec::new();
    if widgets::shows(env, ID.as_str(), "weeks") && days.abs() >= 7 {
        let weeks = env
            .i18n
            .plural("widgets.countdown.weeks", days.abs() / 7, &[]);
        let rest = days.abs() % 7;
        notes.push(if rest == 0 {
            weeks
        } else {
            let rest = env.i18n.plural("widgets.countdown.days", rest, &[]);
            format!("{weeks}, {rest}")
        });
    }
    if widgets::shows(env, ID.as_str(), "show-date") && titled {
        notes.push(date.clone());
    }
    let notes = notes.join(" · ");
    let area = look.area;
    let align = widgets::align(env, ID.as_str());
    let small = line(&font, style.small);
    let notes_h = if notes.is_empty() { 0.0 } else { small };
    if chosen(env, ID.as_str(), "style") == "sentence" {
        let room = Rect::new(
            area.x,
            area.y,
            area.width,
            (f32::from(area.height) - notes_h) as u16,
        );
        let size = text_size(env, &font, &shown.sentence, room);
        let lines = balanced(&font, size, &shown.sentence, f32::from(room.width));
        let height = line(&font, size);
        let total = height * lines.len() as f32 + notes_h;
        let mut y = f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0;
        for text in &lines {
            let row = Rect::new(area.x, y as u16, area.width, height as u16);
            super::label(canvas, &font, size, align, row, text, look.ink);
            y += height;
        }
        if !notes.is_empty() {
            let row = Rect::new(area.x, y as u16, area.width, small as u16);
            super::label(canvas, &font, style.small, align, row, &notes, look.faint);
        }
        return;
    }
    // The number large, what it counts to below it (in two lines where
    // it is wider and there is room).
    let note_size = style
        .text
        .min(f32::from(area.height) / 5.0)
        .max(style.small);
    let note_line = line(&font, note_size);
    let most = if f32::from(area.height) - notes_h >= note_line * 3.0 {
        2
    } else {
        1
    };
    // In one line: smaller, down to the small size, before it is cut.
    let note_size = if most == 1 {
        super::fit_size(&font, note_size, &shown.note, f32::from(area.width)).max(style.small)
    } else {
        note_size
    };
    let note_lines = super::wrap(&font, note_size, &shown.note, f32::from(area.width), most);
    let notes_room = note_line * note_lines.len() as f32;
    let room = (f32::from(area.height) - notes_room - notes_h).max(note_line);
    let size = match widgets::size(env, ID.as_str()) {
        Size::Fit => super::fit_size(&font, room * 0.78, &shown.big, f32::from(area.width)),
        size => size.text(&style) * 1.6,
    };
    let big = line(&font, size).min(room);
    let total = big + notes_room + notes_h;
    let mut y = f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0;
    let row = Rect::new(area.x, y as u16, area.width, big as u16);
    super::label(canvas, &font, size, align, row, &shown.big, look.ink);
    y += big;
    for text in &note_lines {
        let row = Rect::new(area.x, y as u16, area.width, note_line as u16);
        super::label(canvas, &font, note_size, align, row, text, look.ink);
        y += note_line;
    }
    if !notes.is_empty() {
        let row = Rect::new(area.x, y as u16, area.width, small as u16);
        super::label(canvas, &font, style.small, align, row, &notes, look.faint);
    }
}

/// The size of the sentence: as large as fits, or as set.
fn text_size(env: &Env<'_>, font: &FontHandle<'_>, text: &str, room: Rect) -> f32 {
    let (style, _) = ui(env);
    match widgets::size(env, ID.as_str()) {
        Size::Fit => largest(
            font,
            text,
            room,
            style.small,
            f32::from(room.height).min(style.title * 2.4),
        ),
        size => size.text(&style),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quireink_i18n::Catalog;

    #[test]
    fn dates_are_read() {
        assert_eq!(parse("2026-12-24"), Some(Target::Once(2026, 12, 24)));
        assert_eq!(parse(" 12-24 "), Some(Target::Yearly(12, 24)));
        assert_eq!(parse("--02-29"), Some(Target::Yearly(2, 29)));
        assert_eq!(parse("24.12.2026"), Some(Target::Once(2026, 12, 24)));
        assert_eq!(parse("24.12."), Some(Target::Yearly(12, 24)));
        assert_eq!(parse("2026-02-29"), None);
        assert_eq!(parse("2028-02-29"), Some(Target::Once(2028, 2, 29)));
        assert_eq!(parse("13-01"), None);
        assert_eq!(parse("Christmas"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn days_are_counted() {
        let today = (2026, 10, 14);
        assert_eq!(
            days_until(today, Target::Yearly(12, 24)),
            (71, (2026, 12, 24))
        );
        assert_eq!(days_until(today, Target::Yearly(10, 14)).0, 0);
        assert_eq!(days_until(today, Target::Yearly(10, 15)).0, 1);
        // Passed this year: next year's.
        assert_eq!(
            days_until(today, Target::Yearly(10, 13)),
            (364, (2027, 10, 13))
        );
        // Once, passed: counted since.
        assert_eq!(days_until(today, Target::Once(2026, 10, 11)).0, -3);
        assert_eq!(days_until(today, Target::Once(2027, 1, 1)).0, 79);
        // 29 February each year: the 28th where there is none.
        assert_eq!(days_until(today, Target::Yearly(2, 29)).1, (2027, 2, 28));
        assert_eq!(
            days_until((2027, 3, 1), Target::Yearly(2, 29)).1,
            (2028, 2, 29)
        );
    }

    #[test]
    fn kind_words() {
        let mut t = quireink_sdk::Translator::new();
        t.add(Catalog::parse(include_str!("../../i18n/time.en.ini")).unwrap());
        t.add(Catalog::parse(include_str!("../../i18n/time.de.ini")).unwrap());
        t.set_language("en");
        assert_eq!(shown(&t, 12, "Holiday").sentence, "12 days until Holiday");
        assert_eq!(shown(&t, 12, "Holiday").note, "days until Holiday");
        assert_eq!(shown(&t, 0, "Holiday").sentence, "Holiday is today!");
        assert_eq!(shown(&t, 1, "Holiday").sentence, "Holiday is tomorrow");
        assert_eq!(shown(&t, -3, "Holiday").sentence, "Holiday was 3 days ago");
        assert_eq!(shown(&t, -3, "Holiday").note, "days since Holiday");
        t.set_language("de");
        assert_eq!(shown(&t, 12, "Urlaub").sentence, "Noch 12 Tage bis Urlaub");
        assert_eq!(shown(&t, 0, "Urlaub").sentence, "Heute ist Urlaub!");
        assert_eq!(shown(&t, 2, "Urlaub").big, "2");
    }
}
