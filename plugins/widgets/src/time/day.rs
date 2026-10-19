//! The day and the date large, a calm alternative to the home screen's
//! date: the weekday large with the date below it, the day's number large
//! beside the weekday and the month, or a sheet of a tear-off calendar.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use quireink_sdk::time;
use quireink_sdk::widgets::{self, Align, Drawn, Frame, Size};
use quireink_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar};

use super::{chosen, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.day");

/// The values of the style setting: the weekday large, the number large,
/// a calendar sheet.
const STYLES: [&str; 3] = [
    "widgets.day.style.weekday",
    "widgets.day.style.number",
    "widgets.day.style.sheet",
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        &ID,
        "widgets.day",
        Drawn::new(height, draw).shows(super::today_shows),
        Align::Left,
        Frame::None,
    )?;
    r.add_setting(widgets::choice(
        &ID,
        "style",
        "widgets.setting.style",
        &STYLES,
        STYLES[0],
    ))?;
    r.add_setting(widgets::size_setting(&ID, Size::Fit, true))?;
    r.add_setting(super::date_format_setting(&ID, super::DATE_FORMATS[0]))?;
    for (part, title, default) in [
        ("year", "widgets.setting.year", true),
        ("week", "widgets.setting.week-number", false),
    ] {
        r.add_setting(widgets::part_setting(&ID, part, title, default))?;
    }
    Ok(())
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    let size = big_size(env, None);
    widgets::line_height(env, size)
        + widgets::line_height(env, style.text) * 2
        + frame_height(env, ID.as_str())
}

