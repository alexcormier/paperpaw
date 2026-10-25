//! The bundled table hyphenates known words as dictionaries do, keeps its
//! minimums (2 and 3), ignores case, and never breaks a word anywhere but
//! inside it, whatever it holds; the plugin registers its language.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::hyphen::Patterns;
use paperpaw_sdk::{Contribution, Registrar};
use proptest::prelude::*;

use super::*;

fn breaks(table: &[u8], word: &str) -> Vec<usize> {
    let patterns = Patterns::new(table).unwrap();
    let mut found = Vec::new();
    patterns.hyphenate(word, &mut |at| found.push(at));
    found
}

fn hyphenated(table: &[u8], word: &str) -> String {
    let found = breaks(table, word);
    let mut out = String::new();
    for (i, c) in word.char_indices() {
        if found.contains(&i) {
            out.push('-');
        }
        out.push(c);
    }
    out
}

#[test]
fn english_words_as_the_dictionary_has_them() {
    for expected in [
        // Not "com-put-er": three letters at least after a hyphen.
        "com-puter",
        "al-go-rithm",
        "beau-ti-ful",
        "in-for-ma-tion",
        "pos-si-bil-ity",
        // From the exception list.
        "as-so-ciate",
        "project",
        "present",
        "the",
        "an",
    ] {
        let word: String = expected.chars().filter(|&c| c != '-').collect();
        assert_eq!(hyphenated(EN, &word), expected);
    }
}

#[test]
fn the_minimums_are_kept() {
    let en = Patterns::new(EN).unwrap();
    assert_eq!(en.minimums(), (2, 3));
    // Words too short to break at all.
    for word in ["", "a", "ab", "abc", "abcd"] {
        assert!(breaks(EN, word).is_empty(), "{word}");
    }
}

#[test]
fn case_does_not_matter() {
    for (table, word) in [(EN, "Computer"), (EN, "information")] {
        let lower = breaks(table, &word.to_lowercase());
        assert!(!lower.is_empty(), "{word}");
        assert_eq!(breaks(table, &word.to_uppercase()), lower, "{word}");
        assert_eq!(breaks(table, word), lower, "{word}");
    }
}

#[test]
fn the_plugin_registers_its_language() {
    let mut registrar = Registrar::new(PLUGIN);
    Hyphenation.register(&mut registrar).unwrap();
    let mut languages: Vec<(&str, &str)> = registrar
        .contributions()
        .iter()
        .filter_map(|c| match c {
            Contribution::Hyphenation(spec) => {
                assert!(Patterns::new(spec.patterns).is_ok(), "{}", spec.id);
                Some((spec.id.as_str(), spec.language.as_ref()))
            }
            _ => None,
        })
        .collect();
    languages.sort();
    assert_eq!(languages, [("hyphenation.en", "en")]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Any word, with digits, apostrophes, capitals and letters of other
    /// scripts: breaks are inside it, ascending, on character boundaries,
    /// with the minimums before and after.
    #[test]
    fn breaks_are_always_inside_the_word(
        word in "[a-zäöüßA-ZÄÖÜ0-9'’çéñøИя-]{0,60}",
    ) {
        let (left, right) = (2, 3);
        let found = breaks(EN, &word);
        for (k, &at) in found.iter().enumerate() {
            prop_assert!(k == 0 || found[k - 1] < at);
            prop_assert!(0 < at && at < word.len() && word.is_char_boundary(at));
            prop_assert!(word[..at].chars().count() >= left);
            prop_assert!(word[at..].chars().count() >= right);
        }
    }
}
