//! `bookmarks.txt` and `history.txt`: what version 1 wrote is read and
//! written alike, any list round trips, and the history never grows past
//! its limit whatever is remembered.

extern crate std;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::{Entry, HISTORY, history_path, load, page_path, remember, save};

/// What version 1 wrote stays readable (see `docs/data-files.md`).
#[test]
fn history_of_version_1_is_read_and_written_alike() {
    let v1 = include_str!("../../testdata/history.txt.v1");
    let storage = MemoryStorage::new().with(&history_path(), v1);
    let entries = load(&storage, &history_path()).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0].title,
        "Die Verwandlung by Franz Kafka - Free Ebook"
    );
    assert_eq!(
        entries[1].url,
        "https://de.m.wikipedia.org/wiki/Franz_Kafka"
    );
    assert_eq!(
        entries[2].url,
        "https://html.duckduckgo.com/html/?q=Kafka+Verwandlung"
    );
    save(&storage, "again.txt", &entries);
    assert_eq!(storage.get("again.txt").unwrap(), v1.as_bytes());
}

#[test]
fn bookmarks_of_version_1_are_written_alike() {
    let v1 = include_str!("../../testdata/bookmarks.txt.v1");
    let storage = MemoryStorage::new().with("b.txt", v1);
    let entries = load(&storage, "b.txt").unwrap();
    save(&storage, "again.txt", &entries);
    assert_eq!(storage.get("again.txt").unwrap(), v1.as_bytes());
}

#[test]
fn a_list_never_saved_is_none_and_an_empty_one_is_empty() {
    let storage = MemoryStorage::new();
    assert_eq!(load(&storage, "never.txt"), None);
    save(&storage, "empty.txt", &[]);
    assert_eq!(load(&storage, "empty.txt"), Some(Vec::new()));
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![".{0,40}", "[\t\r\n a-z]{0,8}"]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(200)))]

    #[test]
    fn lists_round_trip(list in prop::collection::vec((text(), text()), 0..8)) {
        let entries: Vec<Entry> = list.iter().map(|(t, u)| Entry::new(t, u)).collect();
        let storage = MemoryStorage::new();
        save(&storage, "l.txt", &entries);
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let expected: Vec<Entry> = entries
            .iter()
            .filter(|e| !e.url.is_empty())
            .map(|e| Entry::new(&clean(&e.title), &clean(&e.url)))
            .collect();
        prop_assert_eq!(load(&storage, "l.txt").unwrap(), expected);
    }

    #[test]
    fn the_history_stays_within_its_limit(urls in prop::collection::vec(0u8..50, 1..80)) {
        let storage = MemoryStorage::new();
        let mut last = Vec::new();
        for n in &urls {
            let url = format!("https://h.example/{n}");
            storage.write(&page_path(&url), b"page").unwrap();
            last = remember(&storage, Entry::new("T", &url));
        }
        prop_assert!(last.len() <= HISTORY);
        let first = format!("https://h.example/{}", urls[urls.len() - 1]);
        prop_assert_eq!(&last[0].url, &first);
        // Each page once; the pages kept are those listed.
        let mut seen = last.iter().map(|e| e.url.clone()).collect::<Vec<_>>();
        seen.sort();
        seen.dedup();
        prop_assert_eq!(seen.len(), last.len());
        for n in 0u8..50 {
            let url = format!("https://h.example/{n}");
            let listed = last.iter().any(|e| e.url == url);
            let visited = urls.contains(&n);
            prop_assert_eq!(storage.exists(&page_path(&url)), listed && visited);
        }
    }
}
