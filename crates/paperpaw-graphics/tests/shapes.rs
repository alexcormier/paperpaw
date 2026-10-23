//! Shapes come out as their geometry says: circles symmetric and of
//! their area, arcs of a ring that add up to the ring, lines through
//! their ends, outlines as thick as asked, rounded rectangles inside
//! their rectangle and symmetric; and the whole-number trigonometry of
//! `round` close to the real one for any angle.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::config;
use paperpaw_graphics::{Framebuffer, round};
use paperpaw_platform::{Color, PixelFormat, Point};
use proptest::prelude::*;

const S: u16 = 64;

/// How dark each pixel of a gray screen is, 0 to 255, after `draw`.
fn darkness(draw: impl FnOnce(&mut paperpaw_graphics::Canvas<'_>)) -> Vec<Vec<u32>> {
    let mut fb = Framebuffer::new(S, S, PixelFormat::Gray8).unwrap();
    draw(&mut fb.canvas());
    let view = fb.view();
    (0..S)
        .map(|y| {
            (0..S)
                .map(|x| 255 - u32::from(view.pixel(x, y).r))
                .collect()
        })
        .collect()
}

proptest! {
    #![proptest_config(config(128))]

    /// For any angle: sine and cosine on the unit circle (to the table's
    /// precision), the same a turn later, and odd and even as they are.
    #[test]
    fn sines_and_cosines_hold_for_any_angle(degrees in any::<i32>()) {
        let (sin, cos) = round::sin_cos(degrees);
        let unit = i64::from(round::UNIT);
        let norm = i64::from(sin) * i64::from(sin) + i64::from(cos) * i64::from(cos);
        prop_assert!((norm - unit * unit).abs() <= 2 * unit, "{}", norm);
        let real = f64::from(degrees.rem_euclid(360)).to_radians();
        prop_assert!((f64::from(sin) - real.sin() * 1e4).abs() <= 1.0);
        prop_assert!((f64::from(cos) - real.cos() * 1e4).abs() <= 1.0);
        if let Some(later) = degrees.checked_add(360) {
            prop_assert_eq!(round::sin_cos(later), (sin, cos));
        }
        if let Some(negative) = degrees.checked_neg() {
            prop_assert_eq!(round::sin_cos(negative), (-sin, cos));
        }
    }

    /// Points `radius` from the centre lie that far from it, to a pixel
    /// and a half (see `points_round_to_the_nearest_pixel_either_way`
    /// for the half).
    #[test]
    fn points_lie_on_their_circle(
        cx in -10_000i32..10_000,
        cy in -10_000i32..10_000,
        radius in 0i32..20_000,
        degrees in -720i32..720,
    ) {
        let p = round::point_at(Point::new(cx, cy), radius, degrees);
        let (dx, dy) = (f64::from(p.x - cx), f64::from(p.y - cy));
        let distance = (dx * dx + dy * dy).sqrt();
        prop_assert!((distance - f64::from(radius)).abs() <= 1.5 + f64::from(radius) * 2e-4, "{}", distance);
    }

    /// A disc is symmetric about its centre's axes and its diagonals,
    /// and covers its area.
    #[test]
    fn discs_are_symmetric_and_of_their_area(radius in 1u32..30) {
        let c = 32i32;
        let disc = darkness(|canvas| canvas.fill_circle(Point::new(c, c), radius, Color::BLACK));
        for y in 0..64usize {
            for x in 0..64usize {
                let v = disc[y][x];
                prop_assert_eq!(v, disc[y][63 - x]);
                prop_assert_eq!(v, disc[63 - y][x]);
                prop_assert_eq!(v, disc[x][y]);
            }
        }
        let area: f64 = disc.iter().flatten().map(|&v| f64::from(v) / 255.0).sum();
        let expected = std::f64::consts::PI * f64::from(radius * radius);
        prop_assert!((area - expected).abs() <= expected * 0.02 + 2.0, "{} for {}", area, expected);
        // Nothing beyond its radius.
        for (y, row) in disc.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                let (dx, dy) = (x as f64 + 0.5 - 32.0, y as f64 + 0.5 - 32.0);
                if (dx * dx + dy * dy).sqrt() > f64::from(radius) + 0.75 {
                    prop_assert_eq!(v, 0, "({}, {})", x, y);
                }
            }
        }
    }

    /// An arc and the rest of its ring cover the ring between them.
    /// (Arcs at least a quarter turn, of rings with a hole: a pixel is
    /// sampled only where one of its corners lies on the arc, so an arc
    /// narrower than a pixel may pass between them.)
    #[test]
    fn an_arc_and_the_rest_make_the_ring(
        radius in 4u32..30,
        hole in 2u32..30,
        from in -360i32..360,
        sweep in 90u32..=270,
    ) {
        let thickness = radius - hole.min(radius - 2);
        let c = Point::new(32, 32);
        let ring = darkness(|canvas| canvas.fill_ring(c, radius, thickness, from, 360, Color::BLACK));
        let arc = darkness(|canvas| canvas.fill_ring(c, radius, thickness, from, sweep, Color::BLACK));
        let rest = darkness(|canvas| {
            canvas.fill_ring(c, radius, thickness, from + sweep as i32, 360 - sweep, Color::BLACK);
        });
        for y in 0..64 {
            for x in 0..64 {
                let sum = arc[y][x] + rest[y][x];
                // Rounding of the blend: a level each (half turns: see
                // `two_half_rings_make_the_ring`).
                prop_assert!(
                    sum.abs_diff(ring[y][x]) <= 2 || sweep == 180,
                    "({}, {}): {} + {} for {}", x, y, arc[y][x], rest[y][x], ring[y][x]
                );
            }
        }
    }

    /// A line covers both its ends and nothing farther from it than its
    /// pen reaches.
    #[test]
    fn lines_go_through_their_ends(
        (x0, y0, x1, y1) in (0i32..64, 0i32..64, 0i32..64, 0i32..64),
        width in 1u32..6,
    ) {
        let (a, b) = (Point::new(x0, y0), Point::new(x1, y1));
        let line = darkness(|canvas| canvas.draw_line(a, b, width, Color::BLACK));
        prop_assert_eq!(line[y0 as usize][x0 as usize], 255);
        prop_assert_eq!(line[y1 as usize][x1 as usize], 255);
        let (dx, dy) = (f64::from(x1 - x0), f64::from(y1 - y0));
        let length = (dx * dx + dy * dy).sqrt();
        for (y, row) in line.iter().enumerate() {
            for (x, &v) in row.iter().enumerate() {
                if v == 0 {
                    continue;
                }
                let (px, py) = (x as f64 - f64::from(x0), y as f64 - f64::from(y0));
                let t = if length == 0.0 { 0.0 } else { ((px * dx + py * dy) / (length * length)).clamp(0.0, 1.0) };
                let (ex, ey) = (px - t * dx, py - t * dy);
                prop_assert!((ex * ex + ey * ey).sqrt() <= f64::from(width) * 0.75 + 1.0, "({}, {})", x, y);
            }
        }
    }

    /// Rounded rectangles: inside their rectangle, symmetric, their
    /// middle filled, their outline as thick as asked along the sides.
    #[test]
    fn rounded_rectangles_keep_their_shape(
        (w, h) in (1u32..60, 1u32..60),
        radius in 0u32..40,
        thickness in 1u32..10,
    ) {
        let (x0, y0) = (2i32, 2i32);
        let fill = darkness(|canvas| canvas.fill_round_rect(x0, y0, w, h, radius, Color::BLACK));
        let outline = darkness(|canvas| canvas.stroke_round_rect(x0, y0, w, h, thickness, radius, Color::BLACK));
        let inside = |x: usize, y: usize| {
            (x0 as usize..x0 as usize + w as usize).contains(&x) && (y0 as usize..y0 as usize + h as usize).contains(&y)
        };
        for y in 0..64usize {
            for x in 0..64usize {
                if !inside(x, y) {
                    prop_assert_eq!(fill[y][x] + outline[y][x], 0);
                    continue;
                }
                // Mirrored within the rectangle.
                let mx = 2 * x0 as usize + w as usize - 1 - x;
                let my = 2 * y0 as usize + h as usize - 1 - y;
                prop_assert_eq!(fill[y][x], fill[y][mx]);
                prop_assert_eq!(fill[y][x], fill[my][x]);
                prop_assert_eq!(outline[y][x], outline[y][mx]);
                prop_assert_eq!(outline[y][x], outline[my][x]);
                // The outline lies on the filled shape.
                prop_assert!(outline[y][x] <= fill[y][x]);
            }
        }
        // The middle of the top and bottom: as thick as asked, at most
        // half the height and one (where the sides leave it free).
        let mid_x = (x0 + w as i32 / 2) as usize;
        // Where neither the sides nor the hole's rounding reach it.
        let t = thickness.min(w / 2 + 1).min(h / 2 + 1);
        let r = radius.min(w / 2).min(h / 2);
        if t.max(r) < w.div_ceil(2) {
            let t = t as usize;
            let thick = (0..h as usize).filter(|&dy| outline[y0 as usize + dy][mid_x] == 255).count();
            prop_assert_eq!(thick, (2 * t).min(h as usize));
        }
        // Its middle filled (but two pixels wide and rounded it is
        // empty: see `rounded_rectangles_reach_their_sides_where_the_circle_does`).
        if r == 0 || w > 2 {
            prop_assert_eq!(fill[(y0 + h as i32 / 2) as usize][mid_x], 255);
        }
    }
}

