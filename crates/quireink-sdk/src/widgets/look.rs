//! A look widgets share, so that widgets of different plugins sit well
//! together on a page and the reader shapes them all alike: a frame
//! around a widget ([`Frame`]: none, a line, a light card, a dark card
//! with light text), how large its text is ([`Size`]), a small caption
//! above it ([`draw_caption`]), and switches for what it shows
//! ([`part_setting`]). Each is a setting in the widget's settings group
//! ([`settings_group`](super::settings_group)), named after the widget:
//! `<widget>.frame`, `<widget>.size`, `<widget>.<part>`.
//!
//! A widget draws its frame first and then its content into the area the
//! frame leaves, in the frame's colours:
//!
//! ```ignore
//! fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
//!     let look = widgets::draw_frame(canvas, env, ID, area);
//!     let size = widgets::size(env, ID).text(&UiStyle::new(env));
//!     // ... text in look.ink, notes in look.faint, inside look.area
//! }
//! ```

use alloc::borrow::Cow;
use alloc::format;

use quireink_graphics::Canvas;
use quireink_platform::{Color, Rect};

use crate::env::Env;
use crate::id::Id;
use crate::settings::{Choices, SettingKind, SettingSpec, SettingValue};
use crate::ui::{self, UiStyle};

/// The values of a widget's frame setting ([`frame_setting`]).
pub const FRAMES: [&str; 4] = [
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];

/// What a widget is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// Nothing: the widget on the page itself.
    None,
    /// A line round it, with the style's corners.
    Line,
    /// A light card behind it (dithered where the panel has few grays).
    Card,
    /// A dark card, the widget's text light on it.
    Dark,
}

impl Frame {
    /// The frame a value of [`FRAMES`] names; none for others.
    pub fn of(value: &str) -> Self {
        match FRAMES.iter().position(|f| *f == value) {
            Some(1) => Frame::Line,
            Some(2) => Frame::Card,
            Some(3) => Frame::Dark,
            _ => Frame::None,
        }
    }

    /// Its value in [`FRAMES`].
    pub const fn value(self) -> &'static str {
        FRAMES[self as usize]
    }
}

/// The values of a widget's size setting ([`size_setting`]).
pub const SIZES: [&str; 4] = [
    "core.size.small",
    "core.size.medium",
    "core.size.large",
    "core.size.fit",
];

/// [`SIZES`] but "as large as fits".
const SIZES_SET: [&str; 3] = [SIZES[0], SIZES[1], SIZES[2]];

/// The sizes offered: with `fit`, also "as large as fits".
const fn sizes(fit: bool) -> &'static [&'static str] {
    if fit { &SIZES } else { &SIZES_SET }
}

/// How large a widget's text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
    /// As large as its place allows: the widget works it out.
    Fit,
}

impl Size {
    /// The size a value of [`SIZES`] names; medium for others.
    pub fn of(value: &str) -> Self {
        match SIZES.iter().position(|s| *s == value) {
            Some(0) => Size::Small,
            Some(2) => Size::Large,
            Some(3) => Size::Fit,
            _ => Size::Medium,
        }
    }

    /// Its value in [`SIZES`].
    pub const fn value(self) -> &'static str {
        SIZES[self as usize]
    }

    /// The text size in pixels: the body's, a title's, or larger (also
    /// for [`Fit`](Size::Fit), where a widget has nothing better).
    pub fn text(self, style: &UiStyle) -> f32 {
        match self {
            Size::Small => style.text,
            Size::Medium => style.title,
            Size::Large | Size::Fit => style.title * 1.6,
        }
    }
}

/// A widget's place inside its frame, and the colours to draw in there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    /// Where its content goes.
    pub area: Rect,
    /// Text and lines.
    pub ink: Color,
    /// Text that matters less (captions, notes).
    pub faint: Color,
    /// What lies behind it: white, or black on a dark card.
    pub paper: Color,
    pub frame: Frame,
}

/// The id of `widget`'s setting `key`: `<widget>.<key>`.
pub fn setting_id(widget: &Id, key: &str) -> Id {
    Id::parse(&format!("{widget}.{key}")).unwrap_or_else(|_| widget.clone())
}

/// The setting of what `widget` is drawn in, `<widget>.frame`, in the
/// widget's settings group; [`frame`] reads it.
pub fn frame_setting(widget: &Id, default: Frame) -> SettingSpec {
    choice(
        widget,
        "frame",
        "core.setting.frame",
        &FRAMES,
        default.value(),
    )
}

