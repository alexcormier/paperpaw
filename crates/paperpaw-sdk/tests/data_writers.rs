//! Every SDK data file of `docs/data-files.md` has its version 1 in
//! `testdata` (favourites too), and what the current writers make of it
//! the readers read as before: version 1 read, written again and read
//! once more gives the same.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

mod common;

use paperpaw_sdk::builtin::events::Reading;
use paperpaw_sdk::firmware::{self, TRUSTED_KEYS};
use paperpaw_sdk::secrets::{FILE, Secrets};
use paperpaw_sdk::{MemoryStorage, annotations, library};

const BOOK_RECORD: &str = ".paperpaw/library/books/0c6afbf2778c9bbf.ini";

#[test]
fn favourites_of_version_1_are_read() {
    let storage = MemoryStorage::new().with(
        ".paperpaw/library/favourites.txt",
        include_str!("../testdata/favourites.txt.v1"),
    );
    assert_eq!(
        library::favourites(&storage),
        [
            "books/Ångström & Co.epub",
            "books/b.epub",
            "notes/ with spaces .txt"
        ]
    );
    // Written again: the same paths, a line each.
    library::set_favourite(&storage, "books/b.epub", true);
    let text = storage.get(".paperpaw/library/favourites.txt").unwrap();
    assert_eq!(
        String::from_utf8(text).unwrap(),
        "books/b.epub\nbooks/Ångström & Co.epub\nnotes/ with spaces .txt"
    );
}

#[test]
fn opened_books_of_version_1_are_written_as_read() {
    let mut h = common::Harness::new();
    h.storage = MemoryStorage::new().with(
        ".paperpaw/library/opened.txt",
        include_str!("../testdata/opened.txt.v1"),
    );
    let env = h.env();
    let reading = Reading {
        path: "books/c.epub".into(),
        title: String::new(),
        progress: 0.0,
        section: 0,
        offset: 0,
        page: None,
        pages: None,
        chapter: String::new(),
        chapter_left: None,
    };
    library::record(&env, &reading, true);
    assert_eq!(
        library::opened_books(&h.storage),
        ["books/c.epub", "books/b.epub", "books/a b.txt"]
    );
}

#[test]
fn a_book_record_of_version_1_is_written_and_read_again() {
    let storage = MemoryStorage::new()
        .with("books/a.epub", b"12345".to_vec())
        .with(BOOK_RECORD, include_str!("../testdata/library-book.ini.v1"));
    let first = library::book(&storage, "books/a.epub").unwrap();
    library::save(&storage, "books/a.epub", &first);
    let again = library::book(&storage, "books/a.epub").unwrap();
    assert_eq!(again, first);
    // The quoted series keeps its spaces, the plugins' fields their
    // section: as version 1 wrote them.
    let text = String::from_utf8(storage.get(BOOK_RECORD).unwrap()).unwrap();
    assert!(text.contains("series = \"  Classics \""), "{text}");
    assert!(text.contains("[field]\ncalibre.rating = 4"), "{text}");
    for key in [
        "size = 5",
        "tag.1 = Novel",
        "tag.2 = Russia",
        "cover-width = 120",
        "progress = 42",
    ] {
        assert!(text.contains(key), "{key} in {text}");
    }
}

#[test]
fn annotations_of_version_1_are_written_and_read_again() {
    let path = annotations::file("books/a.epub");
    let storage = MemoryStorage::new().with(&path, include_str!("../testdata/annotations.txt.v1"));
    let first = annotations::load(&storage, "books/a.epub");
    assert_eq!(first.len(), 2);
    annotations::save(&storage, "books/a.epub", &first);
    assert_eq!(annotations::load(&storage, "books/a.epub"), first);
    // Lines without times are written with them now: 10 fields.
    let text = String::from_utf8(storage.get(&path).unwrap()).unwrap();
    assert!(
        text.lines().all(|line| line.split('\t').count() == 10),
        "{text}"
    );
}

#[test]
fn secrets_of_version_1_are_written_as_they_were() {
    let v1 = include_str!("../testdata/secrets.txt.v1");
    assert_eq!(Secrets::decode(v1).encode(), v1);
    assert_eq!(FILE, ".paperpaw/secrets.txt");
}

#[test]
fn trusted_keys_of_version_1_are_kept_when_one_is_added() {
    let storage = MemoryStorage::new().with(
        TRUSTED_KEYS,
        include_str!("../testdata/firmware-keys.txt.v1"),
    );
    firmware::trust(&storage, &[0x34; 32]).unwrap();
    // One already trusted is not added again.
    firmware::trust(&storage, &[0x12; 32]).unwrap();
    assert_eq!(
        firmware::trusted_keys(&storage),
        [[0x12; 32], [0xAB; 32], [0x34; 32]]
    );
}

#[test]
fn trusted_keys_skip_lines_that_are_no_keys() {
    let key = "12".repeat(32);
    let text = format!(
        "\n{key}\r\nnot a key\n{}\n{}\n  {key}  \n\u{fffd}\n",
        "1".repeat(64 + 2),
        "zz".repeat(32)
    );
    let storage = MemoryStorage::new().with(TRUSTED_KEYS, text);
    let keys = firmware::trusted_keys(&storage);
    assert!(keys.contains(&[0x12; 32]));
    assert!(keys.iter().all(|k| *k == [0x12; 32]));
    // Invalid UTF-8 anywhere: no key is read, none trusted by mistake.
    let storage = MemoryStorage::new().with(TRUSTED_KEYS, [key.as_bytes(), b"\n\xff\n"].concat());
    assert!(firmware::trusted_keys(&storage).is_empty());
    // None at all.
    assert!(firmware::trusted_keys(&MemoryStorage::new()).is_empty());
}
