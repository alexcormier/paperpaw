//! The annotations file (`.paperpaw/annotations/<hash>.txt`): what is
//! saved comes back (any text and note, tabs and line breaks folded into
//! spaces), and hostile files (wrong field counts, bad numbers, invalid
//! UTF-8, huge lines, duplicates) are read without panics.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

const BOOK: &str = "books/a.epub";

fn annotation(
    kind: Kind,
    start: (u32, u32),
    end: (u32, u32),
    text: &str,
    note: &str,
) -> Annotation {
    Annotation {
        mark: Mark {
            kind,
            start: Locator::new(start.0, start.1),
            end: Locator::new(end.0, end.1),
            noted: !note.is_empty(),
        },
        percent: 50,
        created: 1_700_000_000,
        updated: 1_700_000_001,
        text: text.into(),
        note: note.into(),
    }
}

fn with_file(text: &[u8]) -> MemoryStorage {
    MemoryStorage::new().with(&annotations::file(BOOK), text.to_vec())
}

/// What a saved annotation reads back as: text and note on one line.
fn as_read(mut a: Annotation) -> Annotation {
    a.text = annotations::one_line(&a.text);
    a.note = annotations::one_line(&a.note);
    a.mark.noted = !a.note.is_empty();
    a
}

fn any_annotation() -> impl Strategy<Value = Annotation> {
    (
        any::<bool>(),
        (any::<u32>(), any::<u32>(), any::<u32>(), any::<u32>()),
        0u8..=100,
        (any::<i64>(), any::<i64>()),
        ".{0,40}",
        ".{0,40}",
    )
        .prop_map(
            |(bookmark, (s1, o1, s2, o2), percent, (created, updated), text, note)| Annotation {
                mark: Mark {
                    kind: if bookmark {
                        Kind::Bookmark
                    } else {
                        Kind::Highlight
                    },
                    start: Locator::new(s1, o1),
                    end: Locator::new(s2, o2),
                    noted: false,
                },
                percent,
                created,
                updated,
                text,
                note,
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Any annotations come back sorted by position, with their numbers
    /// exact and their texts as one line.
    #[test]
    fn saved_annotations_come_back(list in proptest::collection::vec(any_annotation(), 0..8)) {
        let storage = MemoryStorage::new();
        annotations::save(&storage, BOOK, &list);
        let mut expected: Vec<Annotation> = list.into_iter().map(as_read).collect();
        expected.sort_by_key(|a| (a.mark.start, a.mark.end));
        let read = annotations::load(&storage, BOOK);
        prop_assert_eq!(read.len(), expected.len());
        // Equal positions may come in either order: compare as sets.
        for a in &expected {
            prop_assert!(read.contains(a), "{a:?} missing from {read:?}");
        }
        let positions: Vec<_> = read.iter().map(|a| (a.mark.start, a.mark.end)).collect();
        let mut sorted = positions.clone();
        sorted.sort();
        prop_assert_eq!(positions, sorted);
    }

    /// Notes with tabs, line breaks and any Unicode survive as one line
    /// and never split the file's fields.
    #[test]
    fn notes_with_tabs_and_breaks_stay_one_field(text in "[^\r]{0,30}", note in "[^\r]{0,30}") {
        let storage = MemoryStorage::new();
        let start = Locator::new(1, 10);
        annotations::add(&storage, BOOK, annotation(Kind::Highlight, (1, 10), (1, 20), &text, ""));
        prop_assert!(annotations::set_note(&storage, BOOK, start, &note, 5));
        let all = annotations::load(&storage, BOOK);
        prop_assert_eq!(all.len(), 1);
        prop_assert_eq!(&all[0].text, &annotations::one_line(&text));
        prop_assert_eq!(&all[0].note, &annotations::one_line(&note));
        prop_assert_eq!(all[0].mark.noted, !annotations::one_line(&note).is_empty());
        prop_assert_eq!(all[0].updated, 5);
        prop_assert!(!all[0].note.contains(['\t', '\n']));
    }

    /// Whatever bytes the file holds, reading it does not panic, and
    /// every annotation read is in range.
    #[test]
    fn any_file_is_read_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let storage = with_file(&bytes);
        for a in annotations::load(&storage, BOOK) {
            prop_assert!(a.percent <= 100);
        }
    }

    /// Lines made of tab-separated numbers and words, of any field count,
    /// are read or skipped, never misread into a panic.
    #[test]
    fn lines_of_any_field_count_are_read_or_skipped(
        fields in proptest::collection::vec(
            prop_oneof!["[bh]", "-?[0-9]{1,20}", "[a-z ]{0,5}"],
            0..14,
        )
    ) {
        let line = fields.join("\t");
        let storage = with_file(line.as_bytes());
        let read = annotations::load(&storage, BOOK);
        prop_assert!(read.len() <= 1);
    }

    /// `snippet` keeps at most 240 bytes (and an ellipsis), on one line,
    /// cut at a character boundary.
    #[test]
    fn snippets_are_short_and_on_one_line(text in ".{0,300}") {
        let snippet = annotations::snippet(&text);
        let body = snippet.strip_suffix('…').unwrap_or(&snippet);
        prop_assert!(body.len() <= 240);
        prop_assert!(!snippet.contains(['\n', '\t', '\r']));
        if text.len() <= 240 {
            prop_assert_eq!(snippet.chars().count(), text.chars().count());
        }
    }
}

#[test]
fn the_two_field_counts_are_told_apart() {
    // 8 fields: written before times were kept; the 7th is the text.
    let old = "h\t1\t2\t1\t9\t40\t1700000000\tnote";
    // 10 fields: the 7th and 8th are the times.
    let new = "b\t3\t0\t3\t0\t60\t1700000000\t1700000100\ttext\tnote";
    let storage = with_file(format!("{old}\n{new}\n").as_bytes());
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 2);
    // A number in the text of an old line stays text.
    assert_eq!(all[0].text, "1700000000");
    assert_eq!((all[0].created, all[0].updated), (0, 0));
    assert_eq!(all[0].note, "note");
    assert_eq!(
        (all[1].created, all[1].updated),
        (1_700_000_000, 1_700_000_100)
    );
    assert_eq!(
        (all[1].text.as_str(), all[1].note.as_str()),
        ("text", "note")
    );
}

