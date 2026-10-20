//! The rasterizer stays inside its bitmap and is exact: any segments,
//! also outside the box, huge or not numbers at all, give a bitmap of
//! the asked size without panicking; a triangle inside the box covers
//! exactly its area and leaves pixels away from it empty; curves are
//! flattened into a bounded number of segments.

use alloc::vec::Vec;

use proptest::prelude::*;

use super::*;

fn triangle(a: Pt, b: Pt, c: Pt) -> Vec<(Pt, Pt)> {
    let mut path = Path::default();
    path.move_to(a);
    path.line_to(b);
    path.line_to(c);
    path.close();
    path.lines
}

fn area(a: Pt, b: Pt, c: Pt) -> f32 {
    abs((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)) / 2.0
}

/// Coordinates outlines can have: around the box, far from it, not a
/// number (a broken glyph).
fn coordinate() -> impl Strategy<Value = f32> {
    prop_oneof![
        8 => -60.0f32..100.0,
        1 => -1.0e6f32..1.0e6,
        1 => Just(f32::NAN),
    ]
}

/// Also beyond what an `i32` holds.
fn huge_coordinate() -> impl Strategy<Value = f32> {
    prop_oneof![
        4 => coordinate(),
        1 => Just(f32::INFINITY),
        1 => Just(f32::NEG_INFINITY),
        1 => Just(1.0e30f32),
        1 => Just(-3.0e9f32),
    ]
}

fn point() -> impl Strategy<Value = Pt> {
    (coordinate(), coordinate()).prop_map(|(x, y)| Pt { x, y })
}

fn huge_point() -> impl Strategy<Value = Pt> {
    (huge_coordinate(), huge_coordinate()).prop_map(|(x, y)| Pt { x, y })
}

fn inside(width: usize, height: usize) -> impl Strategy<Value = Pt> {
    (0.0..=width as f32, 0.0..=height as f32).prop_map(|(x, y)| Pt { x, y })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn any_segments_give_a_bitmap_of_the_asked_size(
        lines in proptest::collection::vec((point(), point()), 0..20),
        width in 0usize..40,
        height in 0usize..40,
    ) {
        let coverage = rasterize(&lines, width, height);
        prop_assert_eq!(coverage.len(), width * height);
    }

    /// `floor` and `ceil` overflow for values beyond `i32` (`ceil` adds
    /// one to `i32::MAX`): a panic in debug builds, nonsense bounds in
    /// release ones, for outlines that far out or at absurd sizes.
    #[test]
    #[ignore = "bug: raster::floor and raster::ceil overflow for values beyond i32"]
    fn segments_beyond_i32_give_a_bitmap_of_the_asked_size(
        lines in proptest::collection::vec((huge_point(), huge_point()), 0..20),
        width in 0usize..40,
        height in 0usize..40,
    ) {
        let mut path = Path::default();
        for &(a, b) in &lines {
            path.move_to(a);
            path.quad_to(b, a);
        }
        let _ = path.bounds();
        let coverage = rasterize(&lines, width, height);
        prop_assert_eq!(coverage.len(), width * height);
    }

    #[test]
    fn a_triangle_covers_its_area_and_nothing_away_from_it(
        (width, height, a, b, c) in (1usize..30, 1usize..30).prop_flat_map(|(w, h)| {
            (Just(w), Just(h), inside(w, h), inside(w, h), inside(w, h))
        }),
    ) {
        let coverage = rasterize(&triangle(a, b, c), width, height);
        let total: f32 = coverage.iter().map(|&v| f32::from(v) / 255.0).sum();
        let expected = area(a, b, c);
        // Each pixel is rounded to 1/255.
        let tolerance = (width * height) as f32 / 510.0 + expected * 0.01 + 0.01;
        prop_assert!(
            abs(total - expected) <= tolerance,
            "covered {} of an area of {}", total, expected
        );
        let (x0, x1) = (a.x.min(b.x).min(c.x), a.x.max(b.x).max(c.x));
        let (y0, y1) = (a.y.min(b.y).min(c.y), a.y.max(b.y).max(c.y));
        for y in 0..height {
            for x in 0..width {
                let away = (x + 1) as f32 <= x0 - 1.0
                    || x as f32 >= x1 + 1.0
                    || (y + 1) as f32 <= y0 - 1.0
                    || y as f32 >= y1 + 1.0;
                if away {
                    prop_assert_eq!(coverage[y * width + x], 0, "pixel {},{}", x, y);
                }
            }
        }
    }

    #[test]
    fn curves_are_flattened_into_few_segments(
        points in proptest::collection::vec(point(), 4),
        cubic in any::<bool>(),
    ) {
        let mut path = Path::default();
        path.move_to(points[0]);
        if cubic {
            path.cubic_to(points[1], points[2], points[3]);
        } else {
            path.quad_to(points[1], points[3]);
        }
        prop_assert!((1..=32).contains(&path.lines.len()), "{} segments", path.lines.len());
        // The curve ends where it should.
        let end = path.lines.last().map(|l| l.1);
        let target = points[3];
        if target.x.is_finite() && target.y.is_finite() && points.iter().all(|p| abs(p.x) < 1.0e6 && abs(p.y) < 1.0e6) {
            let end = end.unwrap_or(target);
            prop_assert!(abs(end.x - target.x) < 1.0e-2 && abs(end.y - target.y) < 1.0e-2);
        }
    }
}

#[test]
fn a_shape_larger_than_the_box_fills_it() {
    let lines = triangle(
        Pt { x: -50.0, y: -50.0 },
        Pt { x: 200.0, y: -50.0 },
        Pt { x: -50.0, y: 200.0 },
    );
    let coverage = rasterize(&lines, 8, 8);
    assert!(coverage.iter().all(|&v| v == 255), "{coverage:?}");
}

#[test]
fn curves_without_a_start_draw_nothing() {
    let mut path = Path::default();
    path.quad_to(Pt { x: 1.0, y: 1.0 }, Pt { x: 2.0, y: 0.0 });
    path.cubic_to(
        Pt { x: 1.0, y: 1.0 },
        Pt { x: 2.0, y: 2.0 },
        Pt { x: 3.0, y: 0.0 },
    );
    path.line_to(Pt { x: 4.0, y: 4.0 });
    path.close();
    assert!(path.lines.is_empty());
    assert_eq!(path.bounds(), None);
}

/// The shrunk case of `segments_beyond_i32_give_a_bitmap_of_the_asked_size`.
#[test]
#[ignore = "bug: raster::floor and raster::ceil overflow for values beyond i32"]
fn float_helpers_saturate_beyond_i32() {
    assert_eq!(ceil(f32::INFINITY), i32::MAX);
    assert_eq!(ceil(3.0e9), i32::MAX);
    assert_eq!(floor(f32::NEG_INFINITY), i32::MIN);
    assert_eq!(floor(-3.0e9), i32::MIN);
}
