//! The catalog: every book under the library folder with what the
//! library knows of it (author, title, series, number, tags, how far it
//! is read, whether it is a favourite or was opened, when it was added,
//! the fields plugins keep in it), for views of the library, its filters
//! and searches, without reading each book's details.
//!
//! It is kept in `.paperpaw/library/catalog.txt`, a book a line, and made
//! anew by the kernel in the background (`Walk`, as a job; see
//! [`super::request_catalog`]): from the details the library keeps of
//! each book, opening only books not seen before, a few entries at a
//! time. Until a new one is complete, the old one is read. It is read a
//! piece at a time, never held in memory.
//!
//! How far a book is read and whether it is a favourite change while
//! the catalog stands: such changes are appended to a short log beside it
//! (`status.log`, by the kernel and the library's setters) and read with
//! it, and folded into it when the log grows long, so a page turned never
//! rewrites the catalog.
//!
//! The first line of a catalog of this format is a header (`\tcatalog\t2`
//! and its [`generation`]); a catalog of format 1 (six columns, no
//! status) is still read.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[cfg(any(test, feature = "kernel"))]
use super::listing::{Entry, Listing};
use super::{Book, DIR};
use crate::{Env, Storage, files};
use paperpaw_digest::fnv1a64;

/// Between a book's tags (and its fields) in the catalog.
const SEPARATOR: char = '\u{1f}';
/// Books [`search`] finds at most: a page of them fits the smallest
/// device (a record takes some 200 bytes).
pub const MAX_FOUND: usize = 200;
/// Entries looked at, and books opened, in one step of making the catalog.
const STEP_ENTRIES: usize = 12;
/// The start of the first line of a catalog of this format; its
/// generation follows.
const HEADER: &str = "\tcatalog\t2";
/// Changes of books' status the log holds before they are folded into
/// the catalog; read with it, 16 bytes each.
pub const MAX_LOG: usize = 256;

fn catalog_path() -> String {
    format!("{DIR}/catalog.txt")
}

fn building_path() -> String {
    format!("{DIR}/catalog.tmp")
}

fn log_path() -> String {
    format!("{DIR}/status.log")
}

fn folding_path() -> String {
    format!("{DIR}/catalog.fold")
}

/// How far a book is read and what the reader made of it: what changes
/// while the catalog stands (noted beside it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
    /// How far it is read, in percent, once opened.
    pub progress: Option<u8>,
    pub favourite: bool,
    /// It was opened (it is among the books opened last, or was).
    pub opened: bool,
}

impl Status {
    pub fn new(progress: Option<u8>, favourite: bool, opened: bool) -> Self {
        Self {
            progress,
            favourite,
            opened,
        }
    }

    /// As written in a line: the progress (empty never), then `f` for a
    /// favourite and `o` for opened.
    fn columns(self) -> (String, String) {
        let progress = self.progress.map(|p| p.to_string()).unwrap_or_default();
        let mut flags = String::new();
        if self.favourite {
            flags.push('f');
        }
        if self.opened {
            flags.push('o');
        }
        (progress, flags)
    }

    fn parse(progress: &str, flags: &str) -> Self {
        Self {
            progress: progress.parse::<u8>().ok().map(|p| p.min(100)),
            favourite: flags.contains('f'),
            opened: flags.contains('o'),
        }
    }
}

/// A book in the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Record {
    pub path: String,
    pub author: String,
    pub title: String,
    pub series: String,
    /// Its number in the series.
    pub number: String,
    /// Separated by [`SEPARATOR`]; see [`Record::tags`].
    tags: String,
    pub status: Status,
    /// The day it was added ([`Book::added`]).
    pub added: Option<u32>,
    /// Fields plugins keep in the catalog (`id=value`, separated by
    /// [`SEPARATOR`]); see [`Record::field`].
    fields: String,
}

