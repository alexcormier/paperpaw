//! *Reading streak*: the days read in a row (any reading, or the daily
//! goal reached), the longest streak so far, and the last 7 or 14 days
//! as marks: filled (with a check, if chosen) for the days that count,
//! an outline for the others, today's darker while it does not count yet.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Point, Rect, RegisterError, Registrar, SettingSpec};

use super::{caption_room, chosen, frame_height, goal, line, middle};
use crate::fmt;
use crate::totals::{self, Streaks};

pub(super) const STREAK: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.streak";
/// Which days count: any reading, or the daily goal reached.
const COUNTS: [&str; 2] = ["stats.streak.any", "stats.streak.goal"];
/// How the days show: dots, or dots with a check.
const MARKS: [&str; 2] = ["stats.marks.dots", "stats.marks.checks"];
/// How many days show.
const DAYS: [&str; 2] = ["stats.marks.7", "stats.marks.14"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 10] = [
    widgets::frame_spec("stats.widget.streak.frame", STREAK, Frame::None),
    widgets::size_spec("stats.widget.streak.size", STREAK, Size::Medium, true),
    widgets::align_spec("stats.widget.streak.align", STREAK, Align::Center),
    SettingSpec::fixed_choice(
        "stats.widget.streak.counts",
        "stats.setting.counts",
        &COUNTS,
        COUNTS[0],
        STREAK,
    )
    .described("stats.setting.counts.help"),
    SettingSpec::fixed_choice(
        "stats.widget.streak.style",
        "stats.setting.marks",
        &MARKS,
        MARKS[1],
        STREAK,
    )
    .described("stats.setting.marks.help"),
    SettingSpec::fixed_choice(
        "stats.widget.streak.days",
        "stats.setting.days",
        &DAYS,
        DAYS[0],
        STREAK,
    )
    .described("stats.setting.days.help"),
    widgets::part_spec(
        "stats.widget.streak.caption",
        STREAK,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec(
        "stats.widget.streak.longest",
        STREAK,
        "stats.part.longest",
        true,
    ),
    widgets::part_spec(
        "stats.widget.streak.marks",
        STREAK,
        "stats.part.marks",
        true,
    )
    .described("stats.part.marks.help"),
    widgets::part_spec(
        "stats.widget.streak.weekdays",
        STREAK,
        "stats.part.weekdays",
        true,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&STREAK, "stats.widget.streak"))?;
    r.add_settings(&SETTINGS)?;
    r.add_widget(
        WidgetSpec::new(STREAK, "stats.widget.streak", |_| {
            Box::new(Streak::default())
        })
        .category(widgets::categories::READING),
    )
}

/// A day of the marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    weekday: u8,
    counts: bool,
    today: bool,
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    streaks: Streaks,
    marks: Vec<Mark>,
}

/// The least seconds of a day that counts: the goal, if chosen and set.
fn least(env: &Env<'_>) -> u32 {
    if chosen(env, NAME, "counts") != COUNTS[1] {
        return 1;
    }
    goal(env).map_or(1, |g| g.min(u64::from(u32::MAX)) as u32)
}

fn shown(env: &Env<'_>) -> Option<Shown> {
    let (today, days) = super::days(env)?;
    let least = least(env);
    let count = if chosen(env, NAME, "days") == DAYS[1] {
        14
    } else {
        7
    };
    let marks = (today - count + 1..=today)
        .map(|day| Mark {
            weekday: fmt::date_of(day).weekday,
            counts: fmt::seconds_on(&days, day) >= least,
            today: day == today,
        })
        .collect();
    Some(Shown {
        streaks: totals::streaks(&days, today, least),
        marks,
    })
}

/// The streak, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Streak {
    shown: Option<Shown>,
    read_for: Option<super::Stamp>,
}

impl Widget for Streak {
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

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = NAME;
        let size = widgets::size(env, id).text(&style);
        let small = line(&font, style.small);
        frame_height(env, id)
            + caption_room(env, id)
            + line(&font, size)
            + small
            + marks_height(env, small, 0, widgets::shows(env, id, "weekdays"))
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match &self.shown {
            Some(shown) => draw(canvas, env, &look, shown),
            None => widgets::draw_message(canvas, env, &look, env.i18n.tr("stats.no-clock")),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        super::open_statistics(area)
    }
}

