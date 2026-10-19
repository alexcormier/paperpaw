//! The statistics' widgets for the home screen (or any screen showing
//! widgets), each in the shared look of widgets (its frame, size and
//! alignment) with settings of its own for what it shows:
//!
//! - *Read today* (`today`): today's reading time and pages, the way to
//!   the daily goal ([`GOAL`]) and the week or the streak;
//! - *Reading per day* (`week`): the last days as bars;
//! - *Daily goal* (`ring`): today's reading as a ring towards the goal;
//! - *Reading calendar* (`calendar`): this month, or the last weeks as a
//!   heat map, each day shaded by how long one read;
//! - *Books this year* (`year`): the books read to the end this year,
//!   towards a yearly goal;
//! - *Reading streak* (`streak`): the days read in a row, the longest
//!   streak and the last days as marks.
//!
//! They read the statistics when they are updated (when shown and after
//! events, as `live` says nothing changes by itself), never as they are
//! drawn, and keep only the few numbers they show.

mod calendar;
mod ring;
mod streak;
mod today;
mod week;
mod year;

use alloc::borrow::Cow;
use alloc::vec::Vec;

use quireink_sdk::round::point_at;
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, WidgetTarget};
use quireink_sdk::{
    Canvas, Color, Command, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar,
    SettingSpec, SettingValue,
};

use crate::store::{self, Day};
use crate::{OPEN, fmt};
use quireink_sdk::setting::{Setting, Text};

pub(crate) use today::TODAY;

/// Setting: minutes a day to read, or none.
pub(crate) const GOAL: Setting<Text> = Setting::new("stats.goal");
/// Choices of [`GOAL`]: minutes, `off` for none.
const GOALS: [&str; 12] = [
    "stats.goal.off",
    "stats.goal.10",
    "stats.goal.15",
    "stats.goal.20",
    "stats.goal.30",
    "stats.goal.45",
    "stats.goal.60",
    "stats.goal.90",
    "stats.goal.120",
    "stats.goal.180",
    "stats.goal.240",
    "stats.goal.300",
];

/// The number at the end of a choice's value (`stats.goal.30`: 30).
fn number_of(value: &str) -> Option<u32> {
    value.rsplit('.').next()?.parse().ok()
}

/// The daily goal in seconds, if one is set.
pub(crate) fn goal(env: &Env<'_>) -> Option<u64> {
    number_of(env.settings.text(GOAL.as_str()))
        .filter(|&m| m > 0)
        .map(|m| u64::from(m) * 60)
}

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting(
        SettingSpec::choice(
            &GOAL,
            "stats.setting.goal",
            GOALS.iter().map(|g| Cow::Borrowed(*g)).collect(),
            GOALS[0],
        )
        .group(crate::GROUP)
        .description("stats.setting.goal.help"),
    )?;
    // A goal set shows on the home screen: the widget goes onto its first
    // page, after the book being read, if no page shows it yet.
    r.on(
        quireink_sdk::builtin::events::SETTING_CHANGED,
        |env, changed, out| {
            if changed.id != GOAL || goal(env).is_none() {
                return;
            }
            let pages: [&str; 4] = ["home.page-1", "home.page-2", "home.page-3", "home.page-4"];
            let shown = pages.iter().any(|page| {
                // Also placed in a grid: `stats.today 0/2 4x1`.
                quireink_sdk::status::item_ids(env.settings.text(page))
                    .any(|item| item.split_whitespace().next() == Some(TODAY.as_str()))
            });
            if let Some(SettingValue::Text(list)) = env.settings.get(pages[0])
                && !shown
            {
                let mut ids: Vec<&str> = quireink_sdk::status::item_ids(list).collect();
                let at = ids
                    .iter()
                    .position(|item| item.split_whitespace().next() == Some("home.continue"))
                    .map_or(ids.len(), |i| i + 1);
                let today = TODAY;
                ids.insert(at, today.as_str());
                out.push(Command::SetSetting(
                    Id::new_static("home.page-1"),
                    SettingValue::Text(Cow::Owned(ids.join(", "))),
                ));
            }
        },
    )?;
    today::register(r)?;
    week::register(r)?;
    ring::register(r)?;
    calendar::register(r)?;
    year::register(r)?;
    streak::register(r)
}

/// A widget's settings group with its frame, its size (where it has one,
/// with "as large as fits") and its alignment, in that order.
fn look_settings(
    r: &mut Registrar,
    id: &Id,
    title: &'static str,
    align: Align,
    size: Option<Size>,
) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(id, title))?;
    r.add_setting(widgets::frame_setting(id, Frame::None))?;
    if let Some(size) = size {
        r.add_setting(widgets::size_setting(id, size, true))?;
    }
    r.add_setting(widgets::align_setting(id, align))
}