#[test]
fn lines_with_too_few_fields_or_bad_numbers_are_skipped() {
    let text = [
        "",
        "h",
        "h\t1\t2\t3",
        "h\t1\t2\t3\t4",
        // A kind that is neither.
        "x\t1\t2\t3\t4\t5\ttext\tnote",
        // Negative and too large positions.
        "h\t-1\t2\t3\t4\t5\ttext\tnote",
        "h\t1\t4294967296\t3\t4\t5\ttext\tnote",
        // Times that are no numbers, with 10 fields.
        "h\t1\t2\t3\t4\t5\tsoon\tlater\ttext\tnote",
        // The progress is no number.
        "h\t1\t2\t3\t4\thalf\ttext\tnote",
        // Read: only the progress, no text.
        "b\t1\t2\t3\t4\t5",
    ]
    .join("\n");
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].mark.kind, Kind::Bookmark);
    assert_eq!((all[0].text.as_str(), all[0].note.as_str()), ("", ""));
    assert!(!all[0].mark.noted);
}

#[test]
fn progress_out_of_range_is_clamped() {
    let text = "h\t1\t0\t1\t5\t250\tt\t\nh\t1\t5\t1\t9\t-3\tt\t\n";
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all.iter().map(|a| a.percent).collect::<Vec<_>>(), [100, 0]);
}

#[test]
fn more_than_ten_fields_keep_the_rest_in_the_note() {
    let text = "h\t1\t0\t1\t5\t3\t1\t2\ttext\tnote\twith\textra";
    let all = annotations::load(&with_file(text.as_bytes()), BOOK);
    assert_eq!(all[0].note, "note\twith\textra");
    // Saved again, the note is one field.
    let storage = MemoryStorage::new();
    annotations::save(&storage, BOOK, &all);
    let again = annotations::load(&storage, BOOK);
    assert_eq!(again[0].note, "note with extra");
    assert_eq!(again[0].text, "text");
}

#[test]
fn invalid_utf8_and_windows_line_ends_are_tolerated() {
    let mut bytes = b"h\t1\t0\t1\t5\t3\t1\t2\tbad \xff\xfe text\tnote\r\n".to_vec();
    bytes.extend_from_slice(b"b\t2\t0\t2\t0\t9\t1\t2\tchapter\t\r\n");
    let all = annotations::load(&with_file(&bytes), BOOK);
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].text, "bad \u{fffd}\u{fffd} text");
    // The carriage return is no part of the last field.
    assert_eq!(all[0].note, "note");
    assert_eq!(all[1].note, "");
}

#[test]
fn a_huge_line_is_read_and_a_huge_note_saved_whole() {
    let note = "n".repeat(100_000);
    let storage = MemoryStorage::new();
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Highlight, (0, 0), (0, 5), "t", &note),
    );
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all[0].note.len(), 100_000);
}

