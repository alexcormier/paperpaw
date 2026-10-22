//! Dictionaries generated at random: every headword and every synonym
//! put in is found with its definition, whether the definitions are
//! plain or in dictzip chunks of any size, offsets 32 or 64 bits wide;
//! words before the first and after the last are not found; block
//! indexes built in small or large steps are the same.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::collections::BTreeSet;

use common::{Spec, build, found, open};
use paperpaw_sdk::read_file;
use paperpaw_stardict::Dictionary;
use proptest::prelude::*;

/// Words with ASCII and other letters, no white space at their ends
/// (lookups trim it), distinct without regard to ASCII case.
fn words(max: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        "[a-zA-Z0-9äöüßøÆ日本-][a-zA-Z0-9 äöüßøÆ日本'-]{0,20}[a-zA-Z0-9]",
        1..max,
    )
    .prop_map(|words| {
        let mut seen = BTreeSet::new();
        words
            .into_iter()
            .filter(|w| seen.insert(w.to_ascii_lowercase()))
            .collect()
    })
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        words(120),
        words(30),
        prop_oneof![Just(None), (64usize..3000).prop_map(Some)],
        prop_oneof![Just(32u8), Just(64u8)],
    )
        .prop_flat_map(|(words, forms, chunk, offset_bits)| {
            let n = words.len();
            let definitions = prop::collection::vec("[a-zA-Z0-9 .,;äöü日本]{0,300}", n);
            let targets = prop::collection::vec(0..n, forms.len());
            (
                Just(words),
                Just(forms),
                definitions,
                targets,
                Just(chunk),
                Just(offset_bits),
            )
        })
        .prop_map(|(words, forms, definitions, targets, chunk, offset_bits)| {
            let lower: BTreeSet<String> = words.iter().map(|w| w.to_ascii_lowercase()).collect();
            let entries: Vec<(String, String)> = words
                .iter()
                .zip(definitions)
                // Each definition names its word: no two are alike.
                .map(|(w, d)| (w.clone(), format!("{w}: {d}").trim().to_string()))
                .collect();
            let synonyms = forms
                .into_iter()
                .zip(targets)
                .filter(|(form, _)| !lower.contains(&form.to_ascii_lowercase()))
                .map(|(form, t)| (form, words[t].clone()))
                .collect();
            Spec {
                entries,
                synonyms,
                chunk,
                offset_bits,
                types: Some("m".to_string()),
            }
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    #[test]
    fn every_word_put_in_is_found_with_its_definition(spec in spec()) {
        let storage = build(&spec);
        let dictionary = open(&storage);
        for (word, definition) in &spec.entries {
            let hits = found(&dictionary, &storage, word);
            prop_assert!(
                hits.contains(&(word.clone(), definition.clone())),
                "{word:?}: {hits:?}"
            );
            // In ASCII upper case too, where that changes it.
            let upper = word.to_ascii_uppercase();
            let hits = found(&dictionary, &storage, &upper);
            prop_assert!(hits.iter().any(|(w, _)| w == word), "{upper:?}: {hits:?}");
        }
        for (form, word) in &spec.synonyms {
            let hits = found(&dictionary, &storage, form);
            let definition = &spec.entries.iter().find(|(w, _)| w == word).unwrap().1;
            prop_assert!(
                hits.contains(&(word.clone(), definition.clone())),
                "{form:?} -> {word:?}: {hits:?}"
            );
        }
        // Before the first word and after the last one.
        for outside in ["!", "\u{1}", "\u{10ffff}", "zzzzzzzzzzzzzzzzzzzzzzzzzz"] {
            prop_assert_eq!(found(&dictionary, &storage, outside), vec![]);
        }
    }
}

/// The block index is the same however small the steps building it.
#[test]
fn block_indexes_do_not_depend_on_the_step_size() {
    let entries: Vec<(String, String)> = (0..1000)
        .map(|i| (format!("w{i:04}{}", "x".repeat(i % 50)), format!("def {i}")))
        .collect();
    let mut spec = Spec::new(&[]);
    spec.entries = entries;
    let mut caches = Vec::new();
    for step in [1, 7, 300, 4096, 1 << 20] {
        let storage = build(&spec);
        let dictionary = Dictionary::open(&storage, "d/test.ifo", "cache").unwrap();
        let mut builders = dictionary.builders(&storage);
        assert_eq!(builders.len(), 1);
        let builder = &mut builders[0];
        let mut steps = 0;
        while !builder.step(&storage, step).unwrap() {
            steps += 1;
            let (done, total) = builder.progress();
            assert!(done <= total);
        }
        let (done, total) = builder.progress();
        assert_eq!(done, total, "all of the index read");
        if step == 1 {
            // A step reads at least a buffer of 4 KiB.
            assert!(steps >= 5, "small steps take several calls: {steps}");
        }
        assert!(dictionary.builders(&storage).is_empty());
        caches.push(read_file(&storage, "cache/idx.qbi", 1 << 20).unwrap());
        let last = format!("w0999{}", "x".repeat(999 % 50));
        assert_eq!(
            found(&dictionary, &storage, &last),
            vec![(last.clone(), "def 999".to_string())]
        );
    }
    assert!(caches.windows(2).all(|w| w[0] == w[1]));
}

/// A dictionary of a single word: the first and the last at once.
#[test]
fn a_dictionary_of_one_word() {
    for chunk in [None, Some(1)] {
        let mut spec = Spec::new(&[("solo", "alone")]);
        spec.chunk = chunk;
        let storage = build(&spec);
        let dictionary = open(&storage);
        assert_eq!(
            found(&dictionary, &storage, "Solo"),
            vec![("solo".to_string(), "alone".to_string())]
        );
        assert_eq!(found(&dictionary, &storage, "a"), vec![]);
        assert_eq!(found(&dictionary, &storage, "z"), vec![]);
        assert_eq!(found(&dictionary, &storage, "sol"), vec![]);
        assert_eq!(found(&dictionary, &storage, "solos"), vec![]);
    }
}

/// Lookups trim the word; an empty or blank one finds nothing.
#[test]
fn blank_words_find_nothing() {
    let storage = build(&Spec::new(&[("a", "first"), ("b", "second")]));
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, ""), vec![]);
    assert_eq!(found(&dictionary, &storage, "   \t\n"), vec![]);
    assert_eq!(
        found(&dictionary, &storage, "  b \n"),
        vec![("b".to_string(), "second".to_string())]
    );
}

