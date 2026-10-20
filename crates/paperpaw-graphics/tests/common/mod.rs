//! Shared by the drawing tests: every drawing call of a canvas as a
//! value, so that property tests can draw any sequence of them, and the
//! pixel formats this build has.

// Each test file uses a part of it.
#![allow(dead_code)]

use paperpaw_graphics::{Canvas, Framebuffer};
use paperpaw_platform::{Color, PixelFormat, Point, Rect};
use proptest::prelude::*;

/// The pixel formats compiled in.
pub fn formats() -> Vec<PixelFormat> {
    let mut formats = vec![
        PixelFormat::Mono1,
        PixelFormat::Gray2,
        PixelFormat::Gray4,
        PixelFormat::Gray8,
    ];
    if cfg!(feature = "color") {
        formats.extend([PixelFormat::Rgb565, PixelFormat::Rgb888]);
    }
    formats
}

pub fn format() -> impl Strategy<Value = PixelFormat> {
    let formats = formats();
    (0..formats.len()).prop_map(move |i| formats[i])
}

/// Property tests of `cases` cases; failures are turned into named
/// tests, not kept in files.
pub fn config(cases: u32) -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        cases,
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    }
}

/// A drawing call.
#[derive(Debug, Clone)]
pub enum Op {
    Clear(Color),
    Fill(Rect, Color),
    StrokeArea(Rect, u32, Color),
    FillRoundArea(Rect, u32, Color),
    StrokeRoundArea(Rect, u32, u32, Color),
    FillRect(i32, i32, u32, u32, Color),
    Stroke(i32, i32, u32, u32, u32, Color),
    FillRound(i32, i32, u32, u32, u32, Color),
    StrokeRound(i32, i32, u32, u32, u32, u32, Color),
    Tint(Rect, Color),
    Shade(Rect, Color),
    ShadeRound(Rect, u32, Color),
    Wash(Rect),
    Line(Point, Point, u32, Color),
    Circle(Point, u32, Color),
    Ring(Point, u32, u32, i32, u32, Color),
    Hand(Point, i32, u32, u32, u32, Color),
    Pixel(Point, Color),
    GrayRow(i32, i32, Vec<u8>),
    RgbRow(i32, i32, Vec<u8>),
    /// A mask whose coverage is a pattern of its seed.
    Mask(i32, i32, u32, u32, u8, Color),
    SmallPictures(Vec<u8>),
}

/// Draws `op`.
pub fn draw(canvas: &mut Canvas<'_>, op: &Op) {
    match op.clone() {
        Op::Clear(color) => canvas.clear(color),
        Op::Fill(area, color) => canvas.fill(area, color),
        Op::StrokeArea(area, t, color) => canvas.stroke(area, t, color),
        Op::FillRoundArea(area, r, color) => canvas.fill_round(area, r, color),
        Op::StrokeRoundArea(area, t, r, color) => canvas.stroke_round(area, t, r, color),
        Op::FillRect(x, y, w, h, color) => canvas.fill_rect(x, y, w, h, color),
        Op::Stroke(x, y, w, h, t, color) => canvas.stroke_rect(x, y, w, h, t, color),
        Op::FillRound(x, y, w, h, r, color) => canvas.fill_round_rect(x, y, w, h, r, color),
        Op::StrokeRound(x, y, w, h, t, r, color) => {
            canvas.stroke_round_rect(x, y, w, h, t, r, color);
        }
        Op::Tint(area, color) => canvas.fill_tint(area, color),
        Op::Shade(area, color) => canvas.fill_shaded(area, color),
        Op::ShadeRound(area, r, color) => canvas.fill_round_shaded(area, r, color),
        Op::Wash(area) => canvas.wash(area),
        Op::Line(from, to, w, color) => canvas.draw_line(from, to, w, color),
        Op::Circle(c, r, color) => canvas.fill_circle(c, r, color),
        Op::Ring(c, r, t, from, sweep, color) => canvas.fill_ring(c, r, t, from, sweep, color),
        Op::Hand(c, degrees, length, tail, w, color) => {
            canvas.draw_hand(c, degrees, length, tail, w, color);
        }
        Op::Pixel(p, color) => canvas.set_pixel(p, color),
        Op::GrayRow(x, y, row) => canvas.draw_gray_row(x, y, &row),
        Op::RgbRow(x, y, row) => canvas.draw_rgb_row(x, y, &row),
        Op::Mask(x, y, w, h, seed, color) => canvas.draw_mask(x, y, w, h, color, |dx, dy| {
            (dx.wrapping_mul(31) ^ dy.wrapping_mul(17) ^ u32::from(seed)) as u8
        }),
        Op::SmallPictures(row) => canvas.small_pictures(|c| c.draw_gray_row(1, 1, &row)),
    }
}

