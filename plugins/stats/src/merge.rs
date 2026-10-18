//! Merging statistics with KOReader's, as KOReader's own sync does
//! (`ReaderStatistics.onSync`):
//!
//! - Books are the same when their file is (the partial MD5). Their title
//!   and authors are taken from KOReader, so KOReader recognises them by
//!   its key (title, authors, MD5) when it merges the file back.
//! - What the last uploaded file (`cached`) has but this device no longer
//!   has was deleted here: it is not taken from the other side. What it
//!   has but the other side no longer has was deleted there: it goes here
//!   too. Books and single stays alike.
//! - The same stay (book, page, start) on both sides keeps the longer
//!   duration.
//!
//! Everything goes through files sorted in small runs ([`extsort`]), so
//! memory stays small whatever the number of stays.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use quireink_sdk::{Storage, StorageError};
use quireink_sqlite::{Database, Error};

use crate::koreader::{self, KoBook, STAT, fields, stat_record};
use crate::store::{self, Book};
use crate::{extsort, totals};

/// What a merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Outcome {
    pub books: usize,
    pub stays: usize,
}

/// The other side's books and stays, by book id there.
struct Side {
    books: Vec<KoBook>,
    path: String,
}

fn open_side(storage: &dyn Storage, path: Option<&str>) -> Option<Side> {
    let path = path?;
    let db = Database::open(storage, path).ok()?;
    let books = koreader::books(&db).ok()?;
    Some(Side {
        books,
        path: path.to_string(),
    })
}

/// The working files of a merge, in one folder.
struct Files {
    /// This device's stays, the other side's, the last upload's, sorted.
    local: String,
    income: String,
    cached: String,
    /// The stays merged, by start; then in the order of the log.
    merged: String,
    time: String,
    /// For sorting, and for writing the upload.
    runs: String,
    write: String,
}

impl Files {
    fn in_dir(dir: &str) -> Self {
        let path = |name: &str| format!("{dir}/{name}");
        Self {
            local: path("merge-local"),
            income: path("merge-income"),
            cached: path("merge-cached"),
            merged: path("merge-out"),
            time: path("merge-time"),
            runs: path("merge-run"),
            write: path("merge-write"),
        }
    }

    fn remove(&self, storage: &dyn Storage) {
        for file in [
            &self.local,
            &self.income,
            &self.cached,
            &self.merged,
            &self.time,
        ] {
            let _ = storage.remove(file);
        }
    }
}

/// Merges the statistics database `income` (another device's, if any)
/// into this device's statistics; `cached` is the file this device last
/// uploaded. With `out`, writes the merged statistics there as a KOReader
/// database. Working files go to `dir`.
pub(crate) fn merge(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    dir: &str,
) -> Result<Outcome, Error> {
    let income = open_side(storage, income);
    let cached = open_side(storage, cached);
    let books = merge_books(store::books(storage), income.as_ref(), cached.as_ref());
    let files = Files::in_dir(dir);
    write_local(storage, &books, &files)?;
    write_side(storage, income.as_ref(), &books, &files.income, &files.runs)?;
    write_side(storage, cached.as_ref(), &books, &files.cached, &files.runs)?;
    let stays = merge_stays(storage, &files, income.is_some())?;
    let per_book = to_time_order(storage, &books, &files)?;
    // The new log replaces the old one.
    store::replace_log(storage, &files.time)?;
    store::save_books(storage, &books);
    if let Some(out) = out {
        write_upload(storage, out, &books, &per_book, income.as_ref(), &files)?;
    }
    files.remove(storage);
    Ok(Outcome {
        books: books.len(),
        stays,
    })
}

