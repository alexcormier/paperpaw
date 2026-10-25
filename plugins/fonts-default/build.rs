//! Generates bitmap font packs when the `packs` feature is enabled or the
//! `outline` feature is disabled.

use std::path::{Path, PathBuf};

/// Pixel sizes for packs; covers UI and reading text on ~220 dpi screens.
const DEFAULT_SIZES: &str = "20,24,28,32,36,40,48";

/// Large sizes of the UI font for what is shown large alone, a clock's
/// time on the home screen: only [`DISPLAY_CHARS`], the rest at those
/// sizes coming from the nearest size that has them. Packs of every
/// character at such sizes would take megabytes.
/// About 14 % apart, so a clock fills its place closely: some 150 KB
/// of flash, no memory while running (packs are read in place).
const DISPLAY_SIZES: &[f32] = &[56.0, 64.0, 74.0, 84.0, 96.0, 110.0, 126.0, 144.0, 164.0];
/// The digits and the colon, and the dashes of a clock not yet set.
const DISPLAY_CHARS: &str = "0123456789:-";

/// (pack name, font file, weight)
const FONTS: &[(&str, &str, f32)] = &[
    ("literata", "fonts/Literata.ttf", 400.0),
    ("literata-bold", "fonts/Literata.ttf", 700.0),
    ("literata-italic", "fonts/Literata-Italic.ttf", 400.0),
    ("literata-bold-italic", "fonts/Literata-Italic.ttf", 700.0),
    // The UI font's weight is `UI_WEIGHT` in the plugin.
    // Source Sans 3 changed (one weight, Latin only, rasterised): a
    // Modified Version under its licence, which reserves the name
    // "Source", so the pack is PaperPaw Sans.
    ("paperpaw-sans", "fonts/SourceSans3.ttf", 430.0),
];

/// The pack of the UI font, which gets [`DISPLAY_SIZES`].
const UI: &str = "paperpaw-sans";

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=fonts");
    println!("cargo::rerun-if-env-changed=PAPERPAW_FONT_SIZES");
    if std::env::var_os("CARGO_FEATURE_OUTLINE").is_some()
        && std::env::var_os("CARGO_FEATURE_PACKS").is_none()
    {
        return;
    }

    let sizes_text = std::env::var("PAPERPAW_FONT_SIZES").unwrap_or(DEFAULT_SIZES.into());
    let sizes: Vec<f32> = sizes_text
        .split(',')
        .map(|s| {
            s.trim()
                .parse()
                .unwrap_or_else(|_| panic!("invalid font size `{s}`"))
        })
        .collect();

    let out = PathBuf::from(
        std::env::var_os("OUT_DIR").unwrap_or_else(|| panic!("OUT_DIR is set by cargo")),
    );
    for &(name, path, weight) in FONTS {
        let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let font = paperpaw_text::OutlineFont::new(data, &[("wght", weight)])
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let chars = paperpaw_text::charset::chars(paperpaw_text::charset::LATIN);
        let pack = if name == UI {
            with_display(&font, &sizes, chars)
        } else {
            paperpaw_text::build_pack(&font, &sizes, chars)
        };
        write(&out.join(format!("{name}.ppfp")), &pack);
    }
    // Icons sit in the private use area and just above it.
    let path = "fonts/PaperPawIcons.ttf";
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let font = paperpaw_text::OutlineFont::new(data, &[]).unwrap_or_else(|e| panic!("{path}: {e}"));
    let chars = ('\u{e000}'..='\u{ffff}').filter(|ch| font.has_char(*ch));
    let pack = paperpaw_text::build_pack(&font, &sizes, chars);
    write(&out.join("icons.ppfp"), &pack);
}

/// The pack of the UI font: `chars` at `sizes`, and [`DISPLAY_CHARS`] at
/// [`DISPLAY_SIZES`] too.
fn with_display(
    font: &paperpaw_text::OutlineFont,
    sizes: &[f32],
    chars: impl Iterator<Item = char> + Clone,
) -> Vec<u8> {
    let mut builder = paperpaw_text::PackBuilder::new();
    let strikes = sizes
        .iter()
        .map(|&size| (size, chars.clone().collect::<Vec<char>>()))
        .chain(
            DISPLAY_SIZES
                .iter()
                .filter(|size| !sizes.contains(size))
                .map(|&size| (size, DISPLAY_CHARS.chars().collect())),
        );
    for (size, chars) in strikes {
        let glyphs = chars
            .into_iter()
            .filter(|&ch| font.has_char(ch))
            .map(|ch| (ch, font.rasterize(ch, size)))
            .collect();
        builder.add_strike(size, font.line_metrics(size), glyphs);
    }
    builder.build()
}

fn write(path: &Path, data: &[u8]) {
    std::fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
