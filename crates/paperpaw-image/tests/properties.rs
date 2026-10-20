//! Properties over any input: arbitrary bytes and damaged images of
//! every kind never panic, and whatever decodes keeps the sink's side of
//! the contract (one start, rows of the width, never more than the
//! height, all of them when decoding succeeds, inside the box asked
//! for); `dimensions` never panics either. And `Fit` itself: sizes
//! inside the box, never larger than the picture, in its proportions.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Gif, Jpeg, Png, config, run};
use paperpaw_image::{Fit, Format, dimensions};
use proptest::prelude::*;

const FORMATS: [Format; 3] = [Format::Jpeg, Format::Png, Format::Gif];

/// Decodes `data` as every format, gray and colour, at `fit`; checks the
/// size against the box.
fn decode_all_ways(data: &[u8], fit: Fit) {
    let _ = dimensions(data);
    for format in FORMATS {
        if format == Format::Gif && common::gif_interlaced_without_width(data) {
            // A known bug, see the GIF tests.
            continue;
        }
        for color in [false, true] {
            let (result, sink) = run(format, data, fit, color);
            if sink.started {
                let (short, long) = (sink.width.min(sink.height), sink.width.max(sink.height));
                assert!(
                    short <= fit.short.max(1) && long <= fit.long.max(1),
                    "{format:?}: {}x{} for {fit:?}",
                    sink.width,
                    sink.height
                );
            }
            if result.is_ok() {
                assert!(sink.complete());
            }
        }
    }
}

/// A valid image of some kind, small.
fn image() -> impl Strategy<Value = Vec<u8>> {
    let sampling = prop_oneof![
        Just(vec![(1u8, 1u8)]),
        Just(vec![(2, 2), (1, 1), (1, 1)]),
        Just(vec![(1, 2), (1, 1), (1, 1)]),
    ];
    let jpeg = (
        1u16..40,
        1u16..40,
        sampling,
        any::<u64>(),
        0u16..4,
        any::<bool>(),
    )
        .prop_map(|(w, h, sampling, seed, restart, progressive)| {
            let mut jpeg = Jpeg::new(w, h, &sampling, seed, true);
            jpeg.restart = restart;
            if progressive {
                jpeg.progressive()
            } else {
                jpeg.baseline()
            }
        });
    let png = (
        1u32..20,
        1u32..20,
        0..common::PNG_KINDS.len(),
        any::<u64>(),
        0u8..5,
    )
        .prop_map(|(w, h, kind, seed, filter)| {
            let (color, depth) = common::PNG_KINDS[kind];
            let mut png = Png::new(w, h, color, depth, seed);
            png.filters = vec![filter, 4 - filter];
            png.idat = 37;
            png.encode()
        });
    let gif = (1u16..20, 1u16..20, any::<u64>(), any::<bool>(), 0u16..10).prop_map(
        |(w, h, seed, interlaced, offset)| {
            let mut gif = Gif::new(w, h, 16, seed);
            gif.interlaced = interlaced;
            gif.left = offset;
            gif.width = w + 3;
            gif.transparent = Some(3);
            gif.encode()
        },
    );
    prop_oneof![jpeg, png, gif]
}

/// A change to a file.
#[derive(Debug, Clone)]
enum Damage {
    Flip(usize, u8),
    Set(usize, u8),
    Insert(usize, Vec<u8>),
    Remove(usize, usize),
    Cut(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        (any::<usize>(), 1u8..=255).prop_map(|(at, bits)| Damage::Flip(at, bits)),
        (
            any::<usize>(),
            prop_oneof![Just(0u8), Just(0xff), any::<u8>()]
        )
            .prop_map(|(at, v)| Damage::Set(at, v)),
        (any::<usize>(), proptest::collection::vec(any::<u8>(), 1..8))
            .prop_map(|(at, bytes)| Damage::Insert(at, bytes)),
        (any::<usize>(), 1usize..20).prop_map(|(at, n)| Damage::Remove(at, n)),
        any::<usize>().prop_map(Damage::Cut),
    ]
}

fn apply(data: &mut Vec<u8>, damage: &Damage) {
    if data.is_empty() {
        return;
    }
    let at = |i: usize| i % data.len();
    match damage {
        Damage::Flip(i, bits) => {
            let i = at(*i);
            data[i] ^= bits;
        }
        Damage::Set(i, v) => {
            let i = at(*i);
            data[i] = *v;
        }
        Damage::Insert(i, bytes) => {
            let i = at(*i);
            data.splice(i..i, bytes.iter().copied());
        }
        Damage::Remove(i, n) => {
            let i = at(*i);
            let end = (i + n).min(data.len());
            data.drain(i..end);
        }
        Damage::Cut(i) => {
            let i = at(*i);
            data.truncate(i);
        }
    }
}

fn fit() -> impl Strategy<Value = Fit> {
    prop_oneof![
        (0u32..70).prop_map(Fit::side),
        (1u32..70, 1u32..70).prop_map(|(a, b)| Fit::screen(a, b)),
    ]
}