/// The books here after the merge, by id: those deleted on either side
/// gone, the other side's added (named as there).
fn merge_books(mut books: Vec<Book>, income: Option<&Side>, cached: Option<&Side>) -> Vec<Book> {
    let in_income = |md5: &str| income.is_some_and(|s| s.books.iter().any(|b| b.md5 == md5));
    let last_upload = || cached.iter().flat_map(|c| c.books.iter());
    // Deleted here: in the last upload, not here (before adding any).
    let deleted_here: Vec<String> = last_upload()
        .filter(|b| !books.iter().any(|ours| ours.md5 == b.md5))
        .map(|b| b.md5.clone())
        .collect();
    // Deleted there: in the last upload, not in the other side's file.
    let deleted_there: Vec<String> = match income {
        Some(_) => last_upload()
            .filter(|b| !in_income(&b.md5))
            .map(|b| b.md5.clone())
            .collect(),
        None => Vec::new(),
    };
    books.retain(|b| !deleted_there.contains(&b.md5));
    // Books from the other side: named as there, added if new here.
    for theirs in income.iter().flat_map(|side| side.books.iter()) {
        if deleted_here.contains(&theirs.md5) {
            continue;
        }
        match books.iter_mut().find(|b| b.md5 == theirs.md5) {
            Some(ours) => take_names(ours, theirs),
            None => {
                let id = books.iter().map(|b| b.id).max().unwrap_or(0) + 1;
                books.push(Book {
                    id,
                    title: theirs.title.clone(),
                    authors: theirs.authors.clone(),
                    md5: theirs.md5.clone(),
                    pages: theirs.pages.max(0) as u32,
                    language: theirs.language.clone(),
                    series: theirs.series.clone(),
                    last_open: theirs.last_open,
                    path: String::new(),
                });
            }
        }
    }
    books.sort_by_key(|b| b.id);
    books
}

/// A book here, named as on the other side, with what that side knows
/// and this one does not.
fn take_names(ours: &mut Book, theirs: &KoBook) {
    ours.title.clone_from(&theirs.title);
    ours.authors.clone_from(&theirs.authors);
    if ours.language == "N/A" || ours.language.is_empty() {
        ours.language.clone_from(&theirs.language);
    }
    if ours.series == "N/A" || ours.series.is_empty() {
        ours.series.clone_from(&theirs.series);
    }
    ours.last_open = ours.last_open.max(theirs.last_open);
    if ours.pages == 0 {
        ours.pages = theirs.pages.max(0) as u32;
    }
}

/// This device's stays of the books kept, sorted.
fn write_local(storage: &dyn Storage, books: &[Book], files: &Files) -> Result<(), Error> {
    let present = |id: u16| books.binary_search_by_key(&id, |b| b.id).is_ok();
    let mut file = Appender::new(storage, &files.local)?;
    store::scan(storage, |s| {
        if present(s.book) {
            file.push(&stat_record([
                u32::from(s.book),
                s.page,
                s.start,
                u32::from(s.duration),
                s.pages,
            ]));
        }
    });
    file.finish()?;
    Ok(extsort::sort(
        storage,
        &files.local,
        &files.local,
        STAT,
        &files.runs,
    )?)
}

/// A side's stays, with the ids of its books here, sorted (empty where
/// there is no such side).
fn write_side(
    storage: &dyn Storage,
    side: Option<&Side>,
    books: &[Book],
    path: &str,
    runs: &str,
) -> Result<(), Error> {
    let mut file = Appender::new(storage, path)?;
    let Some(side) = side else {
        return Ok(());
    };
    // The side's book ids as ids here.
    let ids: Vec<(i64, u16)> = side
        .books
        .iter()
        .filter_map(|theirs| {
            let ours = books.iter().find(|b| b.md5 == theirs.md5)?;
            Some((theirs.id, ours.id))
        })
        .collect();
    let db = Database::open(storage, &side.path)?;
    koreader::stats(&db, |s| {
        let Some((_, id)) = ids.iter().find(|(theirs, _)| *theirs == s.book) else {
            return;
        };
        let clamp = |v: i64| v.clamp(0, i64::from(u32::MAX)) as u32;
        file.push(&stat_record([
            u32::from(*id),
            clamp(s.page),
            clamp(s.start),
            clamp(s.duration),
            clamp(s.pages),
        ]));
    })?;
    file.finish()?;
    Ok(extsort::sort(storage, path, path, STAT, runs)?)
}

