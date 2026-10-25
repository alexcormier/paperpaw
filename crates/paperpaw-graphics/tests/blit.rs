//! Bitmaps blitted come out as their pixels drawn one by one: every
//! bitmap format, with and without a palette, onto every frame format,
//! scaled, dithered or not, cut by the clip on every side, moved by an
//! origin, band by band, in night mode with pictures inverted or kept.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::formats;
use paperpaw_graphics::{Bitmap, BitmapError, BitmapFormat, Blit, Canvas, Dither, Framebuffer};
use paperpaw_platform::{Color, PixelFormat, Point, Rect};

const W: u16 = 200;
const H: u16 = 40;
/// The bitmap's size: wide enough for whole words of a frame of 1 bit a
/// pixel unscaled.
const BW: u16 = 45;
const BH: u16 = 7;

/// Bytes that look random, the same every run.
fn noise(seed: u32, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (state >> 16) as u8
        })
        .collect()
}

/// A bitmap's pixels, its stride and its palette.
struct Source {
    format: BitmapFormat,
    data: Vec<u8>,
    stride: usize,
    palette: Option<Vec<u8>>,
}

impl Source {
    fn new(format: BitmapFormat, seed: u32, padded: bool, palette: bool) -> Self {
        let stride = format.stride(BW) + if padded { 3 } else { 0 };
        let data = noise(seed, stride * usize::from(BH));
        let palette = palette.then(|| noise(seed + 7, usize::from(format.values())));
        Self {
            format,
            data,
            stride,
            palette,
        }
    }

    fn bitmap(&self) -> Bitmap<'_> {
        let bitmap = Bitmap::new(self.format, BW, BH, &self.data)
            .unwrap()
            .stride(self.stride)
            .unwrap();
        match &self.palette {
            Some(palette) => bitmap.palette(palette).unwrap(),
            None => bitmap,
        }
    }

    /// The gray of pixel `(x, y)`, worked out on its own.
    fn gray(&self, x: usize, y: usize) -> u8 {
        let bpp = usize::from(self.format.bits_per_pixel());
        let bit = x * bpp;
        let byte = self.data[y * self.stride + bit / 8];
        let max = (1usize << bpp) - 1;
        let value = usize::from(byte >> (8 - bpp - bit % 8)) & max;
        match &self.palette {
            Some(palette) => palette[value],
            None => (value * 255 / max) as u8,
        }
    }
}

/// What is drawn: where, how large, how dithered, on what kind of frame.
#[derive(Debug, Clone, Copy)]
struct Case {
    format: PixelFormat,
    at: Point,
    scale: u8,
    dither: Dither,
    inverted: bool,
    kept: bool,
    /// Rows a band holds.
    rows: u16,
    /// Drawn into a part of the screen, moved by an origin.
    framed: bool,
}

