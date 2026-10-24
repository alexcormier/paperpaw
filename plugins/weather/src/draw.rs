//! What both widgets draw alike: their settings of the symbols, the
//! temperature's digits placed by their ink, what they say while there is
//! no weather.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, WidgetLook, Wrapped};
use paperpaw_sdk::{Canvas, Color, Env, FontHandle, Id, Rect, SettingSpec, time};

use crate::data::Symbol;
use crate::symbol::{self, Style};
use crate::{Shown, Waiting};

/// The values of a widget's symbol style setting.
pub(crate) const STYLES: [&str; 2] = ["weather.style.line", "weather.style.solid"];

/// The setting of how `widget` draws its symbols, kept in the program:
/// `id` is `<widget>.style`.
pub(crate) const fn style_spec(id: &'static str, widget: Id) -> SettingSpec {
    SettingSpec::choice(id, "weather.setting.style", &STYLES, STYLES[0], widget)
}

/// How `widget` draws its symbols.
pub(crate) fn style(env: &Env<'_>, widget: &str) -> Style {
    if env.settings().text(&format!("{widget}.style")) == STYLES[1] {
        Style::Solid
    } else {
        Style::Line
    }
}

/// How high digits `size` pixels large are (their ink, about the
/// capitals' height).
pub(crate) fn cap(size: f32) -> f32 {
    size * 0.72
}

/// The height of a line of digits `size` pixels large, with a little room
/// above and below.
pub(crate) fn digits_height(size: f32) -> u16 {
    let cap = cap(size);
    (cap + (cap * 0.3).min(ROOM)) as u16
}

/// The room above and below large digits at most, together.
const ROOM: f32 = 24.0;

/// The size of digits whose line is `height` pixels high
/// ([`digits_height`] the other way).
pub(crate) fn digits_size(height: f32) -> f32 {
    let cap = if height >= ROOM / 0.3 * 1.3 {
        height - ROOM
    } else {
        height / 1.3
    };
    cap / cap_ratio()
}

fn cap_ratio() -> f32 {
    cap(1.0)
}

/// Draws `text` `size` pixels large with its digits in the middle of `row`
/// from top to bottom, from `x`.
pub(crate) fn draw_digits(
    ui: &mut ui::Ui<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    row: Rect,
    text: &str,
    color: Color,
) {
    let metrics = font.line_metrics(size);
    let baseline = f32::from(row.y) + (f32::from(row.height) + cap(size)) / 2.0;
    // A label centres the font's line in its area: an area whose
    // baseline falls where the digits' does.
    let top = baseline - metrics.ascent;
    let area = Rect::new(
        row.x,
        top.max(0.0) as u16,
        row.width,
        (metrics.ascent + metrics.descent) as u16,
    );
    ui::Label::new(text)
        .px(size)
        .color(color)
        .font(*font)
        .draw_from(ui, x, area);
}

/// The size digits `size` pixels large are drawn in: smaller where the
/// font has sizes up to a limit (packs of small devices).
pub(crate) fn effective(font: &FontHandle<'_>, size: f32) -> f32 {
    const PROBE: f32 = 20.0;
    let base = font.measure(PROBE, "0000");
    if base <= 0.0 {
        return size;
    }
    (PROBE * font.measure(size, "0000") / base).min(size)
}

/// `x` rounded up to whole pixels.
pub(crate) fn ceil(x: f32) -> u16 {
    let whole = x.max(0.0) as u16;
    if f32::from(whole) < x {
        whole + 1
    } else {
        whole
    }
}

/// Where the ink of `symbol` (by day or night) drawn in the square
/// `side` pixels large lies in it: left, top, width, height in pixels.
pub(crate) fn ink_of(symbol: Symbol, day: bool, side: u16) -> (i32, i32, u16, u16) {
    let (left, top, right, bottom) = symbol::ink_box(symbol, day);
    let at = |units: i32| i32::from(side) * units / 1000;
    let (x, y) = (at(left), at(top));
    (x, y, (at(right) - x) as u16, (at(bottom) - y) as u16)
}

/// Draws `symbol` into the square `area` with its ink in the middle of
/// it, rather than where its shapes lie in their square (most leave
/// more room on one side than the other).
pub(crate) fn draw_symbol(
    canvas: &mut Canvas<'_>,
    area: Rect,
    (symbol, day): (Symbol, bool),
    style: Style,
    color: Color,
) {
    let side = area.width.min(area.height);
    let (x, y, w, h) = ink_of(symbol, day, side);
    let dx = (i32::from(side) - i32::from(w)) / 2 - x;
    let dy = (i32::from(side) - i32::from(h)) / 2 - y;
    let at = Rect::new(
        (i32::from(area.x) + dx).max(0) as u16,
        (i32::from(area.y) + dy).max(0) as u16,
        side,
        side,
    );
    symbol::draw(canvas, at, symbol, day, style, color);
}

