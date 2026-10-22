//! Properties of the merge over random reading on two devices: merging
//! A into B gives what merging B into A gives; merging the same again,
//! or syncing again with nothing new, changes nothing; books are the same
//! by their fingerprint, never by their title; a database from the other
//! side that cannot be read leaves this device's statistics as they were.

#![allow(
    clippy::let_underscore_must_use,
    reason = "tests read what they check; what they leave out does not matter"
)]

extern crate std;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;
use crate::store::PageStat;

const DIR: &str = ".paperpaw/plugins/stats";
const MD5S: [&str; 4] = [
    "0000000000000000000000000000000a",
    "0000000000000000000000000000000b",
    "0000000000000000000000000000000c",
    "0000000000000000000000000000000d",
];

/// A device's statistics: its books (an id and one of [`MD5S`]) and its
/// stays (an index into its books, page, start, duration).
#[derive(Debug, Clone)]
struct Device {
    books: Vec<(u16, usize)>,
    stays: Vec<(usize, u32, u32, u16)>,
}

impl Device {
    fn storage(&self, name: &str) -> MemoryStorage {
        let storage = MemoryStorage::new();
        let books: Vec<Book> = self
            .books
            .iter()
            .map(|&(id, md5)| Book {
                id,
                title: format!("{name} {}", MD5S[md5]),
                authors: String::from("N/A"),
                md5: MD5S[md5].to_string(),
                pages: 50,
                language: String::from("N/A"),
                series: String::from("N/A"),
                last_open: 10,
                path: String::new(),
                finished: 0,
            })
            .collect();
        let mut books_sorted = books;
        books_sorted.sort_by_key(|b| b.id);
        store::save_books(&storage, &books_sorted).unwrap();
        let mut stays = self.stays.clone();
        stays.sort_by_key(|s| s.2);
        for (book, page, start, duration) in stays {
            store::append(
                &storage,
                PageStat {
                    book: self.books[book].0,
                    page,
                    start,
                    duration,
                    pages: 50,
                },
            )
            .unwrap();
        }
        storage
    }
}

fn device() -> impl Strategy<Value = Device> {
    // Distinct books, each with an id of its own on this device.
    (
        proptest::sample::subsequence(alloc::vec![0usize, 1, 2, 3], 1..=4),
        1u16..20,
    )
        .prop_flat_map(|(md5s, first_id)| {
            let count = md5s.len();
            let books: Vec<(u16, usize)> = md5s
                .into_iter()
                .enumerate()
                .map(|(n, md5)| (first_id + n as u16 * 3, md5))
                .collect();
            // Few pages and starts, so the devices share stays.
            let stay = (
                0..count,
                1u32..6,
                (0u32..12).prop_map(|s| 1000 + s * 30),
                1u16..400,
            );
            (Just(books), proptest::collection::vec(stay, 0..24))
        })
        .prop_map(|(books, stays)| Device { books, stays })
}

/// Every stay as (fingerprint, page, start) with its duration; for stays
/// of the same key the longest.
fn totals(storage: &dyn Storage) -> BTreeMap<(String, u32, u32), u16> {
    let books = store::books(storage);
    let mut out = BTreeMap::new();
    let _ = store::scan(storage, |s| {
        let md5 = books
            .iter()
            .find(|b| b.id == s.book)
            .map_or_else(|| format!("unknown {}", s.book), |b| b.md5.clone());
        let duration = out.entry((md5, s.page, s.start)).or_insert(0);
        *duration = (*duration).max(s.duration);
    });
    out
}

fn fingerprints(storage: &dyn Storage) -> Vec<String> {
    let mut md5s: Vec<String> = store::books(storage).into_iter().map(|b| b.md5).collect();
    md5s.sort();
    md5s
}

/// The statistics of `storage` as a KOReader database, as a sync would
/// upload them.
fn export(storage: &MemoryStorage) -> Vec<u8> {
    merge(storage, None, None, Some("export.db"), DIR).unwrap();
    let data = storage.get("export.db").unwrap();
    storage.remove("export.db").unwrap();
    data
}

fn files(storage: &MemoryStorage) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    (
        storage.get(&format!("{DIR}/pages.bin")),
        storage.get(&format!("{DIR}/books.ini")),
    )
}

