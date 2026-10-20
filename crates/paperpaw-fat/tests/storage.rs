//! `FatStorage` at its edges: paths that try to leave the card, long and
//! odd names, names that differ only in case, a full card, deep folders,
//! many files in one folder, moves that cannot be, reads past the end,
//! and what a reset finds after `sync`.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Card, names, read};
use paperpaw_platform::{Storage, StorageError};

const BAD_PATHS: [&str; 14] = [
    "../x",
    "a/../../x",
    "a/../b",
    "..",
    ".",
    "a/./b",
    "/abs",
    "/",
    "a\\b",
    "..\\x",
    "C:/x",
    "a//b",
    "x/",
    "a\0b",
];

#[test]
fn paths_that_leave_the_card_or_are_malformed_are_refused() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("a/b.txt", b"inside").unwrap();
    let mut buf = [0u8; 8];
    for path in BAD_PATHS {
        assert_eq!(
            s.read_at(path, 0, &mut buf),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(s.size(path), Err(StorageError::InvalidPath), "{path:?}");
        assert_eq!(
            s.write(path, b"x"),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(
            s.append(path, b"x"),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(s.remove(path), Err(StorageError::InvalidPath), "{path:?}");
        assert_eq!(
            s.rename("a/b.txt", path),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(
            s.rename(path, "a/c.txt"),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(
            s.remove_dir_all(path),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
        assert_eq!(
            s.list(path, &mut |_| {}),
            Err(StorageError::InvalidPath),
            "{path:?}"
        );
    }
    // The empty path is the root for a listing only: never removed whole.
    assert_eq!(s.remove_dir_all(""), Err(StorageError::InvalidPath));
    assert_eq!(s.remove(""), Err(StorageError::InvalidPath));
    assert_eq!(s.write("", b"x"), Err(StorageError::InvalidPath));
    assert_eq!(read(s, "a/b.txt"), b"inside");
    assert_eq!(names(s, ""), ["a/"]);
    assert_eq!(names(s, "a"), ["b.txt"]);
}

#[test]
fn long_and_unusual_names() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    // Long names: 251 units (with `.tmp` while it is written, 255, the
    // longest FAT keeps).
    let longest = format!("{}.txt", "n".repeat(247));
    s.write(&format!("books/{longest}"), b"long").unwrap();
    assert_eq!(names(s, "books"), [longest]);
    let too_long = format!("{}.txt", "n".repeat(252));
    assert_eq!(
        s.write(&format!("books/{too_long}"), b"x"),
        Err(StorageError::InvalidPath)
    );
    // Letters of other scripts, after an ASCII first letter (see the bug
    // below).
    for name in [
        "Byen Ærøskøbing.epub",
        "Tokyo 東京.txt",
        "greek Ελληνικά.md",
        "a b  c.txt",
        ".hidden",
    ] {
        assert_eq!(
            s.write(&format!("books/{name}"), name.as_bytes()),
            Ok(()),
            "{name}"
        );
        assert_eq!(read(s, &format!("books/{name}")), name.as_bytes(), "{name}");
    }
    assert_eq!(names(s, "books").len(), 6);
    // Outside the BMP (two UTF-16 units), which the FAT library does
    // not write: refused, nothing left.
    assert_eq!(
        s.write("books/shelf 📚.md", b"x"),
        Err(StorageError::InvalidPath)
    );
    // Characters FAT does not allow in names.
    for name in ["a*b", "a?b", "a\"b", "a<b", "a>b", "a|b"] {
        assert_eq!(
            s.write(&format!("books/{name}"), b"x"),
            Err(StorageError::InvalidPath),
            "{name}"
        );
    }
    assert_eq!(names(s, "books").len(), 6, "nothing left behind");
}

/// Books and folders whose names begin with a letter outside ASCII, as
/// many do in German, Danish or Japanese.
#[test]
#[ignore = "bug: fatfs (rev 2aefc2a) dir.rs:1075 slices name[1..] when making a short name: panics on a multi-byte first letter"]
fn names_beginning_with_a_letter_outside_ascii_can_be_created() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    for name in ["Ärger.epub", "Ørsted.txt", "東京.txt", "📚.md"] {
        s.write(&format!("books/{name}"), name.as_bytes()).unwrap();
        assert_eq!(read(s, &format!("books/{name}")), name.as_bytes(), "{name}");
    }
    s.create_dir("Übersetzungen").unwrap();
    assert!(names(s, "").contains(&"Übersetzungen/".to_string()));
}

/// Names of 252 to 255 UTF-16 units are valid on FAT: appended to they
/// work, written whole too.
#[test]
#[ignore = "bug: lib.rs writes through `<name>.tmp`, four units longer: names over 251 units cannot be written"]
fn the_longest_names_can_be_written() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    let longest = format!("{}.txt", "n".repeat(251));
    s.append(&longest, b"appended").unwrap();
    assert_eq!(read(s, &longest), b"appended");
    s.write(&longest, b"written").unwrap();
    assert_eq!(read(s, &longest), b"written");
}

/// FAT does not tell names apart by case: one file, whichever spelling.
#[test]
fn names_differing_in_case_are_one_file() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("Notes/Book.txt", b"first").unwrap();
    assert_eq!(read(s, "notes/BOOK.TXT"), b"first");
    s.write("NOTES/book.TXT", b"second").unwrap();
    assert_eq!(names(s, ""), ["Notes/"]);
    assert_eq!(names(s, "notes").len(), 1);
    assert_eq!(read(s, "Notes/Book.txt"), b"second");
}

/// A file kept open under one spelling, replaced under another: reading
/// the first spelling sees the new content.
#[test]
fn a_file_replaced_under_another_case_is_read_anew() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("book.txt", b"old content").unwrap();
    assert_eq!(read(s, "book.txt"), b"old content");
    s.write("BOOK.TXT", b"new").unwrap();
    assert_eq!(read(s, "book.txt"), b"new");
}

