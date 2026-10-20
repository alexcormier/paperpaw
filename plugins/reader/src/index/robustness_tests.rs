//! The page index read from whatever is in its file: never a panic, never
//! a page beyond those it has, and for an index as the reader writes it,
//! every position found on the page that holds it.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;

const PATH: &str = "pages.bin";

fn written(starts: &[Locator], complete: bool) -> (MemoryStorage, PageIndex) {
    let storage = MemoryStorage::new();
    let mut index = PageIndex::open(&storage, String::from(PATH));
    assert!(index.append(&storage, starts));
    if complete {
        index.finish(&storage);
    }
    (storage, index)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Any bytes: the pages are the whole entries (the last a sentinel if
    /// complete), and looking anything up stays among them.
    #[test]
    fn any_file_is_read_without_a_panic(
        data in proptest::collection::vec(any::<u8>(), 0..200),
        section in any::<u32>(),
        offset in any::<u32>(),
    ) {
        let storage = MemoryStorage::new().with(PATH, data.clone());
        let index = PageIndex::open(&storage, String::from(PATH));
        let entries = (data.len() / 8) as u32;
        prop_assert!(index.pages() <= entries);
        prop_assert_eq!(index.pages() + u32::from(index.is_complete()), entries);
        if let Some((page, _)) = index.locate(&storage, Locator::new(section, offset)) {
            prop_assert!(page < index.pages());
        }
        // Asked again, the same (from what it remembers).
        let again = index.locate(&storage, Locator::new(section, offset));
        prop_assert_eq!(again, index.locate(&storage, Locator::new(section, offset)));
        prop_assert_eq!(index.entry(&storage, entries), None);
        prop_assert_eq!(index.entry(&storage, u32::MAX), None);
        let _ = index.last(&storage);
    }

    /// Page starts in order: a position is on the last page starting at or
    /// before it, and exactly at its start only there.
    #[test]
    fn a_position_is_found_on_its_page(
        mut offsets in proptest::collection::vec(0u32..100_000, 1..60),
        at in 0u32..110_000,
        complete in any::<bool>(),
    ) {
        offsets.sort_unstable();
        offsets.dedup();
        let starts: Vec<Locator> = offsets.iter().map(|&o| Locator::new(0, o)).collect();
        let (storage, index) = written(&starts, complete);
        let reopened = PageIndex::open(&storage, String::from(PATH));
        prop_assert_eq!(reopened.pages(), starts.len() as u32);
        prop_assert_eq!(reopened.is_complete(), complete);
        for index in [index, reopened] {
            let found = index.locate(&storage, Locator::new(0, at));
            let expected = offsets.iter().rposition(|&o| o <= at);
            prop_assert_eq!(found.map(|(p, _)| p as usize), expected);
            if let Some((page, exact)) = found {
                prop_assert_eq!(exact, offsets[page as usize] == at);
                prop_assert_eq!(index.entry(&storage, page), Some(starts[page as usize]));
            }
            prop_assert_eq!(index.last(&storage), starts.last().copied());
        }
    }
}

#[test]
fn a_cut_last_entry_is_not_a_page() {
    let (storage, _) = written(&[Locator::new(0, 0), Locator::new(0, 50)], false);
    let mut data = storage.get(PATH).unwrap();
    data.truncate(13);
    storage.write(PATH, &data).unwrap();
    let index = PageIndex::open(&storage, String::from(PATH));
    assert_eq!(index.pages(), 1);
    assert!(!index.is_complete());
    assert_eq!(index.entry(&storage, 1), None);
    assert_eq!(
        index.locate(&storage, Locator::new(0, 70)),
        Some((0, false))
    );
}

#[test]
fn a_sentinel_alone_is_a_complete_index_of_no_pages() {
    let (storage, index) = written(&[], true);
    assert!(index.is_complete());
    assert_eq!(index.pages(), 0);
    let reopened = PageIndex::open(&storage, String::from(PATH));
    assert!(reopened.is_complete());
    assert_eq!(reopened.pages(), 0);
    assert_eq!(reopened.locate(&storage, Locator::new(0, 0)), None);
    assert_eq!(reopened.last(&storage), None);
}

#[test]
fn a_missing_file_is_an_empty_index() {
    let storage = MemoryStorage::new();
    let index = PageIndex::open(&storage, String::from("nowhere/pages.bin"));
    assert_eq!(index.pages(), 0);
    assert!(!index.is_complete());
    assert_eq!(index.locate(&storage, Locator::new(3, 3)), None);
}