/// The working files of a merge are gone after it.
fn no_working_files_left(storage: &MemoryStorage) -> bool {
    let mut left = Vec::new();
    let _ = storage.list(DIR, &mut |entry| {
        if entry.name.starts_with("merge-") {
            left.push(entry.name.to_string());
        }
    });
    left.is_empty()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    #[test]
    fn merging_either_way_gives_the_same(a in device(), b in device()) {
        let (a, b) = (a.storage("A"), b.storage("B"));
        let (a_file, b_file) = (export(&a), export(&b));
        // What both have, the longer of a stay on both.
        let mut expected = totals(&a);
        for (key, duration) in totals(&b) {
            let kept = expected.entry(key).or_insert(0);
            *kept = (*kept).max(duration);
        }
        let mut expected_books = fingerprints(&a);
        expected_books.extend(fingerprints(&b));
        expected_books.sort();
        expected_books.dedup();

        a.write("income.db", &b_file).unwrap();
        b.write("income.db", &a_file).unwrap();
        merge(&a, Some("income.db"), None, None, DIR).unwrap();
        merge(&b, Some("income.db"), None, None, DIR).unwrap();
        // B has A's file now, A B's: the same either way.
        let (b_into_a, a_into_b) = (totals(&a), totals(&b));
        prop_assert_eq!(&b_into_a, &a_into_b);
        prop_assert_eq!(&b_into_a, &expected);
        prop_assert_eq!(fingerprints(&a), expected_books.clone());
        prop_assert_eq!(fingerprints(&b), expected_books);
        let seconds = |t: &BTreeMap<_, u16>| t.values().map(|&d| u64::from(d)).sum::<u64>();
        prop_assert_eq!(seconds(&b_into_a), seconds(&a_into_b));
        prop_assert!(no_working_files_left(&a));
    }

    #[test]
    fn merging_the_same_again_changes_nothing(a in device(), b in device()) {
        let (a, b) = (a.storage("A"), b.storage("B"));
        b.write("income.db", &export(&a)).unwrap();
        let first = merge(&b, Some("income.db"), None, None, DIR).unwrap();
        let after_first = files(&b);
        let second = merge(&b, Some("income.db"), None, None, DIR).unwrap();
        prop_assert_eq!(first, second);
        prop_assert_eq!(files(&b), after_first);
    }

    #[test]
    fn syncing_again_with_nothing_new_changes_nothing(a in device(), b in device()) {
        let (a, b) = (a.storage("A"), b.storage("B"));
        // B syncs with A's file: merged, uploaded and kept as the last
        // upload; then again, the folder holding what B uploaded.
        b.write("income.db", &export(&a)).unwrap();
        merge(&b, Some("income.db"), None, Some("upload.db"), DIR).unwrap();
        let uploaded = b.get("upload.db").unwrap();
        b.write("cached.db", &uploaded).unwrap();
        b.write("income.db", &uploaded).unwrap();
        let before = (files(&b), totals(&b));
        merge(&b, Some("income.db"), Some("cached.db"), Some("upload.db"), DIR).unwrap();
        prop_assert_eq!((files(&b), totals(&b)), before);
        // And A, merging what B uploaded, ends with the same.
        a.write("income.db", &uploaded).unwrap();
        merge(&a, Some("income.db"), None, None, DIR).unwrap();
        prop_assert_eq!(totals(&a), totals(&b));
    }

    #[test]
    fn any_bytes_from_the_other_side_leave_the_statistics_as_they_were(
        a in device(),
        income in proptest::collection::vec(any::<u8>(), 0..600),
    ) {
        let a = a.storage("A");
        let before = (totals(&a), fingerprints(&a));
        a.write("income.db", &income).unwrap();
        let _ = merge(&a, Some("income.db"), None, None, DIR);
        prop_assert_eq!((totals(&a), fingerprints(&a)), before);
    }
}

#[test]
fn books_are_the_same_by_their_fingerprint_not_their_title() {
    let here = Device {
        books: alloc::vec![(1, 0), (2, 1)],
        stays: alloc::vec![(0, 1, 1000, 30), (1, 1, 1100, 40)],
    }
    .storage("Here");
    // There: the same title for another file, another title for the same.
    let there = MemoryStorage::new();
    let book = |id: u16, title: &str, md5: &str| Book {
        id,
        title: title.to_string(),
        authors: String::from("Someone"),
        md5: md5.to_string(),
        pages: 50,
        ..Book::default()
    };
    store::save_books(
        &there,
        &[
            book(
                7,
                "Here 0000000000000000000000000000000a",
                "0000000000000000000000000000000e",
            ),
            book(8, "Another title", "0000000000000000000000000000000b"),
        ],
    )
    .unwrap();
    let stay = |book: u16, start: u32| PageStat {
        book,
        page: 2,
        start,
        duration: 20,
        pages: 50,
    };
    store::append(&there, stay(7, 2000)).unwrap();
    store::append(&there, stay(8, 2100)).unwrap();
    here.write("income.db", &export(&there)).unwrap();
    let outcome = merge(&here, Some("income.db"), None, None, DIR).unwrap();
    assert_eq!(outcome, Outcome { books: 3, stays: 4 });
    let books = store::books(&here);
    let titles: Vec<(&str, &str)> = books
        .iter()
        .map(|b| (b.md5.as_str(), b.title.as_str()))
        .collect();
    assert_eq!(
        titles,
        [
            (
                "0000000000000000000000000000000a",
                "Here 0000000000000000000000000000000a"
            ),
            // Named as there, so KOReader finds it by its key.
            ("0000000000000000000000000000000b", "Another title"),
            // Its title the same as another's: still a book of its own.
            (
                "0000000000000000000000000000000e",
                "Here 0000000000000000000000000000000a"
            ),
        ]
    );
    let t = totals(&here);
    assert_eq!(
        t.get(&(String::from("0000000000000000000000000000000b"), 2, 2100)),
        Some(&20)
    );
    assert_eq!(
        t.get(&(String::from("0000000000000000000000000000000e"), 2, 2000)),
        Some(&20)
    );
}

