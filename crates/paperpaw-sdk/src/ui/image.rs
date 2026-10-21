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
    draw_image_over(canvas, storage, image, area, 255);
}

/// Draws a decoded image over what is drawn in `area`, as
/// [`draw_image`]: a picture with transparency
/// ([`image::picture_with_alpha`](crate::image::picture_with_alpha))
/// shows what is below through its clear parts, blended at its edges and
/// dithered as pictures are; `opacity` (0 to 255) lets what is below show
/// through all of it, e.g. a picture over the page read. Any screen,
/// widget or overlay may draw so; rows are read and blended one at a
/// time, so little memory is taken.
pub fn draw_image_over(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    image: &ImageRef,
    area: Rect,
    opacity: u8,
) {
    // Frames are drawn in bands: nothing read for those it is not in.
    if area.intersection(&canvas.clip()).is_none() || opacity == 0 {
        return;
    }
    // Its kind from one read of its header, band by band.
    let source = match crate::image::Decoded::peek(storage, &image.path) {
        Some(crate::image::Decoded::Alpha(file)) => ImageSource::Alpha(file),
        Some(crate::image::Decoded::Color(file)) if opacity == 255 => {
            return draw_color_image(canvas, storage, &file, area);
        }
        Some(crate::image::Decoded::Color(file)) => ImageSource::Color(file),
        _ => ImageSource::Gray(GrayFile {
            path: image.path.clone(),
            width: image.width,
            height: image.height,
        }),
    };
    draw_source(canvas, storage, &source, area, opacity);
}

/// Where the rows of a picture drawn as gray come from.
enum ImageSource {
    Gray(GrayFile),
    /// A colour picture drawn as gray (to be seen through).
    Color(crate::image::ColorFile),
    Alpha(crate::image::AlphaFile),
}

impl ImageSource {
    fn size(&self) -> (usize, usize) {
        let (w, h) = match self {
            ImageSource::Gray(f) => (f.width, f.height),
            ImageSource::Color(f) => (f.width, f.height),
            ImageSource::Alpha(f) => (f.width, f.height),
        };
        (w as usize, h as usize)
    }

    /// Bytes a pixel in its rows.
    fn bytes(&self) -> usize {
        match self {
            ImageSource::Gray(_) => 1,
            ImageSource::Color(_) => 3,
            ImageSource::Alpha(_) => 2,
        }
    }

    fn read_rows(&self, storage: &dyn Storage, y: u32, out: &mut [u8]) -> u32 {
        match self {
            ImageSource::Gray(f) => f.read_rows(storage, y, out),
            ImageSource::Color(f) => f.read_rows(storage, y, out),
            ImageSource::Alpha(f) => f.read_rows(storage, y, out),
        }
    }

    /// A row of it as gray and opacity, two bytes a pixel, into `out`.
    fn gray_alpha(&self, row: &[u8], out: &mut [u8]) {
        match self {
            ImageSource::Alpha(_) => out.copy_from_slice(row),
            ImageSource::Gray(_) => {
                for (pixel, &g) in out.as_chunks_mut::<2>().0.iter_mut().zip(row) {
                    *pixel = [g, 255];
                }
            }
            ImageSource::Color(_) => {
                let rgb = row.as_chunks::<3>().0;
                for (pixel, c) in out.as_chunks_mut::<2>().0.iter_mut().zip(rgb) {
                    let luma = crate::image::luma(c[0].into(), c[1].into(), c[2].into());
                    *pixel = [luma, 255];
                }
            }
        }
    }
}

/// Where output pixel `i` of `out` takes its source from when an image
/// of `source` pixels is made larger: the two pixels around it and how
/// far towards the second (in 256ths), as the pixels' centres lie.
fn between(i: usize, out: usize, source: usize) -> (usize, usize, u32) {
    let at = ((2 * i + 1) * source * 128 / out).saturating_sub(128);
    let first = (at >> 8).min(source - 1);
    (first, (first + 1).min(source - 1), (at & 255) as u32)
}

