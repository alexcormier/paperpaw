//! The bookmarks and highlights of books, shared by plugins: the reader
//! makes and shows them, others export or sync them (e.g. with KOReader).
//! Whoever changes those of a book emits
//! [`ANNOTATIONS_CHANGED`](events::ANNOTATIONS_CHANGED)
//! (see [`changed`]), so readers show them anew and syncs pass them on.
//!
//! A book's annotations are kept in `.paperpaw/annotations/<hash>.txt`
//! (the hash of the book's path), one per line, sorted by position: kind
//! (`b` or `h`), start and end (section and offset, so they outlast font
//! sizes and layouts), the reading progress, when it was made and last
//! changed (seconds since 1970, 0 if not known), a snippet of the text
//! and the note on it, separated by tabs.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use crate::builtin::events;
use crate::document::Locator;
use crate::{Command, Storage, StorageError, files};

/// Longest snippet kept with an annotation, in bytes, with the `…` that
/// ends a cut one.
const MAX_TEXT: usize = 240;
/// Longest note kept, in bytes: a line of the file stays within what
/// [`files::for_each_line`] reads whole.
const MAX_NOTE: usize = 3 * 1024;
/// Longest annotations file changed.
const MAX_FILE: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    Bookmark,
    Highlight,
}

/// Where an annotation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub kind: Kind,
    /// For a highlight, its first character; for a bookmark, where the
    /// page begins.
    pub start: Locator,
    /// Just after the highlight's last character.
    pub end: Locator,
    /// It has a note.
    pub noted: bool,
}

impl Mark {
    /// Whether the mark covers part of `start..end`.
    pub fn overlaps(&self, start: Locator, end: Locator) -> bool {
        self.start < end && start < self.end
    }
}

/// An annotation with what is shown of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub mark: Mark,
    /// Reading progress at the start, in percent.
    pub percent: u8,
    /// When it was made and last changed, in seconds since 1970 (UTC); 0
    /// if not known.
    pub created: i64,
    pub updated: i64,
    /// A snippet of the text (see [`snippet`]).
    pub text: String,
    /// What the reader noted on it, if anything (one line).
    pub note: String,
}

/// The file with the annotations of the book at `book`.
pub fn file(book: &str) -> String {
    format!(
        "{}/annotations/{}.txt",
        crate::data::DIR,
        crate::library::key(book)
    )
}

/// Every annotation of the book at `book`, by position, for showing them;
/// none where they cannot be read. Read them with [`load_for_update`] to
/// change them.
pub fn load(storage: &dyn Storage, book: &str) -> Vec<Annotation> {
    each(storage, book, parse).unwrap_or_default()
}

/// Every annotation of the book at `book`, by position, to change them
/// and [`save`] them: an error where the file is there but cannot be read
/// (or is larger than a megabyte), so it is not written over.
pub fn load_for_update(storage: &dyn Storage, book: &str) -> Result<Vec<Annotation>, StorageError> {
    match storage.size(&file(book)) {
        Err(StorageError::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e),
        Ok(size) if size > MAX_FILE => return Err(StorageError::TooLarge),
        Ok(_) => {}
    }
    each(storage, book, parse)
}

/// Only where the annotations are, for drawing them: read line by line,
/// without their texts.
pub fn marks(storage: &dyn Storage, book: &str) -> Vec<Mark> {
    each(storage, book, |line| fields(line).map(|(mark, _)| mark)).unwrap_or_default()
}

