//! Clocks, calendars and the year:
//!
//! - analogue clock: a round face with hands (`analogue`);
//! - word clock: the time in words, as text or as a grid of letters
//!   (`word_clock`);
//! - clock in tiles: each digit on a tile of its own (`tiles`);
//! - world clock: the time in up to three other places (`world`);
//! - calendar: this month as a grid, and the next (`calendar`);
//! - the year so far: a bar or a ring (`year`);
//! - countdown: the days until a date the reader sets (`countdown`);
//! - moon phase: today's moon, drawn (`moon`);
//! - day and date: the weekday and the date large (`day`).
//!
//! Each has a settings group of its own, with its frame, where its text
//! sits, and switches for what it shows. They read nothing from the
//! storage: they draw from the clock and their settings.

mod analogue;
mod calendar;
mod countdown;
mod dates;
mod day;
mod moon;
mod tiles;
mod word_clock;
mod world;
mod year;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, Align, Drawn, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, FontHandle, Id, Params, Rect, RegisterError, Registrar,
    SettingSpec,
};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/time.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/time.de.ini")))?;
    analogue::register(r)?;
    word_clock::register(r)?;
    tiles::register(r)?;
    world::register(r)?;
    calendar::register(r)?;
    year::register(r)?;
    countdown::register(r)?;
    moon::register(r)?;
    day::register(r)?;
    Ok(())
}

/// Adds widget `id`, drawn by `drawn`, to the kind "Time and date", with
/// its settings group (its settings come from a list kept in the
/// program, each module's `SETTINGS`).
fn add(r: &mut Registrar, id: &Id, title: &'static str, drawn: Drawn) -> Result<(), RegisterError> {
    r.add_widget(WidgetSpec::drawn(id.clone(), title, drawn).category(widgets::categories::TIME))?;
    r.add_setting_group(widgets::settings_group(id, title))
}

/// Adds widget `id` like [`add`], with a tap on it opening its settings:
/// for widgets that show what the reader chose (a date, places).
fn add_settable(
    r: &mut Registrar,
    id: &Id,
    title: &'static str,
    drawn: Drawn,
) -> Result<(), RegisterError> {
    let widget = id.clone();
    r.add_widget(
        WidgetSpec::new(id.clone(), title, move |_| {
            Box::new(Settable {
                id: widget.clone(),
                drawn,
                shown: None,
            })
        })
        .category(widgets::categories::TIME),
    )?;
    r.add_setting_group(widgets::settings_group(id, title))
}

/// A widget that draws from the clock and its settings, which a tap
/// opens.
struct Settable {
    id: Id,
    drawn: Drawn,
    shown: Option<String>,
}

impl Widget for Settable {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let Some(shows) = self.drawn.shows else {
            return false;
        };
        let now = Some(shows(env));
        let changed = self.shown.is_some() && now != self.shown;
        self.shown = now;
        changed
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        (self.drawn.height)(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        (self.drawn.draw)(canvas, env, area);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let params = Params::new().with(params::ID, alloc::string::ToString::to_string(&self.id));
        alloc::vec![WidgetTarget {
            area,
            command: Command::OpenSlot(slots::SETTINGS, params),
        }]
    }
}

/// The height `widget`'s frame takes above and below its content.
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 100, 100);
    100 - widgets::look::look(env, widget, probe).area.height
}

/// The value of `widget`'s choice `key`, the part after the last dot
/// (e.g. `circle` of `widgets.mark.circle`).
fn chosen<'a>(env: &'a Env<'_>, widget: &str, key: &str) -> &'a str {
    let value = env.settings.text(&format!("{widget}.{key}"));
    value.rsplit('.').next().unwrap_or(value)
}

/// The colour lighter of the look's ink and paper, and the darker one:
/// e.g. the lit and the dark part of the moon.
fn light_and_dark(look: &widgets::Look) -> (Color, Color) {
    if look.ink.luma() > look.paper.luma() {
        (look.ink, look.paper)
    } else {
        (look.paper, look.ink)
    }
}

/// The name of weekday `n` (0: Sunday).
fn weekday_name<'a>(env: &'a Env<'_>, n: u8) -> &'a str {
    const KEYS: [&str; 7] = [
        "core.weekday.0",
        "core.weekday.1",
        "core.weekday.2",
        "core.weekday.3",
        "core.weekday.4",
        "core.weekday.5",
        "core.weekday.6",
    ];
    env.i18n.tr(KEYS[usize::from(n % 7)])
}

