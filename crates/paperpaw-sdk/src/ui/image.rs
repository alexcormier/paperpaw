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
    if let Some(file) = crate::image::AlphaFile::open(storage, &image.path) {
        return draw_alpha_image(canvas, storage, &file, area, opacity);
    }
    if let Some(file) = crate::image::ColorFile::open(storage, &image.path) {
        if opacity == 255 {
            return draw_color_image(canvas, storage, &file, area);
        }
        return draw_gray_image(canvas, storage, &gray_of_color(file), area, opacity);
    }
    let file = GrayFile {
        path: image.path.clone(),
        width: image.width,
        height: image.height,
    };
    draw_gray_image(canvas, storage, &ImageSource::Gray(file), area, opacity);
}

/// Where the rows of a picture drawn over others come from.
enum ImageSource {
    Gray(GrayFile),
    /// A colour picture drawn as gray (to be seen through).
    Color(crate::image::ColorFile),
}

fn gray_of_color(file: crate::image::ColorFile) -> ImageSource {
    ImageSource::Color(file)
}

impl ImageSource {
    fn width(&self) -> u32 {
        match self {
            ImageSource::Gray(f) => f.width,
            ImageSource::Color(f) => f.width,
        }
    }

    fn height(&self) -> u32 {
        match self {
            ImageSource::Gray(f) => f.height,
            ImageSource::Color(f) => f.height,
        }
    }

    /// Bytes a pixel in its rows.
    fn bytes(&self) -> usize {
        match self {
            ImageSource::Gray(_) => 1,
            ImageSource::Color(_) => 3,
        }
    }

    fn read_rows(&self, storage: &dyn Storage, y: u32, out: &mut [u8]) -> u32 {
        match self {
            ImageSource::Gray(f) => f.read_rows(storage, y, out),
            ImageSource::Color(f) => f.read_rows(storage, y, out),
        }
    }
}

/// [`draw_image_over`] for a gray picture (or a colour one as gray):
/// opaque, as see-through as `opacity` says.
fn draw_gray_image(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    file: &ImageSource,
    area: Rect,
    opacity: u8,
) {
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (i32::from(area.width), i32::from(area.height));
    let (sw, sh) = (file.width(), file.height());
    if w <= 0 || h <= 0 || sw == 0 || sh == 0 {
        return;
    }
    let clip = canvas.clip();
    let first = top.max(i32::from(clip.y));
    let last = (top + h).min(clip.bottom() as i32);
    if first >= last {
        return;
    }
    let bytes = file.bytes();
    let mut rows = RowChunk::new(sw as usize * bytes);
    // Gray, or gray and opacity where it is to be seen through.
    let blended = opacity < 255;
    let mut row = alloc::vec![0u8; w as usize * if blended { 2 } else { 1 }];
    let mut loaded = None;
    for y in first..last {
        let sy = ((y - top) as u64 * u64::from(sh) / h as u64) as u32;
        if loaded != Some(sy) {
            let Some(source) = rows.get(sy, |y, out| file.read_rows(storage, y, out)) else {
                return;
            };
            loaded = Some(sy);
            let sw = sw as usize;
            for x in 0..w as usize {
                let from = x * sw / w as usize;
                let to = ((x + 1) * sw / w as usize).max(from + 1).min(sw);
                let sum: u32 = (from..to)
                    .map(|i| match bytes {
                        3 => u32::from(crate::image::luma(
                            u32::from(source[i * 3]),
                            u32::from(source[i * 3 + 1]),
                            u32::from(source[i * 3 + 2]),
                        )),
                        _ => u32::from(source[i]),
                    })
                    .sum();
                let gray = (sum / (to - from) as u32) as u8;
                if blended {
                    row[x * 2] = gray;
                    row[x * 2 + 1] = 255;
                } else {
                    row[x] = gray;
                }
            }
        }
        if blended {
            canvas.draw_gray_alpha_row(left, y, &row, opacity);
        } else {
            canvas.draw_gray_row(left, y, &row);
        }
    }
}

/// [`draw_image_over`] for a picture with transparency: each smaller
/// pixel the mean of those it covers, its gray weighted by how opaque
/// each is.
fn draw_alpha_image(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    file: &crate::image::AlphaFile,
    area: Rect,
    opacity: u8,
) {
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
    let sw = file.width as usize;
    let mut rows = RowChunk::new(sw * 2);
    let mut row = alloc::vec![0u8; w as usize * 2];
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
                let (mut weighted, mut opaque) = (0u32, 0u32);
                for i in from..to {
                    let (g, a) = (u32::from(source[i * 2]), u32::from(source[i * 2 + 1]));
                    weighted += g * a;
                    opaque += a;
                }
                row[x * 2] = weighted.checked_div(opaque).map_or(255, |g| g as u8);
                row[x * 2 + 1] = (opaque / (to - from) as u32) as u8;
            }
        }
        canvas.draw_gray_alpha_row(left, y, &row, opacity);
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
