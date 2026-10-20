//! Line breaking with the real German and English hyphenation patterns
//! (those of the hyphenation plugin): words are only ever broken where
//! the patterns allow it for the whole word, also when a word runs over
//! more than two lines, and breaking never changes the text.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::OnceLock;

use paperpaw_hyphen::{Patterns, compile};
use paperpaw_text::wrap_lines_hyphenated;

fn german() -> &'static [u8] {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    TABLE.get_or_init(|| {
        compile(
            include_str!("../../../plugins/hyphenation/patterns/hyph-de-1996.pat.txt"),
            "",
            2,
            2,
        )
    })
}

fn english() -> &'static [u8] {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    TABLE.get_or_init(|| {
        compile(
            include_str!("../../../plugins/hyphenation/patterns/hyph-en-us.pat.txt"),
            include_str!("../../../plugins/hyphenation/patterns/hyph-en-us.hyp.txt"),
            2,
            3,
        )
    })
}

/// The byte offsets where `patterns` allow breaking `word`.
fn points(patterns: &Patterns<'_>, word: &str) -> Vec<usize> {
    let mut points = Vec::new();
    patterns.hyphenate(word, &mut |at| points.push(at));
    points
}

/// Where lines `width` characters wide break `text` inside words.
fn cuts(patterns: &Patterns<'_>, text: &str, width: f32) -> Vec<usize> {
    let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| patterns.hyphenate(word, out);
    let lines = wrap_lines_hyphenated(text, |_| width, |_, _| 1.0, Some(&hyphenate));
    lines
        .iter()
        .filter(|l| l.hyphen)
        .map(|l| l.range.end)
        .collect()
}

const GERMAN_WORDS: [&str; 6] = [
    "Donaudampfschifffahrtsgesellschaftskapitän",
    "Rindfleischetikettierungsüberwachungsaufgaben",
    "Kraftfahrzeughaftpflichtversicherung",
    "Grundstücksverkehrsgenehmigungszuständigkeit",
    "Silbentrennungsalgorithmus",
    "Bundesverfassungsgericht",
];

const ENGLISH_WORDS: [&str; 4] = [
    "incomprehensibilities",
    "internationalization",
    "counterrevolutionaries",
    "electroencephalographically",
];

/// A word broken over two lines is broken where the patterns allow.
#[test]
fn a_word_over_two_lines_breaks_where_the_patterns_allow() {
    for (table, words) in [
        (german(), &GERMAN_WORDS[..]),
        (english(), &ENGLISH_WORDS[..]),
    ] {
        let patterns = Patterns::new(table).unwrap();
        for word in words {
            let allowed = points(&patterns, word);
            assert!(!allowed.is_empty(), "{word} has break points");
            let half = (word.chars().count() / 2 + 1) as f32;
            let text = format!("ab {word}");
            for cut in cuts(&patterns, &text, half + 3.0).into_iter().take(1) {
                assert!(allowed.contains(&(cut - 3)), "{word} broken at {}", cut - 3);
            }
        }
    }
}

/// A word over three or more lines (a narrow column, a long compound) is
/// only broken where the patterns allow it for the whole word, also
/// where the rest after the first break is hyphenated again.
#[test]
fn a_word_over_many_lines_breaks_only_where_the_patterns_allow() {
    let mut wrong = Vec::new();
    for (table, words) in [
        (german(), &GERMAN_WORDS[..]),
        (english(), &ENGLISH_WORDS[..]),
    ] {
        let patterns = Patterns::new(table).unwrap();
        for word in words {
            let allowed = points(&patterns, word);
            for width in 5..16 {
                for cut in cuts(&patterns, word, width as f32) {
                    if !allowed.contains(&cut) {
                        wrong.push(format!("{}-{} ({width})", &word[..cut], &word[cut..]));
                    }
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "broken where the patterns do not allow: {wrong:#?}"
    );
}

/// Hyphenating adds breaks only: the lines and the gaps between them give
/// back the text, every break inside a word of letters.
#[test]
fn hyphenating_keeps_the_text() {
    let text = "Die Donaudampfschifffahrtsgesellschaft und die internationalization \
                of Grundstücksverkehrsgenehmigungen, Silbentrennung (s) und GROSSBUCHSTABEN.";
    let caps = text.find("GROSS").unwrap();
    let caps = caps..caps + "GROSSBUCHSTABEN".len();
    for table in [german(), english()] {
        let patterns = Patterns::new(table).unwrap();
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| patterns.hyphenate(word, out);
        for width in 4..40 {
            let lines = wrap_lines_hyphenated(text, |_| width as f32, |_, _| 1.0, Some(&hyphenate));
            let mut rebuilt = String::new();
            let mut at = 0;
            for line in &lines {
                rebuilt.push_str(&text[at..line.range.end]);
                at = line.range.end;
                if line.hyphen {
                    let before = text[..at].chars().next_back().unwrap();
                    let after = text[at..].chars().next().unwrap();
                    assert!(
                        before.is_alphabetic() && after.is_alphabetic(),
                        "broken between {before:?} and {after:?} at {width}"
                    );
                    assert!(!(caps.contains(&at)), "capitals are not hyphenated");
                }
            }
            rebuilt.push_str(&text[at..]);
            assert_eq!(rebuilt, text);
        }
    }
}