/// A file of the reader's that is named like the temporary file of a
/// write beside it.
#[test]
#[ignore = "bug: lib.rs writes `<name>.tmp` beside the file and so replaces a file of that name"]
fn a_write_keeps_a_file_named_like_its_temporary() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("notes.txt.tmp", b"the reader's own").unwrap();
    s.write("notes.txt", b"notes").unwrap();
    assert_eq!(names(s, ""), ["notes.txt", "notes.txt.tmp"]);
    assert_eq!(read(s, "notes.txt.tmp"), b"the reader's own");
}

#[test]
fn reads_past_the_end_read_nothing() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("a.bin", &[7u8; 3000]).unwrap();
    let mut buf = [0u8; 100];
    assert_eq!(s.read_at("a.bin", 2950, &mut buf), Ok(50));
    assert_eq!(s.read_at("a.bin", 3000, &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", 1 << 31, &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", u64::from(u32::MAX), &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", 0, &mut []), Ok(0));
    assert_eq!(
        s.read_at("missing.bin", 0, &mut buf),
        Err(StorageError::NotFound)
    );
    s.write("empty", b"").unwrap();
    assert_eq!(s.read_at("empty", 0, &mut buf), Ok(0));
    assert_eq!(s.size("empty"), Ok(0));
}

/// Offsets past what FAT files can reach (4 GiB) are past the end too.
#[test]
#[ignore = "bug: lib.rs maps fatfs' InvalidInput for a seek past 4 GiB to InvalidPath instead of reading nothing"]
fn reads_past_4_gib_read_nothing() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("a.bin", &[7u8; 3000]).unwrap();
    let mut buf = [0u8; 100];
    assert_eq!(s.read_at("a.bin", 1 << 40, &mut buf), Ok(0));
    assert_eq!(s.read_at("a.bin", u64::MAX, &mut buf), Ok(0));
}

/// A card that fills up: the write that does not fit fails as `Full`,
/// the old content stays, and the space is there again for what fits.
#[test]
fn a_full_card_keeps_the_old_content() {
    let card = Card::new(256 * 1024);
    let s = card.storage;
    s.write("keep.txt", b"precious").unwrap();
    s.write("big.bin", &[1u8; 100_000]).unwrap();
    assert_eq!(s.write("big.bin", &[2u8; 400_000]), Err(StorageError::Full));
    assert_eq!(read(s, "big.bin"), vec![1u8; 100_000]);
    assert_eq!(read(s, "keep.txt"), b"precious");
    // Room for a write of the same size as before.
    s.write("big.bin", &[3u8; 100_000]).unwrap();
    assert_eq!(read(s, "big.bin"), vec![3u8; 100_000]);
}

/// After a write that did not fit, no temporary file is left taking the
/// card's space.
#[test]
#[ignore = "bug: lib.rs leaves `<name>.tmp` behind when writing it fails (e.g. the card is full)"]
fn a_write_that_does_not_fit_leaves_no_temporary_file() {
    let card = Card::new(256 * 1024);
    let s = card.storage;
    s.write("big.bin", &[1u8; 1000]).unwrap();
    assert_eq!(s.write("big.bin", &[2u8; 400_000]), Err(StorageError::Full));
    assert_eq!(names(s, ""), ["big.bin"]);
}

/// Appending until the card is full: the error comes back and what is
/// in the file is a prefix of what was appended.
#[test]
fn appending_to_a_full_card_fails_cleanly() {
    let card = Card::new(256 * 1024);
    let s = card.storage;
    let piece: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    let mut appended = 0;
    let error = loop {
        match s.append("log.bin", &piece) {
            Ok(()) => appended += 1,
            Err(e) => break e,
        }
        assert!(appended < 100, "a card of 256 KiB fills up");
    };
    assert_eq!(error, StorageError::Full);
    s.sync().unwrap();
    let data = read(s, "log.bin");
    assert!(data.len() >= appended * piece.len(), "{} bytes", data.len());
    let expected: Vec<u8> = piece.iter().copied().cycle().take(data.len()).collect();
    assert_eq!(data, expected);
}

