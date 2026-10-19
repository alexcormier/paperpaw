//! The widget "Weather now": a symbol, the temperature large, the
//! condition's name, today's highest and lowest, and what else the reader
//! chooses (how warm it feels, wind, chance of rain, humidity, sunrise and
//! sunset), with the place's name small above.
//!
//! The symbol sits beside the text where the place is wide, above it where
//! it is tall (or as set), or small before the temperature; with the size
//! "as large as it fits", the temperature takes the room the other lines
//! leave. Lines that do not fit are left out, the least important first.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::ui::{self, UiStyle};
use quireink_sdk::widgets::{self, Align, Frame, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{Arg, Canvas, Env, FontHandle, Id, Rect, RegisterError, Registrar};

use crate::data::{self, Current, Day};
use crate::draw::{self, cap, digits_height, draw_digits};
use crate::symbol;
use crate::{Shared, Shown};

pub(crate) const ID: Id = Id::new_static("weather.now");

/// The values of the symbol's size.
const SYMBOLS: [&str; 3] = [
    "weather.symbol.large",
    "weather.symbol.small",
    "weather.symbol.none",
];
/// The values of where the symbol sits.
const PLACINGS: [&str; 3] = [
    "weather.placing.auto",
    "weather.placing.beside",
    "weather.placing.above",
];
/// Its parts: the setting's key, its title, whether shown at first.
const PARTS: [(&str, &str, bool); 10] = [
    ("place", "weather.part.place", true),
    ("condition", "weather.part.condition", true),
    ("range", "weather.part.range", true),
    ("feels", "weather.part.feels", false),
    ("wind", "weather.part.wind", false),
    ("rain", "weather.part.rain", false),
    ("humidity", "weather.part.humidity", false),
    ("sun", "weather.part.sun", false),
    ("unit", "weather.part.unit", false),
    ("age", "weather.part.age", true),
];

pub(crate) fn register(r: &mut Registrar, state: &Shared) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&ID, "weather.widget.now"))?;
    r.add_setting(widgets::frame_setting(&ID, Frame::None))?;
    r.add_setting(widgets::size_setting(&ID, Size::Fit, true))?;
    r.add_setting(widgets::align_setting(&ID, Align::Center))?;
    r.add_setting(widgets::choice(
        &ID,
        "symbol",
        "weather.setting.symbol",
        &SYMBOLS,
        SYMBOLS[0],
    ))?;
    r.add_setting(widgets::choice(
        &ID,
        "placing",
        "weather.setting.placing",
        &PLACINGS,
        PLACINGS[0],
    ))?;
    r.add_setting(draw::style_setting(&ID))?;
    for (part, title, default) in PARTS {
        r.add_setting(widgets::part_setting(&ID, part, title, default))?;
    }
    let state = state.clone();
    r.add_widget(
        WidgetSpec::new(ID, "weather.widget.now", move |env| {
            Box::new(Now {
                shown: Shown::read(env, &state),
                state: state.clone(),
            })
        })
        .category(widgets::categories::WORLD),
    )
}

/// "Weather now": what it shows, read when updated.
struct Now {
    state: Shared,
    shown: Shown,
}

impl Widget for Now {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = Shown::read(env, &self.state);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let probe = Rect::new(0, 0, 100, 100);
        let frame = 100 - widgets::look::look(env, ID.as_str(), probe).area.height;
        let size = temperature_size(env, &style).unwrap_or(style.title * 2.4);
        digits_height(size) + widgets::line_height(env, style.small) * 3 + frame
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        match (
            &self.shown.weather,
            self.shown.weather.as_ref().and_then(|w| w.current),
        ) {
            (Some(weather), Some(current)) => {
                let today = weather.days_from(self.shown.today).first().copied();
                draw_now(canvas, env, &look, &self.shown, current, today);
            }
            _ => draw::draw_waiting(canvas, env, ID.as_str(), &look, &self.shown),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget {
            area,
            command: crate::tap(&self.shown),
        }]
    }
}

/// The temperature's size as set; `None` for "as large as it fits".
fn temperature_size(env: &Env<'_>, style: &UiStyle) -> Option<f32> {
    match widgets::size(env, ID.as_str()) {
        Size::Small => Some(style.title * 1.6),
        Size::Medium => Some(style.title * 2.4),
        Size::Large => Some(style.title * 3.4),
        Size::Fit => None,
    }
}

