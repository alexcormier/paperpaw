//! QR codes and pictures (covers, illustrations) drawn into an area.

#[allow(clippy::wildcard_imports)]
use super::*;

// ---- QR codes ----------------------------------------------------------

/// Draws `qr` as large as fits in `area`, centred, each module a whole
/// number of pixels, with the four modules of white around it scanners
/// expect; returns where it went with that white, or `None` (nothing
/// drawn) if it does not fit.
pub fn draw_qr(canvas: &mut Canvas<'_>, qr: &crate::qr::QrCode, area: Rect) -> Option<Rect> {
    let modules = u16::try_from(qr.size()).ok()?.checked_add(8)?;
    let scale = area.width.min(area.height) / modules;
    if scale == 0 {
        return None;
    }
    let side = scale * modules;
    let x = area.x + (area.width - side) / 2;
    let y = area.y + (area.height - side) / 2;
    let (x0, y0) = (x + 4 * scale, y + 4 * scale);
    canvas.fill(Rect::new(x, y, side, side), Color::WHITE);
    for my in 0..qr.size() {
        for mx in 0..qr.size() {
            if qr.dark(mx, my) {
                canvas.fill_rect(
                    i32::from(x0 + mx as u16 * scale),
                    i32::from(y0 + my as u16 * scale),
                    u32::from(scale),
                    u32::from(scale),
                    Color::BLACK,
                );
            }
        }
    }
    Some(Rect::new(x, y, side, side))
}

// ---- Images ------------------------------------------------------------

/// Draws a decoded image (e.g. a book's cover or an illustration), gray
/// or in colour, scaled into `area`: each screen row reads one source row
/// from storage, columns are averaged when the image is reduced, and only
/// rows inside the canvas clip are read. Grays are dithered on two-level
/// panels; colour shows as colour on colour screens, as gray elsewhere.
pub fn draw_image(canvas: &mut Canvas<'_>, storage: &dyn Storage, image: &ImageRef, area: Rect) {
    // Frames are drawn in bands: nothing read for those it is not in, nor
    // where it is only described.
    if canvas.is_blind() || area.intersection(&canvas.clip()).is_none() {
        return;
    }
    if let Some(file) = crate::image::ColorFile::open(storage, &image.path) {
        return draw_color_image(canvas, storage, &file, area);
    }
    let file = GrayFile {
        path: image.path.clone(),
        width: image.width,
        height: image.height,
    };
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (i32::from(area.width), i32::from(area.height));
    if w <= 0 || h <= 0 || file.width == 0 || file.height == 0 {
        return;
    }
    let clip = canvas.clip();
    let first = top.max(i32::from(clip.y));
    let last = (top + h).min(clip.bottom() as i32);
    if first >= last {
        return;
    }
    let mut rows = RowChunk::new(file.width as usize);
    let mut row = alloc::vec![0u8; w as usize];
    // Where each column's pixels start in a source row, worked out once
    // (not two divisions a pixel a row); the last is the row's end.
    let sw = file.width as usize;
    let starts: Vec<u16> = (0..=w as usize)
        .map(|x| (x * sw / w as usize).min(sw) as u16)
        .collect();
    let mut loaded = None;
    for y in first..last {
        let sy = ((y - top) as u64 * u64::from(file.height) / h as u64) as u32;
        if loaded != Some(sy) {
            let Some(source) = rows.get(sy, |y, out| file.read_rows(storage, y, out)) else {
                return;
            };
            loaded = Some(sy);
            if sw <= w as usize {
                // As large or larger: a source pixel each.
                for (out, &from) in row.iter_mut().zip(&starts) {
                    *out = source[usize::from(from).min(sw - 1)];
                }
            } else {
                for (x, out) in row.iter_mut().enumerate() {
                    let from = usize::from(starts[x]);
                    let to = usize::from(starts[x + 1]).max(from + 1).min(sw);
                    let sum: u32 = source[from..to].iter().map(|&v| u32::from(v)).sum();
                    *out = (sum / (to - from) as u32) as u8;
                }
            }
        }
        canvas.draw_gray_row(left, y, &row);
    }
}

/// Bytes of a picture's rows read at a time.
pub(super) const ROWS_BYTES: usize = 8192;

/// Rows of a picture read several at a time: a card serves whole blocks
/// in one command faster than a row at a time.
pub(super) struct RowChunk {
    data: Vec<u8>,
    row: usize,
    first: u32,
    count: u32,
}

impl RowChunk {
    /// For rows of `row` bytes.
    fn new(row: usize) -> Self {
        let rows = (ROWS_BYTES / row.max(1)).max(1);
        Self {
            data: alloc::vec![0; rows * row],
            row,
            first: 0,
            count: 0,
        }
    }

    /// Row `y`, read with those after it unless at hand; `read` reads rows
    /// from a row on and says how many.
    fn get(&mut self, y: u32, read: impl FnOnce(u32, &mut [u8]) -> u32) -> Option<&[u8]> {
        if !(self.first..self.first + self.count).contains(&y) {
            self.first = y;
            self.count = read(y, &mut self.data);
            if self.count == 0 {
                return None;
            }
        }
        let at = (y - self.first) as usize * self.row;
        Some(&self.data[at..at + self.row])
    }
}

/// [`draw_image`] for a colour image.
pub(super) fn draw_color_image(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    file: &crate::image::ColorFile,
    area: Rect,
) {
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (i32::from(area.width), i32::from(area.height));
    if w <= 0 || h <= 0 {
        return;
    }
    let clip = canvas.clip();
    let first = top.max(i32::from(clip.y));
    let last = (top + h).min(clip.bottom() as i32);
    if first >= last {
        return;
    }
    let sw = file.width as usize;
    let mut rows = RowChunk::new(sw * 3);
    let mut row = alloc::vec![0u8; w as usize * 3];
    let mut loaded = None;
    for y in first..last {
        let sy = ((y - top) as u64 * u64::from(file.height) / h as u64) as u32;
        if loaded != Some(sy) {
            let Some(source) = rows.get(sy, |y, out| file.read_rows(storage, y, out)) else {
                return;
            };
            loaded = Some(sy);
            for x in 0..w as usize {
                let from = x * sw / w as usize;
                let to = ((x + 1) * sw / w as usize).max(from + 1).min(sw);
                for c in 0..3 {
                    let sum: u32 = (from..to).map(|i| u32::from(source[i * 3 + c])).sum();
                    row[x * 3 + c] = (sum / (to - from) as u32) as u8;
                }
            }
        }
        canvas.draw_rgb_row(left, y, &row);
    }
}

/// The largest size of `image` that fits `width` × `height`, keeping its
/// proportions.
pub fn fit_image(image: &ImageRef, width: u16, height: u16) -> (u16, u16) {
    if image.width == 0 || image.height == 0 {
        return (0, 0);
    }
    let by_width = u64::from(width) * u64::from(image.height) / u64::from(image.width);
    if by_width <= u64::from(height) {
        (width, by_width as u16)
    } else {
        let w = u64::from(height) * u64::from(image.width) / u64::from(image.height);
        (w as u16, height)
    }
}
