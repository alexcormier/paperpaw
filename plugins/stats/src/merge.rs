//! Merging statistics with KOReader's, as KOReader's own sync does
//! (`ReaderStatistics.onSync`), except for what this device lost:
//!
//! - Books are the same when their file is (the partial MD5; for a book
//!   KOReader knows no file of, KOReader's key of title, authors and
//!   MD5). Their title and authors are taken from KOReader, so KOReader
//!   recognises them by its key (title, authors, MD5) when it merges the
//!   file back.
//! - What the last uploaded file (`cached`) has but the other side no
//!   longer has was deleted there: it goes here too. Books and single
//!   stays alike.
//! - What the last upload has but this device no longer has is not taken
//!   for deleted here, as KOReader would take it: PaperPaw deletes no
//!   statistics, so it was lost, and comes back from the other side.
//! - The same stay (book, page, start) on both sides keeps the longer
//!   duration.
//!
//! Nothing is merged from what cannot be read whole: an unreadable file
//! of the other side, `books.ini` or log ends the merge with an error
//! before anything here is written. A last upload that cannot be read is
//! left out: then nothing counts as deleted there.
//!
//! Everything goes through files sorted in small runs ([`extsort`]), so
//! memory stays small whatever the number of stays.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_digest::{hex, md5::Md5};
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_sqlite::{Database, Error};

use crate::koreader::{self, KoBook, STAT, fields, stat_record};
use crate::store::{self, Book};
use crate::{extsort, totals};

/// What a merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Outcome {
    pub books: usize,
    pub stays: usize,
}

/// Why a merge failed. The statistics here are as they were, unless
/// writing them failed halfway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The other side's file cannot be read: no statistics database (e.g.
    /// a web page), cut short, or of a kind not understood.
    Income(Error),
    /// The statistics here cannot be read or written.
    Here(Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Income(e) | Failure::Here(e) => write!(f, "{e}"),
        }
    }
}

impl From<StorageError> for Failure {
    fn from(error: StorageError) -> Self {
        Failure::Here(Error::Storage(error))
    }
}

/// What tells books apart: KOReader's partial MD5 of the file; for a book
/// KOReader knows no file of, a digest of its key (title, authors, MD5),
/// so such books stay apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key([u8; 16]);

impl Key {
    fn of(md5: &str, title: &str, authors: &str) -> Self {
        let bytes = |hex: &str| -> Option<[u8; 16]> { hex::decode(hex)?.try_into().ok() };
        Self(bytes(md5).unwrap_or_else(|| {
            let mut digest = Md5::new();
            for part in [title, authors, md5] {
                digest.update(part.as_bytes());
                digest.update(&[0]);
            }
            bytes(&digest.hex()).unwrap_or_default()
        }))
    }

    fn of_book(book: &Book) -> Self {
        Self::of(&book.md5, &book.title, &book.authors)
    }

    fn of_ko(book: &KoBook) -> Self {
        Self::of(&book.md5, &book.title, &book.authors)
    }
}

/// A book of the other side's file, as far as it matters once the books
/// are merged.
#[derive(Debug, Clone, Copy)]
struct Known {
    key: Key,
    /// Its id there.
    id: i64,
    notes: i64,
    highlights: i64,
}

/// A file of the other side (or the last upload): its books by key.
struct Side {
    path: String,
    books: Vec<Known>,
}

impl Side {
    /// The side at `path`, and its books whole.
    fn open(storage: &dyn Storage, path: &str) -> Result<(Self, Vec<KoBook>), Error> {
        let db = Database::open(storage, path)?;
        let books = koreader::books(&db)?;
        let mut known: Vec<Known> = books
            .iter()
            .map(|b| Known {
                key: Key::of_ko(b),
                id: b.id,
                notes: b.notes,
                highlights: b.highlights,
            })
            .collect();
        known.sort_unstable_by_key(|k| k.key);
        let side = Self {
            path: path.to_string(),
            books: known,
        };
        Ok((side, books))
    }