/// The three files side by side by (book, page, start): what was deleted
/// on either side left out, the longer of a stay on both sides kept;
/// written by start. How many stays there are.
fn merge_stays(storage: &dyn Storage, files: &Files, has_income: bool) -> Result<usize, Error> {
    let mut merged = Appender::new(storage, &files.merged)?;
    let mut sources = [
        Source::new(storage, &files.local),
        Source::new(storage, &files.income),
        Source::new(storage, &files.cached),
    ];
    let key = |r: &Vec<u8>| -> [u8; 12] {
        let mut k = [0u8; 12];
        k.copy_from_slice(&r[..12]);
        k
    };
    // Whether `b` was read longer than `a`.
    let longer = |a: &Vec<u8>, b: &Vec<u8>| fields(b)[3] > fields(a)[3];
    let mut stays = 0;
    while let Some(next) = sources.iter().filter_map(|s| s.peek().map(key)).min() {
        // Each side's record for this key, if any (duplicates within a
        // side are merged too).
        let mut found: [Option<Vec<u8>>; 3] = [None, None, None];
        for (n, source) in sources.iter_mut().enumerate() {
            while source.peek().is_some_and(|r| key(r) == next) {
                let r = source.next().unwrap_or_default();
                if found[n].as_ref().is_none_or(|f| longer(f, &r)) {
                    found[n] = Some(r);
                }
            }
        }
        let [here, there, before] = found;
        let deleted_here = before.is_some() && here.is_none();
        let deleted_there = has_income && before.is_some() && there.is_none();
        if deleted_here || deleted_there {
            continue;
        }
        let record = match (here, there) {
            (Some(a), Some(b)) if longer(&a, &b) => b,
            (Some(a), _) => a,
            (None, Some(b)) => b,
            (None, None) => continue,
        };
        // In time order next: start first.
        let [book, page, start, duration, pages] = fields(&record).map(|v| v as u32);
        merged.push(&stat_record([start, book, page, duration, pages]));
        stays += 1;
    }
    merged.finish()?;
    for source in &mut sources {
        // A read error ends a source early: better no merge than a partial.
        if let Some(e) = source.records.failed.take() {
            return Err(Error::Storage(e));
        }
    }
    extsort::sort(storage, &files.merged, &files.merged, STAT, &files.runs)?;
    Ok(stays)
}

/// Back to (book, page, start, …) in time order, for the new log and the
/// upload's rows; each book's totals: the seconds read, and the pages
/// read as bits.
fn to_time_order(
    storage: &dyn Storage,
    books: &[Book],
    files: &Files,
) -> Result<Vec<(u64, Vec<u8>)>, Error> {
    let mut time = Appender::new(storage, &files.time)?;
    let mut per_book: Vec<(u64, Vec<u8>)> = vec![(0, Vec::new()); books.len()];
    extsort::each(storage, &files.merged, STAT, |r| {
        let [start, book, page, duration, pages] = fields(r).map(|v| v as u32);
        time.push(&stat_record([book, page, start, duration, pages]));
        if let Ok(at) = books.binary_search_by_key(&(book as u16), |b| b.id) {
            let total = &mut per_book[at];
            total.0 += u64::from(duration);
            let now = if books[at].pages > 0 {
                books[at].pages
            } else {
                pages
            };
            for p in totals::rescaled(page, pages, now) {
                let (byte, bit) = ((p / 8) as usize, p % 8);
                if total.1.len() <= byte {
                    total.1.resize(byte + 1, 0);
                }
                total.1[byte] |= 1 << bit;
            }
        }
    })?;
    time.finish()?;
    Ok(per_book)
}

/// The merged statistics as a KOReader database at `out`, the books'
/// notes and highlights as the other side counted them.
fn write_upload(
    storage: &dyn Storage,
    out: &str,
    books: &[Book],
    per_book: &[(u64, Vec<u8>)],
    income: Option<&Side>,
    files: &Files,
) -> Result<(), Error> {
    let theirs = |md5: &str| income.and_then(|s| s.books.iter().find(|b| b.md5 == md5));
    let ko_books: Vec<KoBook> = books
        .iter()
        .zip(per_book.iter())
        .map(|(b, (seconds, read))| KoBook {
            id: i64::from(b.id),
            title: b.title.clone(),
            authors: b.authors.clone(),
            notes: theirs(&b.md5).map_or(0, |t| t.notes),
            last_open: b.last_open,
            highlights: theirs(&b.md5).map_or(0, |t| t.highlights),
            pages: i64::from(b.pages),
            series: na(&b.series),
            language: na(&b.language),
            md5: b.md5.clone(),
            total_read_time: *seconds as i64,
            total_read_pages: read.iter().map(|x| x.count_ones()).sum::<u32>() as i64,
        })
        .collect();
    koreader::write(storage, out, &ko_books, &files.time, &files.write)
}