impl Record {
    /// The record of the book at `path` with the details `book` (its
    /// progress and the day it was added; neither favourite nor opened,
    /// see [`Record::status`]), without fields.
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
                .map(|t| t.replace(SEPARATOR, " "))
                .collect::<Vec<_>>()
                .join(&SEPARATOR.to_string()),
            status: Status::new(book.progress, false, false),
            added: book.added,
            fields: String::new(),
        }
    }

    /// With `status`.
    pub fn with_status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// With the fields of `book` named by `ids` (those kept in the
    /// catalog, [`crate::library::BookField::in_catalog`]).
    pub fn with_fields(mut self, book: &Book, ids: &[&str]) -> Self {
        self.fields = book
            .fields
            .iter()
            .filter(|(id, _)| ids.contains(&id.as_str()))
            .map(|(id, value)| format!("{id}={}", value.replace(SEPARATOR, " ")))
            .collect::<Vec<_>>()
            .join(&SEPARATOR.to_string());
        self
    }

    /// The book's tags.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.split(SEPARATOR).filter(|t| !t.is_empty())
    }

    /// The value of the field `id` kept in the catalog, if set.
    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields
            .split(SEPARATOR)
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == id)
            .map(|(_, value)| value)
    }

    /// How far it is read, as the filters tell it apart.
    pub fn reading(&self) -> Reading {
        match self.status.progress {
            Some(100) => Reading::Finished,
            Some(p) if p > 0 => Reading::Begun,
            _ if self.status.opened => Reading::Begun,
            _ => Reading::Unread,
        }
    }

    fn parse(text: &str) -> Option<Record> {
        let mut fields = text.split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_string();
        let mut next = || fields.next().unwrap_or("").to_string();
        let (author, title, series, number, tags) = (next(), next(), next(), next(), next());
        let (progress, flags, added, extra) = (next(), next(), next(), next());
        Some(Record {
            path,
            author,
            title,
            series,
            number,
            tags,
            status: Status::parse(&progress, &flags),
            added: added.parse().ok(),
            fields: extra,
        })
    }

    fn text(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let (progress, flags) = self.status.columns();
        let added = self.added.map(|a| a.to_string()).unwrap_or_default();
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{progress}\t{flags}\t{added}\t{}\n",
            clean(&self.path),
            clean(&self.author),
            clean(&self.title),
            clean(&self.series),
            clean(&self.number),
            clean(&self.tags),
            clean(&self.fields)
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
            self.tags.replace(SEPARATOR, " "),
            files::name(&self.path)
        )
        .to_lowercase();
        query.iter().all(|word| text.contains(word.as_str()))
    }
}

/// How far a book is read, as filters tell it apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reading {
    /// Never opened.
    Unread,
    /// Opened, not finished.
    Begun,
    Finished,
}

/// A book's number in its series in thousandths, to order a series by
/// (`"2"`, `"2.5"`, `"2,5"`); those without a number last.
pub fn series_order(number: &str) -> u32 {
    let number = number.trim();
    let (whole, part) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let Ok(whole) = whole.parse::<u32>() else {
        return u32::MAX;
    };
    let mut thousandths = 0;
    for (n, digit) in part.bytes().take(3).enumerate() {
        if !digit.is_ascii_digit() {
            break;
        }
        thousandths += u32::from(digit - b'0') * [100, 10, 1][n];
    }
    whole
        .saturating_mul(1000)
        .saturating_add(thousandths)
        .min(u32::MAX - 1)
}

// ---- Reading the catalog ----------------------------------------------

/// The status changes logged since the catalog was made or folded, the
/// last of each book, by its path hashed (FNV-1a, 64 bits); at most
/// [`MAX_LOG`] twice.
pub fn changes(storage: &dyn Storage) -> Vec<(u64, Status)> {
    let mut log: Vec<(u64, Status)> = Vec::new();
    lines(storage, &log_path(), &mut |_, line| {
        let mut columns = line.split('\t');
        let hash = columns.next().and_then(|h| u64::from_str_radix(h, 16).ok());
        let (progress, flags) = (columns.next().unwrap_or(""), columns.next().unwrap_or(""));
        if let Some(hash) = hash {
            let status = Status::parse(progress, flags);
            if let Some(entry) = log.iter_mut().find(|(h, _)| *h == hash) {
                entry.1 = status;
            } else if log.len() < MAX_LOG * 2 {
                log.push((hash, status));
            }
        }
    });
    log
}