/// The name of month `n` (1 to 12).
fn month_name<'a>(env: &'a Env<'_>, n: u8) -> &'a str {
    const KEYS: [&str; 12] = [
        "core.month.1",
        "core.month.2",
        "core.month.3",
        "core.month.4",
        "core.month.5",
        "core.month.6",
        "core.month.7",
        "core.month.8",
        "core.month.9",
        "core.month.10",
        "core.month.11",
        "core.month.12",
    ];
    env.i18n.tr(KEYS[usize::from(n.clamp(1, 12) - 1)])
}

/// The values of a date format setting ([`date_format_spec`]).
const DATE_FORMATS: [&str; 4] = [
    "widgets.date-format.long",
    "widgets.date-format.weekday",
    "widgets.date-format.numeric",
    "widgets.date-format.iso",
];

/// A setting of how `widget` writes dates: "14 October 2026",
/// "Wednesday, 14 October", "14/10/2026" or "2026-10-14"; `id` is
/// `<widget>.date-format`.
const fn date_format_spec(id: &'static str, widget: Id, default: &'static str) -> SettingSpec {
    SettingSpec::fixed_choice(
        id,
        "widgets.setting.date-format",
        &DATE_FORMATS,
        default,
        widget,
    )
}

/// `year`-`month`-`day` written as `widget`'s date format says
/// ([`date_format_spec`]); `year` left out where `with_year` is not
/// set (but in numbers).
fn date_text(
    env: &Env<'_>,
    widget: &str,
    year: i32,
    month: u8,
    day: u8,
    with_year: bool,
) -> String {
    let month_name = month_name(env, month);
    let weekday = weekday_name(env, dates::weekday(year, month, day));
    let args = [
        ("day", Arg::Int(i64::from(day))),
        ("month", Arg::Str(month_name)),
        ("year", Arg::Int(i64::from(year))),
        ("weekday", Arg::Str(weekday)),
    ];
    match chosen(env, widget, "date-format") {
        "iso" => format!("{year:04}-{month:02}-{day:02}"),
        "numeric" => env.i18n.format(
            "widgets.date.numeric",
            &[
                ("day", Arg::Str(&format!("{day:02}"))),
                ("month", Arg::Str(&format!("{month:02}"))),
                ("year", Arg::Int(i64::from(year))),
            ],
        ),
        "weekday" => env.i18n.format("core.date", &args),
        _ if with_year => env.i18n.format("widgets.date.long", &args),
        _ => env.i18n.format("widgets.date.short", &args),
    }
}

