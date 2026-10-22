//! Properties of line breaking (`wrap_lines_hyphenated`) for random texts:
//! words of any length, several spaces, newlines, hyphens, soft hyphens,
//! non-breaking spaces, CJK without spaces, characters of different
//! widths (zero included) and lines of any width.
//!
//! Checked: lines cover the text in order and lose nothing but the spaces
//! they break at, what each line reports (width, spaces, paragraph end,
//! hyphen) matches its text, no line is wider than its box but for a
//! single glyph, and breaking from the start of any line gives the same
//! lines as breaking the whole text.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_text::{Line, wrap, wrap_lines, wrap_lines_hyphenated};
use proptest::prelude::*;

/// Character widths that differ a lot: narrow letters, wide CJK, zero
/// width soft hyphens.
fn varied(c: char) -> f32 {
    match c {
        ' ' => 0.5,
        '\u{ad}' => 0.0,
        '-' => 0.6,
        c if ('\u{4e00}'..='\u{9fff}').contains(&c) => 2.4,
        'i' | 'l' => 0.5,
        'm' | 'w' => 1.8,
        c if c.is_ascii_lowercase() => 1.0 + (c as u32 % 3) as f32 * 0.25,
        _ => 1.1,
    }
}

/// Every character one unit wide.
fn even(_: char) -> f32 {
    1.0
}

/// Breaks every word of four or more characters after every second.
fn every_second(word: &str, out: &mut dyn FnMut(usize)) {
    let offsets: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    for k in (2..offsets.len().saturating_sub(1)).step_by(2) {
        out(offsets[k]);
    }
}

fn text() -> impl Strategy<Value = String> {
    let c = prop_oneof![
        12 => proptest::char::range('a', 'z'),
        3 => Just(' '),
        1 => Just('\n'),
        1 => Just('-'),
        1 => Just('\u{2014}'),
        1 => Just('\u{ad}'),
        1 => Just('\u{a0}'),
        1 => Just('ä'),
        1 => Just('A'),
        2 => proptest::char::range('\u{4e00}', '\u{4e20}'),
    ];
    proptest::collection::vec(c, 0..120).prop_map(|chars| chars.into_iter().collect())
}

fn measure(text: &str, advance: fn(char) -> f32) -> f32 {
    text.chars().map(advance).sum()
}

fn lines(
    text: &str,
    width: f32,
    first: f32,
    advance: fn(char) -> f32,
    hyphenate: bool,
) -> Vec<Line> {
    let hyphenate = hyphenate.then_some(&every_second as &dyn Fn(&str, &mut dyn FnMut(usize)));
    wrap_lines_hyphenated(
        text,
        |n| if n == 0 { first } else { width },
        |_, c| advance(c),
        hyphenate,
    )
}

/// What every result of line breaking must satisfy, whatever the widths.
fn check_structure(text: &str, lines: &[Line], advance: fn(char) -> f32) {
    assert!(!lines.is_empty(), "even empty text is a line");
    let mut at = 0;
    for (n, line) in lines.iter().enumerate() {
        let range = line.range.clone();
        assert!(range.start >= at && range.start <= range.end, "{lines:?}");
        assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
        // Between lines only the spaces broken at and one newline.
        let gap = &text[at..range.start];
        assert!(
            gap.chars().all(|c| c == ' ' || c == '\n') && gap.matches('\n').count() <= 1,
            "{gap:?} lost before line {n} of {text:?}"
        );
        if n > 0 {
            assert_eq!(
                gap.contains('\n'),
                lines[n - 1].last,
                "a line ends its paragraph exactly before a newline: {text:?}"
            );
        }
        let shown = &text[range.clone()];
        assert!(!shown.contains('\n'));
        assert!(!shown.ends_with(' '), "trailing space in {shown:?}");
        assert_eq!(line.spaces, shown.matches(' ').count(), "{shown:?}");
        let hyphen = if line.hyphen { advance('-') } else { 0.0 };
        let width = measure(shown, advance) + hyphen;
        assert!(
            (line.width - width).abs() < 1e-3,
            "{shown:?} reported {} wide, is {width}",
            line.width
        );
        if line.hyphen {
            assert!(!line.last);
            let next = text[range.end..].chars().next();
            assert!(
                next.is_some_and(|c| c != ' ' && c != '\n'),
                "a hyphen only inside a word: {shown:?}"
            );
        }
        at = range.end;
    }
    // A last paragraph of nothing but spaces may go.
    let rest = &text[at..];
    assert!(
        rest.chars().all(|c| c == ' ' || c == '\n') && rest.matches('\n').count() <= 1,
        "{rest:?} lost at the end"
    );
    assert!(lines.last().unwrap().last);
}

/// No line wider than its box, unless it is a single glyph.
fn check_widths(lines: &[Line], text: &str, width: f32, first: f32, advance: fn(char) -> f32) {
    for (n, line) in lines.iter().enumerate() {
        let available = if n == 0 { first } else { width };
        let shown = &text[line.range.clone()];
        let hyphen = if line.hyphen { advance('-') } else { 0.0 };
        let used = measure(shown, advance) + hyphen;
        assert!(
            used <= available + 1e-3 || shown.chars().count() <= 1,
            "line {n} {shown:?} is {used} wide, its box {available}",
        );
    }
}