/// Switches for the parts of widget `id`: each its key, its title and
/// whether it shows at first.
fn parts(
    r: &mut Registrar,
    id: &Id,
    parts: &[(&str, &'static str, bool)],
) -> Result<(), RegisterError> {
    for (key, title, default) in parts {
        r.add_setting(widgets::part_setting(id, key, *title, *default))?;
    }
    Ok(())
}

/// The value of widget `widget`'s choice `key`.
fn chosen<'e>(env: &Env<'e>, widget: &str, key: &str) -> &'e str {
    env.settings.text(&alloc::format!("{widget}.{key}"))
}

/// The height `widget`'s frame takes around its content.
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 1000, 1000);
    1000 - widgets::look::look(env, widget, probe).area.height
}

/// `height` pixels placed in the middle of `area`, top to bottom.
fn middle(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Rect::new(
        area.x,
        area.y + (area.height - height) / 2,
        area.width,
        height,
    )
}

/// The caption's height, if `widget` shows it.
fn caption_room(env: &Env<'_>, widget: &str) -> u16 {
    if widgets::shows(env, widget, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// Draws `text` as `widget`'s caption, if it shows it; the area below.
fn caption(canvas: &mut Canvas<'_>, env: &Env<'_>, widget: &str, look: &Look, text: &str) -> Rect {
    if widgets::shows(env, widget, "caption") {
        widgets::draw_caption(canvas, env, widget, look, text)
    } else {
        look.area
    }
}

/// The room below `widget`'s caption in `area`.
fn below_caption(env: &Env<'_>, widget: &str, area: Rect) -> Rect {
    let cap = caption_room(env, widget).min(area.height);
    Rect::new(area.x, area.y + cap, area.width, area.height - cap)
}

/// Places content `height` high with `widget`'s caption `text` above
/// it, the two together in the middle of `look`'s area, and draws the
/// caption; where the content goes.
fn captioned(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &Look,
    text: &str,
    height: u16,
) -> Rect {
    let block = middle(look.area, caption_room(env, widget) + height);
    let at = Look {
        area: block,
        ..*look
    };
    let below = caption(canvas, env, widget, &at, text);
    Rect::new(below.x, below.y, below.width, height.min(below.height))
}

/// The colour between `look`'s paper and its ink at `level` of 4 (4: the
/// ink), for shades drawn with `fill_shaded` (dithered on panels with few
/// grays): light on light paper, the other way round on a dark card.
fn shade(look: &Look, level: u8) -> Color {
    const LIGHT: [u8; 5] = [255, 205, 160, 95, 0];
    let gray = LIGHT[usize::from(level.min(4))];
    if look.paper.luma() < 128 {
        Color::gray(255 - gray)
    } else {
        Color::gray(gray)
    }
}

/// Whether text on a shade of `level` is drawn in the paper's colour.
fn on_dark(level: u8) -> bool {
    level >= 3
}

/// How dark a day of `seconds` reading is shaded, 0 (none) to 4: with a
/// daily goal by its share (a third, two thirds, nearly, reached), else
/// by minutes (under 15, under 30, under 60, an hour or more).
pub(crate) fn level(seconds: u32, goal: Option<u64>) -> u8 {
    let seconds = u64::from(seconds);
    if seconds == 0 {
        return 0;
    }
    match goal.filter(|&g| g > 0) {
        Some(goal) if seconds >= goal => 4,
        Some(goal) => 1 + (seconds * 3 / goal) as u8,
        None => match seconds / 60 {
            0..=14 => 1,
            15..=29 => 2,
            30..=59 => 3,
            _ => 4,
        },
    }
}

/// The text size widget `widget`'s size setting gives; "as large as
/// fits" fits `text` into `width` and `height` (up to three times a
/// title's size).
fn text_size(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    widget: &str,
    text: &str,
    width: u16,
    height: u16,
) -> f32 {
    let style = UiStyle::new(env);
    let size = widgets::size(env, widget);
    if size != Size::Fit {
        // Smaller where a line of it is higher than the room.
        let line = font.line_metrics(style.title).line_height.max(1.0);
        let by_height = style.title * f32::from(height) / line;
        return size.text(&style).min(by_height.max(style.small));
    }
    fit_size(font, &style, text, width, height)
}

/// The largest size (a title's at least where there is room, three times
/// it at most) at which `text` fits `width` and a line of it `height`.
fn fit_size(font: &FontHandle<'_>, style: &UiStyle, text: &str, width: u16, height: u16) -> f32 {
    let base = style.title;
    let line = font.line_metrics(base).line_height.max(1.0);
    let wide = font.measure(base, text).max(1.0);
    let by_width = base * f32::from(width) / wide;
    let by_height = base * f32::from(height) / line;
    by_width.min(by_height).clamp(style.small, base * 3.0)
}

/// The height of a line of text `size` pixels large.
fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// Today and the days read, if the device knows the time.
fn days(env: &Env<'_>) -> Option<(i32, Vec<Day>)> {
    let today = fmt::today(env)?;
    Some((
        today,
        store::days(env.storage, |unix| fmt::day_of(env, unix)),
    ))
}

/// Seconds read on `day`.
fn seconds_on(days: &[Day], day: i32) -> u32 {
    days.binary_search_by_key(&day, |d| d.day)
        .map_or(0, |i| days[i].seconds)
}

/// The first letter of a weekday's short name (0 being Sunday).
fn initial<'a>(env: &Env<'a>, weekday: u8) -> &'a str {
    let name = fmt::weekday_short(env, weekday);
    name.char_indices()
        .nth(1)
        .map_or(name, |(at, _)| &name[..at])
}

