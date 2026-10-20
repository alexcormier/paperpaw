//! The catalog: every book under the library folder with what the
//! library knows of it (author, title, series, number, tags), for views
//! by author, series or tag and for searching the library.
//!
//! It is kept in `.paperpaw/library/catalog.txt`, a book a line, and made
//! anew in the background by whoever shows it ([`Walk`]): from the
//! details the library keeps of each book, opening only books not seen
//! before, a few entries at a time. Until a new one is complete, the old
//! one is read. It is read a piece at a time, never held in memory.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::listing::{Entry, Listing};
use super::{Book, DIR};
use crate::{Env, Storage, files};

/// Between a book's tags in the catalog.
const TAG_SEPARATOR: char = '\u{1f}';
/// Books [`search`] finds at most: a page of them fits the smallest
/// device (a record takes some 200 bytes).
pub const MAX_FOUND: usize = 200;
/// Entries looked at, and books opened, in one step of making the catalog.
const STEP_ENTRIES: usize = 12;

fn catalog_path() -> String {
    format!("{DIR}/catalog.txt")
}

fn building_path() -> String {
    format!("{DIR}/catalog.tmp")
}

/// A book in the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub path: String,
    pub author: String,
    pub title: String,
    pub series: String,
    /// Its number in the series.
    pub number: String,
    /// Separated by [`TAG_SEPARATOR`]; see [`Record::tags`].
    tags: String,
}

impl Record {
    /// The record of the book at `path` with the details `book`.
    pub fn of(path: &str, book: &Book) -> Record {
        Record {
            path: path.to_string(),
            author: book.author.clone().unwrap_or_default(),
            title: book.shown_title(path).into_owned(),
            series: book.series.clone().unwrap_or_default(),
            number: book.series_index.clone().unwrap_or_default(),
            tags: book
                .tags
                .iter()
                .map(|t| t.replace(TAG_SEPARATOR, " "))
                .collect::<Vec<_>>()
                .join(&TAG_SEPARATOR.to_string()),
        }
    }

    /// The book's tags.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.split(TAG_SEPARATOR).filter(|t| !t.is_empty())
    }

    fn parse(text: &str) -> Option<Record> {
        let mut fields = text.split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_string();
        let mut next = || fields.next().unwrap_or("").to_string();
        Some(Record {
            path,
            author: next(),
            title: next(),
            series: next(),
            number: next(),
            tags: next(),
        })
    }

    fn text(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            clean(&self.path),
            clean(&self.author),
            clean(&self.title),
            clean(&self.series),
            clean(&self.number),
            clean(&self.tags)
        )
    }

    /// Whether all of the words of `query` (lowercase) are in its title,
    /// author, series, tags or file name.
    pub fn matches(&self, query: &[String]) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.title,
            self.author,
            self.series,
            self.tags.replace(TAG_SEPARATOR, " "),
            files::name(&self.path)
        )
        .to_lowercase();
        query.iter().all(|word| text.contains(word.as_str()))
    }
}

/// Calls `f` with each book of the catalog, reading it in pieces.
pub fn each(storage: &dyn Storage, f: &mut dyn FnMut(Record)) {
    let mut buf = [0u8; 2048];
    let mut rest: Vec<u8> = Vec::new();
    let mut offset = 0u64;
    let path = catalog_path();
    while let Ok(n) = storage.read_at(&path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        offset += n as u64;
        rest.extend_from_slice(&buf[..n]);
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = rest.drain(..=end).collect();
            if let Some(record) = Record::parse(String::from_utf8_lossy(&line).trim_end()) {
                f(record);
            }
        }
    }
    if let Some(record) = Record::parse(String::from_utf8_lossy(&rest).trim_end()) {
        f(record);
    }
}

/// Where a record goes in the order of [`page`]: by title regardless of
/// case, then by path.
fn order_key(record: &Record) -> (String, &str) {
    (record.title.to_lowercase(), &record.path)
}

/// `n` records of the catalog that `keep` keeps, by title regardless of
/// case (then by path), those after `after` (from the first without):
/// one reading of the catalog that holds only `n` records at a time
/// (keyset pagination, as [`Listing`] pages a folder).
pub fn page(
    storage: &dyn Storage,
    keep: &dyn Fn(&Record) -> bool,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let after = after.map(|r| (r.title.to_lowercase(), r.path.clone()));
    // Each with its title in lower case, computed once.
    let mut page: Vec<(String, Record)> = Vec::new();
    each(storage, &mut |record| {
        if n == 0 || !keep(&record) {
            return;
        }
        let (title, path) = order_key(&record);
        if after
            .as_ref()
            .is_some_and(|(t, p)| (title.as_str(), path) <= (t.as_str(), p.as_str()))
        {
            return;
        }
        let at =
            page.partition_point(|(t, r)| (t.as_str(), r.path.as_str()) < (title.as_str(), path));
        if at < n {
            page.insert(at, (title, record));
            page.truncate(n);
        }
    });
    page.into_iter().map(|(_, record)| record).collect()
}

/// How many records of the catalog `keep` keeps.
pub fn count(storage: &dyn Storage, keep: &dyn Fn(&Record) -> bool) -> usize {
    let mut count = 0;
    each(storage, &mut |record| count += usize::from(keep(&record)));
    count
}