/// Breaking from the start of any line gives the same lines.
fn check_stable(text: &str, lines: &[Line], width: f32, advance: fn(char) -> f32, hy: bool) {
    for line in &lines[1..] {
        let from = line.range.start;
        let rest = self::lines(&text[from..], width, width, advance, hy);
        let again = &rest[0];
        assert_eq!(
            &text[from + again.range.start..from + again.range.end],
            &text[line.range.clone()],
            "from {from} of {text:?}"
        );
        assert_eq!(again.hyphen, line.hyphen);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn lines_cover_the_text_and_report_it_truly(
        text in text(),
        width in 0.5f32..30.0,
        first in 0.0f32..30.0,
        hyphenate in any::<bool>(),
    ) {
        for advance in [even as fn(char) -> f32, varied] {
            let result = lines(&text, width, first, advance, hyphenate);
            check_structure(&text, &result, advance);
        }
    }

    /// Glyphs of one width and a first line no wider than the others (an
    /// indent).
    #[test]
    fn lines_of_even_glyphs_fit_and_are_stable(
        text in text(),
        width in 0.5f32..30.0,
        first in 0.0f32..=1.0,
        hyphenate in any::<bool>(),
    ) {
        let first = first * width;
        let result = lines(&text, width, first, even, hyphenate);
        check_widths(&result, &text, width, first, even);
        let result = lines(&text, width, width, even, hyphenate);
        check_stable(&text, &result, width, even, hyphenate);
    }

    #[test]
    fn lines_of_varied_glyphs_fit_and_are_stable(
        text in text(),
        width in 0.5f32..30.0,
        first in 0.0f32..30.0,
        hyphenate in any::<bool>(),
    ) {
        let result = lines(&text, width, first, varied, hyphenate);
        check_widths(&result, &text, width, first, varied);
        let result = lines(&text, width, width, varied, hyphenate);
        check_stable(&text, &result, width, varied, hyphenate);
    }

    #[test]
    fn wrap_returns_the_ranges_of_wrap_lines(text in text(), width in 0.5f32..30.0) {
        let ranges = wrap(&text, width, |s| s.chars().map(varied).sum());
        let lines: Vec<_> = wrap_lines(&text, |_| width, varied)
            .into_iter()
            .map(|l| l.range)
            .collect();
        prop_assert_eq!(ranges, lines);
    }
}

/// After breaking at a space, the character that did not fit must be
/// checked against the new line too: a narrow word, a space and a word of
/// wide glyphs must not give a line wider than the box.
#[test]
fn the_character_after_a_break_must_fit_the_new_line() {
    let advance = |c: char| match c {
        'i' => 1.0,
        ' ' => 0.5,
        _ => 3.0,
    };
    let text = "i WWW";
    let lines = wrap_lines(text, |_| 8.9, advance);
    let shown: Vec<(&str, f32)> = lines
        .iter()
        .map(|l| (&text[l.range.clone()], l.width))
        .collect();
    // "WWW" is 9 wide.
    assert_eq!(shown, [("i", 1.0), ("WW", 6.0), ("W", 3.0)]);
}

/// After a break the text carried to the next line is broken again where
/// it does not fit: the first line may be wider than the next (a hanging
/// indent, which books set with a negative indent).
#[test]
fn text_carried_to_a_narrower_line_is_broken_again() {
    let text = "aaaa-aaaaaaa";
    let lines = wrap_lines(text, |n| if n == 0 { 10.0 } else { 2.0 }, even);
    for line in &lines[1..] {
        assert!(line.width <= 2.0, "{:?}", &text[line.range.clone()]);
    }
}

/// The width of a paragraph's last line leaves out the spaces after it,
/// as its range does, or right-aligned and centred lines would end short
/// of where they should.
#[test]
fn the_last_line_is_reported_without_trailing_spaces() {
    for text in ["ab  ", "ab  \ncd"] {
        let lines = wrap_lines(text, |_| 10.0, even);
        assert_eq!(&text[lines[0].range.clone()], "ab");
        assert_eq!(lines[0].width, 2.0, "{text:?}");
    }
}

/// A word over several lines is hyphenated on each, the hyphen counted.
#[test]
fn hyphenated_lines_fit_with_even_glyphs() {
    let text = "ab abcdefghijkl mn";
    let lines = lines(text, 5.0, 5.0, even, true);
    let shown: Vec<(&str, f32, bool)> = lines
        .iter()
        .map(|l| (&text[l.range.clone()], l.width, l.hyphen))
        .collect();
    // "ab ab-" would be 6 wide: the word moves on and is broken there.
    assert_eq!(
        shown,
        [
            ("ab", 2.0, false),
            ("abcd", 5.0, true),
            ("efgh", 5.0, true),
            ("ijkl", 4.0, false),
            ("mn", 2.0, false)
        ]
    );
}

#[test]
fn a_glyph_wider_than_the_box_gets_a_line_of_its_own() {
    let text = "一二三";
    let lines = wrap_lines(text, |_| 1.0, varied);
    let shown: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
    assert_eq!(shown, ["一", "二", "三"]);
}

#[test]
fn non_breaking_spaces_and_soft_hyphens_do_not_break() {
    let text = "aa\u{a0}bb\u{ad}cc dd";
    let lines = wrap_lines(text, |_| 8.0, even);
    let shown: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
    // "aa bb­cc" is 8 characters: no break inside it.
    assert_eq!(shown, ["aa\u{a0}bb\u{ad}cc", "dd"]);
}

#[test]
fn widths_of_zero_or_less_still_make_progress() {
    for width in [0.0, -5.0] {
        let text = "ab cd";
        let lines = wrap_lines(text, |_| width, even);
        let shown: Vec<&str> = lines.iter().map(|l| &text[l.range.clone()]).collect();
        assert_eq!(shown, ["a", "b", "c", "d"], "width {width}");
    }
}
