//! Text as these widgets show it: in the font the reader chose for it,
//! broken into lines, as large as its place allows, between the
//! quotation marks of its language.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::FontHandle;
use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align};
use paperpaw_sdk::{Canvas, Color, Env, Rect};

/// The values of a widget's font setting: the menus' font, the book's,
/// the book's italic.
pub(super) const FACES: [&str; 3] = [
    "widgets.face.ui",
    "widgets.face.book",
    "widgets.face.italic",
];

/// The font of `face`, one of [`FACES`]: the book's fonts are those the
/// reader reads in, their italic where there is one; the menus' font
/// where there is none.
pub(super) fn font<'a>(env: &Env<'a>, face: &str) -> Option<FontHandle<'a>> {
    let ui = || UiStyle::new(env).font(env);
    let book = || env.fonts.bound(fonts::READING.as_str());
    match FACES.iter().position(|f| *f == face) {
        Some(1) => book().or_else(ui),
        Some(2) => env
            .fonts
            .bound(fonts::READING_ITALIC.as_str())
            .or_else(book)
            .or_else(ui),
        _ => ui(),
    }
}

/// The height of a line of text `size` pixels large in `font`.
pub(super) fn line_height(font: &FontHandle<'_>, size: f32) -> f32 {
    font.line_metrics(size).line_height
}

/// `text` broken between words into lines `width` pixels wide at `size`,
/// at most `max` of them, the last one shortened with "…" where there is
/// more (as is a word wider than a line).
pub(super) fn lines(
    font: &FontHandle<'_>,
    size: f32,
    text: &str,
    width: f32,
    max: usize,
) -> Vec<String> {
    crate::time::wrap(font, size, text, width, max)
}

/// How many lines `text` takes `width` pixels wide at `size`, broken
/// between words, and whether a word is wider than a line alone.
pub(super) fn count_lines(
    font: &FontHandle<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> (usize, bool) {
    let (lines, too_wide) = crate::time::break_words(font, size, text, width);
    (lines.len(), too_wide)
}

/// The largest size from `most` down to `least` at which `fits(size)`
/// holds, measured as drawn ([`widgets::largest_size`]); `least` if it
/// holds at none.
pub(super) fn largest(least: f32, most: f32, fits: impl Fn(f32) -> bool) -> f32 {
    widgets::largest_size(most.max(least), least, fits).unwrap_or(least)
}

/// Draws `lines` at `size`, each `lead` times its line height below the
/// one before, from `top`, each placed in `area` as `align` says.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(super) fn draw_lines(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    lines: &[String],
    area: Rect,
    top: u16,
    align: Align,
    color: Color,
) -> u16 {
    let line = line_height(font, size) as u16;
    // Slanted letters reach past their advance at the end of a line: a
    // line ending at the right edge keeps that much clear of it.
    let overhang = size * 0.12;
    let right = f32::from(area.x) + f32::from(area.width);
    for (n, text) in lines.iter().enumerate() {
        let row = Rect::new(area.x, top + line * n as u16, area.width, line);
        let width = font.measure(size, text);
        let x = align
            .x(area, width)
            .min(right - width - overhang)
            .max(f32::from(area.x));
        ui::draw_label(canvas, font, size, x, row, text, color);
    }
    line * lines.len() as u16
}

/// The quotation marks of `language` (a BCP 47 tag such as `de` or
/// `fr-CA`): where the quote opens and where it closes.
pub(super) fn marks(language: &str) -> (&'static str, &'static str) {
    let lower = language.to_ascii_lowercase();
    let primary = lower.split(['-', '_']).next().unwrap_or("");
    match primary {
        // Swiss German as French, guillemets pointing out.
        "de" if lower.ends_with("ch") => ("«", "»"),
        "de" | "cs" | "sk" | "sl" | "hr" | "et" | "lt" | "bg" | "is" | "ka" => ("„", "“"),
        "pl" | "hu" | "ro" => ("„", "”"),
        // With a narrow space inside.
        "fr" => ("«\u{202f}", "\u{202f}»"),
        "es" | "it" | "pt" | "ru" | "uk" | "ca" | "el" | "nb" | "nn" | "no" | "be" => ("«", "»"),
        "sv" | "fi" => ("”", "”"),
        "da" => ("»", "«"),
        "ja" | "zh" => ("「", "」"),
        _ => ("“", "”"),
    }
}

/// `text` between the quotation marks of `language`, without the plain
/// or other marks it may have come in.
pub(super) fn quoted(text: &str, language: &str) -> String {
    let (open, close) = marks(language);
    format!("{open}{}{close}", unquoted(text))
}

/// `text` without the quotation marks around it, if it has some.
pub(super) fn unquoted(text: &str) -> &str {
    const MARKS: &[char] = &['"', '“', '”', '„', '«', '»', '「', '」'];
    let text = text.trim();
    let inner = text
        .strip_prefix(MARKS)
        .and_then(|rest| rest.strip_suffix(MARKS));
    match inner {
        Some(inner) if !inner.contains(MARKS) => inner.trim(),
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_are_marked_as_their_language_does() {
        assert_eq!(quoted("Read.", "en"), "“Read.”");
        assert_eq!(quoted("Read.", "en-GB"), "“Read.”");
        assert_eq!(quoted("Lies.", "de"), "„Lies.“");
        assert_eq!(quoted("Lies.", "de-CH"), "«Lies.»");
        assert_eq!(quoted("Lis.", "fr"), "«\u{202f}Lis.\u{202f}»");
        assert_eq!(quoted("Czytaj.", "pl"), "„Czytaj.”");
        assert_eq!(quoted("Läs.", "sv"), "”Läs.”");
        assert_eq!(quoted("Leggi.", "it"), "«Leggi.»");
        assert_eq!(quoted("Read.", ""), "“Read.”");
    }

    #[test]
    fn marks_brought_along_are_replaced() {
        assert_eq!(quoted("\"Read.\"", "de"), "„Read.“");
        assert_eq!(quoted(" “Read.” ", "de"), "„Read.“");
        assert_eq!(unquoted("„Lies.“"), "Lies.");
        // Marks inside stay, and with them those around.
        assert_eq!(unquoted("\"a\" and \"b\""), "\"a\" and \"b\"");
        assert_eq!(unquoted("‘Classic.’ A book"), "‘Classic.’ A book");
        assert_eq!(unquoted("\""), "\"");
    }

    #[test]
    fn the_largest_size_that_fits() {
        assert_eq!(largest(10.0, 40.0, |s| s <= 40.0), 40.0);
        let size = largest(10.0, 40.0, |s| s <= 25.0);
        assert!(size <= 25.0 && size > 22.0, "{size}");
        assert_eq!(largest(10.0, 40.0, |_| false), 10.0);
        assert_eq!(largest(10.0, 5.0, |_| true), 10.0);
    }
}
