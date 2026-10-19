//! *Reading calendar*: the days of this month, or the last weeks as a
//! heat map (a column a week, a row a weekday), each day shaded by how
//! long one read ([`level`](super::level): with a daily goal by its
//! share, else by minutes), today marked; below, the streak and the time
//! read in the days shown.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{
    Arg, Canvas, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar, SettingSpec,
};

use super::{caption_room, chosen, frame_height, goal, line, shade};
use crate::{fmt, totals};

pub(super) const CALENDAR: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.calendar";
/// This month as a calendar, or the last weeks as a heat map.
const STYLES: [&str; 2] = ["stats.calendar.month", "stats.calendar.weeks"];
/// The weeks of the heat map (fewer where they do not fit).
const WEEKS: [&str; 4] = [
    "stats.weeks.8",
    "stats.weeks.12",
    "stats.weeks.16",
    "stats.weeks.26",
];
/// The days' shape.
const CELLS: [&str; 2] = ["stats.cells.square", "stats.cells.round"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 10] = [
    widgets::frame_spec("stats.widget.calendar.frame", CALENDAR, Frame::None),
    widgets::align_spec("stats.widget.calendar.align", CALENDAR, Align::Center),
    SettingSpec::fixed_choice(
        "stats.widget.calendar.style",
        "stats.setting.calendar",
        &STYLES,
        STYLES[0],
        CALENDAR,
    ),
    SettingSpec::fixed_choice(
        "stats.widget.calendar.weeks",
        "stats.setting.weeks",
        &WEEKS,
        WEEKS[1],
        CALENDAR,
    ),
    SettingSpec::fixed_choice(
        "stats.widget.calendar.cells",
        "stats.setting.cells",
        &CELLS,
        CELLS[0],
        CALENDAR,
    ),
    widgets::part_spec(
        "stats.widget.calendar.caption",
        CALENDAR,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec(
        "stats.widget.calendar.weekdays",
        CALENDAR,
        "stats.part.weekdays",
        true,
    ),
    widgets::part_spec(
        "stats.widget.calendar.numbers",
        CALENDAR,
        "stats.part.numbers",
        true,
    ),
    widgets::part_spec(
        "stats.widget.calendar.streak",
        CALENDAR,
        "stats.part.streak",
        true,
    ),
    widgets::part_spec(
        "stats.widget.calendar.total",
        CALENDAR,
        "stats.part.total",
        true,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&CALENDAR, "stats.widget.calendar"))?;
    r.add_settings(&SETTINGS)?;
    r.add_widget(
        WidgetSpec::new(CALENDAR, "stats.widget.calendar", |_| {
            Box::new(Calendar::default())
        })
        .category(widgets::categories::READING),
    )
}

/// What the calendar shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    /// The first day shown: the start of a week.
    first: i32,
    today: i32,
    /// The heat map's weeks; 0 for the month.
    weeks: u8,
    /// Each day's shade from `first` on, to the month's end or today.
    levels: Vec<u8>,
    /// Seconds read in the days shown.
    total: u64,
    streak: u32,
}

/// The start of the week `day` is in.
fn week_start(env: &Env<'_>, day: i32) -> i32 {
    let weekday = i32::from(fmt::date_of(day).weekday);
    day - (weekday - i32::from(fmt::week_start(env))).rem_euclid(7)
}

/// The first day of the month of `day`.
fn month_start(day: i32) -> i32 {
    day - i32::from(fmt::date_of(day).day) + 1
}

/// The last day of the month of `day`.
fn month_end(day: i32) -> i32 {
    let next = month_start(month_start(day) + 31);
    next - 1
}

/// The heat map's weeks, by its setting.
fn weeks_set(env: &Env<'_>) -> u8 {
    super::number_of(chosen(env, NAME, "weeks")).map_or(12, |w| w.clamp(1, 53) as u8)
}

/// Each day's shade from `first` to `last`, the seconds read then.
pub(super) fn levels(
    days: &[crate::store::Day],
    first: i32,
    last: i32,
    goal: Option<u64>,
) -> (Vec<u8>, u64) {
    let mut total = 0;
    let levels = (first..=last)
        .map(|day| {
            let seconds = super::seconds_on(days, day);
            total += u64::from(seconds);
            super::level(seconds, goal)
        })
        .collect();
    (levels, total)
}