    fn find(&self, key: Key) -> Option<&Known> {
        let at = self.books.binary_search_by_key(&key, |k| k.key).ok()?;
        Some(&self.books[at])
    }
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
/// uploaded to the same place. With `out`, writes the merged statistics
/// there as a KOReader database. Working files go to `dir`; they are
/// gone afterwards, and so is `out` after an error.
pub(crate) fn merge(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    dir: &str,
) -> Result<Outcome, Failure> {
    let files = Files::in_dir(dir);
    let result = merge_with(storage, income, cached, out, &files);
    files.remove(storage);
    if result.is_err()
        && let Some(out) = out
    {
        let _ = storage.remove(out);
    }
    result
}

fn merge_with(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    files: &Files,
) -> Result<Outcome, Failure> {
    let income = match income {
        Some(path) => Some(Side::open(storage, path).map_err(Failure::Income)?),
        None => None,
    };
    let cached = cached.and_then(|path| Side::open(storage, path).ok().map(|(side, _)| side));
    let books = store::load_books(storage)?;
    let (books, kept, income) = match income {
        Some((side, theirs)) => {
            let (books, kept) = merge_books(books, &side, theirs, cached.as_ref())?;
            (books, kept, Some(side))
        }
        None => {
            let kept = books.iter().map(|b| b.id).collect();
            (books, kept, None)
        }
    };
    write_local(storage, &kept, files)?;
    match &income {
        Some(side) => write_side(storage, side, &books, &files.income, &files.runs)?,
        None => storage.write(&files.income, &[])?,
    }
    let cached_read = match &cached {
        Some(side) => match write_side(storage, side, &books, &files.cached, &files.runs) {
            Ok(()) => true,
            // Its books could be read, its stays cannot: left out.
            Err(Failure::Income(_)) => false,
            Err(e) => return Err(e),
        },
        None => false,
    };
    if !cached_read {
        storage.write(&files.cached, &[])?;
    }
    let stays = merge_stays(storage, files, income.is_some())?;
    let per_book = to_time_order(storage, &books, files)?;
    // The new log replaces the old one.
    store::replace_log(storage, &files.time)?;
    store::save_books(storage, &books)?;
    let outcome = Outcome {
        books: books.len(),
        stays,
    };
    if let Some(out) = out {
        write_upload(storage, out, books, &per_book, income.as_ref(), files)
            .map_err(Failure::Here)?;
    }
    Ok(outcome)
}

/// The books here after the merge, by id: those deleted there gone, the
/// other side's added (named as there); and the ids of those that were
/// here before, whose stays are kept.
fn merge_books(
    mut books: Vec<Book>,
    income: &Side,
    theirs: Vec<KoBook>,
    cached: Option<&Side>,
) -> Result<(Vec<Book>, Vec<u16>), Failure> {
    // Deleted there: in the last upload, not in the other side's file.
    if let Some(cached) = cached {
        books.retain(|b| {
            let key = Key::of_book(b);
            cached.find(key).is_none() || income.find(key).is_some()
        });
    }
    let kept: Vec<u16> = books.iter().map(|b| b.id).collect();
    let mut last = kept.iter().copied().max().unwrap_or(0);
    // This device's books by key (their places in `books`), with those
    // added.
    let mut here: Vec<(Key, usize)> = books
        .iter()
        .enumerate()
        .map(|(at, b)| (Key::of_book(b), at))
        .collect();
    here.sort_unstable();
    for theirs in theirs {
        let key = Key::of_ko(&theirs);
        match here.binary_search_by_key(&key, |(k, _)| *k) {
            Ok(at) => take_names(&mut books[here[at].1], &theirs),
            Err(at) => {
                last = last
                    .checked_add(1)
                    .ok_or(Failure::Here(Error::Corrupt("too many books")))?;
                here.insert(at, (key, books.len()));
                books.push(Book {
                    id: last,
                    pages: theirs.pages.clamp(0, i64::from(u32::MAX)) as u32,
                    last_open: theirs.last_open,
                    title: theirs.title,
                    authors: theirs.authors,
                    md5: theirs.md5,
                    language: theirs.language,
                    series: theirs.series,
                    path: String::new(),
                    finished: 0,
                });
            }
        }
    }
    Ok((books, kept))
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
        ours.pages = theirs.pages.clamp(0, i64::from(u32::MAX)) as u32;
    }
}

