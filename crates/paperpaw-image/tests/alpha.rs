//! Images with transparency (`PPGA`): every PNG kind decoded with its
//! opacity kept, the same over white as decoded without it; made smaller
//! with the gray weighted by opacity, so that clear pixels lend it no
//! gray; images without transparency kept opaque; and files that are not
//! what they claim refused.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;

use common::{Gif, Jpeg, PNG_KINDS, Png, decoded, png_channels};
use paperpaw_image::{
    AlphaFile, AlphaWriter, Fit, Format, ImageFile, ImageSink, decode, over_white,
};
use paperpaw_platform::{DirEntry, Storage, StorageError};

#[derive(Default)]
struct Files {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl Files {
    fn put(&self, path: &str, data: &[u8]) {
        self.files.borrow_mut().insert(path.into(), data.to_vec());
    }
}

impl Storage for Files {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.files.borrow();
        let data = files.get(path).ok_or(StorageError::NotFound)?;
        let start = (offset as usize).min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        Ok(files.get(path).ok_or(StorageError::NotFound)?.len() as u64)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.put(path, data);
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files
            .borrow_mut()
            .entry(path.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }
}

/// `data` decoded through an [`AlphaWriter`]: the file and its pixels.
fn with_alpha(data: &[u8], fit: Fit) -> (AlphaFile, Vec<u8>) {
    let storage = Files::default();
    let mut writer = AlphaWriter::new(&storage, "a");
    let mut source = data;
    decode(Format::detect(data).unwrap(), &mut source, fit, &mut writer).unwrap();
    writer.finish().unwrap();
    let file = AlphaFile::open(&storage, "a").unwrap();
    let mut pixels = vec![0u8; file.width as usize * file.height as usize * 2];
    assert_eq!(file.read_rows(&storage, 0, &mut pixels), file.height);
    assert!(ImageFile::open(&storage, "a").unwrap().alpha);
    (file, pixels)
}

/// How opaque each pixel of `png` is.
fn expected_alpha(png: &Png) -> Vec<u8> {
    let channels = png_channels(png.color);
    let max = (1u32 << png.depth) - 1;
    let to8 = |s: u16| -> u8 {
        match png.depth {
            16 => (s >> 8) as u8,
            _ => (u32::from(s) * 255 / max) as u8,
        }
    };
    png.samples
        .iter()
        .flat_map(|row| row.chunks(channels))
        .map(|p| match png.color {
            3 => png
                .trns
                .as_ref()
                .and_then(|t| t.get(usize::from(p[0])))
                .copied()
                .unwrap_or(255),
            4 => to8(p[1]),
            6 => to8(p[3]),
            _ => 255,
        })
        .collect()
}

#[test]
fn every_png_kind_keeps_its_opacity() {
    for (color, depth) in PNG_KINDS {
        let mut png = Png::new(13, 4, color, depth, u64::from(color) * 7 + u64::from(depth));
        if color == 3 {
            png.trns = Some((0..png.palette.len()).map(|i| (i * 37) as u8).collect());
        }
        let what = format!("type {color} depth {depth}");
        let (file, pixels) = with_alpha(&png.encode(), Fit::side(1000));
        assert_eq!((file.width, file.height), (13, 4), "{what}");
        let alpha: Vec<u8> = pixels.chunks(2).map(|p| p[1]).collect();
        assert_eq!(alpha, expected_alpha(&png), "{what}: opacity");
        // Over white, as decoded without it (but where nothing shows).
        for (i, (p, &flat)) in pixels.chunks(2).zip(&png.expected_gray()).enumerate() {
            let over = over_white(p[0], p[1]);
            assert!(
                over.abs_diff(flat) <= 1,
                "{what}: pixel {i} is {over}, {flat}"
            );
        }
    }
}

#[test]
fn clear_pixels_lend_a_smaller_one_no_gray() {
    // Opaque white beside clear black: made one pixel, it is white, half
    // opaque; flattened first it would be the gray of white over white
    // and black, all of it showing.
    let mut png = Png::new(2, 2, 4, 8, 0);
    png.samples = vec![vec![255, 255, 0, 0]; 2];
    let (file, pixels) = with_alpha(&png.encode(), Fit::side(1));
    assert_eq!((file.width, file.height), (1, 1));
    assert_eq!(pixels[0], 255);
    assert!(pixels[1].abs_diff(127) <= 1, "{}", pixels[1]);
    // All clear: nothing to show, white.
    png.samples = vec![vec![0, 0, 0, 0]; 2];
    let (_, pixels) = with_alpha(&png.encode(), Fit::side(1));
    assert_eq!(pixels, [255, 0]);
}

#[test]
fn smaller_images_with_transparency_are_the_mean_of_what_they_cover() {
    let png = Png::new(40, 24, 6, 8, 5);
    let (file, pixels) = with_alpha(&png.encode(), Fit::side(10));
    assert_eq!((file.width, file.height), (10, 6));
    // Each 4 x 4 square: the opacity's mean, the gray weighted by it.
    let full = expected_alpha(&png);
    let (_, full_pixels) = with_alpha(&png.encode(), Fit::side(1000));
    for oy in 0..6 {
        for ox in 0..10 {
            let (mut weighted, mut opaque) = (0u32, 0u32);
            for y in oy * 4..oy * 4 + 4 {
                for x in ox * 4..ox * 4 + 4 {
                    let i = y * 40 + x;
                    weighted += u32::from(full_pixels[i * 2]) * u32::from(full[i]);
                    opaque += u32::from(full[i]);
                }
            }
            let at = (oy * 10 + ox) * 2;
            assert_eq!(u32::from(pixels[at + 1]), opaque / 16, "opacity {ox},{oy}");
            if let Some(gray) = weighted.checked_div(opaque) {
                assert_eq!(u32::from(pixels[at]), gray, "gray {ox},{oy}");
            }
        }
    }
}

#[test]
fn images_without_transparency_are_opaque() {
    let images = [
        Png::new(21, 9, 2, 8, 3).encode(),
        Jpeg::new(24, 16, &[(2, 2), (1, 1), (1, 1)], 4, true).baseline(),
    ];
    for data in &images {
        let expected = decoded(data, Fit::side(12), false);
        let (file, pixels) = with_alpha(data, Fit::side(12));
        assert_eq!((file.width, file.height), (expected.width, expected.height));
        let gray: Vec<u8> = pixels.chunks(2).map(|p| p[0]).collect();
        assert_eq!(gray, expected.pixels);
        assert!(pixels.chunks(2).all(|p| p[1] == 255));
    }
}

#[test]
fn sinks_that_do_not_want_it_get_it_over_white() {
    struct Plain(Vec<u8>);
    impl ImageSink for Plain {
        fn start(&mut self, _: u32, _: u32) -> Result<(), paperpaw_image::ImageError> {
            Ok(())
        }
        fn row(&mut self, gray: &[u8]) -> Result<(), paperpaw_image::ImageError> {
            self.0.extend_from_slice(gray);
            Ok(())
        }
    }
    let mut sink = Plain(Vec::new());
    sink.row_alpha(&[0, 0, 0, 255, 0, 128, 200, 255]).unwrap();
    assert_eq!(sink.0, [255, 0, 127, 200]);
    assert_eq!((over_white(0, 0), over_white(0, 255)), (255, 0));
}

#[test]
fn alpha_files_are_laid_out_as_documented() {
    // `PPGA`, width and height as little-endian u32, gray and opacity.
    let storage = Files::default();
    let mut data = b"PPGA".to_vec();
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[10, 20, 30, 40]);
    storage.put("a", &data);
    let file = AlphaFile::open(&storage, "a").unwrap();
    let mut out = [0u8; 4];
    assert_eq!(file.read_rows(&storage, 0, &mut out), 1);
    assert_eq!(out, [10, 20, 30, 40]);
    assert_eq!(file.read_rows(&storage, 1, &mut out), 0);
    // Cut short, too long, empty or of another kind: refused.
    for bad in [&data[..15], &[&data[..], &[0]].concat()[..]] {
        storage.put("b", bad);
        assert!(AlphaFile::open(&storage, "b").is_none());
    }
    let mut empty = b"PPGA".to_vec();
    empty.extend_from_slice(&[0; 8]);
    storage.put("b", &empty);
    assert!(AlphaFile::open(&storage, "b").is_none());
    let mut gray = data.clone();
    gray[..4].copy_from_slice(b"PPGI");
    storage.put("b", &gray);
    assert!(AlphaFile::open(&storage, "b").is_none());
}