/// [`frame_setting`] kept in the program ([`SettingSpec::fixed`]), for a
/// `static` list of the widget's settings: `id` must be
/// `<widget>.frame`.
pub const fn frame_spec(id: &'static str, widget: Id, default: Frame) -> SettingSpec {
    SettingSpec::fixed_choice(id, "core.setting.frame", &FRAMES, default.value(), widget)
}

/// [`size_setting`] kept in the program ([`SettingSpec::fixed`]): `id`
/// must be `<widget>.size`.
pub const fn size_spec(id: &'static str, widget: Id, default: Size, fit: bool) -> SettingSpec {
    SettingSpec::fixed_choice(id, "core.setting.size", sizes(fit), default.value(), widget)
}

/// [`part_setting`] kept in the program ([`SettingSpec::fixed`]): `id`
/// must be `<widget>.<part>`.
pub const fn part_spec(
    id: &'static str,
    widget: Id,
    title: &'static str,
    default: bool,
) -> SettingSpec {
    SettingSpec::fixed_switch(id, title, default, widget)
}

/// What `widget` is drawn in, by its [`frame_setting`]; none without one.
pub fn frame(env: &Env<'_>, widget: &str) -> Frame {
    Frame::of(env.settings.text(&format!("{widget}.frame")))
}

/// The setting of how large `widget`'s text is, `<widget>.size`, in the
/// widget's settings group; with `fit`, also "as large as fits". [`size`]
/// reads it.
pub fn size_setting(widget: &Id, default: Size, fit: bool) -> SettingSpec {
    choice(
        widget,
        "size",
        "core.setting.size",
        sizes(fit),
        default.value(),
    )
}

/// How large `widget`'s text is, by its [`size_setting`]; medium without
/// one.
pub fn size(env: &Env<'_>, widget: &str) -> Size {
    Size::of(env.settings.text(&format!("{widget}.size")))
}

/// A switch for a part of what `widget` shows, `<widget>.<part>`, in the
/// widget's settings group; [`shows`] reads it.
pub fn part_setting(
    widget: &Id,
    part: &str,
    title: impl Into<Cow<'static, str>>,
    default: bool,
) -> SettingSpec {
    SettingSpec::new(
        setting_id(widget, part),
        title,
        SettingKind::Bool,
        SettingValue::Bool(default),
    )
    .group(widget.clone())
}

/// Whether `widget` shows `part` ([`part_setting`]).
pub fn shows(env: &Env<'_>, widget: &str, part: &str) -> bool {
    env.settings.bool(&format!("{widget}.{part}"))
}

/// A choice of `values` for `widget`, `<widget>.<key>`, in the widget's
/// settings group.
pub fn choice(
    widget: &Id,
    key: &str,
    title: impl Into<Cow<'static, str>>,
    values: &'static [&'static str],
    default: &'static str,
) -> SettingSpec {
    SettingSpec::new(
        setting_id(widget, key),
        title,
        SettingKind::Choice(Choices::Fixed(values)),
        SettingValue::Text(Cow::Borrowed(default)),
    )
    .group(widget.clone())
}

/// How far a frame's content keeps from its edges.
fn padding(style: &UiStyle, frame: Frame) -> u16 {
    match frame {
        Frame::None => 0,
        _ => (style.gap + style.line * 2) as u16,
    }
}

/// `widget`'s place inside its frame in `area` and the colours there,
/// without drawing (e.g. for its targets).
pub fn look(env: &Env<'_>, widget: &str, area: Rect) -> Look {
    let style = UiStyle::new(env);
    let frame = frame(env, widget);
    let pad = padding(&style, frame);
    let inner = Rect::new(
        area.x + pad,
        area.y + pad,
        area.width.saturating_sub(2 * pad),
        area.height.saturating_sub(2 * pad),
    );
    let (ink, faint, paper) = match frame {
        Frame::Dark => (
            Color::WHITE,
            if style.strong {
                Color::WHITE
            } else {
                Color::gray(200)
            },
            Color::BLACK,
        ),
        _ => (Color::BLACK, style.faint(90), Color::WHITE),
    };
    Look {
        area: inner,
        ink,
        faint,
        paper,
        frame,
    }
}

/// Draws `widget`'s frame in `area` (cleared before) and returns where
/// its content goes and in which colours.
pub fn draw_frame(canvas: &mut Canvas<'_>, env: &Env<'_>, widget: &str, area: Rect) -> Look {
    let style = UiStyle::new(env);
    let look = look(env, widget, area);
    match look.frame {
        Frame::None => {}
        Frame::Line => ui::stroke_round(
            canvas,
            &style,
            area,
            ui::button_border(&style),
            Color::BLACK,
        ),
        Frame::Card => canvas.fill_round_shaded(area, style.corner, Color::gray(225)),
        Frame::Dark => ui::fill_round(canvas, &style, area, Color::BLACK),
    }
    look
}

