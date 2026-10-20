//! The word search (`paperpaw_sdk::words`): folding is settled after
//! one pass, every entry is found as itself, and what `find` gives is in
//! bounds, in order, without overlaps and made of entries.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

use paperpaw_sdk::decorations::PageWord;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::words::{WordSet, fold};
use proptest::prelude::*;

fn page<'a>(texts: &'a [String]) -> Vec<PageWord<'a>> {
    let mut at = 0u32;
    texts
        .iter()
        .map(|text| {
            let start = Locator::new(0, at);
            at += text.len() as u32 + 1;
            PageWord {
                text,
                start,
                end: Locator::new(0, at - 1),
            }
        })
        .collect()
}

/// Words as books have them: letters of several scripts, digits,
/// apostrophes, punctuation around them, combining accents.
fn word() -> impl Strategy<Value = String> {
    "[(\"']?[a-zA-ZäöüßẞÄÖÜéçİıΣσς日本0-9]{1,6}(['’][a-z]{1,2})?\u{0301}?[.,;!?)\"—-]?"
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Folding twice is folding once, and folded text holds only
    /// lowercase words between single spaces.
    #[test]
    fn folding_is_settled(text in "\\PC{0,30}") {
        let once = fold(&text);
        prop_assert_eq!(fold(&once), once.clone());
        prop_assert!(!once.starts_with(' ') && !once.ends_with(' '));
        prop_assert!(!once.contains("  "));
    }

    /// Every entry with a word in it is found again, whatever its case
    /// and the punctuation around it.
    #[test]
    fn entries_are_found_as_themselves(entries in proptest::collection::vec(word(), 0..12)) {
        let set = WordSet::new(&entries);
        for entry in &entries {
            prop_assert!(set.contains(entry), "{entry:?} in {set:?}");
            prop_assert!(set.contains(&entry.to_uppercase()) || fold(&entry.to_uppercase()) != fold(entry));
        }
        let mut folded: Vec<String> = entries.iter().map(|e| fold(e)).filter(|f| !f.is_empty()).collect();
        folded.sort();
        folded.dedup();
        prop_assert_eq!(set.len(), folded.len());
        prop_assert_eq!(set.iter().collect::<Vec<_>>(), folded.iter().map(String::as_str).collect::<Vec<_>>());
    }

    /// What `find` gives are runs of the page's words that are entries,
    /// in order, without overlaps, within the page.
    #[test]
    fn found_runs_are_entries_in_order(
        entries in proptest::collection::vec(proptest::collection::vec(word(), 1..3), 0..8),
        texts in proptest::collection::vec(prop_oneof![word(), Just("—".to_string()), Just("...".to_string())], 0..30),
    ) {
        let phrases: Vec<String> = entries.iter().map(|words| words.join(" ")).collect();
        let set = WordSet::new(&phrases);
        let words = page(&texts);
        let found = set.find(&words);
        let mut end = 0;
        for range in &found {
            prop_assert!(range.start >= end && range.start < range.end && range.end <= words.len());
            end = range.end;
            let joined = texts[range.clone()].join(" ");
            prop_assert!(set.contains(&joined), "{joined:?} is no entry");
        }
        // Every single word that is an entry is part of what is found.
        for (i, text) in texts.iter().enumerate() {
            if set.contains(text) {
                prop_assert!(found.iter().any(|r| r.contains(&i)), "{text:?} at {i} not found");
            }
        }
    }

    /// Any text as entries and as a page is searched without panic.
    #[test]
    fn any_text_is_searched(entries in proptest::collection::vec("\\PC{0,8}", 0..6), texts in proptest::collection::vec("\\PC{0,8}", 0..12)) {
        let set = WordSet::new(&entries);
        let found = set.find(&page(&texts));
        prop_assert!(found.iter().all(|r| r.end <= texts.len()));
    }
}

#[test]
fn case_folds_beyond_ascii() {
    assert_eq!(fold("ΣΟΦΙΑ"), fold("σοφια"));
    assert_eq!(fold("STRAẞE"), "strasse");
    // A dotted capital I folds to an i with a combining dot, kept with it.
    assert_eq!(fold("İstanbul"), "i\u{307}stanbul");
    let set = WordSet::new(["İstanbul"]);
    assert!(set.contains("i\u{307}stanbul"));
    // Digits are words too.
    assert_eq!(fold("Chapter 12."), "chapter 12");
    // Apostrophes at the ends are no part of the word.
    assert_eq!(fold("'tis rock'n'roll'"), "tis rock'n'roll");
}

#[test]
fn a_longer_phrase_wins_over_its_start() {
    let set = WordSet::new(["new", "new york", "new york city", "york"]);
    let texts: Vec<String> = ["In", "New", "York", "City,", "new", "York."]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(set.find(&page(&texts)), [1..4, 4..6]);
    // Where the longest is cut short, the longest that fits.
    let texts: Vec<String> = ["new", "york", "town"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let found = set.find(&page(&texts));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], 0..2);
}

#[test]
fn phrases_with_apostrophes_sort_after_their_words() {
    // `don't` sorts after `don` and before `don x`: both are found.
    let set = WordSet::new(["don", "don't", "don quixote"]);
    let texts: Vec<String> = ["Don", "don't", "Don", "Quixote"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(set.find(&page(&texts)), [0..1, 1..2, 2..4]);
}

#[test]
fn many_entries_are_kept_small() {
    let entries: Vec<String> = (0..2000).map(|i| format!("word{i}")).collect();
    let set = WordSet::new(&entries);
    assert_eq!(set.len(), 2000);
    assert!(set.contains("WORD1999"));
    assert!(!set.contains("word2000"));
}