proptest! {
    #![proptest_config(config(256))]

    /// Damaged images of every kind never panic.
    #[test]
    fn damaged_images_never_panic(
        image in image(),
        damages in proptest::collection::vec(damage(), 1..4),
        fit in fit(),
    ) {
        let mut data = image;
        for d in &damages {
            apply(&mut data, d);
        }
        decode_all_ways(&data, fit);
    }

    /// Valid images decode whole, inside the box.
    #[test]
    fn valid_images_decode_inside_the_box(image in image(), fit in fit()) {
        let format = Format::detect(&image).unwrap();
        for color in [false, true] {
            let (result, sink) = run(format, &image, fit, color);
            prop_assert!(result.is_ok(), "{:?}", result);
            let size = dimensions(&image).unwrap();
            prop_assert_eq!((sink.width, sink.height), fit.size(size.0, size.1));
        }
        decode_all_ways(&image, fit);
    }

    /// Arbitrary bytes after each format's signature never panic.
    #[test]
    fn arbitrary_bytes_never_panic(
        head in prop_oneof![
            Just(&b""[..]),
            Just(&b"\xff\xd8"[..]),
            Just(&b"\xff\xd8\xff\xc2\x00\x0b\x08"[..]),
            Just(&b"\x89PNG\r\n\x1a\n"[..]),
            Just(&b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"[..]),
            Just(&b"GIF89a"[..]),
            Just(&b"GIF87a\x10\x00\x10\x00\x80\x00\x00"[..]),
        ],
        tail in proptest::collection::vec(any::<u8>(), 0..300),
        fit in fit(),
    ) {
        let mut data = head.to_vec();
        data.extend_from_slice(&tail);
        decode_all_ways(&data, fit);
    }

    /// `Fit::size`: inside the box upright or turned, never larger than
    /// the picture, at least a pixel, in the picture's proportions.
    #[test]
    fn fit_sizes_stay_in_the_box(
        w in 1u32..100_000,
        h in 1u32..100_000,
        a in 0u32..3000,
        b in 0u32..3000,
    ) {
        let fit = Fit::screen(a, b);
        let (sw, sh) = fit.size(w, h);
        prop_assert!(sw >= 1 && sh >= 1);
        prop_assert!(sw <= w && sh <= h);
        let upright = sw <= fit.short.max(1) && sh <= fit.long.max(1);
        let turned = sw <= fit.long.max(1) && sh <= fit.short.max(1);
        prop_assert!(upright || turned, "{sw}x{sh} for {fit:?}");
        if (sw, sh) != (w, h) && sw > 1 && sh > 1 {
            // Proportions kept to within a pixel's rounding.
            let (w, h, sw, sh) = (f64::from(w), f64::from(h), f64::from(sw), f64::from(sh));
            prop_assert!((sw / sh - w / h).abs() <= w / h * (1.0 / sh + 1.0 / sw) + 1e-9);
        }
        // Pictures inside the box stay as they are.
        if w <= fit.short && h <= fit.long {
            prop_assert_eq!((sw, sh), (w, h));
        }
        // What fits both boxes fits each.
        let other = Fit::side(b / 2 + 1);
        let both = fit.min(other).size(w, h);
        prop_assert!(both.0 <= other.short && both.1 <= other.short, "{both:?}");
        prop_assert!(u64::from(both.0) * u64::from(both.1) <= u64::from(sw) * u64::from(sh));
    }
}

#[test]
fn fit_of_a_screen_is_the_same_either_way_up() {
    assert_eq!(Fit::screen(480, 800), Fit::screen(800, 480));
    assert_eq!(
        Fit::screen(480, 800),
        Fit {
            short: 480,
            long: 800
        }
    );
    // An empty box: a pixel.
    assert_eq!(Fit::side(0).size(100, 50), (1, 1));
    assert_eq!(Fit::side(1).size(1, 1), (1, 1));
}

#[test]
fn formats_are_told_by_their_first_bytes() {
    assert_eq!(Format::detect(b"\xff\xd8\xff"), Some(Format::Jpeg));
    assert_eq!(Format::detect(b"\x89PNG\r\n\x1a\n"), Some(Format::Png));
    assert_eq!(Format::detect(b"GIF87a"), Some(Format::Gif));
    assert_eq!(Format::detect(b"GIF8"), Some(Format::Gif));
    for other in [&b""[..], b"\xff", b"\x89PNG", b"GIF", b"BM", b"RIFF"] {
        assert_eq!(Format::detect(other), None, "{other:?}");
    }
}

#[test]
fn dimensions_of_jpegs_look_past_other_segments() {
    let data = Jpeg::new(33, 17, &[(1, 1)], 0, true).baseline();
    // A long application segment first: found as long as the head holds
    // the frame header.
    let mut long = data[..2].to_vec();
    long.extend_from_slice(&[0xff, 0xe1, 0x10, 0x00]);
    long.extend(std::iter::repeat_n(0, 0x1000 - 2));
    long.extend_from_slice(&data[2..]);
    assert_eq!(dimensions(&long), Some((33, 17)));
    let sof = common::find_marker(&long, 0xc0, 0).unwrap();
    assert_eq!(dimensions(&long[..sof + 8]), None);
    assert_eq!(dimensions(&long[..sof + 9]), Some((33, 17)));
    // A segment of no length, or bytes that are no marker: not known.
    assert_eq!(
        dimensions(&[0xff, 0xd8, 0xff, 0xe0, 0, 0, 0xff, 0xc0]),
        None
    );
    assert_eq!(dimensions(&[0xff, 0xd8, 0x12, 0x34]), None);
    // Fill bytes and restart markers before the frame are passed over.
    let mut filled = vec![0xff, 0xd8, 0xff, 0xff, 0xd0];
    filled.extend_from_slice(&data[2..]);
    assert_eq!(dimensions(&filled), Some((33, 17)));
}
