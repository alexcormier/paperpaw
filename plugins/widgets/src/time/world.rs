//! The world clock: the time in up to three other places (of the time
//! zones PaperPaw knows), each with its name and whether it is already
//! tomorrow or still yesterday there ("Tokyo, tomorrow 18:41"); as rows
//! one below the other, or side by side in a wide row.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time::{self, LocalTime, TIME_ZONES, TimeZone};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame};
use paperpaw_sdk::{Arg, Canvas, Env, FontHandle, Id, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen, dates, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.world-clock");

/// No place.
const NONE: &str = "widgets.world-clock.none";
/// The settings of the places.
const PLACES: [&str; 3] = ["place-1", "place-2", "place-3"];
/// The values of the first place's setting: the ids of
/// [`TIME_ZONES`], written out to stay in the program (a test keeps
/// them the same).
const ZONES: [&str; 22] = [
    "core.tz.utc",
    "core.tz.honolulu",
    "core.tz.anchorage",
    "core.tz.los-angeles",
    "core.tz.phoenix",
    "core.tz.denver",
    "core.tz.chicago",
    "core.tz.new-york",
    "core.tz.sao-paulo",
    "core.tz.london",
    "core.tz.lisbon",
    "core.tz.berlin",
    "core.tz.helsinki",
    "core.tz.istanbul",
    "core.tz.moscow",
    "core.tz.dubai",
    "core.tz.india",
    "core.tz.bangkok",
    "core.tz.china",
    "core.tz.tokyo",
    "core.tz.sydney",
    "core.tz.auckland",
];
/// The values of the other places' settings: none, or one of [`ZONES`].
const ZONES_OR_NONE: [&str; 23] = {
    let mut values = [NONE; 23];
    let mut n = 0;
    while n < ZONES.len() {
        values[n + 1] = ZONES[n];
        n += 1;
    }
    values
};
/// The values of the layout setting: as fits, rows, side by side.
const LAYOUTS: [&str; 3] = [
    "widgets.layout.auto",
    "widgets.layout.rows",
    "widgets.layout.columns",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 9] = [
    widgets::align_spec("widgets.world-clock.align", ID, Align::Left),
    widgets::frame_spec("widgets.world-clock.frame", ID, Frame::None),
    SettingSpec::fixed_choice(
        "widgets.world-clock.place-1",
        "widgets.setting.place-1",
        &ZONES,
        "core.tz.new-york",
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.world-clock.place-2",
        "widgets.setting.place-2",
        &ZONES_OR_NONE,
        "core.tz.tokyo",
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.world-clock.place-3",
        "widgets.setting.place-3",
        &ZONES_OR_NONE,
        NONE,
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.world-clock.layout",
        "widgets.setting.layout",
        &LAYOUTS,
        LAYOUTS[0],
        ID,
    ),
    widgets::part_spec(
        "widgets.world-clock.here",
        ID,
        "widgets.setting.here",
        false,
    ),
    widgets::part_spec(
        "widgets.world-clock.day",
        ID,
        "widgets.setting.day-difference",
        true,
    ),
    widgets::part_spec(
        "widgets.world-clock.offset",
        ID,
        "widgets.setting.offset",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add_settable(
        r,
        &ID,
        "widgets.world-clock",
        Drawn::new(height, draw).shows(super::minute_shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// A place shown: its name, its time, how many days it is ahead of here,
/// and how many minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Place {
    pub name: String,
    pub time: LocalTime,
    pub days: i64,
    pub minutes: i64,
}

/// The place of time zone `rule` at `unix` seconds, here being `here`
/// seconds east of UTC.
pub(super) fn place(name: &str, rule: &str, unix: i64, here: i64) -> Option<Place> {
    let zone = TimeZone::parse(rule)?;
    let offset = i64::from(zone.offset_at(unix));
    let there = LocalTime::from_seconds(unix + offset);
    let local = LocalTime::from_seconds(unix + here);
    let day = |t: &LocalTime| dates::days_from_civil(t.year, t.month, t.day);
    Some(Place {
        // The first city of "Berlin, Vienna, Zurich, Paris, Rome".
        name: String::from(name.split(',').next().unwrap_or(name).trim()),
        time: there,
        days: day(&there) - day(&local),
        minutes: (offset - here) / 60,
    })
}

/// The places chosen, here first if wished.
fn places(env: &Env<'_>) -> Vec<Place> {
    let Some(unix) = time::unix_seconds(env) else {
        return Vec::new();
    };
    let here = time::local_seconds(env, unix) - unix;
    let mut places = Vec::new();
    if widgets::shows(env, ID.as_str(), "here")
        && let Some(now) = time::local_time(env)
    {
        places.push(Place {
            name: String::from(env.i18n.tr("widgets.world-clock.here")),
            time: now,
            days: 0,
            minutes: 0,
        });
    }
    for key in PLACES {
        let id = env.settings.text(&format!("{ID}.{key}"));
        if let Some((id, rule)) = TIME_ZONES.iter().find(|(z, _)| *z == id)
            && let Some(place) = place(env.i18n.tr(id), rule, unix, here)
        {
            places.push(place);
        }
    }
    places
}

/// "tomorrow", "yesterday" and "+7 h", as the settings want them.
fn note(env: &Env<'_>, place: &Place) -> String {
    let mut parts: Vec<String> = Vec::new();
    if widgets::shows(env, ID.as_str(), "day") {
        let key = match place.days {
            1.. => Some("widgets.world-clock.tomorrow"),
            ..=-1 => Some("widgets.world-clock.yesterday"),
            0 => None,
        };
        if let Some(key) = key {
            parts.push(String::from(env.i18n.tr(key)));
        }
    }
    if widgets::shows(env, ID.as_str(), "offset")
        && place.name != env.i18n.tr("widgets.world-clock.here")
    {
        parts.push(offset_text(env, place.minutes));
    }
    parts.join(" · ")
}

/// A difference of `minutes` in hours: "+7 h", "−5:30 h", "±0 h".
pub(super) fn offset_text(env: &Env<'_>, minutes: i64) -> String {
    let sign = match minutes {
        1.. => "+",
        0 => "±",
        _ => "−",
    };
    let (h, m) = (minutes.abs() / 60, minutes.abs() % 60);
    let number = if m == 0 {
        format!("{sign}{h}")
    } else {
        format!("{sign}{h}:{m:02}")
    };
    env.i18n.format(
        "widgets.world-clock.offset",
        &[("offset", Arg::Str(&number))],
    )
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    let count = places(env).len().max(1) as u16;
    let row = widgets::line_height(env, style.title * 1.3) + widgets::line_height(env, style.small);
    row * count + style.gap as u16 * (count - 1) + frame_height(env, ID.as_str())
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let Some(font) = font else {
        return;
    };
    let mut places = places(env);
    if places.is_empty() {
        widgets::draw_message(canvas, env, &look, env.i18n.tr("widgets.world-clock.empty"));
        return;
    }
    let area = look.area;
    let gap = style.gap as u16;
    let columns = match chosen(env, ID.as_str(), "layout") {
        "rows" => false,
        "columns" => true,
        _ => area.width / places.len() as u16 > area.height * 9 / 10 && places.len() > 1,
    };
    // Where not all have room, those first chosen.
    let least = line(&font, style.small) as u16;
    let room = if columns { area.width } else { area.height };
    places.truncate(super::fitting(room, gap, least, places.len()));
    let n = places.len() as u16;
    // Rows with the name beside the time where every name has room
    // there, else with the name above it.
    let row_h = super::share(area.height, gap, n);
    let stacked = !columns
        && places.iter().any(|place| {
            let clock = super::clock_text(env, &place.time);
            let size = time_size(&font, &clock, f32::from(row_h), f32::from(area.width));
            font.measure(size, &clock) + style.gap as f32 + font.measure(style.small, &place.name)
                > f32::from(area.width)
        });
    // Places side by side or stacked alike: the times as large as the
    // widest allows, room for notes where any has one.
    let small = line(&font, style.small);
    let common = {
        let any_note = !stacked && places.iter().any(|p| !note(env, p).is_empty());
        let notes = if any_note { 2.0 } else { 1.0 };
        let (w, h) = if columns {
            (super::share(area.width, gap, n), area.height)
        } else {
            (area.width, row_h)
        };
        let room = (f32::from(h) - small * notes).max(small);
        let size = places
            .iter()
            .map(|p| {
                let clock = super::clock_text(env, &p.time);
                super::fit_size(&font, room * 0.8, &clock, f32::from(w) * 0.96)
            })
            .fold(room * 0.8, f32::min)
            .max(style.small);
        (size, notes)
    };
    for (i, place) in places.iter().enumerate() {
        let i = i as u16;
        let cell = if columns {
            let w = super::share(area.width, gap, n);
            Rect::new(area.x + i * (w + gap), area.y, w, area.height)
        } else {
            let h = super::share(area.height, gap, n);
            Rect::new(area.x, area.y + i * (h + gap), area.width, h)
        };
        if columns || stacked {
            draw_column(canvas, env, &font, &look, cell, place, stacked, common);
        } else {
            draw_row(canvas, env, &font, &look, cell, place);
        }
        // A fine line between places.
        if i > 0 && look.frame != Frame::Dark {
            let thick = style.line as u16;
            let back = (gap / 2 + thick / 2).min(gap);
            let between = if columns {
                Rect::new(cell.x - back, cell.y, thick.min(gap), cell.height)
            } else {
                Rect::new(cell.x, cell.y - back, cell.width, thick.min(gap))
            };
            canvas.fill_shaded(between, paperpaw_sdk::Color::gray(200));
        }
    }
}

/// A place as a row: its name and note on the left, the time large on
/// the right.
fn draw_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::Look,
    cell: Rect,
    place: &Place,
) {
    let (style, _) = ui(env);
    let clock = super::clock_text(env, &place.time);
    let note = note(env, place);
    let width = f32::from(cell.width);
    let size = time_size(font, &clock, f32::from(cell.height), width).max(style.small);
    let time_width = font.measure(size, &clock);
    let big = line(font, size).min(f32::from(cell.height));
    let time_row = Rect::new(
        cell.x,
        cell.y + ((f32::from(cell.height) - big) / 2.0) as u16,
        cell.width,
        big as u16,
    );
    let flipped = widgets::align(env, ID.as_str()) == Align::Right;
    let (time_align, name_x) = if flipped {
        (Align::Left, cell.x + (time_width + style.gap as f32) as u16)
    } else {
        (Align::Right, cell.x)
    };
    super::label(canvas, font, size, time_align, time_row, &clock, look.ink);
    // The name and its note, together in the middle of the row.
    let name_room = Rect::new(
        name_x,
        cell.y,
        (width - time_width - style.gap as f32).max(0.0) as u16,
        cell.height,
    );
    let name_size = style.text.min(f32::from(cell.height) * 0.5);
    let name_line = line(font, name_size);
    let note_line = if note.is_empty() {
        0.0
    } else {
        line(font, style.small)
    };
    let two = name_line + note_line <= f32::from(cell.height);
    let name_align = if flipped { Align::Right } else { Align::Left };
    if two {
        let top = f32::from(cell.y) + (f32::from(cell.height) - name_line - note_line) / 2.0;
        let row = Rect::new(name_room.x, top as u16, name_room.width, name_line as u16);
        super::label(
            canvas,
            font,
            name_size,
            name_align,
            row,
            &place.name,
            look.ink,
        );
        if !note.is_empty() {
            let row = Rect::new(
                name_room.x,
                (top + name_line) as u16,
                name_room.width,
                note_line as u16,
            );
            super::label(
                canvas,
                font,
                style.small,
                name_align,
                row,
                &note,
                look.faint,
            );
        }
    } else {
        let text = if note.is_empty() {
            place.name.clone()
        } else {
            format!("{}, {note}", place.name)
        };
        super::label(
            canvas,
            font,
            style.small,
            name_align,
            name_room,
            &text,
            look.ink,
        );
    }
}

/// The size of the time in a row `height` high and `width` wide: up to
/// half the width, and most of the height.
fn time_size(font: &FontHandle<'_>, clock: &str, height: f32, width: f32) -> f32 {
    super::fit_size(font, height * 0.62, clock, width * 0.5)
}

/// A place as a column: its name small above, the time large, the note
/// below (`compact`: beside the name); the time `size` large, room
/// left for `notes` lines of small text (as for the other places, so
/// that they line up).
#[allow(clippy::too_many_arguments)]
fn draw_column(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::Look,
    cell: Rect,
    place: &Place,
    compact: bool,
    (size, notes): (f32, f32),
) {
    let (style, _) = ui(env);
    let align = widgets::align(env, ID.as_str());
    let clock = super::clock_text(env, &place.time);
    let mut note = note(env, place);
    let mut name = place.name.clone();
    if compact && !note.is_empty() {
        // One line above: "Tokyo, tomorrow".
        name = format!("{name}, {note}");
        note = String::new();
    }
    let small = line(font, style.small);
    let room = (f32::from(cell.height) - small * notes).max(small);
    let big = line(font, size).min(room);
    let total = small * notes + big;
    let mut y = f32::from(cell.y) + (f32::from(cell.height) - total).max(0.0) / 2.0;
    let row = |y: f32, h: f32| Rect::new(cell.x, y as u16, cell.width, h as u16);
    super::label(
        canvas,
        font,
        style.small,
        align,
        row(y, small),
        &name,
        look.faint,
    );
    y += small;
    super::label(canvas, font, size, align, row(y, big), &clock, look.ink);
    y += big;
    if !note.is_empty() {
        super::label(
            canvas,
            font,
            style.small,
            align,
            row(y, small),
            &note,
            look.faint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday, 14 October 2026, 10:41 in Berlin (summer time).
    const NOW: i64 = 1_791_967_260;
    const BERLIN: i64 = 7200;

    #[test]
    fn the_places_offered_are_the_time_zones() {
        let zones: Vec<&str> = TIME_ZONES.iter().map(|(id, _)| *id).collect();
        assert_eq!(ZONES[..], zones[..]);
        assert_eq!(ZONES_OR_NONE[0], NONE);
        assert_eq!(ZONES_OR_NONE[1..], zones[..]);
    }

    #[test]
    fn places_are_ahead_or_behind() {
        let here = LocalTime::from_seconds(NOW + BERLIN);
        assert_eq!((here.day, here.hour, here.minute), (14, 10, 41));
        let tokyo = place("Tokyo, Seoul", "JST-9", NOW, BERLIN).unwrap();
        assert_eq!(tokyo.name, "Tokyo");
        assert_eq!(
            (tokyo.time.hour, tokyo.time.minute, tokyo.days),
            (17, 41, 0)
        );
        assert_eq!(tokyo.minutes, 7 * 60);
        let new_york = place("New York", "EST5EDT,M3.2.0,M11.1.0", NOW, BERLIN).unwrap();
        assert_eq!(
            (new_york.time.hour, new_york.days, new_york.minutes),
            (4, 0, -360)
        );
        // Late in the evening, Auckland is in tomorrow already.
        let evening = NOW + 12 * 3600;
        let auckland = place("Auckland", "NZST-12NZDT,M9.5.0,M4.1.0/3", evening, BERLIN).unwrap();
        assert_eq!((auckland.time.day, auckland.days), (15, 1));
        // Early in the morning, Honolulu is still in yesterday.
        let morning = NOW - 9 * 3600;
        let honolulu = place("Honolulu", "HST10", morning, BERLIN).unwrap();
        assert_eq!(honolulu.days, -1);
        let india = place("India", "IST-5:30", NOW, BERLIN).unwrap();
        assert_eq!(india.minutes, 210);
    }
}