fn shown(env: &Env<'_>) -> Option<Shown> {
    let (today, days) = super::days(env)?;
    let heat = chosen(env, NAME, "style") == STYLES[1];
    let (first, last, weeks) = if heat {
        let weeks = weeks_set(env);
        let first = week_start(env, today) - 7 * (i32::from(weeks) - 1);
        (first, today, weeks)
    } else {
        let start = month_start(today);
        (week_start(env, start), month_end(today), 0)
    };
    let (levels, _) = levels(&days, first, last, goal(env));
    // The total of the month itself, or of the weeks shown.
    let counted = if heat { first } else { month_start(today) };
    let total = (counted..=today.min(last))
        .map(|day| u64::from(super::seconds_on(&days, day)))
        .sum();
    Some(Shown {
        first,
        today,
        weeks,
        levels,
        total,
        streak: totals::streak(&days, today),
    })
}

/// The calendar, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Calendar {
    shown: Option<Shown>,
    read_for: Option<super::Stamp>,
}

impl Widget for Calendar {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let shown = shown(env);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn live(&self) -> bool {
        // Asked each minute for a new day; read again only then, or
        // when there is more reading.
        true
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = NAME;
        let inner = width.saturating_sub(frame_height(env, id));
        let rows = match &self.shown {
            Some(s) if s.weeks == 0 => s.levels.len().div_ceil(7) as u16,
            _ => 7,
        };
        let header = if self.shown.as_ref().is_some_and(|s| s.weeks == 0) {
            weekdays_height(env, &font, &style)
        } else {
            0
        };
        // Square days across the width, at most twice a line high.
        let cell = (inner / 7).min(line(&font, style.small) * 2);
        let cell = match &self.shown {
            Some(s) if s.weeks > 0 => (inner / u16::from(s.weeks + 1)).min(cell),
            _ => cell,
        };
        frame_height(env, id)
            + caption_room(env, id)
            + header
            + rows * cell
            + footer_height(env, &font, &style)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        let Some(shown) = &self.shown else {
            widgets::draw_message(canvas, env, &look, env.i18n.tr("stats.no-clock"));
            return;
        };
        if shown.weeks == 0 {
            draw_month(canvas, env, &look, shown);
        } else {
            draw_weeks(canvas, env, &look, shown);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        super::open_statistics(area)
    }
}

fn weekdays_height(env: &Env<'_>, font: &FontHandle<'_>, style: &UiStyle) -> u16 {
    if widgets::shows(env, NAME, "weekdays") {
        line(font, style.small)
    } else {
        0
    }
}

fn footer_height(env: &Env<'_>, font: &FontHandle<'_>, style: &UiStyle) -> u16 {
    let id = NAME;
    if widgets::shows(env, id, "streak") || widgets::shows(env, id, "total") {
        line(font, style.small) + style.gap as u16 / 2
    } else {
        0
    }
}

/// "5 days in a row · 6 h 20 min in October", as far as these show.
fn footer(env: &Env<'_>, shown: &Shown) -> String {
    let id = NAME;
    let mut parts: Vec<String> = Vec::new();
    if widgets::shows(env, id, "streak") && shown.streak > 0 {
        parts.push(
            env.i18n
                .plural("stats.goal.streak", i64::from(shown.streak), &[]),
        );
    }
    if widgets::shows(env, id, "total") {
        let time = fmt::duration(env, shown.total);
        parts.push(if shown.weeks == 0 {
            let month = fmt::month(env, fmt::date_of(shown.today).month);
            env.i18n.format(
                "stats.widget.calendar.month-total",
                &[("time", Arg::Str(&time)), ("month", Arg::Str(month))],
            )
        } else {
            env.i18n.format(
                "stats.widget.calendar.weeks-total",
                &[
                    ("time", Arg::Str(&time)),
                    ("count", Arg::Int(i64::from(shown.weeks))),
                ],
            )
        });
    }
    parts.join(" · ")
}

/// Draws the footer at the bottom of `block`.
fn draw_footer(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, block: Rect, shown: &Shown) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let text = footer(env, shown);
    if text.is_empty() {
        return;
    }
    let small = line(&font, style.small);
    let text = ui::fit(&font, style.small, &text, f32::from(block.width));
    let x = widgets::align(env, NAME).x(block, font.measure(style.small, &text));
    let row = Rect::new(block.x, block.y + block.height - small, block.width, small);
    ui::draw_label(canvas, &font, style.small, x, row, &text, look.faint);
}

