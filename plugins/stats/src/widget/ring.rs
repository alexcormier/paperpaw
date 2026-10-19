//! *Daily goal*: today's reading time as a ring that fills towards the
//! daily goal ([`GOAL`](super::GOAL)), the minutes in the middle and the
//! streak below ("5 days in a row"), or what is left to read today.
//!
//! Without a goal the ring stays empty around the minutes read, and below
//! it the widget asks for a goal: choosing it opens the statistics'
//! settings, where the goal is set (it is not measured against a made-up
//! goal, which would tell of a progress nobody asked for).
//!
//! In a place much wider than high the ring sits on the left, the time
//! and the note beside it.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{
    Arg, Canvas, Command, Env, Id, Params, Rect, RegisterError, Registrar, SettingSpec,
};

use super::{caption_room, chosen, fit_size, frame_height, goal, line, middle};
use crate::{fmt, totals};

pub(super) const RING: Id = Id::new_static(NAME);
/// Its id, as text.
const NAME: &str = "stats.widget.goal";
/// A whole ring, or three quarters of one open at the bottom (a gauge).
const STYLES: [&str; 2] = ["stats.ring.full", "stats.ring.gauge"];
/// How thick the ring is.
const LINES: [&str; 3] = ["stats.line.thin", "stats.line.medium", "stats.line.thick"];

/// Its settings, kept in the program: its look, its choices and
/// switches for its parts.
static SETTINGS: [SettingSpec; 8] = [
    widgets::frame_spec("stats.widget.goal.frame", RING, Frame::None),
    widgets::size_spec("stats.widget.goal.size", RING, Size::Fit, true),
    widgets::align_spec("stats.widget.goal.align", RING, Align::Center),
    SettingSpec::fixed_choice(
        "stats.widget.goal.style",
        "stats.setting.ring",
        &STYLES,
        STYLES[0],
        RING,
    ),
    SettingSpec::fixed_choice(
        "stats.widget.goal.line",
        "stats.setting.line",
        &LINES,
        LINES[1],
        RING,
    ),
    widgets::part_spec(
        "stats.widget.goal.caption",
        RING,
        "stats.part.caption",
        true,
    ),
    widgets::part_spec("stats.widget.goal.unit", RING, "stats.part.unit", true),
    widgets::part_spec(
        "stats.widget.goal.note",
        RING,
        "stats.part.streak-left",
        true,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&RING, "stats.widget.goal"))?;
    r.add_settings(&SETTINGS)?;
    r.add_widget(
        WidgetSpec::new(RING, "stats.widget.goal", |_| Box::new(Ring::default()))
            .category(widgets::categories::READING),
    )
}

/// What the ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Numbers {
    today: u64,
    /// Days in a row the goal was reached (any reading without a goal).
    streak: u32,
}

/// Today's goal ring, read from the statistics when updated.
#[derive(Default)]
pub(super) struct Ring {
    numbers: Option<Numbers>,
    read_for: Option<super::Stamp>,
}

