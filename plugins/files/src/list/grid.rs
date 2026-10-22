//! The library as covers ([`crate::VIEW_COVERS`]): a grid of covers from
//! margin to margin, rows spread over the page, each with
//! its title below and how far it is read on it: a bar along its foot
//! while it is read, a check once finished, "New" for a book added lately
//! and never opened. The books of a series show as one stack, with how
//! many they are.
//!
//! Like the rows, a cell draws only what was read when its page was
//! shown ([`super::rows::Loaded`]).

use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, Focus, UiStyle};
use paperpaw_sdk::{Canvas, Color, Env, Rect};

use super::rows::{self, Loaded, Sizes};

/// The grid's measures, worked out once when the library opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grid {
    pub(crate) columns: u16,
    /// A column: a cover with room around it for the focus.
    column: u16,
    /// Where the columns start: the first cover on the margin.
    left: u16,
    /// Rows on a page.
    rows: u16,
    /// A cell's height: the cover, its title, the room around them.
    pub(crate) cell: u16,
    /// A cover's place.
    pub(crate) cover: (u16, u16),
    /// Room between a cover and its column's edges (for the focus).
    pad: u16,
    /// Room between a cell's edges and its focus.
    edge: u16,
}

/// Width of a cover column, at least, in millimetres (on larger screens).
const COLUMN_MM: u32 = 26;

impl Grid {
    /// The grid of a page `height` pixels high.
    pub(crate) fn new(env: &Env<'_>, style: &UiStyle, sizes: &Sizes, height: u16) -> Grid {
        let width = env.device.display.width;
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let columns = if sizes.compact {
            3
        } else {
            (u32::from(width) / env.metrics.mm(COLUMN_MM).max(1)).max(3) as u16
        };
        // Covers a gap apart, the first and the last on the margins; the
        // focus a little around each.
        let pad = gap / 2;
        let edge = gap / 2;
        let inner = width.saturating_sub(2 * margin);
        let cover_w = inner.saturating_sub((columns - 1) * gap) / columns;
        let column = cover_w + gap;
        let left = margin.saturating_sub(gap / 2);
        let cover_h = cover_w * 3 / 2;
        let lines = if sizes.compact { 1 } else { 2 };
        let least = cover_h + 2 * pad + gap / 3 + lines * sizes.small_line + 2 * edge;
        // As many rows as fit, sharing the room left over.
        let rows = (height / least.max(1)).max(1);
        Grid {
            columns,
            column,
            left,
            rows,
            cell: height / rows,
            cover: (cover_w, cover_h),
            pad,
            edge,
        }
    }

    /// Covers on a page.
    pub(crate) fn per_page(&self) -> usize {
        usize::from(self.rows) * usize::from(self.columns)
    }

    /// The cell of the `slot`-th book of a page whose cells start at `top`.
    pub(crate) fn place(&self, area: Rect, top: u16, slot: usize) -> Rect {
        let columns = usize::from(self.columns.max(1));
        let (row, column) = (slot / columns, slot % columns);
        let x = area.x + self.left + column as u16 * self.column;
        Rect::new(x, top + row as u16 * self.cell, self.column, self.cell)
    }

    /// Where the focus is drawn in `cell`: around the cover and its title,
    /// at the top of the cell.
    fn focus_box(&self, cell: Rect, sizes: &Sizes, style: &UiStyle) -> Rect {
        let lines = if sizes.compact { 1 } else { 2 };
        let gap = style.gap as u16;
        let height = self.cover.1 + 2 * self.pad + gap / 3 + lines * sizes.small_line;
        Rect::new(
            cell.x,
            cell.y + self.edge,
            cell.width,
            height.min(cell.height.saturating_sub(self.edge)),
        )
    }
}

/// Draws the focus as every list does ([`ui::draw_focus`]), as a box
/// with the style's corners around a cover.
fn draw_focus(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect) {
    let radius = style.corner;
    let ring = (style.line * 2).max(3);
    match style.focus {
        Focus::Fill => canvas.fill_round(area, radius, Color::BLACK),
        Focus::Soft if style.grays > 2 => {
            let gray = if style.grays >= 16 { 215 } else { 170 };
            canvas.fill_round(area, radius, Color::gray(gray));
        }
        _ => canvas.stroke_round(area, ring, radius, Color::BLACK),
    }
}

