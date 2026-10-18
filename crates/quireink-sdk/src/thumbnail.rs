//! Pictures made small once and kept in memory ([`Thumbnail`]).

use alloc::vec::Vec;

use quireink_document::ImageRef;
use quireink_graphics::Canvas;
use quireink_image::{ColorFile, Downscaler, GrayFile, ImageError, ImageFile, ImageSink, luma};
use quireink_platform::Storage;

/// Bytes of the picture's rows read at a time.
const ROWS_BYTES: usize = 8192;

/// A picture made small once and kept in memory, to draw it again and
/// again without reading the storage: covers in a list, drawn in every
/// band of every frame. Gray at 4 bits a pixel (a cover of 70 by 100
/// pixels takes 3.5 KB), or in colour, 3 bytes a pixel, where asked for
/// and the picture has colour.
///
/// [`draw_image`](crate::ui::draw_image) reads a picture's rows from the
/// storage each time it is drawn; a thumbnail reads it once, when made
/// (e.g. when a page of a list is loaded), each pixel the mean of all it
/// covers.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    width: u16,
    height: u16,
    color: bool,
    /// Gray: two pixels a byte, the first in the high bits, each row
    /// whole bytes. Colour: red, green, blue.
    pixels: Vec<u8>,
}

impl Thumbnail {
    /// `image` made `width` × `height` (at most its own size; pass a size
    /// that keeps its proportions, e.g. from
    /// [`fit_image`](crate::ui::fit_image)), in colour if `color` and the
    /// image has colour. `None` if it cannot be read or there is no
    /// memory for it.
    pub fn new(
        storage: &dyn Storage,
        image: &ImageRef,
        width: u16,
        height: u16,
        color: bool,
    ) -> Option<Self> {
        let file = ImageFile::open(storage, &image.path)?;
        let channels = if file.color { 3 } else { 1 };
        let mut scaler = Downscaler::new(
            file.width,
            file.height,
            (u32::from(width), u32::from(height)),
            channels,
        )
        .ok()?;
        let mut sink = Pack {
            color: color && file.color,
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        sink.start(scaler.out_width(), scaler.out_height()).ok()?;
        let row = file.width as usize * channels;
        let rows = (ROWS_BYTES / row.max(1)).max(1);
        let mut data = Vec::new();
        data.try_reserve_exact(rows * row).ok()?;
        data.resize(rows * row, 0);
        let mut y = 0;
        while y < file.height {
            let got = if file.color {
                ColorFile {
                    path: file.path.clone(),
                    width: file.width,
                    height: file.height,
                }
                .read_rows(storage, y, &mut data)
            } else {
                GrayFile {
                    path: file.path.clone(),
                    width: file.width,
                    height: file.height,
                }
                .read_rows(storage, y, &mut data)
            };
            if got == 0 {
                return None;
            }
            for pixels in data.chunks(row).take(got as usize) {
                scaler.row(pixels, &mut sink).ok()?;
            }
            y += got;
        }
        scaler.finish(&mut sink).ok()?;
        Some(Self {
            width: sink.width,
            height: sink.height,
            color: sink.color,
            pixels: sink.pixels,
        })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// The bytes it keeps.
    pub fn bytes(&self) -> usize {
        self.pixels.len()
    }

    /// Draws it with its top left corner at `x`, `y`: the rows inside the
    /// canvas clip only.
    pub fn draw(&self, canvas: &mut Canvas<'_>, x: u16, y: u16) {
        let clip = canvas.clip();
        let top = u32::from(y);
        let first = top.max(u32::from(clip.y));
        let last = (top + u32::from(self.height)).min(clip.bottom());
        if first >= last {
            return;
        }
        let width = usize::from(self.width);
        if self.color {
            for row_y in first..last {
                let at = (row_y - top) as usize * width * 3;
                canvas.draw_rgb_row(i32::from(x), row_y as i32, &self.pixels[at..at + width * 3]);
            }
            return;
        }
        let packed = width.div_ceil(2);
        let mut row = alloc::vec![0u8; width];
        for row_y in first..last {
            let at = (row_y - top) as usize * packed;
            for (i, out) in row.iter_mut().enumerate() {
                let byte = self.pixels[at + i / 2];
                let level = if i % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                *out = level * 17;
            }
            canvas.draw_gray_row(i32::from(x), row_y as i32, &row);
        }
    }
}

/// Packs the rows of a picture made small.
struct Pack {
    color: bool,
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}

impl Pack {
    fn push_gray(&mut self, gray: impl Iterator<Item = u8>) {
        let mut high = None;
        for g in gray {
            // To 16 levels, rounded.
            let level = (u16::from(g) * 15 + 127) / 255;
            match high.take() {
                None => high = Some(level as u8),
                Some(h) => self.pixels.push(h << 4 | level as u8),
            }
        }
        if let Some(h) = high {
            self.pixels.push(h << 4);
        }
    }
}

impl ImageSink for Pack {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        let (w, h) = (
            u16::try_from(width).map_err(|_| ImageError::Unsupported("picture this large"))?,
            u16::try_from(height).map_err(|_| ImageError::Unsupported("picture this large"))?,
        );
        self.width = w;
        self.height = h;
        let bytes = if self.color {
            usize::from(w) * 3
        } else {
            usize::from(w).div_ceil(2)
        } * usize::from(h);
        self.pixels
            .try_reserve_exact(bytes)
            .map_err(|_| ImageError::OutOfMemory)
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        if self.color {
            for &g in gray {
                self.pixels.extend_from_slice(&[g, g, g]);
            }
        } else {
            self.push_gray(gray.iter().copied());
        }
        Ok(())
    }

    fn wants_color(&self) -> bool {
        self.color
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        if self.color {
            self.pixels.extend_from_slice(rgb);
        } else {
            let gray = rgb
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| luma(u32::from(p[0]), u32::from(p[1]), u32::from(p[2])));
            self.push_gray(gray);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use quireink_graphics::Framebuffer;
    use quireink_image::GrayWriter;
    use quireink_platform::{Color, PixelFormat, Rect};

    fn picture(storage: &MemoryStorage) -> ImageRef {
        // 200 × 100: black on the left half, white on the right.
        let mut writer = GrayWriter::new(storage, "p.gray");
        writer.start(200, 100).unwrap();
        let row: Vec<u8> = (0..200).map(|x| if x < 100 { 0 } else { 255 }).collect();
        for _ in 0..100 {
            writer.row(&row).unwrap();
        }
        writer.finish().unwrap();
        ImageRef {
            path: "p.gray".into(),
            width: 200,
            height: 100,
        }
    }

    #[test]
    fn made_small_once_and_drawn_from_memory() {
        let storage = MemoryStorage::new();
        let image = picture(&storage);
        let thumb = Thumbnail::new(&storage, &image, 21, 10, false).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (21, 10));
        // Two pixels a byte, rows whole bytes.
        assert_eq!(thumb.bytes(), 11 * 10);
        // Gone from the storage: drawn from memory all the same.
        storage.remove("p.gray").unwrap();
        let mut frame = Framebuffer::new(40, 20, PixelFormat::Gray8).unwrap();
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        thumb.draw(&mut canvas, 5, 5);
        let view = frame.view();
        assert_eq!(view.pixel(5, 5), Color::BLACK);
        assert_eq!(view.pixel(25, 14), Color::WHITE);
        // Outside it, untouched.
        assert_eq!(view.pixel(4, 5), Color::WHITE);
        // Only the rows in the clip.
        let mut frame = Framebuffer::new(40, 20, PixelFormat::Gray8).unwrap();
        let mut canvas = frame.canvas();
        canvas.clear(Color::WHITE);
        thumb.draw(&mut canvas.clipped(Rect::new(0, 0, 40, 8)), 5, 5);
        assert_eq!(frame.view().pixel(5, 7), Color::BLACK);
        assert_eq!(frame.view().pixel(5, 9), Color::WHITE);
    }

    #[test]
    fn unreadable_pictures_make_none() {
        let storage = MemoryStorage::new();
        let image = ImageRef {
            path: "none.gray".into(),
            width: 10,
            height: 10,
        };
        assert!(Thumbnail::new(&storage, &image, 5, 5, false).is_none());
    }
}
