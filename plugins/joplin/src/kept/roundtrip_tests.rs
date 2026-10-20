//! The plugin's data files round trip whatever is kept in them: books
//! sent, books waiting (against a model of the queue) and the notebook;
//! long lines, Unicode, files written with Windows line ends, and what a
//! write cut short leaves behind.

extern crate std;

use super::*;
use alloc::string::ToString;
use paperpaw_sdk::MemoryStorage;
use proptest::prelude::*;

/// A path as notes files have them: no tab or line break (no file
/// system allows those in names), not empty, and not starting with a
/// byte order mark (one at the start of a file is left out when it is
/// read, see `paperpaw_sdk::files::for_each_line`).
fn path() -> impl Strategy<Value = String> {
    r"[^\t\n\r\x00]{1,40}".prop_filter("not blank, no mark", |p| {
        !p.trim().is_empty() && !p.starts_with('\u{feff}')
    })
}

fn id() -> impl Strategy<Value = String> {
    "[0-9a-f]{32}"
}

#[derive(Debug, Clone)]
enum Op {
    Add(usize),
    Remove(usize),
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// The queue is what a list with no repeats, the newest at the end,
    /// would be after the same steps.
    #[test]
    fn the_waiting_queue_follows_its_model(
        paths in proptest::collection::vec(path(), 1..6),
        ops in proptest::collection::vec(
            prop_oneof![(0usize..6).prop_map(Op::Add), (0usize..6).prop_map(Op::Remove)],
            0..30,
        ),
    ) {
        let storage = MemoryStorage::new();
        let mut model: Vec<String> = Vec::new();
        for op in ops {
            match op {
                Op::Add(i) => {
                    let p = &paths[i % paths.len()];
                    add_waiting(&storage, p).unwrap();
                    model.retain(|m| m != p);
                    model.push(p.clone());
                }
                Op::Remove(i) => {
                    let p = &paths[i % paths.len()];
                    remove_waiting(&storage, p).unwrap();
                    model.retain(|m| m != p);
                }
            }
            prop_assert_eq!(&waiting(&storage), &model);
        }
        // No copy is left beside the file.
        prop_assert!(storage.get(&file("waiting.txt.new")).is_none());
    }

    /// What is kept of a book sent is read back, whatever else is kept.
    #[test]
    fn books_sent_round_trip(
        books in proptest::collection::vec((path(), id(), id()), 1..8),
    ) {
        let storage = MemoryStorage::new();
        for (p, id, text) in &books {
            keep_sent(&storage, p, &Sent { id: id.clone(), text: text.clone() }).unwrap();
        }
        for (p, _, _) in &books {
            // The last kept for a path wins.
            let (_, id, text) = books.iter().rev().find(|(q, _, _)| q == p).unwrap();
            prop_assert_eq!(
                sent(&storage, p),
                Some(Sent { id: id.clone(), text: text.clone() })
            );
        }
    }

    #[test]
    fn the_notebook_round_trips(
        name in r"[^\t\n\r]{0,40}".prop_filter("no mark", |n| !n.starts_with('\u{feff}')),
        id in id(),
    ) {
        let storage = MemoryStorage::new();
        keep_notebook(&storage, &name, &id).unwrap();
        prop_assert_eq!(notebook(&storage), Some((name, id)));
    }
}

#[test]
fn files_with_windows_line_ends_are_read() {
    let storage = MemoryStorage::new()
        .with(
            &file(SENT),
            "Notes/a.md\t1\tt1\r\nNotes/b.md\t2\tt2\r\n"
                .as_bytes()
                .to_vec(),
        )
        .with(
            &file(WAITING),
            "Notes/a.md\r\n\r\nNotes/b.md\r\n".as_bytes().to_vec(),
        )
        .with(&file(NOTEBOOK), "Books\tabc\r\n".as_bytes().to_vec());
    assert_eq!(
        sent(&storage, "Notes/b.md"),
        Some(Sent {
            id: "2".to_string(),
            text: "t2".to_string()
        })
    );
    assert_eq!(waiting(&storage), ["Notes/a.md", "Notes/b.md"]);
    assert_eq!(
        notebook(&storage),
        Some(("Books".to_string(), "abc".to_string()))
    );
}

#[test]
fn broken_lines_are_no_books_sent() {
    let storage = MemoryStorage::new().with(
        &file(SENT),
        b"Notes/a.md\nNotes/b.md\tonly-id\nNotes/c.md\t1\t2\textra\n\xff\t\t\n".to_vec(),
    );
    assert_eq!(sent(&storage, "Notes/a.md"), None);
    assert_eq!(sent(&storage, "Notes/b.md"), None);
    assert_eq!(sent(&storage, "Notes/c.md"), None);
    // Keeping one anew writes over what was broken of it, and keeps the
    // other lines.
    keep_sent(
        &storage,
        "Notes/b.md",
        &Sent {
            id: "9".to_string(),
            text: "t".to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        sent(&storage, "Notes/b.md"),
        Some(Sent {
            id: "9".to_string(),
            text: "t".to_string()
        })
    );
    let text = String::from_utf8(storage.get(&file(SENT)).unwrap()).unwrap();
    assert!(text.starts_with("Notes/a.md\n"), "{text}");
    assert!(text.contains("Notes/c.md\t1\t2\textra\n"), "{text}");
}

#[test]
fn a_copy_left_by_a_write_cut_short_is_written_over() {
    let storage = MemoryStorage::new()
        .with(&file(WAITING), "Notes/a.md\n".as_bytes().to_vec())
        .with(
            &file("waiting.txt.new"),
            "Notes/stale.md\nNotes/a.md\n".as_bytes().to_vec(),
        );
    add_waiting(&storage, "Notes/b.md").unwrap();
    assert_eq!(waiting(&storage), ["Notes/a.md", "Notes/b.md"]);
    assert!(storage.get(&file("waiting.txt.new")).is_none());
}

#[test]
fn many_books_waiting_are_kept_in_order() {
    let storage = MemoryStorage::new();
    for i in 0..300 {
        add_waiting(&storage, &alloc::format!("Notes/Book {i:03} ✓.md")).unwrap();
    }
    let all = waiting(&storage);
    assert_eq!(all.len(), 300);
    assert_eq!(all[0], "Notes/Book 000 ✓.md");
    assert_eq!(all[299], "Notes/Book 299 ✓.md");
}

/// What the current writer produces is read as version 1 is.
#[test]
fn the_writer_still_writes_version_1() {
    let storage = MemoryStorage::new();
    keep_sent(
        &storage,
        "Notes/Lewis Carroll - Alice.md",
        &Sent {
            id: "1ae79351dd744c2794eebb334b1629a7".to_string(),
            text: "9e107d9d372bb6826bd81d3542a419d6".to_string(),
        },
    )
    .unwrap();
    keep_notebook(&storage, "PaperPaw", "7db793874c624138b3b4520ae5cdf20c").unwrap();
    add_waiting(&storage, "Notes/Jane Austen - Emma.md").unwrap();
    assert_eq!(
        storage.get(&file(NOTEBOOK)).unwrap(),
        include_bytes!("../../testdata/notebook.txt.v1")
    );
    assert_eq!(
        storage.get(&file(WAITING)).unwrap(),
        include_bytes!("../../testdata/waiting.txt.v1")
    );
    let sent_v1 = include_str!("../../testdata/sent.txt.v1");
    let line = String::from_utf8(storage.get(&file(SENT)).unwrap()).unwrap();
    assert!(
        sent_v1.lines().any(|l| alloc::format!("{l}\n") == line),
        "{line}"
    );
}
