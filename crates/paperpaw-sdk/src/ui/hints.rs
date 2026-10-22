//! The row of hints below the screen: what the buttons under it do.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The height of the row of hints below the screen (see
/// [`draw_button_hints`]).
pub fn button_hints_height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    let line = style.font(env).map_or(style.small * 1.3, |f| {
        f.line_metrics(style.small).line_height
    });
    ((line * 1.5) as u32 + style.line).max(env.metrics.mm(4)) as u16
}

/// PaperPaw's paw: the busy mark in a corner of the screen.
pub const PAW_ICON: &str = "paw";

/// The mark beside a button's label in the row of hints where holding
/// the button does something too.
#[deprecated(
    since = "0.4.0",
    note = "use `icons::HOLD`, which a theme may draw another way"
)]
pub const HOLD_ICON: &str = "hand-click";

/// A hint's words below its button, in `cell` (its share of the row),
/// reaching over `span` (its share and the room beside it): too long for
/// `room` (its share less a gap at each side), over the room beside it,
/// then closer to its share's edges (`own`), then smaller, down to six
/// tenths, then on two lines; only then shortened. Always as near its
/// button (`middle`) as it can be.
fn draw_hint_text(
    canvas: &mut Canvas<'_>,
    font: &crate::fonts::FontHandle<'_>,
    style: &UiStyle,
    (cell, span, middle): (Rect, Rect, f32),
    (room, own): (f32, f32),
    text: &str,
    ink: Color,
) {
    let near = |w: f32| {
        let least = f32::from(span.x);
        let most = (f32::from(span.x) + f32::from(span.width) - w).max(least);
        (middle - w / 2.0).clamp(least, most)
    };
    let width = font.measure(style.small, text);
    if width > room + 1.0 && width <= f32::from(span.width) + 1.0 {
        draw_label(canvas, font, style.small, near(width), span, text, ink);
        return;
    }
    let room = own.max(f32::from(span.width));
    let least = style.small * 0.6;
    let size = crate::widgets::largest_size(style.small, least, |size| {
        font.measure(size, text) <= room + 1.0
    })
    .unwrap_or(least);
    // Two lines set close: the row is a line and a half high.
    let metrics = font.line_metrics(size);
    let line = (metrics.ascent + metrics.descent).min(metrics.line_height);
    let lines = font.wrap(size, text, room.max(1.0));
    if lines.len() == 2 && 2.0 * line <= f32::from(cell.height) {
        let mut y = f32::from(cell.y) + (f32::from(cell.height) - 2.0 * line) / 2.0;
        for range in lines {
            let part = text[range].trim();
            let at = Rect::new(span.x, y as u16, span.width, line as u16);
            draw_label(
                canvas,
                font,
                size,
                near(font.measure(size, part)),
                at,
                part,
                ink,
            );
            y += line;
        }
        return;
    }
    let text = fit(font, size, text, room + 1.0);
    draw_label(
        canvas,
        font,
        size,
        near(font.measure(size, &text)),
        span,
        &text,
        ink,
    );
}

/// Where the row's hints show something, each from its start to its end
/// (none for those that show nothing): their own ink, as wide as their
/// share lets it be, around its middle.
fn reaches(env: &Env<'_>, style: &UiStyle, area: Rect, hints: &[Hint]) -> Vec<Option<(u32, u32)>> {
    let count = hints.len() as u32;
    let font = style.font(env);
    hints
        .iter()
        .enumerate()
        .map(|(i, hint)| {
            let (i, width) = (i as u32, u32::from(area.width));
            let left = u32::from(area.x) + width * i / count;
            let right = u32::from(area.x) + width * (i + 1) / count;
            let room = (right - left).saturating_sub(2 * style.gap) as f32;
            let ink = match hint {
                Hint::Text(text) => font.as_ref()?.measure(style.small, text).min(room),
                Hint::Icon(name) => icon_width(env, name, style.text),
                Hint::Default | Hint::Hidden => return None,
            };
            let middle = (left + right) as f32 / 2.0;
            Some(((middle - ink / 2.0) as u32, (middle + ink / 2.0) as u32))
        })
        .collect()
}

/// From where to where hint `index` may show its words: its share, and
/// beside it up to a gap before what the hints beside it show (all of
/// their shares where they show nothing), within the row.
fn free_span(
    reach: &[Option<(u32, u32)>],
    index: usize,
    area: Rect,
    style: &UiStyle,
) -> (u32, u32) {
    let gap = style.gap;
    let start = reach[..index]
        .iter()
        .rev()
        .find_map(|r| r.map(|(_, end)| end + gap))
        .unwrap_or(u32::from(area.x) + gap / 2);
    let end = reach[index + 1..]
        .iter()
        .find_map(|r| r.map(|(start, _)| start.saturating_sub(gap)))
        .unwrap_or(u32::from(area.x) + u32::from(area.width) - gap / 2);
    (start, end.max(start))
}

/// The row of hints below the screen: a line above it, then what each
/// button under it does, centred in an equal share of the width, as the
/// buttons lie. The hint of the button `pressed` is drawn inverted, as a
/// sign that its press is being worked on.
pub fn draw_button_hints(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    hints: &[Hint],
    pressed: Option<usize>,
) {
    let style = UiStyle::new(env);
    let line = style.line.min(u32::from(area.height));
    let painted = style
        .look
        .hints(canvas, &style, super::look::HintsPart { area });
    if !painted {
        canvas.fill(area, Color::WHITE);
    }
    match style.separators {
        _ if painted => {}
        Separators::Line => canvas.fill_rect(
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width),
            line,
            Color::BLACK,
        ),
        Separators::Inset => canvas.fill_tint(
            Rect::new(
                area.x + style.margin as u16,
                area.y,
                area.width.saturating_sub(2 * style.margin as u16),
                line as u16,
            ),
            style.ink(Ink::Rule),
        ),
        Separators::None => {}
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let count = hints.len() as u32;
    let row = Rect::new(
        area.x,
        area.y + line as u16,
        area.width,
        area.height - line as u16,
    );
    let reach = reaches(env, &style, area, hints);
    for (i, hint) in hints.iter().enumerate() {
        let (i, width) = (i as u32, u32::from(area.width));
        let left = u32::from(area.x) + width * i / count;
        let right = u32::from(area.x) + width * (i + 1) / count;
        let cell = Rect::new(left as u16, row.y, (right - left) as u16, row.height);
        let room = (right - left).saturating_sub(2 * style.gap) as f32;
        let middle = left as f32 + (right - left) as f32 / 2.0;
        let ink = if pressed == Some(i as usize) {
            let pressed = cell.inset(style.gap as u16 / 2, style.gap as u16 / 4);
            let radius = style.radius(u32::from(pressed.height));
            canvas.fill_round(pressed, radius, Color::BLACK);
            Color::WHITE
        } else {
            Color::BLACK
        };
        // The label alone, in the middle: no mark beside it.
        match hint {
            Hint::Text(text) => {
                let (from, to) = free_span(&reach, i as usize, area, &style);
                let span = Rect::new(from as u16, row.y, (to - from) as u16, row.height);
                let own = room.max((right - left).saturating_sub(style.gap / 2) as f32);
                draw_hint_text(
                    canvas,
                    &font,
                    &style,
                    (cell, span, middle),
                    (room, own),
                    text,
                    ink,
                );
            }
            Hint::Icon(name) => {
                let width = icon_width(env, name, style.text);
                draw_icon(
                    canvas,
                    env,
                    name,
                    style.text,
                    middle - width / 2.0,
                    cell,
                    ink,
                );
            }
            Hint::Default | Hint::Hidden => {}
        }
    }
}
