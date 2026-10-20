//! Covers in a row or a grid, each with a label below (its number in a
//! series, its title), the books read marked: for the series and the
//! favourites.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{Align, Look, line_height};
use paperpaw_sdk::{Canvas, Env, Point, Rect};

use super::{Book, Covers, cover_size, draw_cover, draw_line, least_cover_width};

/// A book on the shelf, as shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shelved {
    pub book: Book,
    /// Below its cover (its number, its title), if anything.
    pub label: String,
    /// Marked as read.
    pub read: bool,
    /// Marked as the one being read.
    pub current: bool,
}

/// Where a cover goes and its label below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place {
    pub cover: Rect,
    pub label: Rect,
}

/// The places of up to `count` covers in `area`, in a row (`grid`
/// false) or in rows: as large as the area allows them, a gap between
/// them, each with `label` pixels below its cover, together in the
/// middle (or at the side `align` says). Fewer places than books where
/// they would be too small; where even one cover would be, the labels
/// alone in a row (the covers' places empty).
pub(crate) fn places(
    env: &Env<'_>,
    area: Rect,
    count: usize,
    grid: bool,
    label: u16,
    align: Align,
) -> Vec<Place> {
    if count == 0 || area.width == 0 || area.height <= label {
        return Vec::new();
    }
    let gap = UiStyle::new(env).gap as u16;
    let (w, h) = (u32::from(area.width), u32::from(area.height));
    let size = |columns: u32, rows: u32| -> u32 {
        let by_width = w.saturating_sub(u32::from(gap) * (columns - 1)) / columns;
        let rows_h = h.saturating_sub(u32::from(gap / 2) * (rows - 1));
        let cell_h = (rows_h / rows).saturating_sub(u32::from(label));
        by_width.min(cell_h * 2 / 3)
    };
    let n = count as u32;
    let least = u32::from(least_cover_width(env));
    // The columns and rows that make the covers largest, all of them
    // shown where they are not too small.
    let mut best = (1, 1, 0);
    for columns in 1..=n {
        let rows = if grid { n.div_ceil(columns) } else { 1 };
        let width = size(columns, rows);
        if width > best.2 && (grid || columns == n) {
            best = (columns, rows, width);
        }
    }
    let (mut columns, mut rows, mut width) = best;
    if width < least {
        // Fewer, each as small as allowed.
        let step = least + u32::from(gap);
        columns = ((w + u32::from(gap)) / step).clamp(1, n);
        rows = if grid {
            let row = least * 3 / 2 + u32::from(label) + u32::from(gap / 2);
            ((h + u32::from(gap / 2)) / row).clamp(1, n)
        } else {
            1
        };
        width = size(columns, rows);
    }
    if width < least && label > 0 {
        // No cover would be told from another: the labels alone.
        let cell = least as u16 + gap;
        let shown = (area.width / cell).clamp(1, count as u16);
        let left = super::place_x(area, cell * shown, align);
        let top = area.y + (area.height - label) / 2;
        return (0..shown)
            .map(|i| {
                let x = left + i * cell;
                Place {
                    cover: Rect::new(x + cell / 2, top, 0, 0),
                    label: Rect::new(x, top, cell, label),
                }
            })
            .collect();
    }
    let shown = (columns * rows).min(n) as usize;
    let cover_w = width as u16;
    let cover_h = (width * 3 / 2) as u16;
    let step = cover_w + gap;
    let used_columns = (shown as u32).min(columns) as u16;
    let row_w = step * used_columns - gap;
    let left = super::place_x(area, row_w, align);
    let row_h = cover_h + label;
    let used_rows = (shown as u32).div_ceil(columns) as u16;
    let block = (row_h * used_rows + gap / 2 * (used_rows - 1)).min(area.height);
    let top = area.y + (area.height - block) / 2;
    let right = area.x + area.width;
    (0..shown)
        .map(|i| {
            let (col, row) = ((i as u32 % columns) as u16, (i as u32 / columns) as u16);
            let x = left + col * step;
            let y = top + row * (row_h + gap / 2);
            // The label as wide as the cover and half the gap each side.
            let label_x = x.saturating_sub(gap / 2).max(area.x);
            let label_w = (x + cover_w + gap / 2).min(right) - label_x;
            Place {
                cover: Rect::new(x, y, cover_w, cover_h),
                label: Rect::new(label_x, y + cover_h, label_w, label),
            }
        })
        .collect()
}

