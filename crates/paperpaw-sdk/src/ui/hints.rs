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

/// PaperPaw's paw: the busy mark in a corner of the screen.
pub const PAW_ICON: &str = "paw";

/// The mark beside a button's label in the row of hints where holding
/// the button does something too.
#[deprecated(
    since = "0.4.0",
    note = "use `icons::HOLD`, which a theme may draw another way"
)]
pub const HOLD_ICON: &str = "hand-click";

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
                let text = fit(&font, style.small, text, room);
                let width = font.measure(style.small, &text);
                let x = middle - width / 2.0;
                draw_label(canvas, &font, style.small, x, cell, &text, ink);
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
