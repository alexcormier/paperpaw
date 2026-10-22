//! Drawing stays inside the canvas: every drawing call, at any place and
//! size (past the screen's edges, at the extremes of `i32` and `u32`),
//! in every pixel format, changes no pixel outside its clip region, and
//! the region it reports changed holds every pixel it changed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Op, config, draw, format, op, pixels};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat, Point, Rect};
use proptest::prelude::*;

const W: u16 = 61;
const H: u16 = 37;

/// Draws `ops` into a clipped canvas of a fresh framebuffer; checks that
/// nothing outside the clip changed and that the dirty region covers
/// every change and lies inside the clip.
fn check_clipped(format: PixelFormat, clip: Rect, ops: &[Op]) {
    let mut fb = Framebuffer::new(W, H, format).unwrap();
    // A pattern, so that any change shows.
    {
        let mut canvas = fb.canvas();
        for y in 0..i32::from(H) {
            for x in 0..i32::from(W) {
                let gray = if (x * 7 + y * 3) % 5 == 0 { 0 } else { 255 };
                canvas.set_pixel(Point::new(x, y), Color::gray(gray));
            }
        }
    }
    fb.take_dirty();
    let before = pixels(&fb);
    {
        let mut canvas = fb.canvas();
        let mut clipped = canvas.clipped(clip);
        let inside = clip
            .intersection(&Rect::from_size(W, H))
            .unwrap_or_default();
        assert_eq!(clipped.clip(), inside);
        for op in ops {
            draw(&mut clipped, op);
        }
    }
    let dirty = fb.take_dirty();
    let after = pixels(&fb);
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        if a == b {
            continue;
        }
        let p = Point::new((i % usize::from(W)) as i32, (i / usize::from(W)) as i32);
        assert!(
            clip.contains(p),
            "{format:?}: {p:?} changed outside {clip:?}"
        );
        let dirty = dirty.unwrap_or_else(|| panic!("{format:?}: {p:?} changed, nothing dirty"));
        assert!(
            dirty.contains(p),
            "{format:?}: {p:?} changed outside dirty {dirty:?}"
        );
    }
    if let Some(dirty) = dirty {
        assert_eq!(
            dirty.intersection(&clip),
            (!dirty.is_empty()).then_some(dirty),
            "{format:?}: dirty {dirty:?} outside {clip:?}"
        );
    }
}

fn clip() -> impl Strategy<Value = Rect> {
    (0u16..80, 0u16..50, 0u16..80, 0u16..50).prop_map(|(x, y, w, h)| Rect::new(x, y, w, h))
}

/// The drawing calls that report what they change (see
/// `shading_on_few_grays_is_reported_dirty` for those that do not).
fn reported(op: &Op, format: PixelFormat) -> bool {
    let few_grays = matches!(format.gray_levels(), Some(2..=16));
    !(few_grays && matches!(op, Op::Shade(..) | Op::ShadeRound(..)))
}

proptest! {
    #![proptest_config(config(256))]

    #[test]
    fn drawing_stays_inside_the_clip(
        format in format(),
        clip in clip(),
        ops in proptest::collection::vec(op(true), 1..6),
    ) {
        let ops: Vec<Op> = ops.into_iter().filter(|op| reported(op, format)).collect();
        check_clipped(format, clip, &ops);
    }
}

#[test]
fn rectangles_at_the_extremes_are_clipped() {
    // The calls that clip their area first, at any i32 and u32.
    let extremes = [
        i32::MIN,
        i32::MIN + 1,
        -70_000,
        -1,
        0,
        30,
        70_000,
        i32::MAX - 1,
        i32::MAX,
    ];
    let sizes = [0u32, 1, 40, 70_000, u32::MAX / 2, u32::MAX - 1, u32::MAX];
    for format in common::formats() {
        let mut ops = Vec::new();
        for &x in &extremes {
            for &y in &extremes {
                for &size in &sizes {
                    ops.push(Op::FillRect(x, y, size, size, Color::BLACK));
                    ops.push(Op::FillRect(x, y, size, 3, Color::gray(90)));
                    ops.push(Op::Pixel(Point::new(x, y), Color::BLACK));
                    ops.push(Op::GrayRow(x, y, vec![10; 70]));
                    ops.push(Op::RgbRow(x, y, vec![200; 3 * 70]));
                    // Masks from the far left: see
                    // `masks_from_the_far_left_do_not_panic`.
                    if x > i32::MIN + 1 && y > i32::MIN + 1 {
                        ops.push(Op::Mask(x, y, size, size.min(3), 7, Color::BLACK));
                    }
                }
            }
        }
        for clip in [
            Rect::new(5, 6, 20, 10),
            Rect::from_size(W, H),
            Rect::new(60, 36, 9, 9),
        ] {
            check_clipped(format, clip, &ops);
        }
    }
}