/// The marks' place across `width` (as large as they may be for 0): how
/// wide each one's column is and how large the mark.
fn mark_size(env: &Env<'_>, width: u16, count: usize) -> (u32, u32) {
    let most = env.metrics.mm(4);
    // A mark and a half for each, at most: together, not spread out.
    let column = match width {
        0 => most * 3 / 2,
        w => (u32::from(w) / count.max(1) as u32).min(most * 3 / 2),
    };
    (column, (column * 3 / 4).min(most))
}

/// The height of the marks across `width`, with the room above them and
/// their weekdays below if `weekdays`; 0 where they do not show.
fn marks_height(env: &Env<'_>, small: u16, width: u16, weekdays: bool) -> u16 {
    if !widgets::shows(env, NAME, "marks") {
        return 0;
    }
    let style = UiStyle::new(env);
    let count = if chosen(env, NAME, "days") == DAYS[1] {
        14
    } else {
        7
    };
    let (_, d) = mark_size(env, width, count);
    d as u16 + style.gap as u16 / 2 + if weekdays { small } else { 0 }
}

/// "5 days", and below it "in a row, longest 12" (or how to begin one).
fn texts(env: &Env<'_>, streaks: Streaks) -> (String, String) {
    let first = env
        .i18n
        .plural("stats.days", i64::from(streaks.current), &[]);
    let second = if streaks.current == 0 {
        String::from(env.i18n.tr("stats.widget.streak.start"))
    } else if widgets::shows(env, NAME, "longest") {
        env.i18n.format(
            "stats.widget.streak.longest",
            &[("count", Arg::Int(i64::from(streaks.longest)))],
        )
    } else {
        String::from(env.i18n.tr("stats.in-a-row"))
    };
    (first, second)
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, shown: &Shown) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let title = env.i18n.tr("stats.widget.streak.caption");
    let area = super::below_caption(env, id, look.area);
    let small = line(&font, style.small);
    let (first, second) = texts(env, shown.streaks);
    let gap = style.gap as u16 / 2;
    let weekdays = widgets::shows(env, id, "weekdays");
    let headline = match widgets::size(env, id) {
        Size::Fit => style.text,
        size => size.text(&style),
    };
    let least = line(&font, headline) + small;
    let roomy = u32::from(area.width) * 3 / 5 >= env.metrics.mm(3) * shown.marks.len() as u32;
    if area.width >= area.height * 3 && roomy {
        // Wide and low: the marks beside the text, if there is room.
        let side = area.width * 2 / 5;
        let rest = area.width - side;
        let fits = |weekdays| {
            let h = marks_height(env, small, rest, weekdays);
            (h > 0 && h - gap <= area.height).then_some((h - gap, weekdays))
        };
        if let Some((marks, weekdays)) = fits(weekdays).or_else(|| fits(false)) {
            let area = super::captioned(canvas, env, id, look, title, area.height);
            let text = Rect::new(area.x, area.y, side, area.height);
            draw_texts(canvas, env, look, text, (&first, &second), Align::Left);
            let at = middle(Rect::new(area.x + side, area.y, rest, area.height), marks);
            draw_marks(
                canvas,
                env,
                look,
                at,
                &shown.marks,
                (Align::Right, weekdays),
            );
            return;
        }
    }
    // The marks below where there is room for them besides the text,
    // without the weekdays if need be.
    let fits = |weekdays| {
        let h = marks_height(env, small, area.width, weekdays);
        (h > 0 && h + least <= area.height).then_some((h, weekdays))
    };
    let (marks, weekdays) = fits(weekdays).or_else(|| fits(false)).unwrap_or((0, false));
    let align = widgets::align(env, id);
    let room = area.height.saturating_sub(marks + small);
    let size = super::text_size(env, &font, id, &first, area.width, room);
    // The text and the marks below it, together in the middle.
    let texts = line(&font, size) + small;
    let block = super::captioned(canvas, env, id, look, title, texts + marks);
    let at = Rect::new(area.x, block.y, area.width, texts);
    draw_texts(canvas, env, look, at, (&first, &second), align);
    if marks > 0 {
        let at = Rect::new(area.x, block.y + texts + gap, area.width, marks - gap);
        draw_marks(canvas, env, look, at, &shown.marks, (align, weekdays));
    }
}

