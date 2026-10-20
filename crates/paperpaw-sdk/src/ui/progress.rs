//! A bar that shows how far something has come (a download, an update
//! being installed): a track with the style's corners, filled from its
//! start.

#[allow(clippy::wildcard_imports)]
use super::*;

/// Draws a progress bar in `area`, `permille` thousandths done (more than
/// 1000 counts as all): an outline in `ink` with the style's corners,
/// filled inside it as far as it has come. The fill follows the track's
/// rounded ends at every value, a sliver at the start or all of it, so no
/// pixel of it lies outside the track.
pub fn draw_progress(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    area: Rect,
    permille: u16,
    ink: Color,
) {
    let part = super::look::ProgressPart {
        area,
        permille: permille.min(1000),
        ink,
    };
    if style.look.progress(canvas, style, part) {
        return;
    }
    let border = button_border(style);
    let radius = style.radius(u32::from(area.height));
    canvas.stroke_round(area, border, radius, ink);
    // The fill: the track's inside, a gap of a border width all round,
    // with corners as much smaller.
    let inset = border * 2;
    let inner = Rect::new(
        area.x.saturating_add(inset as u16),
        area.y.saturating_add(inset as u16),
        area.width.saturating_sub(2 * inset as u16),
        area.height.saturating_sub(2 * inset as u16),
    );
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let done = u32::from(permille.min(1000));
    let filled = (u32::from(inner.width) * done / 1000) as u16;
    if filled == 0 {
        return;
    }
    // Drawn whole and cut where it has come: its ends round as the track's.
    let inner_radius = radius.saturating_sub(inset);
    let mut cut = canvas.clipped(Rect::new(inner.x, inner.y, filled, inner.height));
    cut.fill_round(inner, inner_radius, ink);
}