/// A line of text: what it says, how large, in which colour, and how
/// soon it is left out where there is no room (lower first).
#[derive(Debug, Clone)]
struct Line {
    text: String,
    size: f32,
    faint: bool,
    rank: u8,
}

/// The lines below the temperature and the caption above it, as set.
fn lines(env: &Env<'_>, shown: &Shown, current: Current, today: Option<Day>) -> Vec<Line> {
    let style = UiStyle::new(env);
    let part = |name: &str| widgets::shows(env, ID.as_str(), name);
    let unit = crate::temperature_unit(env);
    let with_unit = part("unit");
    let degrees = |tenths| data::degrees_text(unit, tenths, with_unit);
    let mut lines = Vec::new();
    let mut caption = Vec::new();
    if part("place") && !shown.place.is_empty() {
        caption.push(shown.place.clone());
    }
    if part("age")
        && let Some(age) = shown.age_text(env)
    {
        caption.push(age);
    }
    if !caption.is_empty() {
        lines.push(Line {
            text: caption.join(" · "),
            size: style.small,
            faint: true,
            rank: 3,
        });
    }
    if part("condition") {
        lines.push(Line {
            text: env.i18n.tr(data::condition(current.code).0).to_string(),
            size: style.text,
            faint: false,
            rank: 5,
        });
    }
    let mut details: Vec<(String, u8)> = Vec::new();
    if part("range")
        && let Some(day) = today
    {
        let text = env.i18n.format(
            "weather.range",
            &[
                ("high", Arg::Str(&degrees(day.high))),
                ("low", Arg::Str(&degrees(day.low))),
            ],
        );
        details.push((text, 4));
    }
    if part("feels")
        && let Some(feels) = current.feels
    {
        let text = env
            .i18n
            .format("weather.feels", &[("degrees", Arg::Str(&degrees(feels)))]);
        details.push((text, 2));
    }
    if part("wind")
        && let Some(wind) = current.wind
    {
        let wind_unit = crate::wind_unit(env);
        let speed = env.i18n.format(
            wind_unit.key(),
            &[("speed", Arg::Int(i64::from(wind_unit.speed(wind))))],
        );
        let text = env
            .i18n
            .format("weather.wind", &[("speed", Arg::Str(&speed))]);
        details.push((text, 2));
    }
    if part("rain")
        && let Some(rain) = today.and_then(|d| d.rain)
    {
        let text = env
            .i18n
            .format("weather.rain", &[("percent", Arg::Int(i64::from(rain)))]);
        details.push((text, 2));
    }
    if part("humidity")
        && let Some(humidity) = current.humidity
    {
        let text = env.i18n.format(
            "weather.humidity",
            &[("percent", Arg::Int(i64::from(humidity)))],
        );
        details.push((text, 1));
    }
    if part("sun")
        && let Some((rise, set)) = today.and_then(|d| d.sunrise.zip(d.sunset))
    {
        let text = env.i18n.format(
            "weather.sun",
            &[
                ("rise", Arg::Str(&draw::clock(env, rise))),
                ("set", Arg::Str(&draw::clock(env, set))),
            ],
        );
        details.push((text, 1));
    }
    for (text, rank) in details {
        lines.push(Line {
            text,
            size: style.small,
            faint: rank < 4,
            rank,
        });
    }
    lines
}

/// `lines` without those ranked below `least`, the details beside each
/// other where they fit `width` (" · " between them).
fn packed(font: &FontHandle<'_>, lines: &[Line], least: u8, width: f32) -> Vec<Line> {
    let small = lines.iter().map(|l| l.size).fold(f32::MAX, f32::min);
    let mut out: Vec<Line> = Vec::new();
    for line in lines.iter().filter(|l| l.rank >= least) {
        if let Some(last) = out.last_mut()
            && last.rank < 5
            && last.size == line.size
            && last.faint == line.faint
            && line.rank < 3
            && last.rank < 3
        {
            let joined = alloc::format!("{} · {}", last.text, line.text);
            if font.measure(line.size, &joined) <= width {
                last.text = joined;
                last.rank = last.rank.max(line.rank);
                continue;
            }
        }
        let mut line = line.clone();
        // The condition smaller rather than cut short.
        if line.rank == 5 && font.measure(line.size, &line.text) > width {
            line.size = small;
        }
        out.push(line);
    }
    out
}