/// Draws the streak and the note below it in the middle of `area`, as
/// `align` places them, the streak as large as the size says (or fits).
fn draw_texts(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    area: Rect,
    (first, second): (&str, &str),
    align: Align,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let small = line(&font, style.small);
    let room = area.height.saturating_sub(small);
    let size = super::text_size(env, &font, NAME, first, area.width, room);
    let big = line(&font, size);
    let block = middle(area, big + small);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    let first = super::number_if_narrow(&font, size, first, area.width);
    let first = ui::fit(&font, size, first, f32::from(area.width));
    let x = align.x(area, font.measure(size, &first));
    ui::draw_label(canvas, &font, size, x, row(block.y, big), &first, look.ink);
    // The longest streak left out where the note is too long.
    let width = f32::from(area.width);
    let in_a_row = env.i18n.tr("stats.in-a-row");
    let second = if font.measure(style.small, second) > width && second.starts_with(in_a_row) {
        in_a_row
    } else {
        second
    };
    let second = ui::fit(&font, style.small, second, width);
    let x = align.x(area, font.measure(style.small, &second));
    let y = block.y + big;
    ui::draw_label(
        canvas,
        &font,
        style.small,
        x,
        row(y, small),
        &second,
        look.faint,
    );
}

/// A mark a day in `area`, placed as `align` says, the weekdays below if
/// `weekdays`.
fn draw_marks(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    area: Rect,
    marks: &[Mark],
    (align, weekdays): (Align, bool),
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let small = line(&font, style.small);
    let n = marks.len().max(1) as u32;
    let (column, d) = mark_size(env, area.width, marks.len());
    // The weekdays where their letters have room.
    let weekdays = weekdays
        && (0..7).all(|w| font.measure(style.small, super::initial(env, w)) * 1.3 <= column as f32);
    let labels = if weekdays { small } else { 0 };
    let d = d.min(u32::from(area.height.saturating_sub(labels)));
    if d < 4 {
        return;
    }
    let checks = chosen(env, id, "style") == MARKS[1];
    let thin = style.line.max(1);
    // Flush with the text: the outer marks' edges on its margins.
    let inset = (column - d) / 2;
    let span = (column * n - 2 * inset) as f32;
    let left = align.x(area, span) as i32 - inset as i32;
    let radius = d / 2;
    for (i, mark) in marks.iter().enumerate() {
        let cx = left + (column * i as u32 + column / 2) as i32;
        let center = Point::new(cx, i32::from(area.y) + radius as i32);
        if mark.counts {
            canvas.fill_circle(center, radius, look.ink);
            if checks && d >= 10 {
                draw_check(canvas, center, radius, look);
            }
        } else if mark.today {
            canvas.stroke_circle(center, radius, (thin * 2).min(radius / 3).max(1), look.ink);
        } else {
            canvas.stroke_circle(center, radius, thin, look.faint);
        }
        if weekdays {
            let name = super::initial(env, mark.weekday);
            let w = font.measure(style.small, name);
            let row = Rect::new(area.x, area.y + d as u16, area.width, labels);
            let color = if mark.today { look.ink } else { look.faint };
            ui::draw_label(
                canvas,
                &font,
                style.small,
                cx as f32 - w / 2.0,
                row,
                name,
                color,
            );
        }
    }
}

/// A check in the paper's colour on a filled mark.
fn draw_check(canvas: &mut Canvas<'_>, center: Point, radius: u32, look: &Look) {
    let r = radius as i32;
    let at = |x: i32, y: i32| Point::new(center.x + x * r / 100, center.y + y * r / 100);
    let width = (radius / 4).max(1);
    let (a, b, c) = (at(-42, 2), at(-12, 32), at(44, -30));
    canvas.draw_line(a, b, width, look.paper);
    canvas.draw_line(b, c, width, look.paper);
}