/// The time `t` as the reader wants to see it: `14:05` or `2:05 PM`
/// (setting `core.clock-format`).
fn clock_text(env: &Env<'_>, t: &LocalTime) -> String {
    let (hour, minute) = (t.hour, t.minute);
    if twelve_hours(env) {
        let key = if hour < 12 {
            "core.clock.am"
        } else {
            "core.clock.pm"
        };
        let time = format!("{}:{minute:02}", twelve(hour));
        env.i18n.format(key, &[("time", Arg::Str(&time))])
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// Whether the reader wants the time in twelve hours.
fn twelve_hours(env: &Env<'_>) -> bool {
    env.settings
        .read(&paperpaw_sdk::builtin::settings::CLOCK_FORMAT)
        == time::CLOCK_12H
}

/// `hour` (0 to 23) on a twelve-hour clock: 1 to 12.
fn twelve(hour: u8) -> u8 {
    match hour % 12 {
        0 => 12,
        h => h,
    }
}

/// Draws in `look`'s area that the time is not known yet, in place of
/// what a widget would show of it: on a device without a clock of its
/// own, until Wi-Fi sets it.
fn draw_no_time(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &widgets::Look) {
    widgets::draw_message(canvas, env, look, env.i18n.tr("widgets.time.unknown"));
}

/// What a widget that shows the date shows: drawn again when the day
/// changes.
fn today_shows(env: &Env<'_>) -> String {
    time::local_time(env).map_or_else(String::new, |t| format!("{}-{}-{}", t.year, t.month, t.day))
}

/// What a widget that shows the time shows: drawn again each minute.
fn minute_shows(env: &Env<'_>) -> String {
    time::local_time(env).map_or_else(String::new, |t| {
        format!("{}-{}-{} {}:{}", t.year, t.month, t.day, t.hour, t.minute)
    })
}

/// Draws `text` `size` pixels large in the row `row`, placed as `align`
/// says, shortened with "…" where it is wider.
#[allow(clippy::too_many_arguments)]
fn label(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    align: Align,
    row: Rect,
    text: &str,
    color: Color,
) {
    let text = paperpaw_sdk::ui::fit(font, size, text, f32::from(row.width));
    let x = align.x(row, font.measure(size, &text));
    paperpaw_sdk::ui::draw_label(canvas, font, size, x, row, &text, color);
}

/// The size at which `text` is `width` pixels wide at most, and `size`
/// at most.
fn fit_size(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> f32 {
    let mut size = size;
    // Widths do not grow quite evenly with the size (each letter's is
    // rounded, fonts of fixed sizes take the nearest they have): steps
    // down to where it fits.
    let mut last = (f32::MAX, size);
    for step in 0..12 {
        let wide = font.measure(size, text);
        if wide <= width || wide <= 0.0 {
            break;
        }
        if wide >= last.0 {
            // The smallest the font has: no smaller.
            return last.1;
        }
        last = (wide, size);
        size *= (width / wide).min(0.99) * if step > 0 { 0.96 } else { 1.0 };
    }
    size
}

/// The size at which each word of `text` is `width` pixels wide at most
/// (so that lines break between words, not in them), and `size` at most.
fn fit_words(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> f32 {
    text.split_whitespace()
        .map(|word| fit_size(font, size, word, width))
        .fold(size, f32::min)
}

/// `text` broken into lines `width` pixels wide at `size`, `most` at
/// most, the last one shortened with "…" where there is more.
fn wrap(font: &FontHandle<'_>, size: f32, text: &str, width: f32, most: usize) -> Vec<String> {
    let ranges = font.wrap(size, text, width);
    let cut = ranges.len() > most;
    let mut lines: Vec<String> = ranges
        .iter()
        .take(most.max(1))
        .map(|r| String::from(text[r.clone()].trim()))
        .collect();
    if cut && let (Some(last), Some(range)) = (lines.last_mut(), ranges.get(most.max(1) - 1)) {
        let rest = text[range.start..].trim();
        *last = paperpaw_sdk::ui::fit(font, size, rest, width);
    }
    lines
}

/// `items` (e.g. "Day 287 of 365", "Week 42") in lines `width` pixels
/// wide at `size`, separated by " · " within a line, `most` lines at
/// most: an item is not broken where it can go whole to the next line.
fn pack(
    font: &FontHandle<'_>,
    size: f32,
    items: &[String],
    width: f32,
    most: usize,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for item in items {
        let full = lines.len() >= most.max(1);
        match lines.last_mut() {
            Some(last) if font.measure(size, &format!("{last} · {item}")) <= width => {
                last.push_str(" · ");
                last.push_str(item);
            }
            Some(last) if full => {
                // No more lines: the rest shortened on the last.
                let joined = format!("{last} · {item}");
                *last = paperpaw_sdk::ui::fit(font, size, &joined, width);
                break;
            }
            _ => lines.push(item.clone()),
        }
    }
    lines
}

/// The size of each of `n` parts side by side in `room` pixels with
/// `gap` between them: nothing where the gaps take all the room.
fn share(room: u16, gap: u16, n: u16) -> u16 {
    room.saturating_sub(gap.saturating_mul(n.saturating_sub(1))) / n.max(1)
}

/// How many of `n` parts fit side by side in `room` pixels with `gap`
/// between them, each `least` pixels at least: all where they do, else
/// as many as do, one at least.
fn fitting(room: u16, gap: u16, least: u16, n: usize) -> usize {
    let each = u32::from(least.max(1)) + u32::from(gap);
    let fit = (u32::from(room) + u32::from(gap)) / each;
    n.min(fit as usize).max(1)
}

/// The height of a line of text `size` pixels large, as a float.
fn line(font: &FontHandle<'_>, size: f32) -> f32 {
    font.line_metrics(size).line_height
}

/// Draws `text` with the middle of its digits (or capitals) at `y` and
/// its middle at `x`: centred by its ink, not by its line, e.g. a
/// numeral on a clock face or a digit on a tile.
fn draw_centred(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    y: f32,
    text: &str,
    color: Color,
) {
    let width = font.measure(size, text);
    let baseline = y + cap_height(font, size) / 2.0;
    font.draw(canvas, size, x - width / 2.0, baseline as i32, text, color);
}

/// The height of digits and capitals at `size`, about.
fn cap_height(font: &FontHandle<'_>, size: f32) -> f32 {
    // Most fonts' digits are about three quarters of their ascent (at
    // the size the font draws: fonts of fixed sizes take the nearest).
    font.line_metrics(size).ascent * 0.74
}

/// The UI font and its style.
fn ui<'a>(env: &Env<'a>) -> (UiStyle, Option<FontHandle<'a>>) {
    let style = UiStyle::new(env);
    let font = style.font(env);
    (style, font)
}
