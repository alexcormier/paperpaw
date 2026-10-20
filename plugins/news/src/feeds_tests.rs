//! `feeds.txt`: what version 1 wrote is written alike, and any list of
//! feeds round trips, one line a feed, whatever the names hold.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

use super::{Feed, data, load, save};

#[test]
fn feeds_of_version_1_are_written_alike() {
    let v1 = include_str!("../testdata/feeds.txt.v1");
    let storage = MemoryStorage::new().with(&data("feeds.txt"), v1);
    let feeds = load(&storage).unwrap();
    save(&storage, &feeds);
    assert_eq!(storage.get(&data("feeds.txt")).unwrap(), v1.as_bytes());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn feeds_round_trip(list in prop::collection::vec(("https://[a-z./?=&]{1,20}", prop_oneof![".{0,30}", "[\t\n\r a-z]{0,8}"]), 0..8)) {
        let feeds: Vec<Feed> = list
            .iter()
            .map(|(url, name)| Feed { url: url.clone(), name: name.clone() })
            .collect();
        let storage = MemoryStorage::new();
        save(&storage, &feeds);
        let expected: Vec<Feed> = feeds
            .iter()
            .map(|f| Feed {
                url: f.url.clone(),
                // Tabs and line breaks become spaces; a carriage return
                // within a name stays.
                name: String::from(f.name.replace(['\t', '\n'], " ").trim()),
            })
            .collect();
        prop_assert_eq!(load(&storage).unwrap(), expected);
    }
}