/// The height of a line of text `size` pixels large in the UI font.
pub fn line_height(env: &Env<'_>, size: f32) -> u16 {
    UiStyle::new(env)
        .font(env)
        .map_or(size as u16, |f| f.line_metrics(size).line_height as u16)
}

/// The height a caption takes ([`draw_caption`]), the gap below it
/// included.
pub fn caption_height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small) + style.gap as u16 / 2
}

/// Draws `text` small at the top of `look`'s area, placed as `widget`'s
/// alignment says ([`align`](super::align)), e.g. "Today" over the
/// numbers; returns the area left below it.
pub fn draw_caption(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &Look,
    text: &str,
) -> Rect {
    let style = UiStyle::new(env);
    let area = look.area;
    let height = caption_height(env).min(area.height);
    if let Some(font) = style.font(env) {
        let text = ui::fit(&font, style.small, text, f32::from(area.width));
        let x = super::align(env, widget).x(area, font.measure(style.small, &text));
        let row = Rect::new(area.x, area.y, area.width, line_height(env, style.small));
        ui::draw_label(canvas, &font, style.small, x, row, &text, look.faint);
    }
    Rect::new(area.x, area.y + height, area.width, area.height - height)
}

/// Draws `text` `size` pixels large in `area`, broken into lines where
/// it is wider, each line placed as `align` says, together in the
/// middle from top to bottom; as many lines as fit, the last one
/// shortened with "…" where there is more. Returns the height drawn.
#[allow(clippy::too_many_arguments)]
pub fn draw_wrapped(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    align: super::Align,
    size: f32,
    text: &str,
    color: Color,
) -> u16 {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return 0;
    };
    let width = f32::from(area.width);
    let line = f32::from(line_height(env, size)).max(1.0);
    let mut lines = font.wrap(size, text, width);
    let fits = ((f32::from(area.height) / line) as usize).max(1);
    let cut = lines.len() > fits;
    lines.truncate(fits);
    let count = lines.len();
    let total = (line * count as f32) as u16;
    let top = area.y + area.height.saturating_sub(total) / 2;
    // Told as one text, drawn as lines.
    let at = Rect::new(area.x, top, area.width, total);
    canvas.describe(quireink_graphics::Role::Text, text, at, false, None);
    let metrics = font.line_metrics(size);
    for (n, range) in lines.into_iter().enumerate() {
        let shown = if cut && n + 1 == count {
            let rest = text[range.start..].trim_end();
            ui::fit(&font, size, &format!("{rest} …"), width)
        } else {
            alloc::string::String::from(text[range].trim_end())
        };
        let y = f32::from(top) + line * n as f32;
        let baseline = y + (line + metrics.ascent - metrics.descent) / 2.0;
        let x = align.x(area, font.measure(size, &shown));
        font.draw(canvas, size, x, baseline as i32, &shown, color);
    }
    total
}

/// Draws `text` in the body size in the middle of `look`'s area, e.g.
/// what a widget would show is not there yet ("No words saved yet").
pub fn draw_message(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, text: &str) {
    let style = UiStyle::new(env);
    let (area, center) = (look.area, super::Align::Center);
    draw_wrapped(canvas, env, area, center, style.text, text, look.ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_sizes_round_trip() {
        for frame in [Frame::None, Frame::Line, Frame::Card, Frame::Dark] {
            assert_eq!(Frame::of(frame.value()), frame);
        }
        for size in [Size::Small, Size::Medium, Size::Large, Size::Fit] {
            assert_eq!(Size::of(size.value()), size);
        }
        assert_eq!(Frame::of("core.frame.cloud"), Frame::None);
        assert_eq!(Size::of(""), Size::Medium);
    }

    #[test]
    fn settings_belong_to_the_widget() {
        let widget = Id::new_static("pack.clock");
        let frame = frame_setting(&widget, Frame::Card);
        assert_eq!(frame.id.as_str(), "pack.clock.frame");
        assert_eq!(frame.group, widget);
        let size = size_setting(&widget, Size::Fit, false);
        assert_eq!(size.id.as_str(), "pack.clock.size");
        // Without "fit", it is not offered.
        assert!(matches!(&size.kind, SettingKind::Choice(v) if v.len() == 3));
        let part = part_setting(&widget, "date", "pack.date", true);
        assert_eq!(part.id.as_str(), "pack.clock.date");
        assert_eq!(part.default, SettingValue::Bool(true));
    }
}
