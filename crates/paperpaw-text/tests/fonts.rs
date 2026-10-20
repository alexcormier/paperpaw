//! Font files from cards and plugins: what `sfnt::read_info` reads from a
//! font's header (family, weight, bold, italic; collections; names in
//! several languages and encodings) and that damaged files (random bytes,
//! truncated tables, offsets and counts past the end) give `None` or an
//! error, never a panic. With the `outline` feature, also loading and
//! drawing damaged scalable fonts.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_text::sfnt::{FontInfo, read_info};
use proptest::prelude::*;

const LITERATA: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/Literata.ttf");
const ICONS: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/PaperPawIcons.ttf");

fn info(data: &[u8]) -> Option<FontInfo> {
    read_info(&mut |offset, buf: &mut [u8]| {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        n
    })
}

/// An sfnt file of `tables` (tag and data), as fonts are laid out.
fn sfnt(version: &[u8; 4], tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = version.to_vec();
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut data = Vec::new();
    for (tag, table) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        data.extend_from_slice(table);
        offset += table.len();
    }
    out.extend_from_slice(&data);
    out
}

/// A `name` table of `(platform, language, name id, bytes)` records.
fn name(records: &[(u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    out.extend_from_slice(&((6 + 12 * records.len()) as u16).to_be_bytes());
    let mut strings = Vec::new();
    for (platform, language, id, bytes) in records {
        for value in [*platform, 1, *language, *id, bytes.len() as u16] {
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        strings.extend_from_slice(bytes);
    }
    out.extend_from_slice(&strings);
    out
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn os2(weight: u16, selection: u16, len: usize) -> Vec<u8> {
    let mut table = vec![0u8; len];
    table[4..6].copy_from_slice(&weight.to_be_bytes());
    if len >= 64 {
        table[62..64].copy_from_slice(&selection.to_be_bytes());
    }
    table
}

fn head(mac_style: u16) -> Vec<u8> {
    let mut table = vec![0u8; 54];
    table[44..46].copy_from_slice(&mac_style.to_be_bytes());
    table
}

const WINDOWS: u16 = 3;
const MAC: u16 = 1;
const EN_US: u16 = 0x409;
const DE: u16 = 0x407;

fn family(name_table: Vec<u8>) -> Option<String> {
    info(&sfnt(b"\0\x01\0\0", &[(b"name", name_table)])).map(|i| i.family)
}

#[test]
fn the_family_is_name_1_in_english_first() {
    let names = name(&[
        (WINDOWS, DE, 1, utf16("Schrift")),
        (WINDOWS, EN_US, 16, utf16("Typographic")),
        (WINDOWS, EN_US, 1, utf16("  Family Light ")),
        (WINDOWS, EN_US, 2, utf16("Regular")),
    ]);
    assert_eq!(family(names).as_deref(), Some("Family Light"));
    // Without name 1, the typographic family.
    let names = name(&[(WINDOWS, EN_US, 16, utf16("Typographic"))]);
    assert_eq!(family(names).as_deref(), Some("Typographic"));
    // Only another language: that one.
    let names = name(&[(WINDOWS, DE, 1, utf16("Schrift"))]);
    assert_eq!(family(names).as_deref(), Some("Schrift"));
    // Blank names and other ids do not count.
    let names = name(&[
        (WINDOWS, EN_US, 1, utf16("   ")),
        (WINDOWS, EN_US, 4, utf16("Full")),
    ]);
    assert_eq!(family(names), None);
    // Unknown platforms are skipped.
    let names = name(&[
        (7, EN_US, 1, utf16("Odd")),
        (MAC, 0, 1, b"Mac Name".to_vec()),
    ]);
    assert_eq!(family(names).as_deref(), Some("Mac Name"));
}

#[test]
fn odd_utf16_names_are_read_lossily() {
    // A lone surrogate and an odd byte at the end.
    let mut bytes = utf16("Ab");
    bytes.extend_from_slice(&[0xd8, 0x00, 0x00, 0x43, 0x44]);
    let got = family(name(&[(WINDOWS, EN_US, 1, bytes)])).unwrap();
    assert_eq!(got, "Ab\u{fffd}C");
}

/// Mac Roman names are read as ASCII only, but letters beyond ASCII
/// become U+007F (DELETE), a control character, in the family name
/// shown to the reader.
#[test]
#[ignore = "bug: non-ASCII Mac Roman bytes in a font name become DEL control characters"]
fn mac_roman_names_have_no_control_characters() {
    // "Café" in Mac Roman.
    let got = family(name(&[(MAC, 0, 1, b"Caf\x8e".to_vec())])).unwrap();
    assert!(!got.chars().any(char::is_control), "{got:?}");
}

#[test]
fn name_records_past_the_end_are_skipped() {
    let mut table = name(&[
        (WINDOWS, EN_US, 1, utf16("Gone")),
        (WINDOWS, DE, 1, utf16("Da")),
    ]);
    // The first record's length now reaches past the table.
    table[6 + 8..6 + 10].copy_from_slice(&0xfff0u16.to_be_bytes());
    assert_eq!(family(table).as_deref(), Some("Da"));
    // More records than the table holds.
    let mut table = name(&[(WINDOWS, EN_US, 1, utf16("Kept"))]);
    table[2..4].copy_from_slice(&500u16.to_be_bytes());
    assert_eq!(family(table), None);
}

#[test]
fn style_comes_from_os2_else_head() {
    let names = || name(&[(WINDOWS, EN_US, 1, utf16("F"))]);
    let style = |tables: Vec<(&[u8; 4], Vec<u8>)>| {
        let i = info(&sfnt(b"\0\x01\0\0", &tables)).unwrap();
        (i.weight, i.bold, i.italic)
    };
    assert_eq!(style(vec![(b"name", names())]), (400, false, false));
    assert_eq!(
        style(vec![(b"name", names()), (b"OS/2", os2(700, 0, 96))]),
        (700, true, false)
    );
    assert_eq!(
        style(vec![(b"name", names()), (b"OS/2", os2(600, 0, 78))]),
        (600, true, false)
    );
    // fsSelection: italic (bit 0), bold (bit 5).
    assert_eq!(
        style(vec![(b"name", names()), (b"OS/2", os2(400, 1, 96))]),
        (400, false, true)
    );
    assert_eq!(
        style(vec![(b"name", names()), (b"OS/2", os2(400, 0x20, 64))]),
        (400, true, false)
    );
    // An OS/2 table too short for fsSelection: the head table's macStyle.
    assert_eq!(
        style(vec![
            (b"name", names()),
            (b"OS/2", os2(900, 0, 40)),
            (b"head", head(3))
        ]),
        (700, true, true)
    );
    assert_eq!(
        style(vec![(b"name", names()), (b"head", head(2))]),
        (400, false, true)
    );
}

#[test]
fn tables_past_the_end_give_none() {
    let names = name(&[(WINDOWS, EN_US, 1, utf16("F"))]);
    let font = sfnt(b"true", &[(b"name", names), (b"OS/2", os2(700, 0, 96))]);
    assert_eq!(info(&font).unwrap().weight, 700);
    // More tables than the directory holds.
    let mut more = font.clone();
    more[4..6].copy_from_slice(&0xffffu16.to_be_bytes());
    assert_eq!(info(&more), None);
    // The OS/2 table past the end: the font is not read.
    let mut far = font.clone();
    let record = 12 + 16;
    far[record + 8..record + 12].copy_from_slice(&0xffff_fff0u32.to_be_bytes());
    assert_eq!(info(&far), None);
    // A name table longer than the file.
    let mut long = font;
    long[12 + 12..12 + 16].copy_from_slice(&0xffffu32.to_be_bytes());
    assert_eq!(info(&long), None);
}

#[test]
fn collections_use_their_first_font() {
    let names = name(&[(WINDOWS, EN_US, 1, utf16("First"))]);
    let font = sfnt(b"OTTO", &[(b"name", names)]);
    // The font's tables are at offsets from the start of the file, so it
    // goes first, then the collection header points back at it.
    let mut collection = b"ttcf\0\x01\0\0\0\0\0\x01".to_vec();
    collection.extend_from_slice(&16u32.to_be_bytes());
    let shift = collection.len() as u32;
    let mut moved = font;
    let at = 12 + 8;
    let offset = u32::from_be_bytes(moved[at..at + 4].try_into().unwrap());
    moved[at..at + 4].copy_from_slice(&(offset + shift).to_be_bytes());
    collection.extend_from_slice(&moved);
    assert_eq!(info(&collection).unwrap().family, "First");
    // A first font past the end.
    collection[12..16].copy_from_slice(&0xffff_0000u32.to_be_bytes());
    assert_eq!(info(&collection), None);
    assert_eq!(info(b"ttcf"), None);
}

#[test]
fn real_fonts_cut_short_give_none_or_their_name() {
    for font in [LITERATA, ICONS] {
        let whole = info(font).unwrap();
        for len in (0..4096).step_by(7).chain([font.len() / 2, font.len() - 1]) {
            if let Some(cut) = info(&font[..len]) {
                assert_eq!(cut.family, whole.family);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_headers_never_panic(
        prefix in prop_oneof![
            Just(&b""[..]),
            Just(&b"\0\x01\0\0"[..]),
            Just(&b"OTTO"[..]),
            Just(&b"ttcf"[..]),
            Just(&b"true"[..]),
        ],
        tail in proptest::collection::vec(any::<u8>(), 0..400),
    ) {
        let mut data = prefix.to_vec();
        data.extend_from_slice(&tail);
        let _ = info(&data);
    }

    #[test]
    fn damaged_headers_never_panic(
        changes in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
    ) {
        let names = name(&[(WINDOWS, EN_US, 1, utf16("Family")), (MAC, 0, 16, b"Mac".to_vec())]);
        let mut data = sfnt(
            b"\0\x01\0\0",
            &[(b"name", names), (b"OS/2", os2(700, 0x21, 96)), (b"head", head(1))],
        );
        for (at, value) in changes {
            let at = at.index(data.len());
            data[at] = value;
        }
        if let Some(info) = info(&data) {
            prop_assert!(!info.family.trim().is_empty());
        }
    }
}

#[cfg(feature = "outline")]
mod outline {
    use super::*;
    use paperpaw_text::{Font, FontError, OutlineFont};

    /// Draws a few characters of a font that loaded; nothing may panic and
    /// every glyph's bitmap is as large as it says.
    fn use_font(font: &OutlineFont) {
        for size in [0.0, 1.0, 13.5, 48.0] {
            let _ = font.line_metrics(size);
            for ch in ['A', 'g', '?', '\u{e000}', '\u{ea01}', '\0'] {
                let _ = (font.advance(ch, size), font.has_char(ch));
                let glyph = font.rasterize(ch, size);
                let glyph = glyph.as_ref();
                if !glyph.is_empty() {
                    let _ = glyph.coverage(glyph.width - 1, glyph.height - 1);
                }
            }
        }
    }

    #[test]
    fn a_font_without_a_character_map_is_refused() {
        // Rename the cmap table's tag: the font has none.
        let mut data = ICONS.to_vec();
        let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
        let record = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&r| &data[r..r + 4] == b"cmap")
            .unwrap();
        data[record..record + 4].copy_from_slice(b"xmap");
        let error = OutlineFont::new(data, &[]).unwrap_err();
        assert!(
            matches!(error, FontError::NoCharmap | FontError::Invalid),
            "{error:?}"
        );
    }

    #[test]
    fn missing_characters_draw_as_a_replacement() {
        let font = OutlineFont::new(LITERATA, &[]).unwrap();
        assert!(!font.has_char('\u{e000}'));
        // U+FFFD or .notdef: something visible, with an advance.
        let glyph = font.rasterize('\u{e000}', 24.0);
        assert!(glyph.advance > 0.0);
        // Unknown axes are ignored.
        assert!(OutlineFont::new(LITERATA, &[("nope", 1.0), ("wght", 9999.0)]).is_ok());
        assert!(matches!(
            Font::from_data(&b"\0\x01\0\0garbage"[..], &[]),
            Err(FontError::Invalid)
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 64,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        #[test]
        fn damaged_fonts_are_refused_or_drawn_safely(
            changes in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12),
            cut in any::<prop::sample::Index>(),
            truncate in any::<bool>(),
        ) {
            let mut data = ICONS.to_vec();
            for (at, value) in changes {
                let at = at.index(data.len());
                data[at] = value;
            }
            if truncate {
                data.truncate(cut.index(data.len()));
            }
            if let Ok(font) = OutlineFont::new(data, &[("wght", 700.0)]) {
                use_font(&font);
            }
        }

        #[test]
        fn random_bytes_are_no_font(tail in proptest::collection::vec(any::<u8>(), 0..2000)) {
            let mut data = b"\0\x01\0\0".to_vec();
            data.extend_from_slice(&tail);
            if let Ok(Font::Outline(font)) = Font::from_data(data, &[]) {
                use_font(&font);
            }
        }
    }
}
