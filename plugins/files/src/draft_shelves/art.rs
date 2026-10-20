//! How a book looks on a shelf: its cover with its state on it (a bar
//! along the bottom edge while it is read, a check once finished, "New"
//! until it is opened), a series or an author as a little stack of
//! covers, and the tile at the end of a shelf that opens all of it.
//! Everything is drawn from memory: covers made small beforehand.

use alloc::string::ToString;

use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{Canvas, Color, Env, Point, Rect, files};

use super::gather::Tile;
use super::summary::{Item, State};

/// Which marks a cover gets: all of them on shelves; on the small covers
/// of a list, and where every book is new, only the bar and the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Marks {
    All,
    Quiet,
    /// None at all.
    None,
}

/// Where the cover of `tile` is drawn in `place`: a group's cover leaves
/// room above and on the right for the covers stacked behind it.
pub(crate) fn cover_place(style: &UiStyle, tile: &Tile, place: Rect) -> Rect {
    match tile {
        Tile::Book(_) => place,
        Tile::Group(_) => {
            let o = stack_offset(style, place);
            Rect::new(
                place.x,
                place.y + 2 * o,
                place.width.saturating_sub(2 * o),
                place.height.saturating_sub(2 * o),
            )
        }
    }
}

fn stack_offset(style: &UiStyle, place: Rect) -> u16 {
    ((style.gap / 2) as u16).min(place.width / 12).max(2)
}

/// Draws `tile` in `place`: its cover (or a stand-in), a group's stack
/// behind it, and the marks of its state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_tile(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    place: Rect,
    tile: &Tile,
    thumb: Option<&Thumbnail>,
    focused: bool,
    marks: Marks,
) {
    let cover = cover_place(style, tile, place);
    if let Tile::Group(group) = tile {
        // Two covers behind it, up and to the right.
        let o = stack_offset(style, place);
        let radius = crate::cover_radius(style, cover);
        let paper = style.focus_paper(focused);
        let ink = style.focus_ink(focused);
        for n in [2, 1] {
            let back = Rect::new(cover.x + n * o, cover.y - n * o, cover.width, cover.height);
            canvas.fill_round(back, radius, paper);
            canvas.stroke_round(back, style.line.max(1), radius, ink);
        }
        let drawn = draw_cover(canvas, env, style, cover, &group.cover, thumb, focused);
        if marks == Marks::All {
            let count = group.count.to_string();
            badge(canvas, env, style, drawn, &count, Corner::BottomRight);
        }
        return;
    }
    let item = tile.item();
    let drawn = draw_cover(canvas, env, style, cover, item, thumb, focused);
    draw_state(canvas, env, style, drawn, item.state(), marks);
}

/// Draws the cover of `item` at the bottom left of `place` (or a
/// stand-in filling it); returns where it went.
pub(crate) fn draw_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    place: Rect,
    item: &Item,
    thumb: Option<&Thumbnail>,
    focused: bool,
) -> Rect {
    match thumb {
        Some(thumb) => {
            let (w, h) = (thumb.width().min(place.width), thumb.height());
            let y = place.y + place.height.saturating_sub(h);
            thumb.draw_round(canvas, place.x, y, crate::picture_radius(style, place));
            Rect::new(place.x, y, w, h)
        }
        None => {
            let name = files::name(&item.path);
            let format = name
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_uppercase())
                .unwrap_or_default();
            let ink = style.focus_ink(focused);
            canvas.fill_round(
                place,
                crate::cover_radius(style, place),
                style.focus_paper(focused),
            );
            crate::draw_placeholder(canvas, env, style, place, &item.title, &format, ink);
            place
        }
    }
}

/// Draws the marks of `state` on a cover drawn at `cover`.
pub(crate) fn draw_state(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    cover: Rect,
    state: State,
    marks: Marks,
) {
    if marks == Marks::None {
        return;
    }
    match state {
        State::Reading(percent) => progress_bar(canvas, env, style, cover, percent),
        State::Finished => check(canvas, env, style, cover),
        State::New if marks == Marks::All => {
            let new = env.i18n.tr("files.shelves.new");
            badge(canvas, env, style, cover, new, Corner::TopLeft);
        }
        State::New => {}
    }
}

/// How far into a cover its marks sit.
fn inset(style: &UiStyle, cover: Rect) -> u16 {
    let radius = crate::cover_radius(style, cover) as u16;
    (style.line as u16 * 2)
        .max(radius / 2)
        .max(2)
        .min(cover.width / 8)
}

/// A bar along the bottom edge of a cover, filled as far as it is read:
/// on white with a black frame, so it shows on dark covers and light ones
/// in black and white alone.
pub(crate) fn progress_bar(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    cover: Rect,
    percent: u8,
) {
    let inset = inset(style, cover);
    let border = style.line.max(1) as u16;
    let bar = (env.metrics.tenth_mm(9) as u16).clamp(3, cover.height / 6);
    let height = bar + 2 * border;
    let area = Rect::new(
        cover.x + inset,
        (cover.y + cover.height).saturating_sub(inset + height),
        cover.width.saturating_sub(2 * inset),
        height,
    );
    progress_in(canvas, area, border, percent, false);
}