/// The canvas a case draws on: the band's, or a part of it moved.
fn with_canvas(fb: &mut Framebuffer, case: Case, draw: impl FnOnce(&mut Canvas<'_>)) {
    let mut canvas = fb.canvas();
    if case.framed {
        let mut part = canvas.clipped(Rect::new(9, 3, 150, 31)).offset(5, 2);
        draw(&mut part);
    } else {
        draw(&mut canvas);
    }
}

/// The slow way: every pixel on its own, as gray rows (or, undithered,
/// as filled pixels of the nearest gray).
fn reference(canvas: &mut Canvas<'_>, source: &Source, case: Case) {
    let scale = usize::from(case.scale);
    let keep = case.inverted && case.kept;
    for dy in 0..usize::from(BH) * scale {
        let grays: Vec<u8> = (0..usize::from(BW) * scale)
            .map(|dx| source.gray(dx / scale, dy / scale))
            .collect();
        let y = case.at.y + dy as i32;
        match case.dither {
            Dither::ORDERED => canvas.draw_gray_row(case.at.x, y, &grays),
            _ => {
                for (dx, &gray) in grays.iter().enumerate() {
                    let gray = if keep { 255 - gray } else { gray };
                    canvas.fill_rect(case.at.x + dx as i32, y, 1, 1, Color::gray(gray));
                }
            }
        }
    }
}

/// Draws `case` band by band onto frames that already show something,
/// the fast way and the slow way, and compares them.
fn check(source: &Source, background: &Framebuffer, case: Case) {
    let make = || {
        let mut fb = Framebuffer::banded(W, H, case.rows, case.format).unwrap();
        fb.set_inverted(case.inverted);
        fb.set_pictures_kept(case.kept);
        fb
    };
    let (mut fast, mut slow) = (make(), make());
    let mut top = 0;
    while top < H {
        for fb in [&mut fast, &mut slow] {
            fb.set_top(top);
            fb.write_rows(0, background.view());
        }
        let blit = Blit::at(case.at).scale(case.scale).dither(case.dither);
        with_canvas(&mut fast, case, |canvas| {
            canvas.blit(&source.bitmap(), blit)
        });
        with_canvas(&mut slow, case, |canvas| reference(canvas, source, case));
        let what = format!("{:?} {case:?} band at {top}", source.format);
        assert_eq!(fast.view().data, slow.view().data, "{what}");
        let dirty = slow.take_dirty();
        assert_eq!(fast.take_dirty(), dirty, "{what}");
        let area = dirty.map_or(0, |d| u32::from(d.width) * u32::from(d.height));
        assert_eq!(fast.take_pictured(), area, "{what}");
        top += case.rows;
    }
}

/// A frame of `format` showing grays that look random.
fn background(format: PixelFormat) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H, format).unwrap();
    let mut canvas = fb.canvas();
    for y in 0..H {
        canvas.draw_gray_row(0, i32::from(y), &noise(u32::from(y), usize::from(W)));
    }
    fb
}

fn sources() -> Vec<Source> {
    let formats = [
        BitmapFormat::GRAY1,
        BitmapFormat::GRAY2,
        BitmapFormat::GRAY4,
        BitmapFormat::GRAY8,
    ];
    let mut sources = Vec::new();
    for (i, format) in formats.into_iter().enumerate() {
        let seed = i as u32 * 10;
        sources.push(Source::new(format, seed, false, false));
        sources.push(Source::new(format, seed + 1, true, true));
    }
    sources
}