/// Where the symbol goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placing {
    Beside,
    Above,
    /// Small, before the temperature.
    Inline,
    None,
}

/// What is drawn where: the symbol's square, the temperature's size, the
/// lines kept, and the text's place.
#[derive(Debug, Clone)]
struct Plan {
    placing: Placing,
    symbol: Rect,
    size: f32,
    caption: Option<Line>,
    lines: Vec<Line>,
    text: Rect,
    /// The temperature fits the text's width.
    wide_enough: bool,
}

fn line_height(env: &Env<'_>, line: &Line) -> u16 {
    widgets::line_height(env, line.size)
}

/// The height of the text: the caption, the temperature, the lines.
fn text_height(env: &Env<'_>, size: f32, lines: &[Line]) -> u16 {
    digits_height(size) + lines.iter().map(|l| line_height(env, l)).sum::<u16>()
}

/// The largest temperature that fits `width` and `height` with `lines`,
/// as set or as large as fits (at most `most`).
#[allow(clippy::too_many_arguments)]
fn fitting(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    text: &str,
    lines: &[Line],
    width: f32,
    height: u16,
    most: f32,
    inline: bool,
) -> f32 {
    let style = UiStyle::new(env);
    if let Some(size) = temperature_size(env, &style) {
        return size;
    }
    let rest = lines.iter().map(|l| line_height(env, l)).sum::<u16>();
    let by_height = draw::digits_size(f32::from(height.saturating_sub(rest)));
    let probe = style.title;
    let gap = style.gap as u16;
    let room = width - if inline { f32::from(gap / 2) } else { 0.0 };
    let wide = (temperature_width(font, probe, text, inline, 0)).max(1.0) / probe;
    let by_width = room / wide;
    by_height
        .min(by_width)
        .min(most)
        .max(style.small.min(by_width))
}

/// The small symbol before a temperature `size` pixels large: its side
/// and the room after it (`gap` and some).
fn inline_symbol(size: f32, gap: u16) -> (u16, u16) {
    let side = (cap(size) * 1.25) as u16;
    (side, gap / 2 + side / 8)
}

/// How wide the temperature is, with the small symbol before it if
/// `inline`.
fn temperature_width(font: &FontHandle<'_>, size: f32, text: &str, inline: bool, gap: u16) -> f32 {
    let (side, after) = if inline {
        inline_symbol(size, gap)
    } else {
        (0, 0)
    };
    font.measure(size, text) + f32::from(side + after)
}