/// A day's cell: its shade, today's ring, its number (or a dot where
/// there is no number and nothing was read).
#[allow(clippy::too_many_arguments)]
fn draw_day(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    cell: Rect,
    level: u8,
    number: Option<u8>,
    today: bool,
    to_come: bool,
) {
    let style = UiStyle::new(env);
    let round = chosen(env, NAME, "cells") == CELLS[1];
    let pad = (cell.width / 12).max(1);
    let inner = Rect::new(
        cell.x + pad,
        cell.y + pad,
        cell.width.saturating_sub(2 * pad),
        cell.height.saturating_sub(2 * pad),
    );
    let radius = |r: Rect| {
        u32::from(if round {
            r.width.min(r.height) / 2
        } else {
            r.width.min(r.height) / 5
        })
    };
    // Numbers do not read on darker dithered shades: where the panel has
    // few grays, days with numbers get the lightest shade and a bar as
    // long as their level instead.
    let few = env
        .device
        .display
        .format
        .gray_levels()
        .is_some_and(|g| g <= 4);
    let barred = number.is_some() && few && (1..4).contains(&level);
    if level >= 4 {
        canvas.fill_round(inner, radius(inner), look.ink);
    } else if barred {
        canvas.fill_round_shaded(inner, radius(inner), shade(look, 1));
        let thick = (u32::from(inner.height) / 8).max(2);
        let full = u32::from(inner.width) * 3 / 5;
        let len = (full * u32::from(level) / 3).max(thick);
        let bar = Rect::new(
            inner.x + (inner.width - len as u16) / 2,
            inner.y + inner.height - (thick * 2) as u16,
            len as u16,
            thick as u16,
        );
        canvas.fill_round(bar, thick / 2, look.ink);
    } else if level > 0 {
        canvas.fill_round_shaded(inner, radius(inner), shade(look, level));
    }
    if today {
        let thick = (style.line * 2).clamp(2, u32::from(pad.max(1)) * 2);
        canvas.stroke_round(cell, thick, radius(cell), look.ink);
    }
    match number {
        Some(day) => {
            let Some(font) = style.font(env) else {
                return;
            };
            let text = format!("{day}");
            let size = style.small.min(f32::from(inner.height) * 0.6);
            let w = font.measure(size, &text);
            let color = if super::on_dark(level) && !barred {
                look.paper
            } else if level == 0 && !today {
                look.faint
            } else {
                look.ink
            };
            let x = f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
            // Above the bar, if there is one.
            let lift = if barred { inner.height / 8 } else { 0 };
            let at = Rect::new(cell.x, cell.y, cell.width, cell.height - lift);
            ui::draw_label(canvas, &font, size, x, at, &text, color);
        }
        None if level == 0 && !to_come && !today => {
            let center = Point::new(
                i32::from(cell.x + cell.width / 2),
                i32::from(cell.y + cell.height / 2),
            );
            let r = u32::from(cell.width / 10).max(1);
            canvas.fill_circle(center, r, look.faint);
        }
        None => {}
    }
}

fn draw_month(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, shown: &Shown) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let date = fmt::date_of(shown.today);
    let title = format!("{} {}", fmt::month(env, date.month), date.year);
    let area = super::below_caption(env, id, look.area);
    let mut header = weekdays_height(env, &font, &style);
    let mut footer = footer_height(env, &font, &style);
    let rows = shown.levels.len().div_ceil(7) as u16;
    let cell = |header: u16, footer: u16| {
        let room = area.height.saturating_sub(header + footer);
        (area.width / 7).min(room / rows.max(1))
    };
    // Where the days would be small, the footer goes, then the weekdays.
    let small = line(&font, style.small);
    if cell(header, footer) < small {
        footer = 0;
    }
    if cell(header, footer) < small * 3 / 4 {
        header = 0;
    }
    let cell = cell(header, footer);
    if cell < 4 {
        return;
    }
    let block = super::captioned(canvas, env, id, look, &title, header + cell * rows + footer);
    let align = widgets::align(env, id);
    let left = align.x(area, f32::from(cell * 7)) as u16;
    if header > 0 {
        let from = fmt::week_start(env);
        let two = (0..7).all(|w| {
            font.measure(style.small, fmt::weekday_short(env, w)) * 1.3 <= f32::from(cell)
        });
        for col in 0..7u8 {
            let weekday = (from + col) % 7;
            let name = if two {
                fmt::weekday_short(env, weekday)
            } else {
                super::initial(env, weekday)
            };
            let x = left + cell * u16::from(col);
            let w = font.measure(style.small, name);
            let row = Rect::new(x, block.y, cell, header);
            let at = f32::from(x) + (f32::from(cell) - w) / 2.0;
            ui::draw_label(canvas, &font, style.small, at, row, name, look.faint);
        }
    }
    let month = date.month;
    let numbers = widgets::shows(env, id, "numbers") && cell >= line(&font, style.small) * 11 / 10;
    for (i, level) in shown.levels.iter().enumerate() {
        let day = shown.first + i as i32;
        let d = fmt::date_of(day);
        if d.month != month {
            continue;
        }
        let (row, col) = (i as u16 / 7, i as u16 % 7);
        let at = Rect::new(left + col * cell, block.y + header + row * cell, cell, cell);
        let number = numbers.then_some(d.day);
        let to_come = day > shown.today;
        draw_day(
            canvas,
            env,
            look,
            at,
            *level,
            number,
            day == shown.today,
            to_come,
        );
    }
    if footer > 0 {
        draw_footer(canvas, env, look, block, shown);
    }
}