/// [`draw_image_over`] for a picture drawn as gray, each axis on its
/// own: made smaller, each pixel the mean of the columns it covers (one
/// row read for it), its gray weighted by how opaque each is; made
/// larger, interpolated between the pixels around it (bilinear, in whole
/// numbers). Opaque and not to be seen through, it is drawn as gray
/// rows.
fn draw_source(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    source: &ImageSource,
    area: Rect,
    opacity: u8,
) {
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (usize::from(area.width), usize::from(area.height));
    let (sw, sh) = source.size();
    if w == 0 || h == 0 || sw == 0 || sh == 0 {
        return;
    }
    let clip = canvas.clip();
    let first = top.max(i32::from(clip.y));
    let last = (top + h as i32).min(clip.bottom() as i32);
    if first >= last {
        return;
    }
    // An axis made larger interpolates, one made smaller averages.
    let (wider, taller) = (w > sw, h > sh);
    let blended = opacity < 255 || matches!(source, ImageSource::Alpha(_));
    let mut rows = RowChunk::new(sw * source.bytes());
    // The source row a screen row comes from, as gray and opacity; made
    // taller, the one below it too, and the two mixed.
    let mut upper = alloc::vec![0u8; sw * 2];
    let mut lower = alloc::vec![0u8; if taller { sw * 2 } else { 0 }];
    let mut mixed = alloc::vec![0u8; if taller { sw * 2 } else { 0 }];
    let (mut held_upper, mut held_lower) = (None, None);
    let mut row = alloc::vec![0u8; w * if blended { 2 } else { 1 }];
    let mut made = None;
    for y in first..last {
        let oy = (y - top) as usize;
        let (above, below, ty) = if taller {
            between(oy, h, sh)
        } else {
            let sy = oy * sh / h;
            (sy, sy, 0)
        };
        if made != Some((above, below, ty)) {
            if !load(
                &mut rows,
                source,
                storage,
                above,
                &mut upper,
                &mut held_upper,
            ) {
                return;
            }
            let between_rows = ty > 0 && below != above;
            if between_rows {
                if !load(
                    &mut rows,
                    source,
                    storage,
                    below,
                    &mut lower,
                    &mut held_lower,
                ) {
                    return;
                }
                mix(&upper, &lower, ty, &mut mixed);
            }
            let line: &[u8] = if between_rows { &mixed } else { &upper };
            made_row(line, sw, wider, blended, &mut row);
            made = Some((above, below, ty));
        }
        draw_row(canvas, left, y, &row, blended, opacity);
    }
}

/// Reads source row `y` into `into` as gray and opacity, unless it holds
/// it already (`held`); whether it could be read.
fn load(
    rows: &mut RowChunk,
    source: &ImageSource,
    storage: &dyn Storage,
    y: usize,
    into: &mut [u8],
    held: &mut Option<usize>,
) -> bool {
    if *held == Some(y) {
        return true;
    }
    let Some(read) = rows.get(y as u32, |y, out| source.read_rows(storage, y, out)) else {
        return false;
    };
    source.gray_alpha(read, into);
    *held = Some(y);
    true
}

/// `upper` and `lower` (gray and opacity) mixed into `out`, `t` (in
/// 256ths) towards `lower`, the gray weighted by opacity.
fn mix(upper: &[u8], lower: &[u8], t: u32, out: &mut [u8]) {
    let pairs = upper
        .as_chunks::<2>()
        .0
        .iter()
        .zip(lower.as_chunks::<2>().0);
    for (pixel, (&[g0, a0], &[g1, a1])) in out.as_chunks_mut::<2>().0.iter_mut().zip(pairs) {
        let (a0, a1) = (u32::from(a0) * (256 - t), u32::from(a1) * t);
        let opaque = a0 + a1;
        let weighted = u32::from(g0) * a0 + u32::from(g1) * a1;
        let gray = weighted
            .checked_div(opaque)
            .map_or(255, |g| g.min(255) as u8);
        *pixel = [gray, (opaque / 256).min(255) as u8];
    }
}

/// A screen row `out` made of the source row `line` (gray and opacity,
/// `sw` pixels): interpolated between columns where it is made wider,
/// each pixel the mean of the columns it covers where it is not.
fn made_row(line: &[u8], sw: usize, wider: bool, blended: bool, out: &mut [u8]) {
    let w = out.len() / if blended { 2 } else { 1 };
    let pixel = |i: usize| (u32::from(line[i * 2]), u32::from(line[i * 2 + 1]));
    for x in 0..w {
        let (weighted, opaque, count) = if wider {
            let (x0, x1, tx) = between(x, w, sw);
            let ((g0, a0), (g1, a1)) = (pixel(x0), pixel(x1));
            let (a0, a1) = (a0 * (256 - tx), a1 * tx);
            (g0 * a0 + g1 * a1, a0 + a1, 256)
        } else {
            let from = x * sw / w;
            let to = ((x + 1) * sw / w).max(from + 1).min(sw);
            let (mut weighted, mut opaque) = (0u32, 0u32);
            for i in from..to {
                let (g, a) = pixel(i);
                weighted += g * a;
                opaque += a;
            }
            (weighted, opaque, (to - from) as u32)
        };
        let gray = weighted
            .checked_div(opaque)
            .map_or(255, |g| g.min(255) as u8);
        if blended {
            out[x * 2] = gray;
            out[x * 2 + 1] = (opaque / count).min(255) as u8;
        } else {
            out[x] = gray;
        }
    }
}

/// A row of [`draw_source`]: gray, or gray and opacity blended over what
/// is drawn.
fn draw_row(canvas: &mut Canvas<'_>, left: i32, y: i32, row: &[u8], blended: bool, opacity: u8) {
    if blended {
        canvas.draw_gray_alpha_row(left, y, row, opacity);
    } else {
        canvas.draw_gray_row(left, y, row);
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
