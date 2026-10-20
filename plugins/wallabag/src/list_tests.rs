//! The lists Wallabag sends and the plugin keeps, at random: the list of
//! unread articles is read from any bytes without panic, at most thirty
//! with ids, and `articles.txt` and `finished.txt` are read from any text.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use super::{MAX_ARTICLES, articles, data, finished, unread};

fn json_piece() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::from("{\"_embedded\": {\"items\": [")),
        Just(String::from("{\"id\": ")),
        Just(String::from("\"title\": ")),
        Just(String::from("\"\\u0000\\ud800\\n\\t\"")),
        Just(String::from("\"../../x\"")),
        Just(String::from("12345678901234567890123")),
        Just(String::from("-1e999")),
        Just(String::from("null")),
        Just(String::from("[[[[[[[[")),
        Just(String::from("]]]]")),
        Just(String::from("{\"tags\": [{\"id\": 1}]}")),
        Just(String::from(",")),
        Just(String::from(":")),
        Just(String::from("}")),
        Just(String::from("]")),
        "[a-z0-9 \"{}\\[\\],:]{0,12}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn any_bytes_give_at_most_thirty_articles_with_ids(data_bytes in prop::collection::vec(any::<u8>(), 0..2000)) {
        let storage = MemoryStorage::new().with(&data("entries.json"), data_bytes);
        let items = unread(&storage).unwrap_or_default();
        prop_assert!(items.len() <= MAX_ARTICLES);
        prop_assert!(items.iter().all(|(id, _)| !id.is_empty()));
    }

    #[test]
    fn pieces_of_lists_give_at_most_thirty_articles_with_ids(parts in prop::collection::vec(json_piece(), 0..100)) {
        let storage = MemoryStorage::new().with(&data("entries.json"), parts.concat());
        let items = unread(&storage).unwrap_or_default();
        prop_assert!(items.len() <= MAX_ARTICLES);
        prop_assert!(items.iter().all(|(id, _)| !id.is_empty()));
    }

    #[test]
    fn any_text_reads_as_lists(text in ".{0,400}") {
        let storage = MemoryStorage::new()
            .with(&data("articles.txt"), text.clone())
            .with(&data("finished.txt"), text);
        for article in articles(&storage).unwrap_or_default() {
            prop_assert!(!article.id.contains(['\t', '\n']));
        }
        prop_assert!(finished(&storage).unwrap_or_default().iter().all(|id| !id.is_empty()));
    }
}

/// A list whose first item holds `depth` nested arrays.
fn nested(depth: usize) -> MemoryStorage {
    let deep = format!(
        "{{\"_embedded\": {{\"items\": [{{\"id\": 1, \"title\": \"Kept\", \"x\": {}{}}}]}}}}",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    MemoryStorage::new().with(&data("entries.json"), deep)
}

#[test]
fn deep_nesting_in_the_list_is_no_harm() {
    assert_eq!(
        unread(&nested(2_000)),
        Some(vec![(String::from("1"), String::from("Kept"))])
    );
}

#[test]
#[ignore = "bug: deeply nested lists take time quadratic in their depth (paperpaw_json::stream copies the path at every level)"]
fn very_deep_nesting_is_read_quickly() {
    // 100 KB from a hostile server: on the X4 this must not take long.
    let storage = nested(50_000);
    let started = std::time::Instant::now();
    assert_eq!(unread(&storage).map_or(0, |items| items.len()), 1);
    let took = started.elapsed();
    assert!(took.as_millis() < 2000, "{took:?}");
}

#[test]
fn lists_too_large_are_not_read() {
    let big: Vec<u8> = "1\tWallabag/a.epub\n".repeat(5000).into_bytes();
    let storage = MemoryStorage::new()
        .with(&data("articles.txt"), big.clone())
        .with(&data("finished.txt"), big);
    // Not read, so that nothing is written over them.
    assert!(articles(&storage).is_err());
    assert!(finished(&storage).is_err());
}