#[test]
fn a_blit_is_its_pixels_drawn_one_by_one() {
    let places = [(-5, -4), (3, 2), (7, 20), (61, -3), (170, 25), (0, 0)];
    let sources = sources();
    for format in formats() {
        let background = background(format);
        for source in &sources {
            for scale in 1..=4 {
                for (i, &(x, y)) in places.iter().enumerate() {
                    for dither in [Dither::ORDERED, Dither::NONE] {
                        for (inverted, kept) in [(false, false), (true, false), (true, true)] {
                            let case = Case {
                                format,
                                at: Point::new(x, y),
                                scale,
                                dither,
                                inverted,
                                kept,
                                rows: if i % 2 == 0 { H } else { 16 },
                                framed: i % 3 == 1,
                            };
                            check(source, &background, case);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn bitmaps_that_do_not_fit_their_data_are_refused() {
    let data = [0u8; 8];
    assert_eq!(
        Bitmap::new(BitmapFormat::GRAY2, 0, 4, &data),
        Err(BitmapError::ZeroSize)
    );
    assert_eq!(
        Bitmap::new(BitmapFormat::GRAY2, 8, 5, &data),
        Err(BitmapError::DataTooShort { len: 8, needed: 10 })
    );
    let bitmap = Bitmap::new(BitmapFormat::GRAY2, 8, 3, &data).unwrap();
    assert_eq!(
        bitmap.stride(1),
        Err(BitmapError::StrideTooSmall {
            stride: 1,
            needed: 2
        })
    );
    // The last row need not be followed by a whole stride.
    assert!(bitmap.stride(3).is_ok());
    assert!(bitmap.stride(4).is_err());
    assert_eq!(
        bitmap.palette(&[0, 1, 2]),
        Err(BitmapError::PaletteTooShort { len: 3, needed: 4 })
    );
    assert!(bitmap.palette(&[0, 85, 170, 255]).is_ok());
}

#[test]
fn nothing_is_drawn_on_a_blind_frame_or_outside_the_canvas() {
    let data = [0u8; 8];
    let bitmap = Bitmap::new(BitmapFormat::GRAY2, 8, 4, &data).unwrap();
    let mut blind = Framebuffer::blind(20, 20);
    blind.canvas().blit(&bitmap, Blit::at(Point::new(0, 0)));
    assert_eq!(blind.take_all_pictured(), 0);
    let mut fb = Framebuffer::new(20, 20, PixelFormat::Mono1).unwrap();
    fb.canvas()
        .blit(&bitmap, Blit::at(Point::new(-100, 5)).scale(3));
    fb.canvas().blit(&bitmap, Blit::at(Point::new(20, 5)));
    assert_eq!(fb.take_dirty(), None);
    assert_eq!(fb.take_pictured(), 0);
    // Scales past the range are the nearest in it.
    fb.canvas()
        .blit(&bitmap, Blit::at(Point::new(0, 0)).scale(0));
    assert_eq!(fb.take_dirty(), Some(Rect::new(0, 0, 8, 4)));
    fb.canvas()
        .blit(&bitmap, Blit::at(Point::new(-1, -1)).scale(200));
    assert_eq!(fb.take_dirty(), Some(Rect::new(0, 0, 20, 20)));
}

/// Microseconds per frame of a 160×144 bitmap of 2 bits a pixel at scale
/// 3 onto a 480×800 frame of 1 bit a pixel, blitted and drawn as gray
/// rows, whole and in bands of 160 rows. Run with
/// `cargo test --release -p paperpaw-graphics --test blit -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_a_game_screen() {
    use std::time::Instant;
    const FRAMES: u32 = 2000;
    let data = noise(3, BitmapFormat::GRAY2.stride(160) * 144);
    let bitmap = Bitmap::new(BitmapFormat::GRAY2, 160, 144, &data).unwrap();
    let at = Point::new(0, 184);
    let grays: Vec<Vec<u8>> = (0..432)
        .map(|y| {
            (0..480)
                .map(|x| {
                    let (sx, sy) = (x / 3, y / 3);
                    let byte = data[sy * 40 + sx / 4];
                    ((byte >> (6 - 2 * (sx % 4))) & 3) * 85
                })
                .collect()
        })
        .collect();
    for (rows, label) in [(800, "whole"), (160, "bands of 160")] {
        for dither in [Dither::ORDERED, Dither::NONE] {
            let mut fb = Framebuffer::banded(480, 800, rows, PixelFormat::Mono1).unwrap();
            let blit = Blit::at(at).scale(3).dither(dither);
            let start = Instant::now();
            for _ in 0..FRAMES {
                for top in (0..800).step_by(usize::from(rows)) {
                    fb.set_top(top);
                    fb.canvas().blit(&bitmap, blit);
                }
            }
            let each = start.elapsed().as_secs_f64() * 1e6 / f64::from(FRAMES);
            println!("blit {dither:?}, {label}: {each:.1} us a frame");
        }
        let mut fb = Framebuffer::banded(480, 800, rows, PixelFormat::Mono1).unwrap();
        let start = Instant::now();
        for _ in 0..FRAMES / 10 {
            for top in (0..800).step_by(usize::from(rows)) {
                fb.set_top(top);
                let mut canvas = fb.canvas();
                for (y, row) in grays.iter().enumerate() {
                    canvas.draw_gray_row(at.x, at.y + y as i32, row);
                }
            }
        }
        let each = start.elapsed().as_secs_f64() * 1e6 / f64::from(FRAMES / 10);
        println!("gray rows, {label}: {each:.1} us a frame");
    }
}