/// Calls `f` with each line of the file at `path` and where it starts,
/// reading it in pieces.
fn lines(storage: &dyn Storage, path: &str, f: &mut dyn FnMut(u64, &str)) {
    let mut buf = [0u8; 1024];
    let mut rest: Vec<u8> = Vec::new();
    // Where `rest` starts in the file, and where the next read does.
    let (mut start, mut offset) = (0u64, 0u64);
    while let Ok(n) = storage.read_at(path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        offset += n as u64;
        rest.extend_from_slice(&buf[..n]);
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            f(start, String::from_utf8_lossy(&rest[..end]).trim_end());
            rest.drain(..=end);
            start += end as u64 + 1;
        }
    }
    if !rest.is_empty() {
        f(start, String::from_utf8_lossy(&rest).trim_end());
    }
}

/// Calls `f` with each book of the catalog and where its line starts
/// (to read it again, [`read_at`]), its status as last logged.
pub fn each_at(storage: &dyn Storage, f: &mut dyn FnMut(u64, Record)) {
    let log = changes(storage);
    lines(storage, &catalog_path(), &mut |offset, line| {
        if let Some(mut record) = Record::parse(line) {
            apply(&log, &mut record);
            f(offset, record);
        }
    });
}

/// Calls `f` with each book of the catalog, reading it in pieces.
pub fn each(storage: &dyn Storage, f: &mut dyn FnMut(Record)) {
    each_at(storage, &mut |_, record| f(record));
}

fn apply(log: &[(u64, Status)], record: &mut Record) {
    if log.is_empty() {
        return;
    }
    let hash = fnv1a64(&record.path);
    if let Some((_, status)) = log.iter().find(|(h, _)| *h == hash) {
        record.status = *status;
    }
}

/// The book whose line starts at `offset` in the catalog (as
/// [`each_at`] told), its status as last logged.
pub fn read_at(storage: &dyn Storage, offset: u64) -> Option<Record> {
    let path = catalog_path();
    let mut buf = [0u8; 256];
    let mut bytes = Vec::new();
    let mut at = offset;
    loop {
        let n = storage.read_at(&path, at, &mut buf).ok()?;
        if n == 0 {
            break;
        }
        if let Some(end) = buf[..n].iter().position(|&b| b == b'\n') {
            bytes.extend_from_slice(&buf[..end]);
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        at += n as u64;
        // Values are at most a kilobyte each: a line is never this long.
        if bytes.len() > 16 * 1024 {
            break;
        }
    }
    let mut record = Record::parse(String::from_utf8_lossy(&bytes).trim_end())?;
    // One book's change, found without holding the whole log.
    let hash = fnv1a64(&record.path);
    lines(storage, &log_path(), &mut |_, line| {
        let mut columns = line.split('\t');
        if columns.next().and_then(|h| u64::from_str_radix(h, 16).ok()) == Some(hash) {
            let progress = columns.next().unwrap_or("");
            record.status = Status::parse(progress, columns.next().unwrap_or(""));
        }
    });
    Some(record)
}

/// Whether there is a catalog, and whether it is being made anew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum State {
    /// None was made yet.
    Missing,
    /// None yet, one is being made.
    Building,
    /// There is one; `stamp` changes when it or its log does (views that
    /// keep something of it read it again then).
    Ready { stamp: u64 },
}

/// Whether there is a catalog ([`State`]).
pub fn state(storage: &dyn Storage) -> State {
    match storage.size(&catalog_path()) {
        Ok(size) => {
            let log = storage.size(&log_path()).unwrap_or(0);
            State::Ready {
                stamp: (size << 20) ^ log,
            }
        }
        Err(_) if storage.exists(&building_path()) => State::Building,
        Err(_) => State::Missing,
    }
}

/// Which catalog there is: a number that grows each time it is written
/// anew (made, folded), so that what a view keeps of it (where its lines
/// start, say) is known to be of this one; `None` without a catalog.
pub fn generation(storage: &dyn Storage) -> Option<u64> {
    let mut buf = [0u8; 48];
    let n = storage.read_at(&catalog_path(), 0, &mut buf).ok()?;
    let line = &buf[..n];
    let line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(n)];
    let text = core::str::from_utf8(line).unwrap_or("");
    let Some(rest) = text.strip_prefix(HEADER) else {
        // Of format 1, or without a header: the first.
        return Some(0);
    };
    Some(rest.trim_start_matches('\t').trim().parse().unwrap_or(0))
}

