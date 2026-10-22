//! An address to open on a phone (a page served on the local network, see
//! [`served`](crate::served)): as a QR code, as text, and a hint.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// Lines of the hint below the code, at most.
pub const ADDRESS_HINT_LINES: usize = 5;

/// `url` as a QR code (`qr`, if it could be made) as large as fits at the
/// top of `area`, below it the address as text (on two lines where it is
/// too long for one, to be typed), `hint` (up to [`ADDRESS_HINT_LINES`]
/// lines) and `last`, what happened last; all centred.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_address(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    url: &str,
    qr: Option<&crate::qr::QrCode>,
    hint: &str,
    last: Option<&str>,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let margin = style.margin as u16;
    let line = font.line_metrics(style.text).line_height as u16;
    let small = font.line_metrics(style.small).line_height as u16;
    let width = f32::from(area.width.saturating_sub(2 * margin));
    // The address whole, to be typed: on two lines (the host, then the
    // rest) if it is too long for one.
    let shown = url.strip_prefix("http://").unwrap_or(url);
    let address: Vec<&str> = match shown.find('/') {
        Some(at) if font.measure(style.text, shown) > width => {
            alloc::vec![&shown[..at], &shown[at..]]
        }
        _ => alloc::vec![shown],
    };
    let hint_lines = font.wrap(style.small, hint, width);
    let text_height = line * address.len() as u16
        + small * (hint_lines.len().min(ADDRESS_HINT_LINES) as u16 + 1)
        + style.gap as u16 * 3;
    let room = area.height.saturating_sub(text_height + style.gap as u16);
    let side = room.min(area.width.saturating_sub(2 * margin));
    let mut y = area.y;
    if let Some(qr) = qr
        && draw_qr(canvas, qr, Rect::new(area.x, y, area.width, side)).is_some()
    {
        y += side;
    }
    y += style.gap as u16;
    let row = |y: u16, height: u16| {
        Rect::new(
            area.x + margin,
            y,
            area.width.saturating_sub(2 * margin),
            height,
        )
    };
    let centered = |text: &str, size: f32| {
        let text = fit(&font, size, text, width);
        let x = f32::from(area.x + margin) + (width - font.measure(size, &text)) / 2.0;
        (text, x.max(f32::from(area.x + margin)))
    };
    for part in address {
        let (text, x) = centered(part, style.text);
        draw_label(
            canvas,
            &font,
            style.text,
            x,
            row(y, line),
            &text,
            Color::BLACK,
        );
        y += line;
    }
    y += style.gap as u16;
    for range in hint_lines.into_iter().take(ADDRESS_HINT_LINES) {
        let (text, x) = centered(&hint[range], style.small);
        draw_label(
            canvas,
            &font,
            style.small,
            x,
            row(y, small),
            &text,
            style.faint(60),
        );
        y += small;
    }
    y += style.gap as u16;
    if let Some(last) = last {
        let (text, x) = centered(last, style.small);
        draw_label(
            canvas,
            &font,
            style.small,
            x,
            row(y, small),
            &text,
            Color::BLACK,
        );
    }
}