#[test]
fn a_colour_key_makes_gray_and_rgb_pixels_clear() {
    for (color, depth) in [(0, 1), (0, 4), (0, 8), (0, 16), (2, 8), (2, 16)] {
        let what = format!("type {color} depth {depth}");
        let mut png = Png::new(9, 3, color, depth, u64::from(depth) * 5 + u64::from(color));
        let channels = png_channels(color);
        let key: Vec<u16> = png.samples[1][..channels].to_vec();
        if depth == 16 {
            // The same high byte, another low one: not the key.
            let mut near = key.clone();
            near[0] ^= 1;
            png.samples[2][..channels].copy_from_slice(&near);
        }
        png.trns = Some(key.iter().flat_map(|v| v.to_be_bytes()).collect());
        let (_, pixels) = with_alpha(&png.encode(), Fit::side(1000));
        let flat = decoded(&png.encode(), Fit::side(1000), false);
        let mut cleared = 0;
        for (i, p) in pixels.chunks(2).enumerate() {
            let (x, y) = (i % 9, i / 9);
            let sample = &png.samples[y][x * channels..(x + 1) * channels];
            let clear = sample == key.as_slice();
            assert_eq!(p[1] == 0, clear, "{what}: pixel {x},{y}");
            assert!(p[1] == 0 || p[1] == 255, "{what}");
            if clear {
                cleared += 1;
                assert_eq!(flat.pixels[i], 255, "{what}: over white");
            }
        }
        assert!(cleared >= 1, "{what}");
        if depth == 16 {
            assert_eq!(pixels[9 * 2 * 2 + 1], 255, "{what}: the low byte counts");
        }
    }
}