impl Widget for Ring {
    fn update(&mut self, env: &Env<'_>) -> bool {
        if !super::stale(env, &mut self.read_for) {
            return false;
        }
        let numbers = super::days(env).map(|(today, days)| {
            let least = goal(env).map_or(1, |g| g.min(u64::from(u32::MAX)) as u32);
            Numbers {
                today: u64::from(super::seconds_on(&days, today)),
                streak: totals::streaks(&days, today, least).current,
            }
        });
        let changed = numbers != self.numbers;
        self.numbers = numbers;
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
        let ring = largest(env, &style, &font).min(width);
        frame_height(env, id) + caption_room(env, id) + ring + note_height(env, &font, &style)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        match self.numbers {
            Some(numbers) => draw(canvas, env, &look, numbers),
            None => widgets::draw_message(canvas, env, &look, env.i18n.tr("stats.no-clock")),
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        if goal(env).is_some() {
            return super::open_statistics(area);
        }
        // Without a goal: where it is set.
        alloc::vec![WidgetTarget {
            area,
            command: Command::OpenSlot(
                slots::SETTINGS,
                Params::new().with(params::ID, crate::GROUP.as_str()),
            ),
        }]
    }
}

/// The ring's largest diameter by the size setting (as large as fits:
/// what a medium one has, for the height it asks for).
fn largest(env: &Env<'_>, style: &UiStyle, font: &paperpaw_sdk::FontHandle<'_>) -> u16 {
    let title = line(font, style.title);
    match widgets::size(env, NAME) {
        Size::Small => title * 3,
        Size::Medium | Size::Fit => title * 9 / 2,
        Size::Large => title * 6,
    }
}

fn note_height(env: &Env<'_>, font: &paperpaw_sdk::FontHandle<'_>, style: &UiStyle) -> u16 {
    if widgets::shows(env, NAME, "note") || goal(env).is_none() {
        line(font, style.small) + style.gap as u16 / 2
    } else {
        0
    }
}

/// The note: the streak, what is left today, or the goal reached; without
/// a goal, the request for one.
fn note(env: &Env<'_>, numbers: Numbers) -> Option<String> {
    let Some(goal) = goal(env) else {
        return Some(env.i18n.tr("stats.widget.goal.set").to_string());
    };
    if !widgets::shows(env, NAME, "note") {
        return None;
    }
    Some(if numbers.streak > 0 {
        env.i18n
            .plural("stats.goal.streak", i64::from(numbers.streak), &[])
    } else if numbers.today >= goal {
        env.i18n.tr("stats.goal.reached").to_string()
    } else {
        let left = fmt::duration(env, (goal - numbers.today).max(60));
        env.i18n
            .format("stats.goal.to-go", &[("time", Arg::Str(&left))])
    })
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, numbers: Numbers) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let title = env.i18n.tr("stats.widget.goal.caption");
    let area = super::below_caption(env, id, look.area);
    let note = note(env, numbers);
    let note_h = if note.is_some() {
        note_height(env, &font, &style)
    } else {
        0
    };
    let limit = if widgets::size(env, id) == Size::Fit {
        u16::MAX
    } else {
        largest(env, &style, &font)
    };
    let align = widgets::align(env, id);
    let progress = goal(env).map(|goal| (numbers.today * 1000 / goal.max(1)).min(1000) as u32);
    let small = line(&font, style.small);
    if area.width >= area.height.saturating_mul(2) && area.height >= small * 2 {
        // Wide: the ring on the left, the time and the note beside it.
        let area = super::captioned(canvas, env, id, look, title, area.height);
        let d = area.height.min(area.width * 2 / 5).min(limit);
        let ring = middle(Rect::new(area.x, area.y, d, area.height), d);
        draw_ring(canvas, env, look, ring, progress, numbers.today, false);
        let gap = style.gap as u16;
        let text = Rect::new(
            area.x + d + gap,
            area.y,
            area.width.saturating_sub(d + gap),
            area.height,
        );
        let time = fmt::duration(env, numbers.today);
        let first = match goal(env) {
            Some(goal) => env.i18n.format(
                "stats.widget.goal.of",
                &[
                    ("time", Arg::Str(&time)),
                    ("goal", Arg::Str(&fmt::duration(env, goal))),
                ],
            ),
            None => time,
        };
        let size = fit_size(&font, &style, &first, text.width, text.height / 2).min(style.title);
        let big = line(&font, size);
        let block = middle(text, big + note_h);
        let first = ui::fit(&font, size, &first, f32::from(text.width));
        let row = Rect::new(text.x, block.y, text.width, big);
        ui::draw_label(
            canvas,
            &font,
            size,
            f32::from(text.x),
            row,
            &first,
            look.ink,
        );
        if let Some(note) = note {
            let note = ui::fit(&font, style.small, &note, f32::from(text.width));
            let row = Rect::new(text.x, block.y + big, text.width, small);
            let x = f32::from(text.x);
            ui::draw_label(canvas, &font, style.small, x, row, &note, look.faint);
        }
        return;
    }
    // The ring with the note below, together in the middle; where the
    // note would leave the ring small, the ring alone.
    let ring_in = |note_h: u16| {
        area.width
            .min(area.height.saturating_sub(note_h))
            .min(limit)
    };
    let (note, note_h) = if note_h > 0 && progress.is_some() && ring_in(note_h) < area.width * 2 / 5
    {
        (None, 0)
    } else {
        (note, note_h)
    };
    let d = ring_in(note_h);
    let block = super::captioned(canvas, env, id, look, title, d + note_h);
    let x = align.x(area, f32::from(d)) as u16;
    let ring = Rect::new(x, block.y, d, d);
    draw_ring(canvas, env, look, ring, progress, numbers.today, true);
    if let Some(note) = note {
        let note = ui::fit(&font, style.small, &note, f32::from(area.width));
        let row = Rect::new(area.x, block.y + d + note_h - small, area.width, small);
        let x = align.x(area, font.measure(style.small, &note));
        ui::draw_label(canvas, &font, style.small, x, row, &note, look.faint);
    }
}

/// Draws the ring into the square `at`: a thin track all round (three
/// quarters for a gauge), over it the share of the goal reached
/// (`progress` in thousandths) with round ends, and the minutes read
/// inside (with "min" below them if `unit` and the unit shows).
fn draw_ring(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    at: Rect,
    progress: Option<u32>,
    seconds: u64,
    unit: bool,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = NAME;
    let d = u32::from(at.width.min(at.height));
    if d < 8 {
        return;
    }
    let center_y = at.y + (d / 2) as u16;
    let thick = match LINES.iter().position(|l| *l == chosen(env, id, "line")) {
        Some(0) => d / 20,
        Some(2) => d / 8,
        _ => d / 12,
    }
    // Small rings thicker, to tell the share from the track.
    .max(3)
    .max(if d < 80 { d / 8 } else { 0 });
    let gauge = chosen(env, id, "style") == STYLES[1];
    super::draw_arc(canvas, env, look, at, progress, thick, gauge);
    let track = style.line.max(1).min(thick);
    // The minutes inside.
    let inner = d.saturating_sub(2 * thick + 2 * track) as u16;
    let minutes = format!("{}", seconds / 60);
    let unit = unit && widgets::shows(env, id, "unit");
    let unit_h = if unit { line(&font, style.small) } else { 0 };
    let room = (u32::from(inner) * 45 / 100) as u16;
    let size = fit_size(&font, &style, "000", inner * 3 / 5, room);
    let big = line(&font, size);
    let unit_h = if unit_h + big <= inner { unit_h } else { 0 };
    let shown = middle(
        Rect::new(at.x, center_y.saturating_sub(inner / 2), at.width, inner),
        big + unit_h,
    );
    let w = font.measure(size, &minutes);
    if w > f32::from(inner) * 0.9 || big > inner {
        // A ring too small for the minutes: the ring alone.
        return;
    }
    let row = Rect::new(at.x, shown.y, at.width, big);
    let cx = f32::from(at.x) + d as f32 / 2.0;
    ui::draw_label(canvas, &font, size, cx - w / 2.0, row, &minutes, look.ink);
    if unit_h > 0 {
        let text = env.i18n.tr("stats.unit.minutes");
        let w = font.measure(style.small, text);
        let row = Rect::new(at.x, shown.y + big, at.width, unit_h);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            cx - w / 2.0,
            row,
            text,
            look.faint,
        );
    }
}
