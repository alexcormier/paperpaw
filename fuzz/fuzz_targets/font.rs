//! Any bytes as a font: a font pack held in memory and read a glyph at a
//! time (as on the X4), and an OpenType/TrueType font (as on Android and
//! desktop); then its name and a few glyphs at a few sizes.

#![no_main]

use std::rc::Rc;

use libfuzzer_sys::fuzz_target;
use paperpaw_text::{Font, GlyphCache, PackSource, StoredPack, sfnt};

/// Characters drawn, besides one the input names: Latin, a combining
/// mark, CJK and an emoji.
const CHARS: [char; 5] = ['A', 'g', '\u{0301}', '\u{4e2d}', '\u{1f600}'];
/// Sizes drawn, in pixels; the last one comes from the input.
const SIZES: [f32; 3] = [1.0, 16.0, 120.0];

struct Bytes(Vec<u8>);

impl PackSource for Bytes {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let Ok(at) = usize::try_from(at) else {
            return false;
        };
        match self.0.get(at..).and_then(|rest| rest.get(..buf.len())) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

fn draw(font: &Font, extra: char, size: f32) {
    let mut cache = GlyphCache::new(64 * 1024);
    let _ = font.line_metrics(size);
    for ch in CHARS.into_iter().chain([extra]) {
        let _ = font.has_char(ch);
        let _ = font.advance(ch, size);
        let _ = cache.glyph(0, font, ch, size);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&[ch, size], data)) = data.split_first_chunk::<2>() else {
        return;
    };
    let extra = char::from_u32(u32::from(ch) * 0x101).unwrap_or('?');
    let sizes = SIZES.into_iter().chain([f32::from(size) / 2.0]);

    let mut read_at = |at: u64, buf: &mut [u8]| {
        let Some(rest) = usize::try_from(at).ok().and_then(|at| data.get(at..)) else {
            return 0;
        };
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        n
    };
    let _ = sfnt::read_info(&mut read_at);

    let fonts = [
        Font::from_data(data.to_vec(), &[("wght", 700.0)]).ok(),
        StoredPack::open(Rc::new(Bytes(data.to_vec())))
            .ok()
            .map(Font::Stored),
    ];
    for font in fonts.iter().flatten() {
        for size in sizes.clone() {
            draw(font, extra, size);
        }
    }
});