/// Draws the caption `text` of widget `id` just above the covers in
/// `places` (where it shows: `rest` is the area below it), so that the
/// two stay together.
pub(crate) fn draw_caption(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    id: &str,
    look: &Look,
    rest: Rect,
    places: &[Place],
    text: &str,
) {
    if rest == look.area {
        return;
    }
    let free = places
        .first()
        .map_or(0, |p| p.cover.y.saturating_sub(rest.y));
    let mut above = *look;
    above.area = super::below(look.area, free);
    paperpaw_sdk::widgets::draw_caption(canvas, env, id, &above, text);
}

/// The height of a label below a cover.
pub(crate) fn label_height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small) + style.gap as u16 / 4
}

/// Where `book`'s cover goes in `place`: at its bottom (covers of other
/// shapes stand on the same line), in the middle from side to side.
fn cover_at(place: &Place, book: &Book) -> Rect {
    let slot = place.cover;
    let (w, h) = cover_size(book, slot.width, slot.height);
    Rect::new(
        slot.x + (slot.width - w) / 2,
        slot.y + slot.height - h,
        w,
        h,
    )
}

/// The covers of `books` in their `places`, made small.
pub(crate) fn covers(env: &Env<'_>, places: &[Place], books: &[Shelved]) -> Covers {
    Covers::make(
        env,
        places
            .iter()
            .zip(books)
            .filter(|(place, _)| place.cover.width > 0)
            .map(|(place, shelved)| (&shelved.book, cover_at(place, &shelved.book))),
    )
}

/// Draws `books` in their `places` (their covers made small in
/// `covers`): each cover at the bottom of its place, its label below, a
/// mark on those read and a line below the one being read.
pub(crate) fn draw(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    places: &[Place],
    books: &[Shelved],
    covers: &Covers,
) {
    let style = UiStyle::new(env);
    let font = style.font(env);
    for (place, shelved) in places.iter().zip(books) {
        let at = cover_at(place, &shelved.book);
        draw_cover(canvas, env, look, at, &shelved.book, covers);
        if shelved.read && at.width > 1 {
            draw_read_mark(canvas, env, look, at);
        }
        let label = place.label;
        let labelled = font.is_some() && !shelved.label.is_empty() && label.height > 0;
        if shelved.current && !labelled {
            // A line below the cover: here one is.
            let line = (style.line as u16 * 3).max(3);
            let gap = (style.gap as u16 / 4).max(1);
            canvas.fill(
                Rect::new(at.x, at.y + at.height + gap, at.width, line),
                look.ink,
            );
        }
        let Some(font) = font.as_ref().filter(|_| labelled) else {
            continue;
        };
        let row_h = line_height(env, style.small).min(label.height);
        let row = Rect::new(label.x, label.y + label.height - row_h, label.width, row_h);
        if shelved.current {
            // The book being read: its label light on dark.
            let text =
                paperpaw_sdk::ui::fit(font, style.small, &shelved.label, f32::from(row.width));
            let pad = style.gap as u16 / 2;
            let w = (font.measure(style.small, &text) as u16 + 2 * pad).min(row.width);
            let pill = Rect::new(row.x + (row.width - w) / 2, row.y, w, row.height);
            canvas.fill_round(pill, u32::from(row.height / 2), look.ink);
            draw_line(
                canvas,
                env,
                font,
                style.small,
                row,
                Align::Center,
                &text,
                look.paper,
            );
        } else {
            draw_line(
                canvas,
                env,
                font,
                style.small,
                row,
                Align::Center,
                &shelved.label,
                look.ink,
            );
        }
    }
}

/// A round mark with a tick at the top right corner of a cover: read.
fn draw_read_mark(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, cover: Rect) {
    let style = UiStyle::new(env);
    let radius = u32::from(cover.width / 6)
        .min(style.small as u32 * 2 / 3)
        .max(5);
    let inset = (radius / 3) as i32;
    let center = Point::new(
        i32::from(cover.x + cover.width) - radius as i32 - inset,
        i32::from(cover.y) + radius as i32 + inset,
    );
    // A ring of paper round it, so it stands out on any cover.
    canvas.fill_circle(center, radius + (radius / 4).max(1), look.paper);
    canvas.fill_circle(center, radius, look.ink);
    let r = radius as i32;
    let width = (radius / 4).max(1);
    let a = Point::new(center.x - r / 2, center.y);
    let b = Point::new(center.x - r / 8, center.y + r * 3 / 8);
    let c = Point::new(center.x + r / 2, center.y - r * 3 / 8);
    canvas.draw_line(a, b, width, look.paper);
    canvas.draw_line(b, c, width, look.paper);
}