/// A time of day (minutes after midnight) as the clock shows it.
pub(crate) fn clock(env: &Env<'_>, minutes: u16) -> String {
    let (hour, minute) = (minutes / 60, minutes % 60);
    let twelve = env
        .settings
        .read(&paperpaw_sdk::builtin::settings::CLOCK_FORMAT)
        == time::CLOCK_12H;
    if !twelve {
        return format!("{hour:02}:{minute:02}");
    }
    let shown = match hour % 12 {
        0 => 12,
        h => h,
    };
    let key = if hour < 12 {
        "core.clock.am"
    } else {
        "core.clock.pm"
    };
    let text = format!("{shown}:{minute:02}");
    env.i18n()
        .format(key, &[("time", paperpaw_sdk::Arg::Str(&text))])
}

/// The short form of what a widget says while it has no weather, for
/// where the whole sentence does not fit.
fn short_key(waiting: Waiting) -> &'static str {
    match waiting {
        Waiting::NoPlace => "weather.no-place.short",
        Waiting::Fetching => "weather.fetching.short",
        Waiting::Failed => "weather.failed.short",
        Waiting::NoNetwork => "weather.no-network.short",
    }
}

/// Draws what `widget` says while it has no weather: why, under a faint
/// symbol where there is room. Where the sentence does not fit whole,
/// the symbol goes, then the sentence gives way to its short form.
pub(crate) fn draw_waiting(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    (look, place): (&WidgetLook, Rect),
    shown: &Shown,
) {
    let style = UiStyle::new(env);
    let area = look.area;
    // The short form alone may take half the spacing above and below:
    // the lines' own room above and below the letters keeps them clear of
    // the edges (two lines fit a low place then).
    let lines = Rect::new(area.x, place.y, area.width, place.height);
    let tall = (area.height + place.height) / 2;
    let long = match shown.waiting {
        // The place is set: its name with the message.
        Waiting::NoNetwork if !shown.place.is_empty() => env.i18n().format(
            "weather.no-network-for",
            &[("place", paperpaw_sdk::Arg::Str(&shown.place))],
        ),
        waiting => String::from(env.i18n().tr(waiting.key())),
    };
    let short = env.i18n().tr(short_key(shown.waiting));
    let line = widgets::line_height(env, style.text());
    let gap = style.gap();
    // The symbol's ink at least this high, else none.
    let least = line * 3 / 2;
    let below = area.height.saturating_sub(least + gap);
    let lay = |text: &str, height: u16| {
        Wrapped::fitting(env, text, style.text(), style.small(), area.width, height)
    };
    let tries = [
        (&long[..], below, true),
        (&long[..], area.height, false),
        (short, below, true),
    ];
    let (words, symbol) = tries
        .iter()
        .filter(|(_, height, _)| *height > 0)
        .map(|(text, height, symbol)| (lay(text, *height), *symbol))
        .find(|(words, _)| words.whole())
        .unwrap_or_else(|| (lay(short, tall), false));
    let align = widgets::align(env, widget);
    let words_h = words.height(env);
    let (left, top, right, bottom) = symbol::ink_box(Symbol::PartlyCloudy, true);
    // The ink as high as the room left (three lines at most), not wider
    // than half the area; its square larger by what it leaves blank.
    let room = area.height.saturating_sub(words_h + gap).min(line * 3);
    let ink_h = room.min(
        (u32::from(area.width / 2) * (bottom - top) as u32 / (right - left).max(1) as u32) as u16,
    );
    if !symbol || ink_h < least {
        words.draw(canvas, env, lines, align, look.ink);
        return;
    }
    let size = (u32::from(ink_h) * 1000 / (bottom - top).max(1) as u32) as u16;
    let unit = |v: i32| (i32::from(size) * v / 1000) as i32;
    let ink_w = unit(right - left);
    let total = ink_h + gap + words_h;
    let y = area.y + area.height.saturating_sub(total) / 2;
    let x = align.x(area, ink_w as f32) as i32;
    // The square placed so that its ink, not the square, sits there.
    let at = Rect::new(
        (x - unit(left)).max(0) as u16,
        (i32::from(y) - unit(top)).max(0) as u16,
        size,
        size,
    );
    symbol::draw(
        canvas,
        at,
        Symbol::PartlyCloudy,
        true,
        Style::Line,
        look.faint,
    );
    let rest = Rect::new(area.x, y + ink_h + gap, area.width, words_h);
    words.draw(canvas, env, rest, align, look.ink);
}