#[test]
fn points_round_to_the_nearest_pixel_either_way() {
    // `point_at` says "rounded to the nearest pixel", but its first
    // division truncates toward zero: at 243 degrees and radius 359 the
    // point is (-319, 163) from the centre, the nearest (-320, 163). So
    // a clock's left half is drawn up to a pixel off its right half.
    for radius in 1..300 {
        for degrees in 0..360 {
            let p = round::point_at(Point::new(0, 0), radius, degrees);
            let (sin, cos) = round::sin_cos(degrees);
            let x = f64::from(sin) * f64::from(radius) / 1e4;
            let y = -f64::from(cos) * f64::from(radius) / 1e4;
            assert!(
                (f64::from(p.x) - x).abs() <= 0.5 && (f64::from(p.y) - y).abs() <= 0.5,
                "{degrees} degrees, radius {radius}: {p:?} for ({x}, {y})"
            );
        }
    }
}

#[test]
fn rounded_rectangles_reach_their_sides_where_the_circle_does() {
    // `corner_inset` rounds the inset up to whole pixels, so every row
    // within the radius of an edge loses at least a pixel on each side,
    // however little of it lies outside: a pill (radius half its
    // height) never touches its rectangle's sides, where a circle of
    // its ends does, and a 2 × 2 square of radius 1 draws nothing.
    let pill = darkness(|canvas| canvas.fill_round_rect(0, 0, 40, 20, 10, Color::BLACK));
    let disc = darkness(|canvas| canvas.fill_circle(Point::new(10, 10), 10, Color::BLACK));
    assert!(disc[9][0] > 128 && disc[10][0] > 128);
    assert_eq!((pill[9][0], pill[10][0], pill[9][39]), (255, 255, 255));
    let dot = darkness(|canvas| canvas.fill_round_rect(5, 5, 2, 2, 1, Color::BLACK));
    assert!(dot.iter().flatten().any(|&v| v > 0));
}