/// A tap on the whole widget opens the statistics.
fn open_statistics(area: Rect) -> Vec<WidgetTarget> {
    alloc::vec![WidgetTarget {
        area,
        command: Command::RunAction(OPEN),
    }]
}

/// Draws a ring into the square `at`, `thick` pixels wide: a thin track
/// all round (three quarters open at the bottom for a `gauge`), over it
/// `progress` (thousandths, if any) with round ends.
fn draw_arc(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    at: Rect,
    progress: Option<u32>,
    thick: u32,
    gauge: bool,
) {
    let style = UiStyle::new(env);
    let d = u32::from(at.width.min(at.height));
    let radius = d / 2;
    if radius < 4 {
        return;
    }
    let thick = thick.clamp(1, radius);
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    let (from, sweep) = if gauge { (225, 270) } else { (0, 360) };
    // The track, thin, in the middle of where the ring is drawn.
    let track = style.line.max(1).min(thick);
    let track_radius = radius - (thick - track) / 2;
    canvas.fill_ring(center, track_radius, track, from, sweep, look.faint);
    if let Some(progress) = progress.filter(|p| *p > 0) {
        let done = (sweep * progress.min(1000) / 1000).max(1);
        canvas.fill_ring(center, radius, thick, from, done, look.ink);
        // Round ends, but for a whole ring.
        if done < 360 {
            let middle = (radius - thick / 2) as i32;
            for deg in [from, from + done as i32] {
                canvas.fill_circle(point_at(center, middle, deg), thick / 2, look.ink);
            }
        }
    }
}

/// Draws a dashed line across from `x` to `end` at `y`.
fn dashed(canvas: &mut Canvas<'_>, x: i32, end: i32, y: i32, thickness: u32, color: Color) {
    let dash = (thickness * 4).max(4) as i32;
    let mut at = x;
    while at < end {
        canvas.fill_rect(at, y, dash.min(end - at) as u32, thickness, color);
        at += dash * 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every widget's settings have ids of their own: the plugin loads.
    #[test]
    fn the_widgets_register() {
        use quireink_sdk::contribution::Contribution;
        let mut r = Registrar::new(crate::PLUGIN);
        assert!(register(&mut r).is_ok());
        let mut ids: Vec<&str> = r
            .contributions()
            .iter()
            .filter_map(|c| match c {
                Contribution::Setting(spec) => Some(spec.id.as_str()),
                _ => None,
            })
            .collect();
        let all = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all);
        assert!(all > 30);
    }

    #[test]
    fn goals_are_minutes_or_off() {
        let minutes: Vec<Option<u32>> = GOALS.iter().map(|g| number_of(g)).collect();
        assert_eq!(minutes[0], None);
        assert!(minutes[1..].iter().all(Option::is_some));
    }

    #[test]
    fn days_are_shaded_by_time_or_the_goal() {
        // By minutes.
        let by_minutes: Vec<u8> = [0, 30, 14 * 60, 15 * 60, 45 * 60, 60 * 60, 5 * 3600]
            .into_iter()
            .map(|s| level(s, None))
            .collect();
        assert_eq!(by_minutes, [0, 1, 1, 2, 3, 4, 4]);
        // By the goal's share: 30 minutes.
        let goal = Some(1800);
        let by_goal: Vec<u8> = [0, 60, 600, 1200, 1799, 1800, 9000]
            .into_iter()
            .map(|s| level(s, goal))
            .collect();
        assert_eq!(by_goal, [0, 1, 2, 3, 3, 4, 4]);
    }

    #[test]
    fn shades_turn_round_on_a_dark_card() {
        let light = Look {
            area: Rect::new(0, 0, 10, 10),
            ink: Color::BLACK,
            faint: Color::gray(90),
            paper: Color::WHITE,
            frame: Frame::None,
        };
        let dark = Look {
            ink: Color::WHITE,
            paper: Color::BLACK,
            frame: Frame::Dark,
            ..light
        };
        assert_eq!(shade(&light, 4), Color::BLACK);
        assert_eq!(shade(&dark, 4), Color::WHITE);
        assert!(shade(&light, 1).luma() > shade(&light, 3).luma());
        assert!(shade(&dark, 1).luma() < shade(&dark, 3).luma());
    }
}