/// Draws a book's cell: its cover (as a stack for `count` books of a
/// series), how far it is read on it, and its title (the series' name
/// for a stack) below.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_cell(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    grid: &Grid,
    cell: Rect,
    focused: bool,
    book: &Loaded,
    count: u16,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    canvas.describe(Role::Row, &book.spoken, cell, focused, None);
    let around = grid.focus_box(cell, sizes, style);
    if focused {
        draw_focus(canvas, style, around);
    }
    let ink = style.focus_ink(focused);
    let gap = style.gap as u16;
    let (w, h) = grid.cover;
    let mut cover = Rect::new(around.x + grid.pad, around.y + grid.pad, w, h);
    let stack = count > 1;
    if stack {
        // Two covers' edges behind, up and to the right.
        let lift = (gap / 3).max(3);
        cover = Rect::new(cover.x, cover.y + 2 * lift, w - 2 * lift, h - 2 * lift);
        let radius = crate::cover_radius(style, cover);
        let line = ui::button_border(style);
        for n in (1..=2u16).rev() {
            let behind = Rect::new(
                cover.x + n * lift,
                cover.y - n * lift,
                cover.width,
                cover.height,
            );
            canvas.fill_round(behind, radius, style.focus_paper(focused));
            canvas.stroke_round(behind, line, radius, ink);
        }
    }
    rows::draw_cover(canvas, env, style, cover, book, ink);
    draw_marks(canvas, env, style, sizes, cover, book, count);

    // The title (or the series), on one or two lines below.
    let x = around.x + grid.pad;
    let width = around.width.saturating_sub(2 * grid.pad);
    let mut y = around.y + grid.pad + h + gap / 3;
    for line in &book.lines {
        let area = Rect::new(x, y, width, sizes.small_line);
        ui::draw_label(canvas, &font, style.small, f32::from(x), area, line, ink);
        y += sizes.small_line;
    }
}

/// Makes `book` ready to be drawn as a cell of `grid` ([`draw_cell`]):
/// its title (the series' name for a stack of `count` books) in the
/// lines under the cover, and for a stack how many books it holds.
pub(crate) fn prepare_cell(
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    grid: &Grid,
    book: &mut Loaded,
    count: u16,
) {
    let lines = if sizes.compact { 1 } else { 2 };
    let width = grid.column.saturating_sub(2 * grid.pad);
    let title = if count > 1 { &book.series } else { &book.title };
    book.lines = rows::wrap_lines(env, style, style.small, title, f32::from(width), lines);
    if count > 1 {
        book.stack = env.i18n.plural("files.books", i64::from(count), &[]);
        book.spoken = rows::spoken(&[&book.series, &book.stack]);
    } else {
        let state = rows::state_words(env, book);
        book.spoken = rows::spoken(&[&book.title, &book.author, state]);
    }
}

/// Draws on `cover` how far `book` is read (a bar at its foot, a check
/// when finished, "New"), or for a stack how many books it holds.
fn draw_marks(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    cover: Rect,
    book: &Loaded,
    count: u16,
) {
    let gap = style.gap as u16;
    let inset = (gap / 3).max(2);
    let line = sizes.small_line;
    let foot = Rect::new(
        cover.x + inset,
        (cover.y + cover.height).saturating_sub(line + inset),
        cover.width.saturating_sub(2 * inset),
        line,
    );
    if count > 1 {
        rows::draw_tag(
            canvas,
            env,
            style,
            foot.x,
            foot,
            &book.stack,
            true,
            Color::BLACK,
            Color::WHITE,
        );
        return;
    }
    match book.progress {
        100 => {
            // A check in a dark disc, at the foot's end.
            let d = line;
            let disc = Rect::new((foot.x + foot.width).saturating_sub(d), foot.y, d, d);
            canvas.fill_round(disc, u32::from(d / 2), Color::BLACK);
            let size = style.small * 0.9;
            let w = ui::icon_width(env, "check", size);
            let x = f32::from(disc.x) + (f32::from(d) - w) / 2.0;
            ui::draw_icon(canvas, env, "check", size, x, disc, Color::WHITE);
        }
        p if p > 0 => {
            // A light band with the bar, so it reads on any cover.
            let band = Rect::new(
                foot.x,
                foot.y + foot.height / 4,
                foot.width,
                foot.height / 2,
            );
            let radius = u32::from(band.height / 2);
            canvas.fill_round(band, radius, Color::WHITE);
            let pad = (band.height.saturating_sub(sizes.bar)) / 2;
            let bar = Rect::new(
                band.x + pad,
                band.y,
                band.width.saturating_sub(2 * pad),
                band.height,
            );
            rows::draw_bar(canvas, style, sizes, bar.x, bar, bar.width, p, Color::BLACK);
        }
        _ if book.new => {
            let label = env.i18n.tr("files.list.new");
            rows::draw_tag(
                canvas,
                env,
                style,
                foot.x,
                foot,
                label,
                true,
                Color::BLACK,
                Color::WHITE,
            );
        }
        _ => {}
    }
}
