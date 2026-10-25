//! The table hyphenates known Spanish words as the pattern authors expect,
//! keeps its minimums and registers its language.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::hyphen::Patterns;
use paperpaw_sdk::{Contribution, Registrar};

use super::*;

fn breaks(word: &str) -> Vec<usize> {
    let patterns = Patterns::new(TABLE).unwrap();
    let mut found = Vec::new();
    patterns.hyphenate(word, &mut |at| found.push(at));
    found
}

fn hyphenated(word: &str) -> String {
    let found = breaks(word);
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
fn words_as_the_authors_expect() {
    for expected in [
        "bi-blio-te-ca",
        "uni-ver-si-dad",
        "or-de-na-dor",
        "her-mo-so",
        "pa-la-bra",
        "cons-truc-ción",
    ] {
        let word: String = expected.chars().filter(|&c| c != '-').collect();
        assert_eq!(hyphenated(&word), expected);
    }
}

#[test]
fn the_minimums_are_kept() {
    let (left, right) = Patterns::new(TABLE).unwrap().minimums();
    assert_eq!((left, right), (2, 2));
    for len in 0..left + right {
        let word: String = core::iter::repeat_n('a', len).collect();
        assert!(breaks(&word).is_empty(), "{word}");
    }
}

#[test]
fn case_does_not_matter() {
    for word in ["biblioteca", "universidad"] {
        let lower = breaks(&word.to_lowercase());
        assert!(!lower.is_empty(), "{word}");
        assert_eq!(breaks(&word.to_uppercase()), lower, "{word}");
    }
}

#[test]
fn the_plugin_registers_its_languages() {
    let mut registrar = Registrar::new(PLUGIN);
    Spanish.register(&mut registrar).unwrap();
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
    assert_eq!(languages, [("hyphenation-es.es", "es")]);
}