/// What `parse` makes of each line of the book's file, as it streams; a
/// missing file has none. A file a newer PaperPaw marked with a later
/// version ([`data::Format`](crate::data::Format)) is
/// [`StorageError::InvalidData`]: not read, not written over.
fn each<T>(
    storage: &dyn Storage,
    book: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, StorageError> {
    let mut all = Vec::new();
    let mut first = true;
    let mut newer = false;
    let read = files::for_each_line(storage, &file(book), &mut |line| {
        if core::mem::take(&mut first) && line.starts_with(crate::data::MARK) {
            newer = crate::data::Format::version_of(line) != 1;
            return if newer {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            };
        }
        all.extend(parse(line));
        ControlFlow::Continue(())
    });
    match read {
        _ if newer => Err(StorageError::InvalidData),
        Ok(()) | Err(StorageError::NotFound) => Ok(all),
        Err(e) => Err(e),
    }
}

/// The mark a line describes, and the line's fields.
fn fields(line: &str) -> Option<(Mark, Vec<&str>)> {
    let fields: Vec<&str> = line.splitn(10, '\t').collect();
    let kind = match *fields.first()? {
        "b" => Kind::Bookmark,
        "h" => Kind::Highlight,
        _ => return None,
    };
    let number = |i: usize| fields.get(i)?.parse::<u32>().ok();
    let locator = |i: usize| Some(Locator::new(number(i)?, number(i + 1)?));
    let (start, end) = (locator(1)?, locator(3)?);
    let noted = fields
        .get(text_field(&fields) + 1)
        .is_some_and(|note| !note.is_empty());
    let mark = Mark {
        kind,
        start,
        end,
        noted,
    };
    Some((mark, fields))
}

/// Where the text is among a line's fields: before times were kept, it
/// followed the progress.
fn text_field(fields: &[&str]) -> usize {
    if fields.len() == 10 { 8 } else { 6 }
}

fn parse(line: &str) -> Option<Annotation> {
    let (mark, fields) = fields(line)?;
    let number = |i: usize| fields.get(i)?.parse::<i64>().ok();
    let percent = number(5)?.clamp(0, 100) as u8;
    let rest = text_field(&fields);
    let (created, updated) = if rest == 8 {
        (number(6)?, number(7)?)
    } else {
        (0, 0)
    };
    let field = |i: usize| fields.get(i).copied().unwrap_or_default().to_string();
    Some(Annotation {
        mark,
        percent,
        created,
        updated,
        text: field(rest),
        note: field(rest + 1),
    })
}

/// Keeps `annotations` as those of the book at `book`, sorted by
/// position (a note cut at 3 KB); `false` where they could not be
/// written.
pub fn save(storage: &dyn Storage, book: &str, annotations: &[Annotation]) -> bool {
    let mut all: Vec<&Annotation> = annotations.iter().collect();
    all.sort_by_key(|a| (a.mark.start, a.mark.end));
    let mut out = String::new();
    for a in all {
        let kind = match a.mark.kind {
            Kind::Bookmark => 'b',
            Kind::Highlight => 'h',
        };
        out.push_str(&format!(
            "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            a.mark.start.section,
            a.mark.start.offset,
            a.mark.end.section,
            a.mark.end.offset,
            a.percent,
            a.created,
            a.updated,
            one_line(&a.text),
            cut(one_line(&a.note), MAX_NOTE)
        ));
    }
    let path = file(book);
    if out.is_empty() {
        storage.remove(&path).is_ok()
    } else {
        storage.write(&path, out.as_bytes()).is_ok()
    }
}

/// `text` on one line, at most 240 bytes (with the `…` that ends a cut
/// one).
pub fn snippet(text: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    cut(out, MAX_TEXT)
}

/// `text` in at most `max` bytes, ending in `…` where it is cut.
fn cut(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max - '…'.len_utf8();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

/// `text` on one line: tabs and line breaks become spaces.
pub fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Puts `note` on the highlight that starts at `start` (an empty one
/// removes it), changed at `now` (seconds since 1970, 0 if not known);
/// `false` if there is no such highlight, or the annotations could not be
/// read or written.
pub fn set_note(storage: &dyn Storage, book: &str, start: Locator, note: &str, now: i64) -> bool {
    let Ok(mut all) = load_for_update(storage, book) else {
        return false;
    };
    let Some(a) = all
        .iter_mut()
        .find(|a| a.mark.kind == Kind::Highlight && a.mark.start == start)
    else {
        return false;
    };
    a.note = one_line(note);
    a.mark.noted = !a.note.is_empty();
    a.updated = now;
    save(storage, book, &all)
}

/// The note on the highlight that starts at `start`.
pub fn note_of(storage: &dyn Storage, book: &str, start: Locator) -> String {
    load(storage, book)
        .into_iter()
        .find(|a| a.mark.kind == Kind::Highlight && a.mark.start == start)
        .map(|a| a.note)
        .unwrap_or_default()
}

/// Adds an annotation; `false` where the annotations could not be read
/// or written (those there are kept as they were).
pub fn add(storage: &dyn Storage, book: &str, annotation: Annotation) -> bool {
    let Ok(mut all) = load_for_update(storage, book) else {
        return false;
    };
    all.push(annotation);
    save(storage, book, &all)
}

/// Removes the annotations `remove` picks; `true` if there were any and
/// they are gone.
pub fn remove(storage: &dyn Storage, book: &str, remove: impl Fn(&Mark) -> bool) -> bool {
    let Ok(mut all) = load_for_update(storage, book) else {
        return false;
    };
    let before = all.len();
    all.retain(|a| !remove(&a.mark));
    all.len() != before && save(storage, book, &all)
}

/// The event telling that the annotations of the book at `book` changed.
pub fn changed(book: &str) -> Command {
    events::ANNOTATIONS_CHANGED.emit(events::AnnotationsChanged {
        path: book.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::memory_storage::Unreadable;

    fn highlight(start: u32, end: u32, text: &str) -> Annotation {
        Annotation {
            mark: Mark {
                kind: Kind::Highlight,
                start: Locator::new(1, start),
                end: Locator::new(1, end),
                noted: false,
            },
            percent: 3,
            created: 1_700_000_000,
            updated: 0,
            text: String::from(text),
            note: String::new(),
        }
    }

    #[test]
    fn kept_by_position_with_notes_and_times() {
        let storage = MemoryStorage::new();
        let book = "books/a.epub";
        add(&storage, book, highlight(50, 60, "later"));
        add(&storage, book, highlight(10, 20, "first\tline"));
        assert!(set_note(
            &storage,
            book,
            Locator::new(1, 50),
            "a\nnote",
            1_700_000_100
        ));
        let all = load(&storage, book);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "first line");
        assert_eq!(all[1].note, "a note");
        assert!(all[1].mark.noted);
        assert_eq!(
            (all[1].created, all[1].updated),
            (1_700_000_000, 1_700_000_100)
        );
        assert_eq!(note_of(&storage, book, Locator::new(1, 50)), "a note");
        let marks = marks(&storage, book);
        assert_eq!(marks, [all[0].mark, all[1].mark]);
        // Gone when the last is removed.
        assert!(remove(&storage, book, |_| true));
        assert!(!storage.exists(&file(book)));
    }

    #[test]
    fn unreadable_annotations_are_not_written_over() {
        let book = "books/a.epub";
        let kept = MemoryStorage::new();
        assert!(add(&kept, book, highlight(10, 20, "kept")));
        let before = kept.get(&file(book)).unwrap();
        let storage = Unreadable(kept);
        assert_eq!(load_for_update(&storage, book), Err(StorageError::Io));
        assert!(!add(&storage, book, highlight(30, 40, "new")));
        assert!(!remove(&storage, book, |_| true));
        assert!(!set_note(&storage, book, Locator::new(1, 10), "x", 0));
        assert_eq!(storage.0.get(&file(book)), Some(before));
        // Written by a newer PaperPaw: left as it is.
        let newer = MemoryStorage::new().with(&file(book), "# format 2\nh\t1\t2\t1\t3\t0\tx\t");
        assert!(!add(&newer, book, highlight(1, 2, "x")));
        assert!(load(&newer, book).is_empty());
        assert_eq!(
            load_for_update(&newer, book),
            Err(StorageError::InvalidData)
        );
        // Too large to be changed whole.
        let big = MemoryStorage::new().with(&file(book), alloc::vec![b'\n'; (1 << 20) + 1]);
        assert!(!add(&big, book, highlight(1, 2, "x")));
        assert_eq!(big.size(&file(book)), Ok((1 << 20) + 1));
    }

    #[test]
    fn texts_and_notes_are_cut() {
        let long = "\u{e4}".repeat(200);
        let text = snippet(&long);
        assert!(
            text.len() <= MAX_TEXT && text.ends_with('…'),
            "{}",
            text.len()
        );
        let storage = MemoryStorage::new();
        let mut a = highlight(1, 2, &text);
        a.note = "n".repeat(10_000);
        assert!(add(&storage, "a.epub", a));
        let read = load(&storage, "a.epub");
        assert_eq!(read[0].text, text);
        assert!(read[0].note.len() <= MAX_NOTE && read[0].note.ends_with('…'));
        assert!(storage.size(&file("a.epub")).unwrap() < files::MAX_LINE as u64);
    }

    #[test]
    fn lines_without_times_are_read() {
        let a = parse("h\t1\t10\t1\t20\t5\tsome text\tits note").unwrap();
        assert_eq!((a.created, a.updated), (0, 0));
        assert_eq!(
            (a.text.as_str(), a.note.as_str()),
            ("some text", "its note")
        );
        assert_eq!(a.mark.end, Locator::new(1, 20));
    }
}