/// Records added to a file a few kilobytes at a time; the first error
/// kept for the end.
struct Appender<'s> {
    storage: &'s dyn Storage,
    path: &'s str,
    buffer: Vec<u8>,
    failed: Option<StorageError>,
}

impl<'s> Appender<'s> {
    /// The file at `path`, emptied.
    fn new(storage: &'s dyn Storage, path: &'s str) -> Result<Self, Error> {
        storage.write(path, &[])?;
        Ok(Self {
            storage,
            path,
            buffer: Vec::new(),
            failed: None,
        })
    }

    fn push(&mut self, record: &[u8]) {
        self.buffer.extend_from_slice(record);
        if self.buffer.len() >= 4096 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if let Err(e) = self.storage.append(self.path, &self.buffer) {
            self.failed.get_or_insert(e);
        }
        self.buffer.clear();
    }

    fn finish(mut self) -> Result<(), Error> {
        self.flush();
        match self.failed {
            Some(e) => Err(Error::Storage(e)),
            None => Ok(()),
        }
    }
}

/// KOReader writes `N/A` for what a book does not say.
fn na(text: &str) -> String {
    if text.is_empty() {
        String::from("N/A")
    } else {
        text.to_string()
    }
}

/// A sorted file of stays, read ahead by one.
struct Source<'s> {
    records: extsort::Records<'s>,
    head: Option<Vec<u8>>,
}

impl<'s> Source<'s> {
    fn new(storage: &'s dyn Storage, path: &str) -> Self {
        let mut records = extsort::Records::new(storage, path, STAT);
        let head = records.next();
        Self { records, head }
    }

    fn peek(&self) -> Option<&Vec<u8>> {
        self.head.as_ref()
    }