/// A bar filling `area` (with a frame `border` wide) as far as `percent`;
/// white on black where `inverted` (on a dark focus).
pub(crate) fn progress_in(
    canvas: &mut Canvas<'_>,
    area: Rect,
    border: u16,
    percent: u8,
    inverted: bool,
) {
    let (paper, ink) = if inverted {
        (Color::BLACK, Color::WHITE)
    } else {
        (Color::WHITE, Color::BLACK)
    };
    let radius = u32::from(area.height / 2);
    canvas.fill_round(area, radius, paper);
    canvas.stroke_round(area, u32::from(border), radius, ink);
    let inner = area.inset(border, border);
    let filled = (u32::from(inner.width) * u32::from(percent.min(100)) / 100) as u16;
    if filled > 0 {
        let done = Rect::new(inner.x, inner.y, filled.max(inner.height), inner.height);
        canvas.fill_round(done, u32::from(inner.height / 2), ink);
    }
}

/// A round check at the top right of a finished book's cover.
fn check(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, cover: Rect) {
    let inset = inset(style, cover);
    let radius = (env.metrics.mm(2) as u16).min(cover.width / 5).max(5);
    let center = Point::new(
        i32::from(cover.x + cover.width.saturating_sub(inset + radius)),
        i32::from(cover.y + inset + radius),
    );
    let ring = style.line.max(1) * 2;
    canvas.fill_circle(center, u32::from(radius) + ring, Color::WHITE);
    canvas.fill_circle(center, u32::from(radius), Color::BLACK);
    let size = f32::from(radius) * 1.5;
    let width = ui::icon_width(env, "check", size);
    let area = Rect::new(
        cover.x,
        (center.y - i32::from(radius)) as u16,
        cover.width,
        radius * 2,
    );
    let x = center.x as f32 - width / 2.0;
    ui::draw_icon(canvas, env, "check", size, x, area, Color::WHITE);
}

/// Which corner of a cover a badge sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    BottomRight,
}

/// A small white pill with a black frame and `text` on a cover: "New",
/// or how many books a stack has.
fn badge(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    cover: Rect,
    text: &str,
    at: Corner,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let size = style.small * 0.9;
    let line = font.line_metrics(size).line_height as u16;
    let pad = (style.gap as u16 / 2).max(2);
    let width = (font.measure(size, text) as u16 + 2 * pad)
        .max(line)
        .min(cover.width);
    let height = line;
    let inset = inset(style, cover);
    let (x, y) = match at {
        Corner::TopLeft => (cover.x + inset, cover.y + inset),
        Corner::BottomRight => (
            (cover.x + cover.width).saturating_sub(inset + width),
            (cover.y + cover.height).saturating_sub(inset + height),
        ),
    };
    let area = Rect::new(x, y, width, height);
    let radius = u32::from(height / 2);
    canvas.fill_round(area, radius, Color::WHITE);
    canvas.stroke_round(area, style.line.max(1), radius, Color::BLACK);
    let text_x = f32::from(x) + (f32::from(width) - font.measure(size, text)) / 2.0;
    ui::draw_label(canvas, &font, size, text_x, area, text, Color::BLACK);
}

/// The tile at the end of a shelf: a frame as large as a cover with
/// "See all" and how many there are.
pub(crate) fn draw_see_all(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    place: Rect,
    count: &str,
    focused: bool,
) {
    let ink = style.focus_ink(focused);
    let radius = crate::cover_radius(style, place);
    canvas.stroke_round(place, ui::button_border(style), radius, ink);
    let Some(font) = style.font(env) else {
        return;
    };
    let words = env.i18n.tr("files.shelves.see-all");
    let line = font.line_metrics(style.small).line_height as u16;
    let middle = place.y + place.height / 2;
    let centre = |text: &str| {
        f32::from(place.x) + (f32::from(place.width) - font.measure(style.small, text)) / 2.0
    };
    let room = place.inset(style.gap as u16 / 2, 0);
    let words = ui::fit(&font, style.small, words, f32::from(room.width));
    let top = Rect::new(place.x, middle.saturating_sub(line), place.width, line);
    ui::draw_label(canvas, &font, style.small, centre(&words), top, &words, ink);
    let count = ui::fit(&font, style.small, count, f32::from(room.width));
    let below = Rect::new(place.x, middle, place.width, line);
    let gray = style.focus_faint(focused, 80);
    ui::draw_label(
        canvas,
        &font,
        style.small,
        centre(&count),
        below,
        &count,
        gray,
    );
}