/// The size of what is large: as set, or as fits `room` (height).
fn big_size(env: &Env<'_>, room: Option<f32>) -> f32 {
    let (style, _) = ui(env);
    match (widgets::size(env, ID.as_str()), room) {
        (Size::Fit, Some(room)) => room,
        (Size::Fit, None) => style.title * 2.0,
        (size, _) => size.text(&style) * 1.6,
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let (Some(font), Some(t)) = (font, time::local_time(env)) else {
        return;
    };
    let area = look.area;
    let align = widgets::align(env, ID.as_str());
    let with_year = widgets::shows(env, ID.as_str(), "year");
    let weekday = super::weekday_name(env, t.weekday);
    let month = super::month_name(env, t.month);
    let week = widgets::shows(env, ID.as_str(), "week").then(|| {
        let week = super::dates::iso_week(t.year, t.month, t.day);
        env.i18n
            .format("widgets.year.week", &[("week", Arg::Int(i64::from(week)))])
    });
    let width = f32::from(area.width);
    match chosen(env, ID.as_str(), "style") {
        "number" => {
            // "14" large; beside it the weekday, and the month below it.
            let number = format!("{}", t.day);
            let month_line = if with_year {
                format!("{month} {}", t.year)
            } else {
                String::from(month)
            };
            let mut notes: Vec<&str> = alloc::vec![weekday, &month_line];
            if let Some(week) = &week {
                notes.push(week);
            }
            let room = f32::from(area.height);
            let size = super::fit_size(
                &font,
                big_size(env, Some(room * 0.95)),
                &number,
                width * 0.45,
            );
            let number_w = font.measure(size, &number);
            let gap = style.gap as f32 * 1.5;
            // The notes as large as fits beside it, the weekday larger.
            let rest = (width - number_w - gap).max(0.0);
            let note = notes
                .iter()
                .map(|n| super::fit_size(&font, style.title * 1.2, n, rest))
                .fold(style.title * 1.2, f32::min)
                .min(room / (notes.len() as f32 + 0.6))
                .max(style.small);
            let lines: f32 = line(&font, note * 1.2) + line(&font, note) * (notes.len() - 1) as f32;
            let total_w = number_w
                + gap
                + notes
                    .iter()
                    .enumerate()
                    .map(|(n, text)| font.measure(if n == 0 { note * 1.2 } else { note }, text))
                    .fold(0.0, f32::max);
            let left = align.x(area, total_w.min(width));
            let big = line(&font, size).min(room);
            let row = Rect::new(
                left as u16,
                area.y + ((room - big) / 2.0) as u16,
                number_w as u16 + 2,
                big as u16,
            );
            super::label(canvas, &font, size, Align::Left, row, &number, look.ink);
            let x = left + number_w + gap;
            let mut y = f32::from(area.y) + (room - lines).max(0.0) / 2.0;
            for (n, text) in notes.iter().enumerate() {
                let (s, color) = if n == 0 {
                    (note * 1.2, look.ink)
                } else {
                    (note, look.faint)
                };
                let h = line(&font, s);
                let row = Rect::new(
                    x as u16,
                    y as u16,
                    (f32::from(area.x) + width - x).max(0.0) as u16,
                    h as u16,
                );
                super::label(canvas, &font, s, Align::Left, row, text, color);
                y += h;
            }
        }
        "sheet" => draw_sheet(canvas, env, &font, &look, &t, with_year, week.as_deref()),
        _ => {
            // "Wednesday" large, the date below it.
            let date = super::date_text(env, ID.as_str(), t.year, t.month, t.day, with_year);
            let date = match &week {
                Some(week) => format!("{date} · {week}"),
                None => date,
            };
            let note = style
                .title
                .min(f32::from(area.height) / 3.5)
                .max(style.small);
            let note = super::fit_size(&font, note, &date, width).max(style.small);
            let note_h = line(&font, note);
            let room = (f32::from(area.height) - note_h).max(0.0);
            let size = super::fit_size(&font, big_size(env, Some(room * 0.8)), weekday, width);
            let big = line(&font, size).min(room);
            let top = f32::from(area.y) + (f32::from(area.height) - big - note_h).max(0.0) / 2.0;
            let row = Rect::new(area.x, top as u16, area.width, big as u16);
            super::label(canvas, &font, size, align, row, weekday, look.ink);
            let row = Rect::new(area.x, (top + big) as u16, area.width, note_h as u16);
            super::label(canvas, &font, note, align, row, &date, look.faint);
        }
    }
}

/// A sheet of a tear-off calendar: the month on a dark band at the top,
/// the day's number large, the weekday below it.
#[allow(clippy::too_many_arguments)]
fn draw_sheet(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &quireink_sdk::FontHandle<'_>,
    look: &widgets::Look,
    t: &quireink_sdk::time::LocalTime,
    with_year: bool,
    week: Option<&str>,
) {
    let (style, _) = ui(env);
    let area = look.area;
    // As wide as about four fifths of its height, at most.
    let height = f32::from(area.height);
    let width = f32::from(area.width)
        .min(height * 0.95)
        .max(height * 0.6)
        .min(f32::from(area.width));
    let align = widgets::align(env, ID.as_str());
    let left = align.x(area, width);
    let sheet = Rect::new(left as u16, area.y, width as u16, area.height);
    let radius = style.corner.max((width * 0.06) as u32);
    let month = super::month_name(env, t.month);
    let month = if with_year {
        format!("{month} {}", t.year)
    } else {
        String::from(month)
    };
    let band_size = super::fit_size(font, (height * 0.11).max(style.small), &month, width * 0.85);
    let band_h = (line(font, band_size) * 1.25).min(height * 0.3);
    // The band in ink, the sheet's paper inside a line round it.
    let thick = style.line * 2;
    canvas.fill_round(sheet, radius, look.ink);
    let inner = Rect::new(
        sheet.x + thick as u16,
        sheet.y + band_h as u16,
        sheet.width.saturating_sub(2 * thick as u16),
        sheet.height.saturating_sub(band_h as u16 + thick as u16),
    );
    canvas.fill_round(inner, radius.saturating_sub(thick), look.paper);
    // Square where it meets the band.
    canvas.fill(
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            (radius as u16).min(inner.height),
        ),
        look.paper,
    );
    let cx = left + width / 2.0;
    super::draw_centred(
        canvas,
        font,
        band_size,
        cx,
        f32::from(sheet.y) + band_h / 2.0,
        &month,
        look.paper,
    );
    let weekday = super::weekday_name(env, t.weekday);
    let mut notes: Vec<&str> = alloc::vec![weekday];
    if let Some(week) = week {
        notes.push(week);
    }
    let note = super::fit_size(
        font,
        (height * 0.1).clamp(style.small, style.title),
        weekday,
        width * 0.85,
    );
    let notes_h = line(font, note) * notes.len() as f32;
    let room = f32::from(inner.height) - notes_h;
    let number = format!("{}", t.day);
    let size = super::fit_size(font, room * 0.72, &number, width * 0.8);
    let digits = super::cap_height(font, size);
    let gap = (room - digits) / 2.0 * 0.6;
    let total = digits + gap + notes_h;
    let top = f32::from(inner.y) + (f32::from(inner.height) - total) / 2.0;
    super::draw_centred(
        canvas,
        font,
        size,
        cx,
        top + digits / 2.0,
        &number,
        look.ink,
    );
    let mut y = top + digits + gap;
    for (n, text) in notes.iter().enumerate() {
        let h = line(font, note);
        let row = Rect::new(inner.x, y as u16, inner.width, h as u16);
        let color = if n == 0 { look.ink } else { look.faint };
        super::label(canvas, font, note, Align::Center, row, text, color);
        y += h;
    }
}