/// Where the symbol goes as set; where it does not fit, the places to try
/// after it.
fn placings(env: &Env<'_>, area: Rect) -> &'static [Placing] {
    let symbol = env.settings.text(&alloc::format!("{ID}.symbol"));
    let placing = env.settings.text(&alloc::format!("{ID}.placing"));
    match SYMBOLS.iter().position(|s| *s == symbol) {
        Some(2) => &[Placing::None],
        Some(1) => &[Placing::Inline, Placing::None],
        _ => match PLACINGS.iter().position(|p| *p == placing) {
            Some(1) => &[
                Placing::Beside,
                Placing::Above,
                Placing::Inline,
                Placing::None,
            ],
            Some(2) => &[Placing::Above, Placing::Inline, Placing::None],
            _ if u32::from(area.width) * 10 >= u32::from(area.height) * 13 => &[
                Placing::Beside,
                Placing::Above,
                Placing::Inline,
                Placing::None,
            ],
            _ => &[
                Placing::Above,
                Placing::Beside,
                Placing::Inline,
                Placing::None,
            ],
        },
    }
}

/// The plan: the first place of the symbol where the temperature is not
/// too small, with as many lines as fit.
fn plan(env: &Env<'_>, font: &FontHandle<'_>, area: Rect, temperature: &str, all: &[Line]) -> Plan {
    let style = UiStyle::new(env);
    // The least a temperature should be before lines (or the large
    // symbol) are left out; less where the font has no larger sizes.
    let largest = draw::effective(font, 400.0);
    let least = (style.title * 1.3).min(largest) * 0.9;
    let mut last = None;
    for &placing in placings(env, area) {
        let plan = plan_for(env, font, area, temperature, all, placing, least);
        // A small symbol before the temperature, even smaller, rather
        // than none.
        let enough = if placing == Placing::Inline {
            least * 0.5
        } else {
            least * 0.95
        };
        if (plan.size >= enough && plan.wide_enough) || temperature_size(env, &style).is_some() {
            return plan;
        }
        last = Some(plan);
    }
    last.unwrap_or_else(|| plan_for(env, font, area, temperature, all, Placing::None, least))
}

/// The plan with the symbol placed so, as many lines left out as needed
/// for a temperature of at least `least`.
fn plan_for(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    area: Rect,
    temperature: &str,
    all: &[Line],
    placing: Placing,
    least: f32,
) -> Plan {
    let style = UiStyle::new(env);
    let gap = style.gap as u16;
    let fixed = temperature_size(env, &style);
    // Whether lines are left to leave out from `rank` on.
    let more = |rank: u8| all.iter().any(|l| l.rank >= rank && l.rank > 0) && rank < 6;
    let mut rank = 0u8;
    loop {
        let (side, text_area) = match placing {
            Placing::Beside => {
                let side = match fixed {
                    None => area.height.min(area.width * 2 / 5),
                    Some(size) => area
                        .height
                        .min(area.width * 2 / 5)
                        .min(digits_height(size) * 2),
                };
                let shift = side + gap;
                let text = Rect::new(
                    area.x + shift,
                    area.y,
                    area.width.saturating_sub(shift),
                    area.height,
                );
                (side, text)
            }
            Placing::Above => {
                let lines = packed(font, all, rank, f32::from(area.width));
                let words = text_height(env, fixed.unwrap_or(least), &lines);
                // What the symbol may take; too little: fewer lines.
                let room = area.height.saturating_sub(words + gap);
                if room < area.height / 4 && more(rank) {
                    rank += 1;
                    continue;
                }
                let side = match fixed {
                    None => room.min(area.height * 11 / 20),
                    Some(size) => room.min(digits_height(size) * 2),
                }
                .min(area.width * 3 / 4);
                let shift = side + gap;
                let text = Rect::new(
                    area.x,
                    area.y + shift,
                    area.width,
                    area.height.saturating_sub(shift),
                );
                (side, text)
            }
            Placing::Inline | Placing::None => (0, area),
        };
        let lines = packed(font, all, rank, f32::from(text_area.width));
        let (caption, rest): (Vec<Line>, Vec<Line>) = lines.into_iter().partition(|l| l.rank == 3);
        let caption = caption.into_iter().next();
        let mut kept: Vec<Line> = caption.iter().cloned().collect();
        kept.extend(rest.iter().cloned());
        // Not larger than the symbol beside it, to keep them in balance.
        let most = match placing {
            Placing::Beside => (f32::from(side) * 0.55).max(least),
            Placing::Above => (f32::from(side) * 0.6).max(least),
            // Large, not huge.
            _ => (style.title * 3.5).min(f32::from(text_area.height)),
        };
        let size = fitting(
            env,
            font,
            temperature,
            &kept,
            f32::from(text_area.width),
            text_area.height,
            most,
            placing == Placing::Inline,
        );
        let size = draw::effective(font, size);
        let wide = temperature_width(font, size, temperature, placing == Placing::Inline, gap);
        let fits = text_height(env, size, &kept) <= text_area.height
            && wide <= f32::from(text_area.width)
            && (fixed.is_some() || size >= least * 0.95);
        if fits || !more(rank) {
            let symbol = match placing {
                Placing::Beside => Rect::new(
                    area.x,
                    area.y + area.height.saturating_sub(side) / 2,
                    side,
                    side,
                ),
                Placing::Above => Rect::new(area.x, area.y, side, side),
                _ => Rect::new(0, 0, 0, 0),
            };
            return Plan {
                placing,
                symbol,
                size,
                caption,
                lines: rest,
                text: text_area,
                wide_enough: wide <= f32::from(text_area.width),
            };
        }
        rank += 1;
    }
}

fn draw_now(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &widgets::Look,
    shown: &Shown,
    current: Current,
    today: Option<Day>,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let area = look.area;
    let gap = style.gap as u16;
    let unit = crate::temperature_unit(env);
    let temperature = data::degrees_text(
        unit,
        current.temperature,
        widgets::shows(env, ID.as_str(), "unit"),
    );
    let all = lines(env, shown, current, today);
    let plan = plan(env, &font, area, &temperature, &all);
    let align = widgets::align(env, ID.as_str());
    let symbol_style = draw::style(env, ID.as_str());
    let (_, symbol) = data::condition(current.code);
    let color = |line: &Line| if line.faint { look.faint } else { look.ink };

    // The rows of text, from the top.
    let (inline, inline_gap) = if plan.placing == Placing::Inline {
        inline_symbol(plan.size, gap)
    } else {
        (0, 0)
    };
    let temperature_width = font.measure(plan.size, &temperature) + f32::from(inline + inline_gap);
    let mut rows: Vec<(u16, f32, Option<&Line>)> = Vec::new();
    if let Some(caption) = &plan.caption {
        rows.push((
            line_height(env, caption),
            font.measure(caption.size, &caption.text),
            Some(caption),
        ));
    }
    rows.push((digits_height(plan.size), temperature_width, None));
    for line in &plan.lines {
        rows.push((
            line_height(env, line),
            font.measure(line.size, &line.text),
            Some(line),
        ));
    }
    let total: u16 = rows.iter().map(|(h, _, _)| *h).sum();
    let widest = rows
        .iter()
        .map(|(_, w, _)| *w)
        .fold(0.0, f32::max)
        .min(f32::from(plan.text.width));

    // Beside: the symbol and the text as one group, placed as set; the
    // text starts at one edge.
    let (text, group_x) = match plan.placing {
        Placing::Beside => {
            let group = f32::from(plan.symbol.width + gap) + widest;
            let x = align.x(area, group);
            let shift = x as u16 - area.x;
            let at = Rect::new(
                plan.symbol.x + shift,
                plan.symbol.y,
                plan.symbol.width,
                plan.symbol.height,
            );
            symbol::draw(canvas, at, symbol, current.day, symbol_style, look.ink);
            let text_x = at.x + at.width + gap;
            (
                Rect::new(text_x, plan.text.y, draw::ceil(widest), plan.text.height),
                Some(f32::from(text_x)),
            )
        }
        Placing::Above => {
            let total = plan.symbol.height + gap + total;
            let top = area.y + area.height.saturating_sub(total) / 2;
            let x = align.x(area, f32::from(plan.symbol.width)) as u16;
            let at = Rect::new(x, top, plan.symbol.width, plan.symbol.height);
            symbol::draw(canvas, at, symbol, current.day, symbol_style, look.ink);
            let below = top + plan.symbol.height + gap;
            (
                Rect::new(
                    area.x,
                    below,
                    area.width,
                    area.height.saturating_sub(below - area.y),
                ),
                None,
            )
        }
        _ => (plan.text, None),
    };
    let mut y = match plan.placing {
        Placing::Above => text.y,
        _ => text.y + text.height.saturating_sub(total) / 2,
    };
    for (height, width, line) in rows {
        let row = Rect::new(text.x, y, text.width.max(1), height);
        let x = group_x.unwrap_or_else(|| align.x(text, width));
        match line {
            Some(line) => {
                ui::draw_label(canvas, &font, line.size, x, row, &line.text, color(line));
            }
            None => {
                if inline > 0 {
                    let top = row.y + row.height.saturating_sub(inline) / 2;
                    let at = Rect::new(x as u16, top, inline, inline);
                    symbol::draw(canvas, at, symbol, current.day, symbol_style, look.ink);
                }
                let x = x + f32::from(inline + inline_gap);
                draw_digits(canvas, &font, plan.size, x, row, &temperature, look.ink);
            }
        }
        y += height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_have_settings() {
        let keys: Vec<&str> = PARTS.iter().map(|(k, _, _)| *k).collect();
        assert!(keys.contains(&"place") && keys.contains(&"sun"));
        // Calm at first: the place, the condition, today's range.
        let shown: Vec<&str> = PARTS
            .iter()
            .filter(|(_, _, on)| *on)
            .map(|(k, _, _)| *k)
            .collect();
        assert_eq!(shown, ["place", "condition", "range", "age"]);
    }
}