#[test]
fn two_half_rings_make_the_ring() {
    // `Arc::holds` takes a direction on the arc when it is not
    // clockwise of `from` and clockwise of `to`; for half a turn `to` is
    // `from` reversed, so the directions along that line are neither,
    // for either half. Sampled pixels on it (a diagonal: from 45 or 135
    // degrees) are missing from both halves: a seam where two halves
    // of a ring, or a half-full progress ring and its rest, meet.
    let c = Point::new(32, 32);
    for from in [45, 135, -135] {
        let ring = darkness(|canvas| canvas.fill_ring(c, 20, 6, from, 360, Color::BLACK));
        let one = darkness(|canvas| canvas.fill_ring(c, 20, 6, from, 180, Color::BLACK));
        let other = darkness(|canvas| canvas.fill_ring(c, 20, 6, from + 180, 180, Color::BLACK));
        for y in 0..64 {
            for x in 0..64 {
                let sum = one[y][x] + other[y][x];
                assert!(
                    sum.abs_diff(ring[y][x]) <= 2,
                    "from {from}, ({x}, {y}): {sum} for {}",
                    ring[y][x]
                );
            }
        }
    }
}

#[test]
fn hands_point_where_a_clock_shows() {
    let c = Point::new(32, 32);
    for (degrees, tip) in [
        (0, (32, 12)),
        (90, (52, 32)),
        (180, (32, 52)),
        (270, (12, 32)),
    ] {
        let hand = darkness(|canvas| canvas.draw_hand(c, degrees, 20, 5, 1, Color::BLACK));
        assert_eq!(hand[tip.1][tip.0], 255, "{degrees}");
        // The tail, the other way.
        let back = round::point_at(c, 5, degrees + 180);
        assert_eq!(hand[back.y as usize][back.x as usize], 255);
        // Beyond the tip, nothing.
        let beyond = round::point_at(c, 23, degrees);
        assert_eq!(hand[beyond.y as usize][beyond.x as usize], 0);
    }
}