/// This device's stays of the books `kept` (by id), sorted. A read error
/// fails the merge: what was not read would look deleted.
fn write_local(storage: &dyn Storage, kept: &[u16], files: &Files) -> Result<(), Failure> {
    let mut file = Appender::new(storage, &files.local)?;
    store::scan(storage, |s| {
        if kept.binary_search(&s.book).is_ok() {
            file.push(&stat_record([
                u32::from(s.book),
                s.page,
                s.start,
                u32::from(s.duration),
                s.pages,
            ]));
        }
    })?;
    file.finish()?;
    Ok(extsort::sort(
        storage,
        &files.local,
        &files.local,
        STAT,
        &files.runs,
    )?)
}

/// A side's stays, with the ids of its books here, sorted. What cannot
/// be read of the side is [`Failure::Income`].
fn write_side(
    storage: &dyn Storage,
    side: &Side,
    books: &[Book],
    path: &str,
    runs: &str,
) -> Result<(), Failure> {
    // The side's book ids as ids here, by theirs.
    let mut here: Vec<(Key, u16)> = books.iter().map(|b| (Key::of_book(b), b.id)).collect();
    here.sort_unstable();
    let mut ids: Vec<(i64, u16)> = side
        .books
        .iter()
        .filter_map(|theirs| {
            let at = here.binary_search_by_key(&theirs.key, |(k, _)| *k).ok()?;
            Some((theirs.id, here[at].1))
        })
        .collect();
    ids.sort_unstable();
    drop(here);
    let mut file = Appender::new(storage, path)?;
    let db = Database::open(storage, &side.path).map_err(Failure::Income)?;
    koreader::stats(&db, |s| {
        let Ok(at) = ids.binary_search_by_key(&s.book, |(theirs, _)| *theirs) else {
            return;
        };
        let clamp = |v: i64| v.clamp(0, i64::from(u32::MAX)) as u32;
        file.push(&stat_record([
            u32::from(ids[at].1),
            clamp(s.page),
            clamp(s.start),
            clamp(s.duration),
            clamp(s.pages),
        ]));
    })
    .map_err(|e| match e {
        // Writing the working file was not the side's fault.
        Error::Storage(e) => Failure::from(e),
        e => Failure::Income(e),
    })?;
    file.finish()?;
    Ok(extsort::sort(storage, path, path, STAT, runs)?)
}