/// The words of a query, lowercase, as [`Record::matches`] takes them.
pub fn query_words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// The books of the catalog matching `query` (words, any case), by
/// title: the first [`MAX_FOUND`]. [`search_after`] pages through more.
pub fn search(storage: &dyn Storage, query: &str) -> Vec<Record> {
    search_after(storage, query, None, MAX_FOUND)
}

/// `n` books of the catalog matching `query` after `after`, by title (see
/// [`page`]); none for an empty query.
pub fn search_after(
    storage: &dyn Storage,
    query: &str,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let words = query_words(query);
    if words.is_empty() {
        return Vec::new();
    }
    page(storage, &|record| record.matches(&words), after, n)
}

/// Writes `records` as the catalog, e.g. for tests of views.
pub fn write(storage: &dyn Storage, records: &[Record]) {
    let text: String = records.iter().map(Record::text).collect();
    let _ = storage.write(&catalog_path(), text.as_bytes());
}

/// Making the catalog anew: the folders still to walk, and where the
/// current one is.
#[derive(Debug, Default)]
pub struct Walk {
    dirs: Vec<String>,
    dir: Option<String>,
    last: Option<Entry>,
    /// Books found so far.
    found: usize,
}

impl Walk {
    /// Starts anew with the books under `root`.
    pub fn start(storage: &dyn Storage, root: &str) -> Walk {
        let _ = storage.write(&building_path(), &[]);
        Walk {
            dirs: Vec::from([root.trim_matches('/').to_string()]),
            ..Walk::default()
        }
    }

    /// Books found so far.
    pub fn found(&self) -> usize {
        self.found
    }

    /// One step: a few entries of a folder, at most one book opened;
    /// `true` when the catalog is complete.
    pub fn step(&mut self, env: &Env<'_>) -> bool {
        let Some(dir) = self.dir.clone().or_else(|| self.dirs.pop()) else {
            // Done: the new catalog replaces the old one.
            let _ = files::move_to(env.storage, &building_path(), &catalog_path());
            return true;
        };
        self.dir = Some(dir.clone());
        let listing = Listing {
            storage: env.storage,
            formats: env.formats,
            dir: &dir,
            folders_only: false,
            rank: None,
        };
        let entries = listing.after(self.last.as_ref(), STEP_ENTRIES);
        if entries.is_empty() {
            self.dir = None;
            self.last = None;
            return false;
        }
        for entry in entries {
            let path = files::join(&dir, &entry.name);
            let mut opened = false;
            if entry.is_dir {
                self.dirs.push(path);
            } else {
                let cached = super::book(env.storage, &path).filter(|b| b.indexed);
                let book = cached.unwrap_or_else(|| {
                    opened = true;
                    super::index(env, &path)
                });
                let record = Record::of(&path, &book);
                let _ = env
                    .storage
                    .append(&building_path(), record.text().as_bytes());
                self.found += 1;
            }
            self.last = Some(entry);
            // Opening a book takes a moment: the rest in the next step.
            if opened {
                break;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn written_read_and_searched() {
        let storage = MemoryStorage::new();
        let book = Book {
            title: Some("Second".into()),
            author: Some("Ann".into()),
            series: Some("Tales".into()),
            series_index: Some("2".into()),
            tags: alloc::vec!["Kids".into(), "Old\u{1f}times".into()],
            ..Book::default()
        };
        let record = Record::of("books/b.epub", &book);
        assert_eq!(record.tags().collect::<Vec<_>>(), ["Kids", "Old times"]);
        write(
            &storage,
            &[
                record.clone(),
                Record::of("books/Untitled.txt", &Book::default()),
            ],
        );
        let mut all = Vec::new();
        each(&storage, &mut |r| all.push(r));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], record);
        assert_eq!(all[1].title, "Untitled");
        assert_eq!(search(&storage, "ann TALES"), [record]);
        assert!(search(&storage, "nothing").is_empty());
    }

    #[test]
    fn searches_page_by_title_holding_a_page() {
        let storage = MemoryStorage::new();
        // Titles in no order, two alike but for case and path.
        let records: Vec<Record> = (0..30)
            .map(|i| {
                let book = Book {
                    title: Some(format!(
                        "{}Tale {:02}",
                        if i % 2 == 0 { "a" } else { "A" },
                        (i * 7) % 30
                    )),
                    ..Book::default()
                };
                Record::of(&format!("books/{i}.epub"), &book)
            })
            .collect();
        write(&storage, &records);
        let mut sorted = records.clone();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut seen = Vec::new();
        let mut last: Option<Record> = None;
        loop {
            let page = search_after(&storage, "tale", last.as_ref(), 7);
            if page.is_empty() {
                break;
            }
            assert!(page.len() <= 7);
            last = page.last().cloned();
            seen.extend(page);
        }
        assert_eq!(seen, sorted);
        assert_eq!(count(&storage, &|r| r.title.ends_with('5')), 3);
        assert_eq!(search(&storage, "tale").len(), 30);
        assert!(search_after(&storage, "  ", None, 5).is_empty());
    }
}
