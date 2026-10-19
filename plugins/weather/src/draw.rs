//! What both widgets draw alike: their settings of the symbols, the
//! temperature's digits placed by their ink, what they say while there is
//! no weather.

use alloc::format;
use alloc::string::String;

use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Look};
use quireink_sdk::{Canvas, Color, Env, FontHandle, Id, Rect, SettingSpec, time};

use crate::data::Symbol;
use crate::symbol::{self, Style};
use crate::{Shown, Waiting};

/// The values of a widget's symbol style setting.
pub(crate) const STYLES: [&str; 2] = ["weather.style.line", "weather.style.solid"];

/// The setting of how `widget` draws its symbols.
pub(crate) fn style_setting(widget: &Id) -> SettingSpec {
    widgets::choice(widget, "style", "weather.setting.style", &STYLES, STYLES[0])
}

/// How `widget` draws its symbols.
pub(crate) fn style(env: &Env<'_>, widget: &str) -> Style {
    if env.settings.text(&format!("{widget}.style")) == STYLES[1] {
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
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    row: Rect,
    text: &str,
    color: Color,
) {
    let metrics = font.line_metrics(size);
    let baseline = f32::from(row.y) + (f32::from(row.height) + cap(size)) / 2.0;
    // `draw_label` centres the font's line in its area: an area whose
    // baseline falls where the digits' does.
    let top = baseline - metrics.ascent;
    let area = Rect::new(
        row.x,
        top.max(0.0) as u16,
        row.width,
        (metrics.ascent + metrics.descent) as u16,
    );
    quireink_sdk::ui::draw_label(canvas, font, size, x, area, text, color);
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

/// A time of day (minutes after midnight) as the clock shows it.
pub(crate) fn clock(env: &Env<'_>, minutes: u16) -> String {
    let (hour, minute) = (minutes / 60, minutes % 60);
    let twelve = env
        .settings
        .read(&quireink_sdk::builtin::settings::CLOCK_FORMAT)
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
    env.i18n
        .format(key, &[("time", quireink_sdk::Arg::Str(&text))])
}

/// Draws what `widget` says while it has no weather: why, under a faint
/// symbol where there is room.
pub(crate) fn draw_waiting(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &Look,
    shown: &Shown,
) {
    let style = UiStyle::new(env);
    let area = look.area;
    let text = match shown.waiting {
        // The place is set: its name with the message.
        Waiting::NoNetwork if !shown.place.is_empty() => env.i18n.format(
            "weather.no-network-for",
            &[("place", quireink_sdk::Arg::Str(&shown.place))],
        ),
        waiting => String::from(env.i18n.tr(waiting.key())),
    };
    let line = widgets::line_height(env, style.text);
    // The message's lines, as many as it needs where they fit.
    let lines = style
        .font(env)
        .map_or(1, |f| {
            f.wrap(style.text, &text, f32::from(area.width)).len()
        })
        .clamp(1, 3) as u16;
    let words = line * lines;
    let room = area.height.saturating_sub(words + style.gap as u16);
    let size = room.min(area.width / 2).min(line * 3);
    if size >= line * 3 / 2 {
        let total = size + style.gap as u16 + words;
        let top = area.y + area.height.saturating_sub(total) / 2;
        let x = widgets::align(env, widget).x(area, f32::from(size)) as u16;
        let at = Rect::new(x, top, size, size);
        symbol::draw(
            canvas,
            at,
            Symbol::PartlyCloudy,
            true,
            Style::Line,
            look.faint,
        );
        let below = top + size + style.gap as u16;
        let rest = Rect::new(area.x, below, area.width, words);
        widgets::draw_wrapped(
            canvas,
            env,
            rest,
            widgets::align(env, widget),
            style.text,
            &text,
            look.ink,
        );
    } else {
        widgets::draw_message(canvas, env, look, &text);
    }
}
