//! The calendar: this month as a grid of weeks, the month's name above
//! it, the weekdays' initials, today marked; the weeks' numbers and the
//! next month as well, if wished.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::time;
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame};
use paperpaw_sdk::{Canvas, Color, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::dates::MonthGrid;
use super::{chosen_name, frame_height, ui};

const ID: Id = Id::new_static("widgets.calendar");

/// The values of the week start setting: as set for the device, Monday,
/// Sunday.
pub(super) const WEEK_STARTS: [&str; 3] = [
    "widgets.week-start.auto",
    "widgets.week-start.monday",
    "widgets.week-start.sunday",
];
/// How today is marked: a dark circle, a dark square, a ring, a line
/// below.
const MARKS: [&str; 4] = [
    "widgets.mark.circle",
    "widgets.mark.square",
    "widgets.mark.ring",
    "widgets.mark.underline",
];
/// The weekdays' names above the columns: one letter, two.
const INITIALS: [&str; 2] = ["widgets.initials.one", "widgets.initials.two"];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 12] = [
    widgets::align_spec("widgets.calendar.align", ID, Align::Center),
    widgets::frame_spec("widgets.calendar.frame", ID, Frame::None),
    SettingSpec::fixed_choice(
        "widgets.calendar.week-start",
        "widgets.setting.week-start",
        &WEEK_STARTS,
        WEEK_STARTS[0],
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.calendar.mark",
        "widgets.setting.today-mark",
        &MARKS,
        MARKS[0],
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.calendar.initials",
        "widgets.setting.initials",
        &INITIALS,
        INITIALS[0],
        ID,
    ),
    widgets::part_spec(
        "widgets.calendar.heading",
        ID,
        "widgets.setting.month-name",
        true,
    ),
    widgets::part_spec("widgets.calendar.year", ID, "widgets.setting.year", false),
    widgets::part_spec(
        "widgets.calendar.weekdays",
        ID,
        "widgets.setting.weekdays",
        true,
    ),
    widgets::part_spec(
        "widgets.calendar.weeks",
        ID,
        "widgets.setting.week-numbers",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.others",
        ID,
        "widgets.setting.other-days",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.weekends",
        ID,
        "widgets.setting.weekends-faint",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.next",
        ID,
        "widgets.setting.next-month",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        &ID,
        "widgets.calendar",
        Drawn::new(height, draw).shows(super::today_shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// The first day of the week, 0 (Sunday) or 1 (Monday), as `widget`'s
/// setting `week-start` says, or as set for the device.
pub(super) fn week_start(env: &Env<'_>, widget: &str) -> u8 {
    match chosen_name(env, widget, "week-start") {
        "sunday" => 0,
        "monday" => 1,
        _ => paperpaw_sdk::time::week_start(env),
    }
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let (style, _) = ui(env);
    let months = if widgets::shows(env, ID.as_str(), "next") {
        2
    } else {
        1
    };
    (width * 6 / 7).min((style.row * 6) as u16) * months + frame_height(env, ID.as_str())
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let Some(today) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let area = look.area;
    let month = MonthGrid::new(today.year, today.month, week_start(env, ID.as_str()));
    // Two months: side by side where the place is wide, else one below
    // the other; this one alone where two have no room.
    let gap = style.gap as u16 * 2;
    let wide = area.width > area.height * 5 / 4;
    let least = (super::line(&font, style.small) * 4.0) as u16;
    let room = if wide { area.width } else { area.height };
    if !widgets::shows(env, ID.as_str(), "next") || super::fitting(room, gap, least, 2) < 2 {
        draw_month(
            canvas,
            env,
            &font,
            &look,
            area,
            &month,
            Some(today.day),
            month.weeks,
        );
        return;
    }
    let (first, second) = if wide {
        let w = super::share(area.width, gap, 2);
        (
            Rect::new(area.x, area.y, w, area.height),
            Rect::new(area.x + w + gap, area.y, w, area.height),
        )
    } else {
        let h = super::share(area.height, gap, 2);
        (
            Rect::new(area.x, area.y, area.width, h),
            Rect::new(area.x, area.y + h + gap, area.width, h),
        )
    };
    let weeks = month.weeks.max(month.next().weeks);
    draw_month(
        canvas,
        env,
        &font,
        &look,
        first,
        &month,
        Some(today.day),
        weeks,
    );
    draw_month(
        canvas,
        env,
        &font,
        &look,
        second,
        &month.next(),
        None,
        weeks,
    );
}

/// Draws `month` into `area`, `today` marked, with room for
/// `weeks_shown` weeks (so that two months shown side by side line up).
#[allow(clippy::too_many_arguments)]
fn draw_month(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::Look,
    area: Rect,
    month: &MonthGrid,
    today: Option<u8>,
    weeks_shown: u8,
) {
    let (style, _) = ui(env);
    let widget = ID;
    let id = widget.as_str();
    let mut heading = widgets::shows(env, id, "heading");
    let mut initials = widgets::shows(env, id, "weekdays");
    let mut weeks = widgets::shows(env, id, "weeks");
    // Cells and the numbers' size; where the numbers have no room (the
    // font has no size that small), the week numbers, the heading, the
    // initials left out in turn.
    let (cell_w, cell_h, size) = loop {
        let columns = 7 + u16::from(weeks);
        let rows = u16::from(weeks_shown) + u16::from(initials);
        // The heading as high as a row would be, a little more.
        let rows_f = f32::from(rows) + if heading { 1.3 } else { 0.0 };
        let cell_h = f32::from(area.height) / rows_f;
        let cell_w = f32::from(area.width) / f32::from(columns);
        // Two digits with room round them in each cell.
        let size = (cell_h * 0.5).min(cell_w * 0.42).max(style.small * 0.7);
        let size = super::fit_size(font, size, "00", cell_w * 0.56);
        let wide = font.measure(size, "00") > cell_w * 0.8;
        let high = super::cap_height(font, size) > cell_h * 0.72;
        if wide && weeks {
            weeks = false;
        } else if high && heading {
            heading = false;
        } else if high && initials {
            initials = false;
        } else {
            break (cell_w, cell_h, size);
        }
    };
    let small = size * 0.82;
    // The size the font draws at (the nearest it has).
    let drawn = super::cap_height(font, size) / 0.74;
    let mut top = f32::from(area.y);
    let left = f32::from(area.x) + if weeks { cell_w } else { 0.0 };

    if heading {
        let name = super::month_name(env, month.month);
        let text = if widgets::shows(env, id, "year") {
            format!("{name} {}", month.year)
        } else {
            String::from(name)
        };
        let heading_size = (size * 1.15).min(style.title * 1.6);
        // In line with the numbers' ink below, not the cells' edges.
        let inset = ((cell_w - font.measure(size, "00")) / 2.0).max(0.0);
        let row = Rect::new(
            (left + inset) as u16,
            top as u16,
            (f32::from(area.width) - (left - f32::from(area.x)) - 2.0 * inset).max(0.0) as u16,
            (cell_h * 1.3 * 0.8) as u16,
        );
        let align = widgets::align(env, id);
        super::label(canvas, font, heading_size, align, row, &text, look.ink);
        top += cell_h * 1.3;
    }
    if initials {
        let two = chosen_name(env, id, "initials") == "two";
        for column in 0..7u8 {
            let name = super::weekday_name(env, month.weekday_of(column));
            let initial: String = name.chars().take(if two { 2 } else { 1 }).collect();
            let x = left + cell_w * (f32::from(column) + 0.5);
            let y = top + cell_h / 2.0;
            super::draw_centred(canvas, font, small, x, y, &initial, look.faint);
        }
        top += cell_h;
    }
    let others = widgets::shows(env, id, "others");
    let weekends = widgets::shows(env, id, "weekends");
    let mark = chosen_name(env, id, "mark");
    let first = super::dates::days_from_civil(month.year, month.month, 1);
    for week in 0..month.weeks {
        let y = top + cell_h * (f32::from(week) + 0.5);
        if weeks {
            let number = format!("{}", month.week_number(week));
            let x = f32::from(area.x) + cell_w / 2.0;
            super::draw_centred(canvas, font, small, x, y, &number, look.faint);
        }
        for column in 0..7u8 {
            let day = month.day_at(week, column);
            let x = left + cell_w * (f32::from(column) + 0.5);
            let inside = (0..i32::from(month.days)).contains(&day);
            if !inside && !others {
                continue;
            }
            let date = super::dates::civil_from_days(first + i64::from(day));
            let text = format!("{}", date.2);
            let mut color = if inside { look.ink } else { look.faint };
            if weekends && matches!(month.weekday_of(column), 0 | 6) {
                color = look.faint;
            }
            if inside && today == Some((day + 1) as u8) {
                color = draw_mark(canvas, look, mark, x, y, cell_w, cell_h, drawn);
            }
            let shown = if inside { size } else { small };
            super::draw_centred(canvas, font, shown, x, y, &text, color);
        }
    }
    let label = format!("{} {}", super::month_name(env, month.month), month.year);
    canvas.describe(
        paperpaw_sdk::semantics::Role::Text,
        &label,
        area,
        false,
        None,
    );
}

/// Draws today's mark round the number at `x`, `y` and returns the
/// number's colour on it.
#[allow(clippy::too_many_arguments)]
fn draw_mark(
    canvas: &mut Canvas<'_>,
    look: &widgets::Look,
    mark: &str,
    x: f32,
    y: f32,
    cell_w: f32,
    cell_h: f32,
    size: f32,
) -> Color {
    // Round the number, but never into the next row or column.
    let radius = (cell_w.min(cell_h) * 0.46)
        .min(size * 1.15)
        .max(size * 0.78)
        .min(cell_w.min(cell_h) * 0.5);
    let center = Point::new(x as i32, y as i32);
    match mark {
        "square" => {
            let r = radius as u16;
            let at = Rect::new((x - radius) as u16, (y - radius) as u16, r * 2, r * 2);
            canvas.fill_round(at, (radius * 0.3) as u32, look.ink);
            look.paper
        }
        "ring" => {
            let thick = (radius / 7.0).max(1.5) as u32;
            canvas.stroke_circle(center, radius as u32, thick, look.ink);
            look.ink
        }
        "underline" => {
            let thick = (size / 9.0).max(2.0) as u16;
            let w = (size * 1.1) as u16;
            let at = Rect::new(
                (x - f32::from(w) / 2.0) as u16,
                (y + size * 0.48) as u16,
                w,
                thick,
            );
            canvas.fill_round(at, u32::from(thick / 2), look.ink);
            look.ink
        }
        _ => {
            canvas.fill_circle(center, radius as u32, look.ink);
            look.paper
        }
    }
}