/// How long the catalog is, in bytes.
pub(crate) fn size(storage: &dyn Storage) -> Option<u64> {
    storage.size(&catalog_path()).ok()
}

/// The header of the catalog written next: of the generation after the
/// one there is.
fn next_header(storage: &dyn Storage) -> String {
    let generation = generation(storage).map_or(1, |g| g.wrapping_add(1));
    format!("{HEADER}\t{generation}\n")
}

// ---- Status changes ---------------------------------------------------

/// Notes the status of the book at `path` (how far it is read, favourite,
/// opened) beside the catalog, read with it at once; folds the log into
/// the catalog once it holds [`MAX_LOG`] changes. The kernel calls this
/// from the reading events and the library's setters ([`super::record`],
/// [`super::set_finished`], [`super::set_favourite`]).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn note(storage: &dyn Storage, path: &str, status: Status) {
    noted(storage, path, status);
}

pub(crate) fn noted(storage: &dyn Storage, path: &str, status: Status) {
    if !storage.exists(&catalog_path()) {
        // Made anew from the books' details, which have it.
        return;
    }
    let (progress, flags) = status.columns();
    let line = format!("{:016x}\t{progress}\t{flags}\n", fnv1a64(path));
    let _ = storage.append(&log_path(), line.as_bytes());
    // A line takes some 22 bytes.
    if storage.size(&log_path()).unwrap_or(0) > (MAX_LOG * 22) as u64 {
        folded(storage);
    }
}

/// Writes the logged status changes into the catalog and empties the
/// log: the catalog read and written a piece at a time.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fold(storage: &dyn Storage) {
    folded(storage);
}

fn folded(storage: &dyn Storage) {
    let log = changes(storage);
    if log.is_empty() {
        let _ = storage.remove(&log_path());
        return;
    }
    let folding = folding_path();
    if storage
        .write(&folding, next_header(storage).as_bytes())
        .is_err()
    {
        return;
    }
    let mut piece = String::new();
    let mut failed = false;
    lines(storage, &catalog_path(), &mut |_, line| {
        let Some(mut record) = Record::parse(line) else {
            return;
        };
        apply(&log, &mut record);
        piece.push_str(&record.text());
        if piece.len() > 2048 {
            failed |= storage.append(&folding, piece.as_bytes()).is_err();
            piece.clear();
        }
    });
    failed |= storage.append(&folding, piece.as_bytes()).is_err();
    if failed || files::move_to(storage, &folding, &catalog_path()).is_err() {
        let _ = storage.remove(&folding);
        return;
    }
    let _ = storage.remove(&log_path());
}

// ---- Queries ----------------------------------------------------------

/// Which books a query finds by how far they are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Show {
    #[default]
    All,
    Unread,
    /// Opened, not finished.
    Reading,
    Finished,
    Favourites,
}

/// In what order a query finds books.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sort {
    /// By title, regardless of case.
    #[default]
    Title,
    /// By author, then title.
    Author,
    /// By series, then the number in it.
    Series,
    /// The added last first.
    Added,
}