/// Colours: grays, or any where `rgb`.
pub fn color(rgb: bool) -> BoxedStrategy<Color> {
    if rgb {
        any::<(u8, u8, u8)>()
            .prop_map(|(r, g, b)| Color::rgb(r, g, b))
            .boxed()
    } else {
        prop_oneof![
            Just(Color::BLACK),
            Just(Color::WHITE),
            any::<u8>().prop_map(Color::gray)
        ]
        .boxed()
    }
}

/// Drawing calls about a screen of some 60 × 40 pixels: reaching past
/// its edges on every side.
pub fn op(rgb: bool) -> impl Strategy<Value = Op> {
    let c = || color(rgb);
    let xy = || -80i32..140;
    let size = || 0u32..160;
    let rect =
        || (0u16..90, 0u16..70, 0u16..90, 0u16..70).prop_map(|(x, y, w, h)| Rect::new(x, y, w, h));
    let point = || (-80i32..140, -80i32..140).prop_map(|(x, y)| Point::new(x, y));
    prop_oneof![
        c().prop_map(Op::Clear),
        (rect(), c()).prop_map(|(r, c)| Op::Fill(r, c)),
        (rect(), 0u32..20, c()).prop_map(|(r, t, c)| Op::StrokeArea(r, t, c)),
        (rect(), 0u32..60, c()).prop_map(|(r, radius, c)| Op::FillRoundArea(r, radius, c)),
        (rect(), 0u32..20, 0u32..60, c())
            .prop_map(|(r, t, radius, c)| Op::StrokeRoundArea(r, t, radius, c)),
        (xy(), xy(), size(), size(), c()).prop_map(|(x, y, w, h, c)| Op::FillRect(x, y, w, h, c)),
        (xy(), xy(), size(), size(), 0u32..30, c())
            .prop_map(|(x, y, w, h, t, c)| Op::Stroke(x, y, w, h, t, c)),
        (xy(), xy(), size(), size(), 0u32..90, c())
            .prop_map(|(x, y, w, h, r, c)| Op::FillRound(x, y, w, h, r, c)),
        (xy(), xy(), size(), size(), 0u32..30, 0u32..90, c())
            .prop_map(|(x, y, w, h, t, r, c)| Op::StrokeRound(x, y, w, h, t, r, c)),
        (rect(), c()).prop_map(|(r, c)| Op::Tint(r, c)),
        (rect(), c()).prop_map(|(r, c)| Op::Shade(r, c)),
        (rect(), 0u32..60, c()).prop_map(|(r, radius, c)| Op::ShadeRound(r, radius, c)),
        rect().prop_map(Op::Wash),
        (point(), point(), 0u32..12, c()).prop_map(|(a, b, w, c)| Op::Line(a, b, w, c)),
        (point(), 0u32..70, c()).prop_map(|(p, r, c)| Op::Circle(p, r, c)),
        (point(), 0u32..70, 0u32..80, -720i32..720, 0u32..400, c())
            .prop_map(|(p, r, t, from, sweep, c)| Op::Ring(p, r, t, from, sweep, c)),
        (point(), -720i32..720, 0u32..90, 0u32..30, 0u32..8, c())
            .prop_map(|(p, d, l, t, w, c)| Op::Hand(p, d, l, t, w, c)),
        (point(), c()).prop_map(|(p, c)| Op::Pixel(p, c)),
        (xy(), xy(), proptest::collection::vec(any::<u8>(), 0..120))
            .prop_map(|(x, y, row)| Op::GrayRow(x, y, row)),
        (xy(), xy(), proptest::collection::vec(any::<u8>(), 0..360))
            .prop_map(|(x, y, row)| Op::RgbRow(x, y, row)),
        (xy(), xy(), 0u32..90, 0u32..60, any::<u8>(), c())
            .prop_map(|(x, y, w, h, s, c)| Op::Mask(x, y, w, h, s, c)),
        proptest::collection::vec(any::<u8>(), 0..80).prop_map(Op::SmallPictures),
    ]
}

/// Every pixel of the framebuffer's band, row by row.
pub fn pixels(fb: &Framebuffer) -> Vec<Color> {
    let view = fb.view();
    (0..view.height)
        .flat_map(|y| (0..view.width).map(move |x| (x, y)))
        .map(|(x, y)| view.pixel(x, y))
        .collect()
}