#[test]
fn duplicate_annotations_are_kept_and_the_first_gets_the_note() {
    let a = annotation(Kind::Highlight, (1, 10), (1, 20), "same", "");
    let storage = MemoryStorage::new();
    annotations::save(&storage, BOOK, &[a.clone(), a]);
    assert!(annotations::set_note(
        &storage,
        BOOK,
        Locator::new(1, 10),
        "first",
        9
    ));
    let all = annotations::load(&storage, BOOK);
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|a| a.note == "first").count(), 1);
    assert_eq!(
        annotations::note_of(&storage, BOOK, Locator::new(1, 10)),
        "first"
    );
    // Removing by position takes both.
    assert!(annotations::remove(&storage, BOOK, |m| m.start == Locator::new(1, 10)));
    assert!(annotations::load(&storage, BOOK).is_empty());
    assert!(!storage.exists(&annotations::file(BOOK)));
}

#[test]
fn notes_go_on_highlights_only() {
    let storage = MemoryStorage::new();
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Bookmark, (2, 0), (2, 0), "page", ""),
    );
    assert!(!annotations::set_note(
        &storage,
        BOOK,
        Locator::new(2, 0),
        "x",
        1
    ));
    assert!(!annotations::set_note(
        &storage,
        BOOK,
        Locator::new(9, 0),
        "x",
        1
    ));
    assert_eq!(annotations::note_of(&storage, BOOK, Locator::new(2, 0)), "");
    // An empty note takes the mark away.
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Highlight, (3, 0), (3, 9), "h", "n"),
    );
    assert!(annotations::set_note(
        &storage,
        BOOK,
        Locator::new(3, 0),
        "  \t ",
        2
    ));
    let h = annotations::load(&storage, BOOK).pop().unwrap();
    assert!(!h.mark.noted && h.note.is_empty());
}

#[test]
fn nothing_removed_leaves_the_file_as_it_is() {
    let text = b"h\t1\t0\t1\t5\t3\tunusual   spacing\t\n";
    let storage = with_file(text);
    assert!(!annotations::remove(&storage, BOOK, |_| false));
    assert_eq!(storage.get(&annotations::file(BOOK)).unwrap(), text);
}

#[test]
fn marks_overlap_only_where_they_share_characters() {
    let m = annotation(Kind::Highlight, (1, 10), (1, 20), "", "").mark;
    let at = |s, e| m.overlaps(Locator::new(1, s), Locator::new(1, e));
    assert!(at(15, 30) && at(0, 11) && at(12, 13) && at(0, 99));
    // Touching is no overlap: the end is just after the last character.
    assert!(!at(20, 30) && !at(0, 10));
    assert!(!m.overlaps(Locator::new(2, 0), Locator::new(2, 99)));
}

#[test]
fn files_are_per_book() {
    assert_ne!(annotations::file("a.epub"), annotations::file("b.epub"));
    assert!(annotations::file("a.epub").starts_with(".paperpaw/annotations/"));
    let storage = MemoryStorage::new();
    annotations::add(
        &storage,
        "a.epub",
        annotation(Kind::Bookmark, (1, 0), (1, 0), "", ""),
    );
    assert_eq!(annotations::marks(&storage, "a.epub").len(), 1);
    assert!(annotations::marks(&storage, "b.epub").is_empty());
}

#[test]
fn the_change_event_names_the_book() {
    use paperpaw_sdk::builtin::events;
    let paperpaw_sdk::Command::Emit(id, params) = annotations::changed("books/x.epub") else {
        panic!("an event");
    };
    let read = events::ANNOTATIONS_CHANGED.read(&id, &params).unwrap();
    assert_eq!(read.path, "books/x.epub");
}

/// Annotations kept beyond what `load` reads (1 MiB, e.g. a reference
/// book with thousands of noted highlights) are read as none, and the next
/// one added replaces them all.
#[test]
#[ignore = "bug: annotations::add over a file larger than MAX_FILE overwrites every annotation"]
fn an_oversized_file_is_not_overwritten_by_add() {
    let line = format!("h\t1\t0\t1\t5\t3\t1\t2\ttext\t{}\n", "n".repeat(1000));
    let big = line.repeat(1100);
    assert!(big.len() > 1 << 20);
    let storage = with_file(big.as_bytes());
    annotations::add(
        &storage,
        BOOK,
        annotation(Kind::Bookmark, (9, 0), (9, 0), "new", ""),
    );
    let kept = storage.get(&annotations::file(BOOK)).unwrap();
    assert!(
        kept.len() > big.len(),
        "the annotations kept before are gone"
    );
}