#[test]
fn rect_areas_at_the_extremes_are_clipped() {
    let areas = [
        Rect::new(u16::MAX, u16::MAX, u16::MAX, u16::MAX),
        Rect::new(0, 0, u16::MAX, u16::MAX),
        Rect::new(60, 36, u16::MAX, 1),
        Rect::new(10, 10, 0, 5),
    ];
    for format in common::formats() {
        let mut ops = Vec::new();
        for area in areas {
            ops.extend([
                Op::Fill(area, Color::BLACK),
                Op::Tint(area, Color::gray(200)),
                Op::Wash(area),
            ]);
            if !matches!(format.gray_levels(), Some(2..=16)) {
                ops.push(Op::Shade(area, Color::gray(100)));
            }
        }
        check_clipped(format, Rect::new(3, 4, 30, 20), &ops);
    }
}

#[test]
fn shapes_far_outside_draw_nothing() {
    for format in common::formats() {
        let far = [-1_000_000i32, 1_000_000];
        let mut ops = Vec::new();
        for &x in &far {
            for &y in &far {
                ops.push(Op::Stroke(x, y, 50, 50, 3, Color::BLACK));
                ops.push(Op::FillRound(x, y, 50, 50, 10, Color::BLACK));
                ops.push(Op::StrokeRound(x, y, 50, 50, 3, 10, Color::BLACK));
                ops.push(Op::Circle(Point::new(x, y), 1000, Color::BLACK));
                ops.push(Op::Ring(Point::new(x, y), 1000, 10, 30, 200, Color::BLACK));
                ops.push(Op::Line(
                    Point::new(x, y),
                    Point::new(x + 40_000, y),
                    3,
                    Color::BLACK,
                ));
            }
        }
        let mut fb = Framebuffer::new(W, H, format).unwrap();
        let before = pixels(&fb);
        {
            let mut canvas = fb.canvas();
            for op in &ops {
                draw(&mut canvas, op);
            }
        }
        assert_eq!(pixels(&fb), before);
    }
}

#[test]
fn a_circle_much_larger_than_the_screen_covers_it() {
    // Radius 10 000 about the middle: every pixel inside.
    let mut fb = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    fb.canvas()
        .fill_circle(Point::new(30, 18), 10_000, Color::BLACK);
    assert!(pixels(&fb).iter().all(|&c| c == Color::BLACK));
}

/// Draws `op` into a small framebuffer, catching a panic.
fn panics(op: Op) -> bool {
    std::panic::catch_unwind(|| {
        let mut fb = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
        draw(&mut fb.canvas(), &op);
    })
    .is_err()
}

#[test]
fn outlines_at_the_extremes_do_not_panic() {
    // `x + width as i32 - t as i32` and `y + height as i32 - ...`
    // overflow: a panic in debug builds.
    assert!(!panics(Op::Stroke(
        i32::MAX - 5,
        0,
        10,
        10,
        1,
        Color::BLACK
    )));
    assert!(!panics(Op::Stroke(0, i32::MAX, 10, 10, 1, Color::BLACK)));
}

#[test]
fn rounded_rectangles_at_the_extremes_do_not_panic() {
    // `y + row as i32` and `x + inset as i32` overflow.
    assert!(!panics(Op::FillRound(
        0,
        i32::MAX - 2,
        10,
        10,
        4,
        Color::BLACK
    )));
    assert!(!panics(Op::StrokeRound(
        0,
        i32::MAX - 2,
        10,
        10,
        2,
        4,
        Color::BLACK
    )));
}

#[test]
fn long_lines_do_not_panic() {
    // `dx * step` is an i32: past 46340 pixels it overflows (a panic in
    // debug builds, a line in the wrong place in release); so does
    // `to.x - from.x` between the extremes.
    assert!(!panics(Op::Line(
        Point::new(0, 5),
        Point::new(50_000, 5),
        1,
        Color::BLACK
    )));
    assert!(!panics(Op::Line(
        Point::new(i32::MIN, 5),
        Point::new(i32::MAX, 5),
        1,
        Color::BLACK
    )));
}

#[test]
fn masks_from_the_far_left_do_not_panic() {
    // The clip is reckoned in i64, but the place within the mask handed
    // to the coverage, `i32::from(px) - x`, is an i32: from x = i32::MIN
    // to any pixel at 0 or right of it, it overflows.
    assert!(!panics(Op::Mask(i32::MIN, 0, u32::MAX, 3, 7, Color::BLACK)));
    assert!(!panics(Op::Mask(
        0,
        i32::MIN + 1,
        3,
        u32::MAX,
        7,
        Color::BLACK
    )));
}

#[test]
fn round_shapes_at_the_extremes_do_not_panic() {
    // `from + sweep as i32` (round::Arc::new), `2 * radius` and
    // `center.x - r` (fill_ring), `degrees + 180` (draw_hand) overflow.
    let c = Point::new(30, 18);
    assert!(!panics(Op::Ring(c, 10, 2, i32::MAX, 90, Color::BLACK)));
    assert!(!panics(Op::Circle(c, u32::MAX / 2 + 1, Color::BLACK)));
    assert!(!panics(Op::Circle(
        Point::new(i32::MIN, 0),
        10,
        Color::BLACK
    )));
    assert!(!panics(Op::Hand(c, i32::MAX, 10, 5, 1, Color::BLACK)));
}
