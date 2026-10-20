//! Finding text in a block and the snippet around it, for any text: the
//! hits are whole characters, do not overlap, are the text looked for in
//! any case, and every place of it is found; a snippet never panics and
//! holds its hit.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::Block;
use proptest::prelude::*;

use super::*;

fn needle(text: &str) -> Vec<char> {
    text.chars().map(lower).collect()
}

fn block(text: &str) -> Block {
    use paperpaw_sdk::document::{Align, BlockKind, OffsetMap};
    Block {
        kind: BlockKind::Paragraph,
        continues: false,
        start: 0,
        end: text.len() as u32,
        text: String::from(text),
        map: OffsetMap::IDENTITY,
        runs: Vec::new(),
        align: Align::Auto,
        indent: None,
        space: None,
        size: None,
        image: None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn hits_are_whole_characters_of_the_text_looked_for(
        text in "[aAbBäÄß€😀İ .,\n]{0,60}",
        query in "[abäß€😀İ ]{1,4}",
    ) {
        let n = needle(&query);
        let hits = find(&text, &n);
        let mut end = 0;
        for &(start, stop) in &hits {
            prop_assert!(start >= end, "overlap in {hits:?}");
            prop_assert!(text.is_char_boundary(start) && text.is_char_boundary(stop));
            let found: Vec<char> = text[start..stop].chars().map(lower).collect();
            prop_assert_eq!(&found, &n);
            end = stop;
        }
    }

    #[test]
    fn every_place_of_it_is_found(
        parts in proptest::collection::vec("[xyz ]{0,6}", 1..6),
        word in "[a-dä]{1,4}",
    ) {
        // The word between the parts, in upper case every other time.
        let mut text = String::new();
        for (i, part) in parts.iter().enumerate() {
            text.push_str(part);
            if i + 1 < parts.len() {
                if i % 2 == 0 {
                    text.push_str(&word.to_uppercase());
                } else {
                    text.push_str(&word);
                }
            }
        }
        let hits = find(&text, &needle(&word));
        prop_assert_eq!(hits.len(), parts.len() - 1, "{} in {}", word, text);
    }

    #[test]
    fn a_snippet_holds_its_hit(
        before in "[a-zäö€😀 \n\t]{0,80}",
        hit in "[A-Z]{1,8}",
        after in "[a-zäö€😀 \n\t]{0,120}",
    ) {
        let text = alloc::format!("{before}{hit}{after}");
        let start = before.len();
        let end = start + hit.len();
        let shown = snippet(&block(&text), start, end);
        prop_assert!(shown.contains(&hit), "{shown}");
        prop_assert!(!shown.contains(['\n', '\t']));
        // At most the context on either side, and the marks for more.
        prop_assert!(shown.chars().count() <= BEFORE + hit.chars().count() + AFTER + 2);
        prop_assert_eq!(shown.starts_with('…'), before.chars().count() > BEFORE);
        prop_assert_eq!(shown.ends_with('…'), after.chars().count() > AFTER);
    }
}

#[test]
fn nothing_is_found_in_nothing() {
    assert!(find("", &needle("a")).is_empty());
    assert!(find("abc", &needle("abcd")).is_empty());
    assert!(find("abc", &[]).is_empty());
}

#[test]
fn a_hit_at_either_end_is_found_whole() {
    let text = "€uro and €uro";
    let hits = find(text, &needle("€URO"));
    assert_eq!(hits, [(0, 6), (11, 17)]);
    assert_eq!(snippet(&block(text), 11, 17), "€uro and €uro");
}

#[test]
fn hits_do_not_overlap() {
    assert_eq!(find("aaaa", &needle("aa")), [(0, 2), (2, 4)]);
}