#[test]
fn outlines_grow_inwards_and_cover_small_rectangles() {
    let outline = darkness(|canvas| canvas.stroke_rect(10, 10, 20, 12, 3, Color::BLACK));
    for (y, row) in outline.iter().enumerate() {
        for (x, &v) in row.iter().enumerate() {
            let inside = (10..30).contains(&x) && (10..22).contains(&y);
            let border = inside && !((13..27).contains(&x) && (13..19).contains(&y));
            assert_eq!(v == 255, border, "({x}, {y})");
        }
    }
    // Thicker than half the rectangle: all of it.
    let full = darkness(|canvas| canvas.stroke_rect(0, 0, 5, 4, 10, Color::BLACK));
    for row in &full[..4] {
        assert!(row[..5].iter().all(|&v| v == 255));
    }
    // Of no thickness: nothing.
    let none = darkness(|canvas| canvas.stroke_rect(0, 0, 5, 4, 0, Color::BLACK));
    assert!(none.iter().flatten().all(|&v| v == 0));
}

#[test]
fn rings_and_arcs_of_nothing_draw_nothing() {
    let c = Point::new(32, 32);
    for (radius, thickness, sweep) in [(0, 5, 360), (10, 0, 360), (10, 5, 0)] {
        let ring =
            darkness(|canvas| canvas.fill_ring(c, radius, thickness, 0, sweep, Color::BLACK));
        assert!(
            ring.iter().flatten().all(|&v| v == 0),
            "{radius} {thickness} {sweep}"
        );
    }
    // Sweeps past a whole turn: the whole ring.
    let whole = darkness(|canvas| canvas.fill_ring(c, 10, 3, 0, 360, Color::BLACK));
    let more = darkness(|canvas| canvas.fill_ring(c, 10, 3, 77, 1000, Color::BLACK));
    assert_eq!(whole, more);
}

/// A blind framebuffer keeps no pixels: everything drawn on it is only
/// described, in the clips it was drawn in, as on a real one.
#[test]
fn a_blind_framebuffer_describes_what_is_drawn_and_keeps_no_pixels() {
    use paperpaw_graphics::{Described, Role};
    use paperpaw_platform::{Color, Point, Rect};
    let mut blind = Framebuffer::blind(800, 480);
    assert_eq!(blind.byte_len(), 0);
    assert_eq!(blind.bounds(), Rect::new(0, 0, 800, 480));
    let mut described: Vec<Described> = Vec::new();
    {
        let mut canvas = blind.canvas().describing(&mut described);
        canvas.clear(Color::WHITE);
        canvas.fill_round(Rect::new(10, 10, 200, 60), 12, Color::BLACK);
        canvas.stroke_round(Rect::new(10, 100, 200, 60), 3, 12, Color::gray(90));
        canvas.draw_line(Point::new(0, 0), Point::new(799, 479), 3, Color::BLACK);
        canvas.fill_circle(Point::new(400, 240), 50, Color::gray(150));
        canvas.draw_gray_row(0, 300, &[0, 128, 255, 64]);
        canvas.set_pixel(Point::new(5, 5), Color::BLACK);
        let mut row = canvas.clipped(Rect::new(0, 400, 800, 80));
        row.describe(Role::Button, "OK", Rect::new(20, 410, 100, 50))
            .focused(true);
    }
    assert_eq!(blind.byte_len(), 0);
    assert_eq!(blind.view().height, 0);
    assert_eq!(described.len(), 1);
    assert_eq!(described[0].label, "OK");
    assert_eq!(described[0].clip, Rect::new(0, 400, 800, 80));
}
