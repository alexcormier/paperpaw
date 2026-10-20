//! Liang hyphenation on compiled tables: for random pattern sets the
//! table finds exactly the breaks a direct reading of the patterns gives
//! (an oracle), breaks are always inside the word, ascending, on
//! character boundaries and keep the minimums; case does not matter;
//! exceptions win; damaged or random tables are refused or used without
//! panicking.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_hyphen::{Patterns, compile};
use proptest::prelude::*;

fn breaks(patterns: &Patterns<'_>, word: &str) -> Vec<usize> {
    let mut found = Vec::new();
    patterns.hyphenate(word, &mut |at| found.push(at));
    found
}

fn hyphenated(patterns: &Patterns<'_>, word: &str) -> String {
    let found = breaks(patterns, word);
    let mut out = String::new();
    for (i, c) in word.char_indices() {
        if found.contains(&i) {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// Liang's algorithm read straight from the patterns, without a table.
fn oracle(patterns: &[String], word: &str, left_min: usize, right_min: usize) -> Vec<usize> {
    // (letters, points between and around them); the first of equal keys
    // counts, as `compile` keeps it.
    let mut parsed: Vec<(Vec<char>, Vec<u8>)> = Vec::new();
    for pattern in patterns {
        let mut key = Vec::new();
        let mut points = vec![0u8];
        for c in pattern.chars() {
            match c.to_digit(10) {
                Some(d) => *points.last_mut().unwrap() = d as u8,
                None => {
                    key.push(c);
                    points.push(0);
                }
            }
        }
        if !key.is_empty() && !parsed.iter().any(|(k, _)| *k == key) {
            parsed.push((key, points));
        }
    }
    let mut chars = vec!['.'];
    chars.extend(word.chars().flat_map(char::to_lowercase));
    chars.push('.');
    let n = chars.len() - 2;
    let (left_min, right_min) = (left_min.max(1), right_min.max(1));
    if n < left_min + right_min || n > 48 {
        return Vec::new();
    }
    let mut values = vec![0u8; chars.len() + 1];
    for start in 0..chars.len() {
        for (key, points) in &parsed {
            if chars[start..].starts_with(key) {
                for (k, &point) in points.iter().enumerate() {
                    values[start + k] = values[start + k].max(point);
                }
            }
        }
    }
    let offsets: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    (left_min..=n - right_min)
        .filter(|p| values[p + 1] % 2 == 1)
        .map(|p| offsets[p])
        .collect()
}

fn pattern() -> impl Strategy<Value = String> {
    (
        any::<bool>(),
        proptest::collection::vec(
            (0u32..10, prop::sample::select(vec!['a', 'b', 'c', 'ä'])),
            1..6,
        ),
        0u32..10,
        any::<bool>(),
    )
        .prop_map(|(start, letters, last, end)| {
            let mut out = String::new();
            if start {
                out.push('.');
            }
            for (point, letter) in letters {
                if point > 0 {
                    out.push(char::from_digit(point, 10).unwrap());
                }
                out.push(letter);
            }
            if last > 0 {
                out.push(char::from_digit(last, 10).unwrap());
            }
            if end {
                out.push('.');
            }
            out
        })
}

fn word() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop::sample::select(vec!['a', 'b', 'c', 'ä', 'A', 'Ä']),
        0..20,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

/// What every break must be: inside the word, ascending, on a character
/// boundary, with the minimums before and after.
fn check_breaks(word: &str, found: &[usize], left_min: usize, right_min: usize) {
    for (k, &at) in found.iter().enumerate() {
        assert!(k == 0 || found[k - 1] < at, "ascending: {word}: {found:?}");
        assert!(0 < at && at < word.len(), "inside: {word}: {found:?}");
        assert!(word.is_char_boundary(at), "{word}: {found:?}");
        assert!(word[..at].chars().count() >= left_min, "{word}: {found:?}");
        assert!(word[at..].chars().count() >= right_min, "{word}: {found:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn the_table_breaks_where_the_patterns_say(
        patterns in proptest::collection::vec(pattern(), 0..60),
        words in proptest::collection::vec(word(), 1..10),
        left_min in 0u8..4,
        right_min in 0u8..4,
    ) {
        let table = compile(&patterns.join(" "), "", left_min, right_min);
        let compiled = Patterns::new(&table).unwrap();
        prop_assert_eq!(
            compiled.minimums(),
            (usize::from(left_min).max(1), usize::from(right_min).max(1))
        );
        for word in &words {
            let found = breaks(&compiled, word);
            let expected = oracle(&patterns, word, left_min.into(), right_min.into());
            prop_assert_eq!(&found, &expected, "{}", word);
            check_breaks(word, &found, left_min.max(1).into(), right_min.max(1).into());
            // Case does not matter.
            prop_assert_eq!(breaks(&compiled, &word.to_lowercase()), found);
        }
    }

    #[test]
    fn random_tables_are_refused_or_used_safely(
        header in proptest::collection::vec(any::<u8>(), 0..24),
        body in proptest::collection::vec(any::<u8>(), 0..200),
        words in proptest::collection::vec(word(), 1..5),
    ) {
        let mut data = b"PPHP\x01".to_vec();
        data.extend_from_slice(&header);
        data.extend_from_slice(&body);
        if let Ok(patterns) = Patterns::new(&data) {
            for word in &words {
                let found = breaks(&patterns, word);
                let (left, right) = patterns.minimums();
                // Without exceptions in the way (a damaged table may have
                // any), the minimums hold.
                check_breaks(word, &found, left.min(1), right.min(1));
            }
        }
    }
}

/// Exceptions are words with `-` where they may break; one with a `-` at
/// its start or end names a break outside the word: at the start it is
/// reported at offset 0, at the end the word's letter offsets are
/// indexed past their end and hyphenating panics.
#[test]
#[ignore = "bug: an exception with a hyphen at its end panics, one at its start breaks at 0"]
fn exceptions_never_break_outside_the_word() {
    let table = compile("", "table- -chair", 1, 1);
    let patterns = Patterns::new(&table).unwrap();
    for word in ["table", "chair"] {
        let found = breaks(&patterns, word);
        check_breaks(word, &found, 1, 1);
    }
}

/// TeX keeps `\lefthyphenmin` and `\righthyphenmin` also for words from
/// its exception list; here exceptions break wherever they say.
#[test]
#[ignore = "bug: exceptions ignore the minimum characters before and after a hyphen"]
fn exceptions_keep_the_minimums() {
    let table = compile("", "a-ble-s", 2, 2);
    let patterns = Patterns::new(&table).unwrap();
    assert_eq!(hyphenated(&patterns, "ables"), "ables");
}

/// A capital letter in a pattern whose lower case is two characters (İ:
/// i and a combining dot) gives a key of more characters than the pattern
/// has points for, and the entries after it are read from the wrong
/// bytes.
#[test]
#[ignore = "bug: compile miscounts points for letters whose lower case is two characters"]
fn letters_lowercased_to_two_characters_do_not_break_the_table() {
    // "İ1" becomes "i̇" with one point short; "z" is stored after it.
    let patterns = ["1b", "İ1", "c1d", "1z"];
    let table = compile(&patterns.join(" "), "", 1, 1);
    let compiled = Patterns::new(&table).unwrap();
    for word in ["abcd", "azaz", "cdcd"] {
        let lowered: Vec<String> = patterns.iter().map(|p| p.to_lowercase()).collect();
        assert_eq!(
            breaks(&compiled, word),
            oracle(&lowered, word, 1, 1),
            "{word}"
        );
    }
}

#[test]
fn exceptions_win_over_patterns_and_ignore_case() {
    let table = compile("1b 1c", "abc-d", 1, 1);
    let patterns = Patterns::new(&table).unwrap();
    assert_eq!(hyphenated(&patterns, "abcd"), "abc-d");
    assert_eq!(hyphenated(&patterns, "ABCD"), "ABC-D");
    // Other words still use the patterns.
    assert_eq!(hyphenated(&patterns, "abcb"), "a-b-c-b");
    // An exception without hyphens: never broken.
    let table = compile("1b", "abab", 1, 1);
    assert_eq!(hyphenated(&Patterns::new(&table).unwrap(), "abab"), "abab");
}

#[test]
fn short_long_and_odd_words() {
    let table = compile("1b 1c 1d", "", 1, 1);
    let patterns = Patterns::new(&table).unwrap();
    for word in ["", "a", "b", "ab"] {
        let found = breaks(&patterns, word);
        check_breaks(word, &found, 1, 1);
    }
    assert_eq!(hyphenated(&patterns, "ab"), "a-b");
    // Longer than 48 letters: left alone.
    let long = "ab".repeat(25);
    assert!(breaks(&patterns, &long).is_empty());
    assert_eq!(breaks(&patterns, &"ab".repeat(24)).len(), 24);
    // Digits, apostrophes and the like match no pattern of letters.
    assert_eq!(hyphenated(&patterns, "a1'2a"), "a1'2a");
    // Letters that change length when lowercased leave the word alone.
    assert!(breaks(&patterns, "İbc").is_empty());
}

#[test]
fn damaged_headers_are_refused() {
    let table = compile("1b 1c", "ab-c", 1, 1);
    assert!(Patterns::new(&table).is_ok());
    let mut old = table.clone();
    old[..4].copy_from_slice(b"QIHP");
    assert!(Patterns::new(&old).is_ok(), "the old name is still read");
    for len in 0..24 {
        assert!(Patterns::new(&table[..len]).is_err(), "{len} bytes");
    }
    let mut other = table.clone();
    other[4] = 2;
    assert!(Patterns::new(&other).is_err(), "another version");
    // Index blocks or exceptions past the end.
    for (at, value) in [(12, u32::MAX), (12, 1000), (16, u32::MAX)] {
        let mut data = table.clone();
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Patterns::new(&data).is_err(), "{at}: {value}");
    }
    // Exception counts past the end: what is there is used.
    let mut data = table.clone();
    data[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    let patterns = Patterns::new(&data).unwrap();
    assert_eq!(hyphenated(&patterns, "abc"), "ab-c");
    assert_eq!(hyphenated(&patterns, "cbc"), "c-b-c");
}

#[test]
fn compiling_skips_comments_and_keeps_the_first_of_equal_patterns() {
    let table = compile("% 1a comment\n1b\nb1 % not a comment here\n", "", 1, 1);
    let patterns = Patterns::new(&table).unwrap();
    // "1b" first, "b1" is the same key and dropped: break before b only.
    assert_eq!(hyphenated(&patterns, "abab"), "a-ba-b");
    assert_eq!(hyphenated(&patterns, "aaaa"), "aaaa");
}