fn draw_weeks(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, shown: &Shown) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let title = env.i18n.format(
        "stats.widget.calendar.weeks",
        &[("count", Arg::Int(i64::from(shown.weeks)))],
    );
    let area = super::below_caption(env, id, look.area);
    let mut footer = footer_height(env, &font, &style);
    // Where the days would be small, the footer goes.
    if area.height.saturating_sub(footer) / 7 < line(&font, style.small) / 2 {
        footer = 0;
    }
    let from = fmt::week_start(env);
    let labels = widgets::shows(env, id, "weekdays");
    let label_w = if labels {
        (0..7)
            .map(|w| font.measure(style.small, super::initial(env, w)))
            .fold(0.0, f32::max) as u16
            + style.gap as u16 / 2
    } else {
        0
    };
    let room = area.height.saturating_sub(footer);
    // As many weeks as fit days at least 1.5 mm wide, the newest.
    let least = env.metrics.tenth_mm(15).max(4) as u16;
    let across = area.width.saturating_sub(label_w);
    let weeks = u16::from(shown.weeks).min(across / least).max(1);
    let cell = (across / weeks).min(room / 7);
    if cell < 3 {
        return;
    }
    let width = label_w + cell * weeks;
    let block = super::captioned(canvas, env, id, look, &title, cell * 7 + footer);
    let left = widgets::align(env, id).x(area, f32::from(width)) as u16;
    let skipped = (u16::from(shown.weeks) - weeks) * 7;
    if labels {
        // Every other weekday where the rows are low.
        let every = if cell >= line(&font, style.small) {
            1
        } else {
            2
        };
        for row in (0..7u8).step_by(every) {
            let name = super::initial(env, (from + row) % 7);
            let r = Rect::new(left, block.y + cell * u16::from(row), label_w, cell);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(left),
                r,
                name,
                look.faint,
            );
        }
    }
    for (i, level) in shown.levels.iter().enumerate().skip(usize::from(skipped)) {
        let day = shown.first + i as i32;
        let n = i as u16 - skipped;
        let (col, row) = (n / 7, n % 7);
        let at = Rect::new(
            left + label_w + col * cell,
            block.y + row * cell,
            cell,
            cell,
        );
        draw_day(
            canvas,
            env,
            look,
            at,
            *level,
            None,
            day == shown.today,
            false,
        );
    }
    if footer > 0 {
        draw_footer(canvas, env, look, block, shown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Day;

    /// The local day of a date.
    fn day(year: i32, month: u8, d: u8) -> i32 {
        (19_000..22_000)
            .find(|&n| {
                let t = fmt::date_of(n);
                (t.year, t.month, t.day) == (year, month, d)
            })
            .unwrap_or(0)
    }

    #[test]
    fn months_have_their_ends() {
        assert_eq!(month_start(day(2026, 10, 14)), day(2026, 10, 1));
        assert_eq!(month_end(day(2026, 10, 14)), day(2026, 10, 31));
        assert_eq!(month_end(day(2028, 2, 3)), day(2028, 2, 29));
    }

    #[test]
    fn days_are_shaded_in_order() {
        let days: Vec<Day> = [(10, 600), (12, 4000)]
            .into_iter()
            .map(|(day, seconds)| Day {
                day,
                seconds,
                stays: 1,
            })
            .collect();
        let (levels, total) = levels(&days, 9, 13, None);
        assert_eq!(levels, [0, 1, 0, 4, 0]);
        assert_eq!(total, 4600);
        // With a goal of an hour, ten minutes are a sixth of it.
        let (levels, _) = super::levels(&days, 10, 12, Some(3600));
        assert_eq!(levels, [1, 0, 4]);
    }
}
