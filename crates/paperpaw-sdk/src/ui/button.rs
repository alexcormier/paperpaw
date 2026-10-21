//! Buttons drawn in rows and dialogs.

#[allow(clippy::wildcard_imports)]
use super::*;

/// The height of buttons in a row of height `row.height`: the `−` and `+`
/// of numbers, the buttons of panels. One size everywhere: seven tenths
/// of the row, and on touch screens at least as large as a fingertip
/// ([`UiStyle::finger`]), no higher than the row (rows made with
/// [`UiStyle::touch_row`] keep room around them).
pub fn button_height(style: &UiStyle, row: Rect) -> u16 {
    let height = u32::from(row.height);
    (height * 7 / 10).max(style.finger).min(height) as u16
}

/// The height of a bar of buttons along the top or bottom edge of the
/// screen: its buttons as in a row made with [`UiStyle::touch_row`], the
/// room of such a row toward the content, and toward the edge the margin
/// the screen's sides have, so the buttons keep it from every edge.
pub fn edge_bar_height(style: &UiStyle) -> u16 {
    let row = style.touch_row(style.row) as u16;
    let button = button_height(style, Rect::new(0, 0, 0, row));
    let room = (row - button) / 2;
    button + room + style.margin as u16
}

/// Where the buttons of `bar`, a bar along the screen's bottom edge (or
/// its top, `at_top`) made [`edge_bar_height`] high, go: a band as high as
/// they are, the side margin from the edge (see [`edge_bar_height`]).
pub fn edge_bar_band(style: &UiStyle, bar: Rect, at_top: bool) -> Rect {
    let row = style.touch_row(style.row) as u16;
    let button = button_height(style, Rect::new(0, 0, 0, row));
    let margin = style.margin as u16;
    let y = if at_top {
        bar.y + margin
    } else {
        (bar.y + bar.height).saturating_sub(margin + button)
    };
    Rect::new(bar.x, y, bar.width, button)
}

/// The room to leave between a row of buttons `row` high and the edge of
/// the screen it lies along, so that its buttons, centred in it, keep the
/// side margin from that edge as well.
pub fn edge_room(style: &UiStyle, row: u16) -> u16 {
    let button = button_height(style, Rect::new(0, 0, 0, row));
    (style.margin as u16).saturating_sub((row - button) / 2)
}

/// The width of buttons' and boxes' borders.
pub fn button_border(style: &UiStyle) -> u32 {
    style.line.max(2)
}

/// A button: outlined, or filled when `on`; `focused` (chosen with
/// buttons) draws a frame around it. Labels of one character (e.g. `+`)
/// are set larger.
#[allow(clippy::too_many_arguments)]
pub fn draw_button(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    label: &str,
    on: bool,
    focused: bool,
) {
    canvas.describe(Role::Button, label, rect, focused, on.then_some(true));
    let Some(ink) = button_frame(canvas, style, rect, on, focused) else {
        return;
    };
    let Some(font) = style.font(env) else {
        return;
    };
    button_label(canvas, style, &font, rect, label, ink);
}

/// A button with a drawing of its own instead of a word (an arrow, a
/// sign), drawn empty for the caller to draw on with the ink it returns;
/// `name` says what it does, for screen readers and remote control.
pub fn draw_icon_button(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    name: &str,
    on: bool,
    focused: bool,
) -> Color {
    canvas.describe(Role::Button, name, rect, focused, on.then_some(true));
    button_frame(canvas, style, rect, on, focused).unwrap_or(Color::BLACK)
}

/// A button's background, border and focus frame; the ink for what goes
/// on it.
pub(super) fn button_frame(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    rect: Rect,
    on: bool,
    focused: bool,
) -> Option<Color> {
    let part = super::look::ButtonPart { rect, on, focused };
    if let Some(ink) = style.look.button(canvas, style, part) {
        return Some(ink);
    }
    let (back, ink) = if on {
        (Color::BLACK, Color::WHITE)
    } else {
        (Color::WHITE, Color::BLACK)
    };
    let border = button_border(style);
    let r = style.radius(u32::from(rect.height.min(rect.width)));
    let (x, y, w, h) = (
        i32::from(rect.x),
        i32::from(rect.y),
        u32::from(rect.width),
        u32::from(rect.height),
    );
    if r > 0 {
        // Round: the corners outside keep what is behind.
        canvas.fill_round_rect(x, y, w, h, r, back);
    } else {
        canvas.fill(rect, back);
    }
    canvas.stroke_round_rect(x, y, w, h, border, r, Color::BLACK);
    if focused {
        let out = (border * 2) as i32;
        canvas.stroke_round_rect(
            x - out,
            y - out,
            w + 2 * out as u32,
            h + 2 * out as u32,
            border,
            r + out as u32,
            Color::BLACK,
        );
    }
    Some(ink)
}

/// A button's word, centred and shortened to fit.
pub(super) fn button_label(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    rect: Rect,
    label: &str,
    ink: Color,
) {
    let single = label.chars().count() == 1;
    let mut size = if single { style.text } else { style.small };
    // A single character wider than the button (an `m` on a narrow key)
    // is set smaller rather than shortened to nothing.
    if single {
        let width = font.measure(size, label);
        let room = f32::from(rect.width) * 0.8;
        if width > room && width > 0.0 {
            size *= room / width;
        }
    }
    // A single character (− or +) is never shortened; narrow buttons (a
    // keyboard's keys) keep less room at their sides, so short words fit.
    let pad = (style.gap as f32).min(f32::from(rect.width) / 12.0);
    let room = if single {
        f32::from(rect.width)
    } else {
        f32::from(rect.width) - 2.0 * pad
    };
    let label = fit(font, size, label, room);
    let label_x = f32::from(rect.x) + (f32::from(rect.width) - font.measure(size, &label)) / 2.0;
    draw_label(canvas, font, size, label_x, rect, &label, ink);
}