#[test]
fn a_database_read_in_part_loses_or_changes_nothing_here() {
    let here = Device {
        books: alloc::vec![(1, 0), (2, 1)],
        stays: alloc::vec![(0, 1, 1000, 30), (1, 1, 1100, 40), (0, 2, 1200, 50)],
    }
    .storage("Here");
    let there = Device {
        books: alloc::vec![(1, 2)],
        stays: (0..40).map(|n| (0, n % 5 + 1, 1000 + n * 30, 60)).collect(),
    }
    .storage("There");
    let good = export(&there);
    let before = (files(&here), totals(&here));
    let mut broken: Vec<(&str, Vec<u8>)> = alloc::vec![
        ("empty", Vec::new()),
        ("not a database", b"<html>404 Not Found</html>".to_vec()),
        ("the header only", good[..100].to_vec()),
        ("the first page only", good[..good.len().min(4096)].to_vec()),
    ];
    for cut in [good.len() / 2, good.len() - 1] {
        broken.push(("cut off", good[..cut].to_vec()));
    }
    let mut zeroed = good.clone();
    zeroed[1000..3000].fill(0);
    broken.push(("pages zeroed", zeroed));
    let theirs = totals(&there);
    for (what, data) in broken {
        here.write("income.db", &data).unwrap();
        let _ = merge(&here, Some("income.db"), None, None, DIR);
        let after = totals(&here);
        // Nothing of this device lost or changed; of the other side at
        // most what could be read of it, as it was there.
        for (key, duration) in &before.1 {
            assert_eq!(after.get(key), Some(duration), "{what}: {key:?}");
        }
        for (key, duration) in &after {
            if !before.1.contains_key(key) {
                assert_eq!(theirs.get(key), Some(duration), "{what}: {key:?}");
            }
        }
        assert!(no_working_files_left(&here), "{what}");
        // As it was, for the next.
        here.write(&format!("{DIR}/pages.bin"), before.0.0.as_deref().unwrap())
            .unwrap();
        here.write(&format!("{DIR}/books.ini"), before.0.1.as_deref().unwrap())
            .unwrap();
    }
}

/// After a sync, the folder holds what this device uploaded; downloaded
/// cut off anywhere, the stays it lacks must not count as deleted there.
#[test]
fn a_download_cut_off_deletes_nothing_here() {
    let here = Device {
        books: alloc::vec![(1, 0), (2, 1), (3, 2)],
        stays: (0..120)
            .map(|n| ((n % 3) as usize, n % 7 + 1, 1000 + n * 30, 45))
            .collect(),
    }
    .storage("Here");
    merge(&here, None, None, Some("upload.db"), DIR).unwrap();
    let uploaded = here.get("upload.db").unwrap();
    here.write("cached.db", &uploaded).unwrap();
    let before = (files(&here), totals(&here), fingerprints(&here));
    let mut cut = 0;
    while cut < uploaded.len() {
        here.write("income.db", &uploaded[..cut]).unwrap();
        let _ = merge(&here, Some("income.db"), Some("cached.db"), None, DIR);
        assert_eq!(totals(&here), before.1, "cut after {cut} bytes");
        assert_eq!(fingerprints(&here), before.2, "cut after {cut} bytes");
        here.write(&format!("{DIR}/pages.bin"), before.0.0.as_deref().unwrap())
            .unwrap();
        here.write(&format!("{DIR}/books.ini"), before.0.1.as_deref().unwrap())
            .unwrap();
        cut += 512;
    }
}

#[test]
fn a_book_deleted_on_both_sides_stays_deleted() {
    let here = Device {
        books: alloc::vec![(1, 0)],
        stays: alloc::vec![(0, 1, 1000, 30)],
    }
    .storage("Here");
    // The last upload had a second book, deleted since on both sides.
    let before = Device {
        books: alloc::vec![(1, 0), (2, 1)],
        stays: alloc::vec![(0, 1, 1000, 30), (1, 1, 1100, 40)],
    }
    .storage("Before");
    let there = Device {
        books: alloc::vec![(5, 0)],
        stays: alloc::vec![(0, 1, 1000, 30), (0, 2, 1300, 20)],
    }
    .storage("There");
    here.write("cached.db", &export(&before)).unwrap();
    here.write("income.db", &export(&there)).unwrap();
    let outcome = merge(&here, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
    assert_eq!(outcome, Outcome { books: 1, stays: 2 });
    assert_eq!(fingerprints(&here), ["0000000000000000000000000000000a"]);
}