    fn next(&mut self) -> Option<Vec<u8>> {
        let head = self.head.take();
        self.head = self.records.next();
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::PageStat;
    use quireink_sdk::MemoryStorage;

    const DIR: &str = ".quireink/plugins/stats";

    fn book(id: u16, md5: &str) -> Book {
        Book {
            id,
            title: format!("Ours {id}"),
            authors: String::from("N/A"),
            md5: md5.to_string(),
            pages: 100,
            language: String::from("de"),
            series: String::from("N/A"),
            last_open: 10,
            path: format!("books/{id}.epub"),
        }
    }

    fn stay(book: u16, page: u32, start: u32, duration: u16) -> PageStat {
        PageStat {
            book,
            page,
            start,
            duration,
            pages: 100,
        }
    }

    fn log(storage: &MemoryStorage) -> Vec<(u16, u32, u32, u16)> {
        let mut out = Vec::new();
        store::scan(storage, |s| out.push((s.book, s.page, s.start, s.duration)));
        out
    }

    /// A KOReader file from another device: its own ids and names.
    fn other_device(
        storage: &MemoryStorage,
        path: &str,
        books: &[(i64, &str, &str)],
        stays: &[(i64, u32, u32, u32)],
    ) {
        let ko: Vec<KoBook> = books
            .iter()
            .map(|(id, title, md5)| KoBook {
                id: *id,
                title: title.to_string(),
                authors: String::from("Autorin"),
                pages: 200,
                md5: md5.to_string(),
                series: String::from("N/A"),
                language: String::from("de"),
                last_open: 99,
                ..KoBook::default()
            })
            .collect();
        let mut records = Vec::new();
        let mut sorted = stays.to_vec();
        sorted.sort_by_key(|s| s.2);
        for (book, page, start, duration) in sorted {
            records.extend_from_slice(&stat_record([book as u32, page, start, duration, 200]));
        }
        storage.write("stays.tmp", &records).unwrap();
        koreader::write(storage, path, &ko, "stays.tmp", "tmp").unwrap();
    }

    #[test]
    fn merges_like_koreader() {
        let storage = MemoryStorage::new();
        store::save_books(&storage, &[book(1, "aaa"), book(2, "bbb")]);
        store::append(&storage, stay(1, 5, 1000, 30));
        store::append(&storage, stay(1, 6, 1040, 20));
        store::append(&storage, stay(2, 1, 1100, 10));
        // The other device: book "aaa" as its id 7 (a stay the same, one
        // longer, one new), and a book unknown here.
        other_device(
            &storage,
            "income.db",
            &[(7, "Their title", "aaa"), (8, "Neu", "ccc")],
            &[
                (7, 5, 1000, 25),
                (7, 6, 1040, 50),
                (7, 9, 2000, 40),
                (8, 3, 3000, 60),
            ],
        );
        let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 3, stays: 5 });
        let books = store::books(&storage);
        assert_eq!(
            books.iter().map(|b| b.md5.as_str()).collect::<Vec<_>>(),
            ["aaa", "bbb", "ccc"]
        );
        // Named as KOReader names them, the path here kept.
        assert_eq!(
            (books[0].title.as_str(), books[0].authors.as_str()),
            ("Their title", "Autorin")
        );
        assert_eq!(books[0].path, "books/1.epub");
        assert_eq!(books[2].id, 3);
        // In time order; the longer duration of a stay wins.
        assert_eq!(
            log(&storage),
            [
                (1, 5, 1000, 30),
                (1, 6, 1040, 50),
                (2, 1, 1100, 10),
                (1, 9, 2000, 40),
                (3, 3, 3000, 60)
            ]
        );
        // The file to upload reads back with the same.
        let db = Database::open(&storage, "out.db").unwrap();
        assert_eq!(db.user_version, koreader::SCHEMA_VERSION);
        let ko = koreader::books(&db).unwrap();
        assert_eq!(ko.len(), 3);
        assert_eq!(ko[0].total_read_time, 120);
        let mut stays = 0;
        koreader::stats(&db, |_| stays += 1).unwrap();
        assert_eq!(stays, 5);
    }

    #[test]
    fn deletions_go_both_ways() {
        let storage = MemoryStorage::new();
        store::save_books(&storage, &[book(1, "aaa"), book(2, "bbb")]);
        store::append(&storage, stay(1, 1, 100, 10));
        store::append(&storage, stay(1, 2, 200, 10));
        store::append(&storage, stay(2, 1, 300, 10));
        // The last upload had both books and three stays, plus a stay of
        // book 1 that was since deleted here.
        other_device(
            &storage,
            "cached.db",
            &[(1, "Ours 1", "aaa"), (2, "Ours 2", "bbb")],
            &[
                (1, 1, 100, 10),
                (1, 2, 200, 10),
                (1, 3, 250, 10),
                (2, 1, 300, 10),
            ],
        );
        // There, book "bbb" and the stay at 200 were deleted since; the
        // stay at 250 (deleted here) is still there.
        other_device(
            &storage,
            "income.db",
            &[(4, "Ours 1", "aaa")],
            &[(4, 1, 100, 10), (4, 3, 250, 10)],
        );
        let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 1, stays: 1 });
        assert_eq!(log(&storage), [(1, 1, 100, 10)]);
        assert_eq!(store::books(&storage).len(), 1);
    }

    /// Merges a real KOReader file into made-up statistics and writes the
    /// result for SQLite to check:
    /// `QUIREINK_KO_IN=… QUIREINK_KO_OUT=… cargo test -p quireink-plugin-stats -- --ignored`.
    #[test]
    #[ignore]
    fn real_koreader_file() {
        extern crate std;
        let (Ok(input), Ok(output)) = (
            std::env::var("QUIREINK_KO_IN"),
            std::env::var("QUIREINK_KO_OUT"),
        ) else {
            return;
        };
        let storage = MemoryStorage::new().with("income.db", std::fs::read(input).unwrap());
        store::save_books(&storage, &[book(1, "made-up-md5")]);
        store::append(&storage, stay(1, 1, 1_700_000_000, 30));
        let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
        std::println!("{outcome:?}");
        std::fs::write(output, storage.get("out.db").unwrap()).unwrap();
    }
}