/// Case: ASCII letters match without case; other letters only through
/// the word in lower case.
#[test]
fn case_folding() {
    let storage = build(&Spec::new(&[
        ("Paris", "the city"),
        ("paris", "a young man"),
        ("øy", "island"),
        ("Ærø", "an island"),
        ("straße", "street"),
    ]));
    let dictionary = open(&storage);
    let words = |w: &str| -> Vec<String> {
        found(&dictionary, &storage, w)
            .into_iter()
            .map(|(w, _)| w)
            .collect()
    };
    assert_eq!(words("PARIS"), ["Paris", "paris"]);
    assert_eq!(words("pArIs"), ["Paris", "paris"]);
    // Not ASCII: "ØY" is found through "øy".
    assert_eq!(words("ØY"), ["øy"]);
    // A headword with a capital that is not ASCII: found as written,
    // not in lower case.
    assert_eq!(words("Ærø"), ["Ærø"]);
    assert_eq!(words("ærø"), Vec::<String>::new());
    assert_eq!(words("STRASSE"), Vec::<String>::new());
    assert_eq!(words("STRAßE"), ["straße"]);
}

/// More matches than are returned: the first eight in the index.
#[test]
fn many_matches_are_cut_at_eight() {
    let variants: Vec<String> = (0..16)
        .map(|bits: u32| {
            "abcd"
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if bits & (1 << i) != 0 {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    }
                })
                .collect()
        })
        .collect();
    let entries: Vec<(&str, &str)> = variants.iter().map(|v| (v.as_str(), "x")).collect();
    let storage = build(&Spec::new(&entries));
    let dictionary = open(&storage);
    let hits = found(&dictionary, &storage, "abcd");
    assert_eq!(hits.len(), 8);
    let mut sorted = variants.clone();
    sorted.sort();
    let words: Vec<&str> = hits.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(words, sorted[..8]);
}
