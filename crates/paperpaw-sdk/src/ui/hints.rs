//! The row of hints below the screen: what the buttons under it do.

#[allow(clippy::wildcard_imports)]
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

/// The row of hints below the screen: a line above it, then what each
/// button under it does, centred in an equal share of the width, as the
/// buttons lie. The hint of the button `pressed` is drawn inverted, as a
/// sign that its press is being worked on.
/// The mark beside a button's label in the row of hints where holding
/// the button does something too ([`draw_button_hints`]).
pub const HOLD_ICON: &str = "hand-click";

/// PaperPaw's paw: on empty screens ([`draw_empty`]) and as the busy
/// mark, also in the hint of a button whose press is being worked on.
pub const PAW_ICON: &str = "paw";

pub fn draw_button_hints(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    hints: &[Hint],
    held: &[bool],
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
    for (i, hint) in hints.iter().enumerate() {
        let (i, width) = (i as u32, u32::from(area.width));
        let left = u32::from(area.x) + width * i / count;
        let right = u32::from(area.x) + width * (i + 1) / count;
        let cell = Rect::new(left as u16, row.y, (right - left) as u16, row.height);
        let mut room = (right - left).saturating_sub(2 * style.gap) as f32;
        let middle = left as f32 + (right - left) as f32 / 2.0;
        let ink = if pressed == Some(i as usize) {
            let pressed = cell.inset(style.gap as u16 / 2, style.gap as u16 / 4);
            let radius = style.radius(u32::from(pressed.height));
            canvas.fill_round(pressed, radius, Color::BLACK);
            Color::WHITE
        } else {
            Color::BLACK
        };
        // Holding the button does something too: a small hand beside its
        // label, the two together in the middle; while its press is being
        // worked on, the paw in its place.
        let busy = pressed == Some(i as usize);
        let hold = (busy || held.get(i as usize).copied().unwrap_or(false))
            && matches!(hint, Hint::Text(_) | Hint::Icon(_));
        let sign = if busy { PAW_ICON } else { HOLD_ICON };
        // Smaller than the label, close to it, the cell's margins halved:
        // the label keeps its room (German words are long).
        let mark_size = style.small * 0.8;
        let mark = if hold {
            room += style.gap as f32;
            icon_width(env, sign, mark_size) + style.gap as f32 / 4.0
        } else {
            0.0
        };
        let end = match hint {
            Hint::Text(text) => {
                let text = fit(&font, style.small, text, room - mark);
                let width = font.measure(style.small, &text);
                let x = middle - (width + mark) / 2.0;
                draw_label(canvas, &font, style.small, x, cell, &text, ink);
                x + width
            }
            Hint::Icon(name) => {
                let width = icon_width(env, name, style.text);
                let x = middle - (width + mark) / 2.0;
                draw_icon(canvas, env, name, style.text, x, cell, ink);
                x + width
            }
            Hint::Default | Hint::Hidden => continue,
        };
        if hold {
            let x = end + style.gap as f32 / 4.0;
            draw_icon(canvas, env, sign, mark_size, x, cell, ink);
        }
    }
}