/// What books to find in the catalog ([`query`]): all of the conditions
/// set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Query {
    pub show: Show,
    /// Words, each in its title, author, series, tags or file name.
    pub words: Vec<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub tag: Option<String>,
    /// Books in this folder or below it.
    pub folder: Option<String>,
    /// Books added on this day or later.
    pub added_since: Option<u32>,
    pub sort: Sort,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(mut self, show: Show) -> Self {
        self.show = show;
        self
    }

    /// The words of `text` (any case).
    pub fn words(mut self, text: &str) -> Self {
        self.words = query_words(text);
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn series(mut self, series: impl Into<String>) -> Self {
        self.series = Some(series.into());
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    pub fn folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = Some(folder.into());
        self
    }

    pub fn added_since(mut self, day: u32) -> Self {
        self.added_since = Some(day);
        self
    }

    pub fn sort(mut self, sort: Sort) -> Self {
        self.sort = sort;
        self
    }

    /// Whether `record` is found.
    pub fn finds(&self, record: &Record) -> bool {
        let shown = match self.show {
            Show::All => true,
            Show::Unread => record.reading() == Reading::Unread,
            Show::Reading => record.reading() == Reading::Begun,
            Show::Finished => record.reading() == Reading::Finished,
            Show::Favourites => record.status.favourite,
        };
        let folder = self.folder.as_deref().map(|f| f.trim_end_matches('/'));
        shown
            && self.author.as_deref().is_none_or(|a| record.author == a)
            && self.series.as_deref().is_none_or(|s| record.series == s)
            && self
                .tag
                .as_deref()
                .is_none_or(|t| record.tags().any(|x| x == t))
            && folder.is_none_or(|f| {
                record
                    .path
                    .strip_prefix(f)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            && self
                .added_since
                .is_none_or(|d| record.added.is_some_and(|a| a >= d))
            && (self.words.is_empty() || record.matches(&self.words))
    }

    /// Where `record` goes in this query's order.
    fn key(&self, record: &Record) -> (u32, String, String) {
        let title = record.title.to_lowercase();
        let (first, text) = match self.sort {
            Sort::Title => (0, title),
            Sort::Author => (0, format!("{}\u{1}{title}", record.author.to_lowercase())),
            Sort::Series => (
                0,
                format!(
                    "{}\u{1}{:010}",
                    record.series.to_lowercase(),
                    series_order(&record.number)
                ),
            ),
            Sort::Added => (u32::MAX - record.added.unwrap_or(0), title),
        };
        (first, text, record.path.clone())
    }
}

/// `n` books `query` finds, in its order, those after `after` (from the
/// first without): one reading of the catalog that holds only `n` books
/// at a time (keyset pagination).
pub fn query(
    storage: &dyn Storage,
    query: &Query,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let after = after.map(|r| query.key(r));
    let mut page: Vec<((u32, String, String), Record)> = Vec::new();
    each(storage, &mut |record| {
        if n == 0 || !query.finds(&record) {
            return;
        }
        let key = query.key(&record);
        if after.as_ref().is_some_and(|a| key <= *a) {
            return;
        }
        let at = page.partition_point(|(k, _)| *k < key);
        if at < n {
            page.insert(at, (key, record));
            page.truncate(n);
        }
    });
    page.into_iter().map(|(_, record)| record).collect()
}

/// How many books `query` finds.
pub fn found(storage: &dyn Storage, query: &Query) -> usize {
    let mut count = 0;
    each(storage, &mut |record| {
        count += usize::from(query.finds(&record))
    });
    count
}

/// `n` records of the catalog that `keep` keeps, by title regardless of
/// case (then by path), those after `after` (from the first without).
pub fn page(
    storage: &dyn Storage,
    keep: &dyn Fn(&Record) -> bool,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let by_title = Query::new();
    let after = after.map(|r| by_title.key(r));
    let mut page: Vec<((u32, String, String), Record)> = Vec::new();
    each(storage, &mut |record| {
        if n == 0 || !keep(&record) {
            return;
        }
        let key = by_title.key(&record);
        if after.as_ref().is_some_and(|a| key <= *a) {
            return;
        }
        let at = page.partition_point(|(k, _)| *k < key);
        if at < n {
            page.insert(at, (key, record));
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

/// Writes `records` as the catalog (emptying the log), e.g. for tests of
/// views.
pub fn write(storage: &dyn Storage, records: &[Record]) {
    let mut text = next_header(storage);
    text.extend(records.iter().map(Record::text));
    let _ = storage.write(&catalog_path(), text.as_bytes());
    let _ = storage.remove(&log_path());
}

// ---- Making the catalog -----------------------------------------------

/// Making the catalog anew: the folders still to walk, and where the
/// current one is. The kernel walks it as a job; views wait for
/// [`crate::builtin::events::CATALOG_CHANGED`].
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
#[derive(Debug, Default)]
pub struct Walk {
    dirs: Vec<String>,
    dir: Option<String>,
    last: Option<Entry>,
    /// Books found so far.
    found: usize,
    /// The paths (hashed) of the favourites and the books opened last.
    favourites: Vec<u64>,
    opened: Vec<u64>,
}

#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
impl Walk {
    /// Starts anew with the books under `root`.
    pub fn start(storage: &dyn Storage, root: &str) -> Walk {
        let _ = storage.write(&building_path(), next_header(storage).as_bytes());
        let hashed = |paths: Vec<String>| paths.iter().map(fnv1a64).collect();
        Walk {
            dirs: Vec::from([root.trim_matches('/').to_string()]),
            favourites: hashed(super::favourites(storage)),
            opened: hashed(super::opened_books(storage)),
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
            // Done: the new catalog replaces the old one. The log stays:
            // what was noted while it was made is read with it (each
            // book's last change is the newest).
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
        let fields: Vec<&str> = env
            .book_fields
            .iter()
            .filter(|f| f.in_catalog)
            .map(|f| f.id.as_str())
            .collect();
        for entry in entries {
            let path = files::join(&dir, &entry.name);
            let mut opened = false;
            if entry.is_dir {
                self.dirs.push(path);
            } else {
                let cached =
                    super::book_sized(env.storage, &path, entry.size).filter(|b| b.indexed);
                let book = cached.unwrap_or_else(|| {
                    opened = true;
                    super::index(env, &path)
                });
                let hash = fnv1a64(&path);
                let status = Status::new(
                    book.progress,
                    self.favourites.contains(&hash),
                    self.opened.contains(&hash),
                );
                let record = Record::of(&path, &book)
                    .with_status(status)
                    .with_fields(&book, &fields);
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
        let book = Book::default()
            .title(Some("Second".into()))
            .author(Some("Ann".into()))
            .series(Some("Tales".into()))
            .series_index(Some("2".into()))
            .tags(alloc::vec!["Kids".into(), "Old\u{1f}times".into()])
            .progress(Some(40))
            .added(Some(20_000))
            .fields(alloc::vec![
                ("stars.rating".into(), "4".into()),
                ("other.x".into(), "y".into()),
            ]);
        let record = Record::of("books/b.epub", &book)
            .with_status(Status::new(Some(40), true, true))
            .with_fields(&book, &["stars.rating"]);
        assert_eq!(record.tags().collect::<Vec<_>>(), ["Kids", "Old times"]);
        assert_eq!(record.field("stars.rating"), Some("4"));
        assert_eq!(record.field("other.x"), None, "not kept in the catalog");
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
        assert_eq!(all[1].reading(), Reading::Unread);
        assert_eq!(all[0].reading(), Reading::Begun);
        assert_eq!(search(&storage, "ann TALES"), [record]);
        assert!(search(&storage, "nothing").is_empty());
    }

    #[test]
    fn a_catalog_of_format_1_is_read() {
        let storage = MemoryStorage::new();
        storage
            .write(
                &catalog_path(),
                b"books/a.epub\tAnn\tFirst\tTales\t1\tKids\nbooks/b.txt\t\tSecond\t\t\t\n",
            )
            .unwrap();
        let mut all = Vec::new();
        each(&storage, &mut |r| all.push(r));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].series, "Tales");
        assert_eq!(all[0].status, Status::default());
        assert_eq!(all[1].title, "Second");
    }

    #[test]
    fn status_changes_are_logged_read_with_the_catalog_and_folded() {
        let storage = MemoryStorage::new();
        let books: Vec<Record> = (0..5)
            .map(|n| Record::of(&format!("books/{n}.txt"), &Book::default()))
            .collect();
        write(&storage, &books);
        note(&storage, "books/2.txt", Status::new(Some(30), false, true));
        note(&storage, "books/2.txt", Status::new(Some(55), true, true));
        note(&storage, "books/4.txt", Status::new(Some(100), false, true));
        let found = |show| found(&storage, &Query::new().show(show));
        assert_eq!(found(Show::Reading), 1);
        assert_eq!(found(Show::Finished), 1);
        assert_eq!(found(Show::Favourites), 1);
        assert_eq!(found(Show::Unread), 3);
        // Read one by where its line starts: with its change.
        let mut at = None;
        each_at(&storage, &mut |offset, r| {
            if r.path == "books/2.txt" {
                at = Some(offset);
            }
        });
        let two = read_at(&storage, at.unwrap()).unwrap();
        assert_eq!(two.status.progress, Some(55));
        assert!(two.status.favourite);
        // Folded: the same, the log gone, a catalog of the next
        // generation.
        let stamp = state(&storage);
        let before = generation(&storage).unwrap();
        fold(&storage);
        assert!(!storage.exists(&log_path()));
        assert_ne!(state(&storage), stamp);
        assert_eq!(generation(&storage), Some(before + 1));
        assert_eq!(found(Show::Reading), 1);
        assert_eq!(found(Show::Finished), 1);
        // Many changes fold by themselves.
        for n in 0..(MAX_LOG * 2) {
            let status = Status::new(Some((n % 100) as u8), false, true);
            note(&storage, &format!("books/{}.txt", n % 5), status);
        }
        assert!(storage.size(&log_path()).unwrap_or(0) <= (MAX_LOG * 22) as u64);
        assert_eq!(found(Show::All), 5);
    }

    #[test]
    fn queries_find_filter_sort_and_page() {
        let storage = MemoryStorage::new();
        let mut records = Vec::new();
        for n in 0..12u32 {
            let book = Book::default()
                .title(Some(format!(
                    "{} Tale {n:02}",
                    if n % 2 == 0 { "a" } else { "B" }
                )))
                .author(Some(String::from(if n < 6 { "Ann" } else { "Bob" })))
                .series(Some(String::from("Tales")))
                .series_index(Some(format!("{}", 12 - n)))
                .added(Some(100 + n));
            let folder = if n % 3 == 0 { "books/Old" } else { "books" };
            records.push(Record::of(&format!("{folder}/{n}.epub"), &book));
        }
        write(&storage, &records);
        let all = |q: &Query| query(&storage, q, None, 100);
        assert_eq!(all(&Query::new().author("Bob")).len(), 6);
        assert_eq!(all(&Query::new().folder("books/Old")).len(), 4);
        assert_eq!(all(&Query::new().folder("books/Ol")).len(), 0);
        assert_eq!(all(&Query::new().added_since(110)).len(), 2);
        assert_eq!(all(&Query::new().words("bob TALE")).len(), 6);
        // A series by its numbers; added last first.
        let series = all(&Query::new().sort(Sort::Series));
        assert_eq!(series[0].number, "1");
        assert_eq!(series[11].number, "12");
        let added = all(&Query::new().sort(Sort::Added));
        assert_eq!(added[0].added, Some(111));
        // Pages of 5 make up the whole, by title.
        let by_title = all(&Query::new());
        let mut seen = Vec::new();
        let mut last: Option<Record> = None;
        loop {
            let page = query(&storage, &Query::new(), last.as_ref(), 5);
            if page.is_empty() {
                break;
            }
            last = page.last().cloned();
            seen.extend(page);
        }
        assert_eq!(seen, by_title);
        assert_eq!(found(&storage, &Query::new().author("Ann")), 6);
    }

    #[test]
    fn series_numbers_order() {
        assert_eq!(series_order("2"), 2000);
        assert_eq!(series_order(" 2.5 "), 2500);
        assert_eq!(series_order("2.25"), 2250);
        assert_eq!(series_order("II"), u32::MAX);
        assert!(series_order("10") > series_order("9"));
    }

    #[test]
    fn its_state() {
        let storage = MemoryStorage::new();
        assert_eq!(state(&storage), State::Missing);
        let walk = Walk::start(&storage, "books");
        assert_eq!(walk.found(), 0);
        assert_eq!(state(&storage), State::Building);
        write(&storage, &[]);
        assert!(matches!(state(&storage), State::Ready { .. }));
    }

    #[test]
    fn searches_page_by_title_holding_a_page() {
        let storage = MemoryStorage::new();
        // Titles in no order, two alike but for case and path.
        let records: Vec<Record> = (0..30)
            .map(|i| {
                let book = Book::default().title(Some(format!(
                    "{}Tale {:02}",
                    if i % 2 == 0 { "a" } else { "A" },
                    (i * 7) % 30
                )));
                Record::of(&format!("books/{i}.epub"), &book)
            })
            .collect();
        write(&storage, &records);
        let by_title = Query::new();
        let mut sorted = records.clone();
        sorted.sort_by_key(|r| by_title.key(r));
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