/// The three files side by side by (book, page, start): what was deleted
/// on the other side left out, the longer of a stay on both sides kept;
/// written by start. How many stays there are.
fn merge_stays(storage: &dyn Storage, files: &Files, has_income: bool) -> Result<usize, Failure> {
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
        let deleted_there = has_income && before.is_some() && there.is_none();
        if deleted_there {
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
            return Err(e.into());
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
) -> Result<Vec<(u64, Vec<u8>)>, Failure> {
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
            totals::mark_read(&mut total.1, page, pages, now);
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
    books: Vec<Book>,
    per_book: &[(u64, Vec<u8>)],
    income: Option<&Side>,
    files: &Files,
) -> Result<(), Error> {
    let ko_books: Vec<KoBook> = books
        .into_iter()
        .zip(per_book.iter())
        .map(|(b, (seconds, read))| {
            let theirs = income.and_then(|s| s.find(Key::of_book(&b)));
            KoBook {
                id: i64::from(b.id),
                notes: theirs.map_or(0, |t| t.notes),
                last_open: b.last_open,
                highlights: theirs.map_or(0, |t| t.highlights),
                pages: i64::from(b.pages),
                series: na(b.series),
                language: na(b.language),
                total_read_time: *seconds as i64,
                total_read_pages: read.iter().map(|x| x.count_ones()).sum::<u32>() as i64,
                title: b.title,
                authors: b.authors,
                md5: b.md5,
            }
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
    fn new(storage: &'s dyn Storage, path: &'s str) -> Result<Self, StorageError> {
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

    fn finish(mut self) -> Result<(), StorageError> {
        self.flush();
        match self.failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// KOReader writes `N/A` for what a book does not say.
fn na(text: String) -> String {
    if text.is_empty() {
        String::from("N/A")
    } else {
        text
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
    use crate::testing::Faulty;
    use paperpaw_sdk::MemoryStorage;

    const DIR: &str = ".paperpaw/plugins/stats";

    fn md5(n: u8) -> String {
        format!("{n:032x}")
    }

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
            finished: 0,
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

    fn log(storage: &dyn Storage) -> Vec<(u16, u32, u32, u16)> {
        let mut out = Vec::new();
        store::scan(storage, |s| out.push((s.book, s.page, s.start, s.duration))).unwrap();
        out
    }

    /// A KOReader file from another device: its own ids and names.
    fn other_device(
        storage: &dyn Storage,
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
        storage.remove("stays.tmp").unwrap();
    }

    /// Two books here and three stays.
    fn this_device(storage: &dyn Storage) {
        store::save_books(storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
        store::append(storage, stay(1, 5, 1000, 30)).unwrap();
        store::append(storage, stay(1, 6, 1040, 20)).unwrap();
        store::append(storage, stay(2, 1, 1100, 10)).unwrap();
    }

    /// The files of the statistics here, to see they did not change.
    fn files(storage: &MemoryStorage) -> [Option<Vec<u8>>; 2] {
        ["books.ini", "pages.bin"].map(|name| storage.get(&format!("{DIR}/{name}")))
    }

    /// What is in the statistics' folder.
    fn folder(storage: &dyn Storage) -> Vec<String> {
        let mut names = Vec::new();
        storage
            .list(DIR, &mut |entry| names.push(entry.name.to_string()))
            .unwrap();
        names.sort();
        names
    }

    #[test]
    fn merges_like_koreader() {
        let storage = MemoryStorage::new();
        this_device(&storage);
        // The other device: book A as its id 7 (a stay the same, one
        // longer, one new), and a book unknown here.
        other_device(
            &storage,
            "income.db",
            &[(7, "Their title", &md5(0xa)), (8, "Neu", &md5(0xc))],
            &[
                (7, 5, 1000, 25),
                (7, 6, 1040, 50),
                (7, 9, 2000, 40),
                (8, 3, 3000, 60),
            ],
        );
        let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 3, stays: 5 });
        let books = store::load_books(&storage).unwrap();
        assert_eq!(
            books.iter().map(|b| b.md5.clone()).collect::<Vec<_>>(),
            [md5(0xa), md5(0xb), md5(0xc)]
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
        // Only what was there before, and the upload.
        assert_eq!(folder(&storage), ["books.ini", "pages.bin"]);
    }

    /// What was deleted on the other side since the last upload goes here
    /// too; what was lost here comes back from there.
    #[test]
    fn deletions_there_come_here_and_losses_here_come_back() {
        let storage = MemoryStorage::new();
        store::save_books(&storage, &[book(1, &md5(0xa)), book(2, &md5(0xb))]).unwrap();
        store::append(&storage, stay(1, 1, 100, 10)).unwrap();
        store::append(&storage, stay(1, 2, 200, 10)).unwrap();
        store::append(&storage, stay(2, 1, 300, 10)).unwrap();
        // The last upload had both books and four stays; the stay at 250
        // was lost here since.
        other_device(
            &storage,
            "cached.db",
            &[(1, "Ours 1", &md5(0xa)), (2, "Ours 2", &md5(0xb))],
            &[
                (1, 1, 100, 10),
                (1, 2, 200, 10),
                (1, 3, 250, 10),
                (2, 1, 300, 10),
            ],
        );
        // There, book B and the stay at 200 were deleted since.
        other_device(
            &storage,
            "income.db",
            &[(4, "Ours 1", &md5(0xa))],
            &[(4, 1, 100, 10), (4, 3, 250, 10)],
        );
        let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 1, stays: 2 });
        assert_eq!(log(&storage), [(1, 1, 100, 10), (1, 3, 250, 10)]);
        assert_eq!(store::load_books(&storage).unwrap().len(), 1);
    }

    /// A book lost here (`books.ini` rewritten without it) that the last
    /// upload had is not taken for deleted: it comes back with its stays,
    /// and the upload keeps it, so KOReader does not delete it either.
    #[test]
    fn a_book_lost_here_comes_back() {
        let storage = MemoryStorage::new();
        store::save_books(&storage, &[book(1, &md5(0xa))]).unwrap();
        store::append(&storage, stay(1, 1, 100, 10)).unwrap();
        let both = [(1, "Ours 1", md5(0xa)), (2, "Ours 2", md5(0xb))];
        let both: Vec<(i64, &str, &str)> =
            both.iter().map(|(i, t, m)| (*i, *t, m.as_str())).collect();
        let stays = [(1, 1, 100, 10), (2, 1, 300, 10), (2, 2, 400, 20)];
        other_device(&storage, "cached.db", &both, &stays);
        other_device(&storage, "income.db", &both, &stays);
        let outcome = merge(
            &storage,
            Some("income.db"),
            Some("cached.db"),
            Some("out.db"),
            DIR,
        )
        .unwrap();
        assert_eq!(outcome, Outcome { books: 2, stays: 3 });
        assert_eq!(
            log(&storage),
            [(1, 1, 100, 10), (2, 1, 300, 10), (2, 2, 400, 20)]
        );
        let db = Database::open(&storage, "out.db").unwrap();
        assert_eq!(koreader::books(&db).unwrap().len(), 2);
    }

    /// A downloaded file that is no statistics database (a web page with
    /// status 200), or one cut short: the merge fails and nothing here
    /// changes, nor is anything written to upload.
    #[test]
    fn an_unreadable_income_changes_nothing() {
        let storage = MemoryStorage::new();
        this_device(&storage);
        other_device(
            &storage,
            "good.db",
            &[(1, "Neu", &md5(0xc))],
            &[(1, 1, 50, 10)],
        );
        let good = storage.get("good.db").unwrap();
        let before = files(&storage);
        let page = b"<html><body>Log in</body></html>".to_vec();
        // The first page (with the schema) and no more.
        let cut = good[..4096 + 100].to_vec();
        for (what, data) in [("page", page), ("cut", cut), ("empty", Vec::new())] {
            storage.write("income.db", &data).unwrap();
            let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
            assert!(
                matches!(result, Err(Failure::Income(_))),
                "{what}: {result:?}"
            );
            assert_eq!(files(&storage), before, "{what}");
            assert!(!storage.exists("out.db"), "{what}");
            assert_eq!(folder(&storage), ["books.ini", "pages.bin"], "{what}");
        }
    }

    /// Statistics here that cannot be read whole are not merged: what was
    /// not read would look deleted.
    #[test]
    fn unreadable_local_statistics_change_nothing() {
        // `books.ini` saved as Latin-1.
        let storage = MemoryStorage::new();
        this_device(&storage);
        other_device(
            &storage,
            "income.db",
            &[(1, "Neu", &md5(0xc))],
            &[(1, 1, 50, 10)],
        );
        storage
            .write(
                &format!("{DIR}/books.ini"),
                b"[book.1]\ntitle = M\xfcller\n",
            )
            .unwrap();
        let before = files(&storage);
        let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
        assert!(matches!(result, Err(Failure::Here(_))), "{result:?}");
        assert_eq!(files(&storage), before);
        assert!(!storage.exists("out.db"));
        // The log cannot be read to the end.
        let storage = Faulty::new(MemoryStorage::new());
        this_device(&storage);
        for i in 0..100 {
            store::append(&storage, stay(2, i + 2, 2000 + i * 10, 10)).unwrap();
        }
        other_device(
            &storage,
            "income.db",
            &[(1, "Neu", &md5(0xc))],
            &[(1, 1, 50, 10)],
        );
        storage.break_at(&format!("{DIR}/pages.bin"), 16 * 70);
        let before = files(&storage.inner);
        let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
        assert_eq!(result, Err(Failure::Here(Error::Storage(StorageError::Io))));
        assert_eq!(files(&storage.inner), before);
        assert!(!storage.exists("out.db"));
    }

    /// A last upload that cannot be read tells nothing: nothing counts as
    /// deleted there.
    #[test]
    fn an_unreadable_last_upload_deletes_nothing() {
        let storage = MemoryStorage::new();
        this_device(&storage);
        other_device(
            &storage,
            "income.db",
            &[(1, "Neu", &md5(0xc))],
            &[(1, 1, 50, 10)],
        );
        storage.write("cached.db", b"not a database").unwrap();
        let outcome = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 3, stays: 4 });
    }

    /// KOReader's books without a file's MD5 are told apart by title and
    /// authors, as KOReader does, and stay so in later merges.
    #[test]
    fn books_without_md5_stay_apart() {
        let storage = MemoryStorage::new();
        other_device(
            &storage,
            "income.db",
            &[(1, "Erstes", ""), (2, "Zweites", "")],
            &[(1, 1, 100, 10), (2, 1, 200, 20)],
        );
        let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
        assert_eq!(outcome, Outcome { books: 2, stays: 2 });
        let books = store::load_books(&storage).unwrap();
        assert_eq!(
            books.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(),
            ["Erstes", "Zweites"]
        );
        assert_eq!(log(&storage), [(1, 1, 100, 10), (2, 1, 200, 20)]);
        // Again, with what was uploaded as the last upload.
        storage.rename("out.db", "cached.db").unwrap();
        let again = merge(&storage, Some("income.db"), Some("cached.db"), None, DIR).unwrap();
        assert_eq!(again, outcome);
        assert_eq!(log(&storage), [(1, 1, 100, 10), (2, 1, 200, 20)]);
    }

    /// A card that fills up at any point of a merge: the error comes back,
    /// no working file is left, and the statistics are as they were (or
    /// merged whole, once the log was swapped).
    #[test]
    fn working_files_go_after_an_error() {
        let make = || {
            let storage = Faulty::new(MemoryStorage::new());
            this_device(&storage);
            other_device(
                &storage,
                "income.db",
                &[(7, "Neu", &md5(0xc))],
                &[(7, 1, 50, 10), (7, 2, 60, 10)],
            );
            storage
        };
        let mut failed = 0;
        for room in 0..200 {
            let storage = make();
            let before = files(&storage.inner);
            storage.fill_after(room);
            let result = merge(&storage, Some("income.db"), None, Some("out.db"), DIR);
            if result.is_ok() {
                break;
            }
            failed += 1;
            assert_eq!(folder(&storage), ["books.ini", "pages.bin"], "room {room}");
            assert!(!storage.exists("out.db"), "room {room}");
            let after = files(&storage.inner);
            let merged = log(&storage).len() == 5;
            assert!(after == before || merged, "room {room}");
        }
        assert!(failed > 10, "{failed}");
    }

    /// Merges a real KOReader file into made-up statistics and writes the
    /// result for SQLite to check:
    /// `PAPERPAW_KO_IN=… PAPERPAW_KO_OUT=… cargo test -p paperpaw-plugin-stats -- --ignored`.
    #[test]
    #[ignore]
    fn real_koreader_file() {
        extern crate std;
        let (Ok(input), Ok(output)) = (
            std::env::var("PAPERPAW_KO_IN"),
            std::env::var("PAPERPAW_KO_OUT"),
        ) else {
            return;
        };
        let storage = MemoryStorage::new().with("income.db", std::fs::read(input).unwrap());
        store::save_books(&storage, &[book(1, &md5(0xd))]).unwrap();
        store::append(&storage, stay(1, 1, 1_700_000_000, 30)).unwrap();
        let outcome = merge(&storage, Some("income.db"), None, Some("out.db"), DIR).unwrap();
        std::println!("{outcome:?}");
        std::fs::write(output, storage.get("out.db").unwrap()).unwrap();
    }
}

#[cfg(test)]
mod props_tests;