#[test]
fn deep_folders_are_written_read_listed_and_removed() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    let parts: Vec<String> = (0..40).map(|i| format!("level {i}")).collect();
    let deep = parts.join("/");
    s.write(&format!("{deep}/leaf.txt"), b"deep down").unwrap();
    // Also a file at every level, by turns (more folders than are kept).
    for depth in 1..=40 {
        let dir = parts[..depth].join("/");
        s.write(&format!("{dir}/f.txt"), dir.as_bytes()).unwrap();
    }
    for depth in (1..=40).rev() {
        let dir = parts[..depth].join("/");
        assert_eq!(read(s, &format!("{dir}/f.txt")), dir.as_bytes());
    }
    assert_eq!(read(s, &format!("{deep}/leaf.txt")), b"deep down");
    assert_eq!(names(s, &deep), ["f.txt", "leaf.txt"]);
    s.remove_dir_all("level 0").unwrap();
    assert_eq!(names(s, ""), Vec::<String>::new());
    assert_eq!(
        s.size(&format!("{deep}/leaf.txt")),
        Err(StorageError::NotFound)
    );
}

/// More entries than one cluster of a folder holds.
#[test]
fn many_files_in_one_folder() {
    let card = Card::new(2 << 20);
    let s = card.storage;
    let names_in: Vec<String> = (0..300)
        .map(|i| format!("a book with a rather long name {i:03}.epub"))
        .collect();
    for (i, name) in names_in.iter().enumerate() {
        s.write(&format!("library/{name}"), &[i as u8]).unwrap();
    }
    let mut sorted = names_in.clone();
    sorted.sort();
    assert_eq!(names(s, "library"), sorted);
    for (i, name) in names_in.iter().enumerate().step_by(37) {
        assert_eq!(read(s, &format!("library/{name}")), [i as u8]);
    }
    // Half removed, the rest still there.
    for name in names_in.iter().step_by(2) {
        s.remove(&format!("library/{name}")).unwrap();
    }
    assert_eq!(names(s, "library").len(), 150);
}

/// A folder moved into itself would cut it off from the card.
#[test]
fn a_folder_is_not_moved_into_itself() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("books/a.txt", b"a").unwrap();
    let result = s.rename("books", "books/inner");
    let root = names(s, "");
    assert!(result.is_err(), "{result:?}, the root then holds {root:?}");
    assert_eq!(read(s, "books/a.txt"), b"a");
}

#[test]
fn moves_that_cannot_be_leave_everything_in_place() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("file.txt", b"file").unwrap();
    s.write("dir/inside.txt", b"inside").unwrap();
    // Onto a folder.
    assert!(s.rename("file.txt", "dir").is_err());
    assert_eq!(read(s, "file.txt"), b"file");
    assert_eq!(read(s, "dir/inside.txt"), b"inside");
    // Below a file.
    assert!(s.rename("dir/inside.txt", "file.txt/x").is_err());
    assert_eq!(read(s, "dir/inside.txt"), b"inside");
    assert_eq!(read(s, "file.txt"), b"file");
    // Nothing to move.
    assert_eq!(
        s.rename("none.txt", "other.txt"),
        Err(StorageError::NotFound)
    );
    // Onto itself.
    assert_eq!(s.rename("file.txt", "file.txt"), Ok(()));
    assert_eq!(read(s, "file.txt"), b"file");
}

/// Writing below a file: an error, and the file stays.
#[test]
fn a_file_is_no_folder() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    s.write("a.txt", b"a file").unwrap();
    assert!(s.write("a.txt/b.txt", b"x").is_err());
    assert!(s.append("a.txt/b.txt", b"x").is_err());
    assert!(s.create_dir("a.txt").is_err());
    assert_eq!(read(s, "a.txt"), b"a file");
    assert!(!s.exists("a.txt/b.txt"));
}

/// What is written, appended, moved and removed is on the card after a
/// sync: mounted anew from its bytes, everything is as it was.
#[test]
fn after_sync_a_reset_finds_everything() {
    let card = Card::new(1 << 20);
    let s = card.storage;
    let big: Vec<u8> = (0..50_000u32).map(|i| (i * 13 % 256) as u8).collect();
    s.write("books/big.bin", &big).unwrap();
    s.write("books/gone.txt", b"gone").unwrap();
    s.write("notes/old.txt", b"moved").unwrap();
    for i in 0..50u8 {
        s.append("logs/a.log", &[i; 33]).unwrap();
    }
    s.remove("books/gone.txt").unwrap();
    s.rename("notes/old.txt", "notes/new/moved.txt").unwrap();
    s.create_dir("empty/folder").unwrap();
    s.sync().unwrap();
    let after = card.remounted();
    let r = after.storage;
    assert_eq!(read(r, "books/big.bin"), big);
    assert_eq!(names(r, "books"), ["big.bin"]);
    assert_eq!(read(r, "notes/new/moved.txt"), b"moved");
    assert_eq!(names(r, "notes"), ["new/"]);
    let log: Vec<u8> = (0..50u8).flat_map(|i| [i; 33]).collect();
    assert_eq!(read(r, "logs/a.log"), log);
    assert_eq!(names(r, "empty"), ["folder/"]);
}
