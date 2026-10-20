//! Pictures of what was drawn: RGB pixels, cut out, scaled, compared,
//! written as PNG.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use paperpaw_platform::Rect;

/// An RGB picture, 8 bits a channel, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Image {
    pub fn from_rgb(width: u32, height: u32, rgb: Vec<u8>) -> Self {
        debug_assert_eq!(rgb.len(), (width * height * 3) as usize);
        Image { width, height, rgb }
    }

    /// A picture of one colour.
    pub fn filled(width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&rgb);
        }
        Image::from_rgb(width, height, pixels)
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 3) as usize;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        if x < self.width && y < self.height {
            let i = ((y * self.width + x) * 3) as usize;
            self.rgb[i..i + 3].copy_from_slice(&rgb);
        }
    }

    /// The pixel's lightness, 0 (black) to 255 (white).
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        let [r, g, b] = self.pixel(x, y);
        ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
    }

    /// The part of the picture in `area` (cut to the picture).
    pub fn crop(&self, area: Rect) -> Image {
        let x0 = u32::from(area.x).min(self.width);
        let y0 = u32::from(area.y).min(self.height);
        let x1 = (u32::from(area.x) + u32::from(area.width)).min(self.width);
        let y1 = (u32::from(area.y) + u32::from(area.height)).min(self.height);
        let (w, h) = (x1 - x0, y1 - y0);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in y0..y1 {
            let start = ((y * self.width + x0) * 3) as usize;
            rgb.extend_from_slice(&self.rgb[start..start + (w * 3) as usize]);
        }
        Image::from_rgb(w, h, rgb)
    }

    /// The picture made smaller by `factor` (each pixel the mean of a
    /// square), for contact sheets.
    pub fn shrink(&self, factor: u32) -> Image {
        let factor = factor.max(1);
        let (w, h) = (self.width / factor, self.height / factor);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0u32; 3];
                for dy in 0..factor {
                    for dx in 0..factor {
                        let p = self.pixel(x * factor + dx, y * factor + dy);
                        for c in 0..3 {
                            sum[c] += u32::from(p[c]);
                        }
                    }
                }
                let n = factor * factor;
                rgb.extend(sum.iter().map(|s| (s / n) as u8));
            }
        }
        Image::from_rgb(w, h, rgb)
    }

    /// Puts `other` into this picture with its top left corner at `x`, `y`.
    pub fn paste(&mut self, other: &Image, x: u32, y: u32) {
        for row in 0..other.height {
            for col in 0..other.width {
                self.set_pixel(x + col, y + row, other.pixel(col, row));
            }
        }
    }

    /// A frame of `rgb` around `area`, `thickness` pixels wide, inside it.
    pub fn outline(&mut self, area: Rect, thickness: u32, rgb: [u8; 3]) {
        let (x0, y0) = (u32::from(area.x), u32::from(area.y));
        let (w, h) = (u32::from(area.width), u32::from(area.height));
        for t in 0..thickness.min(w).min(h) {
            for x in x0..x0 + w {
                self.set_pixel(x, y0 + t, rgb);
                self.set_pixel(x, y0 + h - 1 - t, rgb);
            }
            for y in y0..y0 + h {
                self.set_pixel(x0 + t, y, rgb);
                self.set_pixel(x0 + w - 1 - t, y, rgb);
            }
        }
    }

    /// The smallest rectangle holding every pixel in `area` that differs
    /// from `background` by more than `tolerance` in lightness: where ink
    /// is. `None` if there is none.
    pub fn ink_bounds(&self, area: Rect, background: u8, tolerance: u8) -> Option<Rect> {
        let x1 = (u32::from(area.x) + u32::from(area.width)).min(self.width);
        let y1 = (u32::from(area.y) + u32::from(area.height)).min(self.height);
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in u32::from(area.y)..y1 {
            for x in u32::from(area.x)..x1 {
                if self.luma(x, y).abs_diff(background) > tolerance {
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        (left != u32::MAX).then(|| {
            Rect::new(
                left as u16,
                top as u16,
                (right - left + 1) as u16,
                (bottom - top + 1) as u16,
            )
        })
    }

    /// How many pixels differ from `other` (of the same size; all if the
    /// sizes differ) by more than `tolerance` in a channel.
    pub fn differing(&self, other: &Image, tolerance: u8) -> usize {
        if (self.width, self.height) != (other.width, other.height) {
            return (self.width * self.height).max(other.width * other.height) as usize;
        }
        self.rgb
            .as_chunks::<3>()
            .0
            .iter()
            .zip(other.rgb.as_chunks::<3>().0)
            .filter(|(a, b)| {
                a.iter()
                    .zip(b.iter())
                    .any(|(x, y)| x.abs_diff(*y) > tolerance)
            })
            .count()
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        writer
            .write_image_data(&self.rgb)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load_png(path: &Path) -> Result<Image, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder
            .read_info()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        buf.truncate(info.buffer_size());
        let rgb = match info.color_type {
            png::ColorType::Rgb => buf,
            png::ColorType::Rgba => buf
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
            png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g]).collect(),
            png::ColorType::GrayscaleAlpha => buf
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[0], p[0]])
                .collect(),
            other => return Err(format!("{}: {other:?} pictures not read", path.display())),
        };
        Ok(Image::from_rgb(info.width, info.height, rgb))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_is_found_and_pictures_compared() {
        let mut image = Image::filled(20, 10, [255, 255, 255]);
        image.set_pixel(3, 2, [0, 0, 0]);
        image.set_pixel(7, 5, [0, 0, 0]);
        let ink = image.ink_bounds(Rect::new(0, 0, 20, 10), 255, 40).unwrap();
        assert_eq!(ink, Rect::new(3, 2, 5, 4));
        assert_eq!(image.crop(Rect::new(2, 1, 4, 4)).pixel(1, 1), [0, 0, 0]);
        let white = Image::filled(20, 10, [255, 255, 255]);
        assert_eq!(image.differing(&white, 10), 2);
        assert_eq!(white.shrink(2).width, 10);
    }

    #[test]
    fn png_round_trip() {
        let dir = std::env::temp_dir().join(format!("paperpaw-dev-png-{}", std::process::id()));
        let mut image = Image::filled(5, 4, [10, 200, 30]);
        image.outline(Rect::new(0, 0, 5, 4), 1, [0, 0, 0]);
        let path = dir.join("a.png");
        image.save_png(&path).unwrap();
        assert_eq!(Image::load_png(&path).unwrap(), image);
        let _ = std::fs::remove_dir_all(dir);
    }
}