#[test]
fn the_kind_of_a_decoded_file_is_read_from_its_header_once() {
    use paperpaw_image::Decoded;
    let storage = Files::default();
    for (magic, bytes) in [(b"PPGI", 1), (b"PPCI", 3), (b"PPGA", 2)] {
        let mut data = magic.to_vec();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend(std::iter::repeat_n(7u8, 6 * bytes));
        storage.put("f", &data);
        let opened = Decoded::open(&storage, "f").unwrap();
        assert_eq!(opened.size(), (2, 3));
        let kind = match &opened {
            Decoded::Gray(_) => 1,
            Decoded::Color(_) => 3,
            Decoded::Alpha(_) => 2,
        };
        assert_eq!(kind, bytes);
        let file = ImageFile::open(&storage, "f").unwrap();
        assert_eq!((file.color, file.alpha), (bytes == 3, bytes == 2));
        // Cut short: refused when opened, trusted when peeked at.
        storage.put("f", &data[..data.len() - 1]);
        assert!(Decoded::open(&storage, "f").is_none());
        assert_eq!(Decoded::peek(&storage, "f"), Some(opened));
    }
    storage.put("f", b"PPXX\x01\0\0\0\x01\0\0\0\0");
    assert!(Decoded::peek(&storage, "f").is_none());
}

#[test]
fn a_gifs_transparent_colour_and_its_screen_are_clear() {
    for interlaced in [false, true] {
        let mut gif = Gif::new(12, 10, 16, 9);
        gif.left = 2;
        gif.top = 1;
        gif.frame_width = 8;
        gif.frame_height = 7;
        gif.indices = (0..8 * 7).map(|i| (i % 16) as u8).collect();
        gif.interlaced = interlaced;
        gif.transparent = Some(3);
        let (file, pixels) = with_alpha(&gif.encode(), Fit::side(1000));
        assert_eq!((file.width, file.height), (12, 10));
        let flat = decoded(&gif.encode(), Fit::side(1000), false);
        for y in 0..10usize {
            for x in 0..12usize {
                let at = y * 12 + x;
                let inside = (2..10).contains(&x) && (1..8).contains(&y);
                let index = inside.then(|| ((y - 1) * 8 + (x - 2)) % 16);
                let clear = index.is_none_or(|i| i == 3);
                assert_eq!(
                    pixels[at * 2 + 1] == 0,
                    clear,
                    "{x},{y} interlaced {interlaced}"
                );
                if clear {
                    assert_eq!(flat.pixels[at], 255);
                } else {
                    assert_eq!(pixels[at * 2], flat.pixels[at]);
                }
            }
        }
    }
}
