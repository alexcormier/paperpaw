//! What the wall knows of every book, held small: where its line of the
//! catalog starts, its folder and series, where it sorts by title, when it
//! was read last and its state (new, reading, finished, favourite).
//!
//! Made in the background ([`Build`]), a few books a step, from the
//! catalog (`catalog.txt`) and each book's details: the details are files
//! of their own, so a book's state costs a small read once, never while
//! drawing. Titles, authors and paths are not held; the books shown are
//! read from their lines of the catalog by offset when a page is loaded
//! ([`record_at`]). 20 bytes a book are held.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::library;
use paperpaw_sdk::{Env, Storage, files};

/// No folder, series or number.
pub(crate) const NONE: u16 = u16::MAX;
/// Not among the books read lately.
pub(crate) const UNRANKED: u8 = u8::MAX;

/// A book's state, in the low bits of [`Line::state`]: never opened,
/// being read, read to the end.
pub(crate) const NEW: u8 = 1;
pub(crate) const READING: u8 = 2;
pub(crate) const FINISHED: u8 = 3;
const STATE: u8 = 3;
/// A favourite (a bit of its own beside the state).
pub(crate) const FAVOURITE: u8 = 4;

/// Books whose details are read in one step of [`Build`].
const STEP_BOOKS: usize = 6;
/// Bytes of the catalog read at a time.
const CHUNK: usize = 1024;

/// The catalog's file (as `library::catalog` keeps it).
pub(crate) fn catalog_path() -> String {
    alloc::format!("{}/catalog.txt", library::DIR)
}

/// A short hash of a book's path, to find it among the books read lately
/// and the favourites without holding paths.
pub(crate) fn key(path: &str) -> u32 {
    path.bytes().fold(0x811c_9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Where a title sorts: its first four letters or digits in small
/// letters, after an article ("The Hound" sorts as "houn"), as a number.
pub(crate) fn sort_key(title: &str) -> u32 {
    let lower = title.trim().to_lowercase();
    let mut rest = lower.as_str();
    for article in ["the ", "a ", "an ", "der ", "die ", "das "] {
        if let Some(after) = rest.strip_prefix(article)
            && !after.trim().is_empty()
        {
            rest = after;
            break;
        }
    }
    let mut key = 0u32;
    let mut taken = 0;
    for c in rest.chars().filter(|c| c.is_alphanumeric()) {
        let byte = if c.is_ascii() { c as u8 } else { b'z' + 1 };
        key = (key << 8) | u32::from(byte);
        taken += 1;
        if taken == 4 {
            break;
        }
    }
    key << (8 * (4 - taken))
}

/// The letter a sort key starts with, `#` for digits and other letters.
pub(crate) fn initial(sort: u32) -> u8 {
    match (sort >> 24) as u8 {
        c @ b'a'..=b'z' => c.to_ascii_uppercase(),
        _ => b'#',
    }
}

/// One book of the catalog, as held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Line {
    /// [`key`] of its path.
    pub(crate) key: u32,
    /// Where its line of the catalog starts.
    pub(crate) offset: u32,
    pub(crate) sort: u32,
    /// Its folder ([`Index::dirs`]).
    pub(crate) dir: u16,
    /// Its series ([`Index::series`]), or [`NONE`].
    pub(crate) series: u16,
    /// Its number in the series, in tenths, or [`NONE`].
    pub(crate) number: u16,
    /// Its place among the books read lately (0 the last), or
    /// [`UNRANKED`].
    pub(crate) rank: u8,
    pub(crate) state: u8,
}

impl Line {
    /// [`NEW`], [`READING`] or [`FINISHED`].
    pub(crate) fn reading(&self) -> u8 {
        self.state & STATE
    }

    pub(crate) fn favourite(&self) -> bool {
        self.state & FAVOURITE != 0
    }
}

/// Every book of the library, as held (see the module documentation).
#[derive(Debug, Default)]
pub(crate) struct Index {
    pub(crate) lines: Vec<Line>,
    /// The folders books are in.
    pub(crate) dirs: Vec<String>,
    /// The names of the series.
    pub(crate) series: Vec<String>,
    /// [`key`]s of the books read lately, the last first.
    pub(crate) opened: Vec<u32>,
    /// [`key`]s of the favourites, the newest first.
    pub(crate) favourites: Vec<u32>,
}

impl Index {
    /// The index of folder `dir`, added if new.
    pub(crate) fn dir(&mut self, dir: &str) -> u16 {
        index_of(&mut self.dirs, dir)
    }

    /// Reads which books were read lately and which are favourites again
    /// (e.g. back from a book), and places every book anew.
    pub(crate) fn rerank(&mut self, storage: &dyn Storage) {
        self.opened = library::opened_books(storage)
            .iter()
            .map(|p| key(p))
            .collect();
        self.favourites = library::favourites(storage)
            .iter()
            .map(|p| key(p))
            .collect();
        for line in &mut self.lines {
            line.rank = rank_of(&self.opened, line.key);
            line.state &= !FAVOURITE;
            if self.favourites.contains(&line.key) {
                line.state |= FAVOURITE;
            }
        }
    }

    /// Reads the state of the book with path `path` again, e.g. after it
    /// was marked finished.
    pub(crate) fn refresh(&mut self, storage: &dyn Storage, path: &str) {
        let key = key(path);
        if let Some(line) = self.lines.iter_mut().find(|l| l.key == key) {
            line.state = (line.state & FAVOURITE) | state_of(storage, path);
        }
    }

    /// The position of the favourite `line` among the favourites.
    pub(crate) fn favourite_rank(&self, line: &Line) -> usize {
        self.favourites
            .iter()
            .position(|k| *k == line.key)
            .unwrap_or(usize::MAX)
    }
}

fn index_of(names: &mut Vec<String>, name: &str) -> u16 {
    match names.iter().position(|n| n == name) {
        Some(i) => i as u16,
        None if names.len() < usize::from(NONE) => {
            names.push(name.to_string());
            (names.len() - 1) as u16
        }
        None => NONE,
    }
}

fn rank_of(opened: &[u32], key: u32) -> u8 {
    opened
        .iter()
        .position(|k| *k == key)
        .map_or(UNRANKED, |r| r.min(254) as u8)
}

/// [`NEW`], [`READING`] or [`FINISHED`], from the book's details.
fn state_of(storage: &dyn Storage, path: &str) -> u8 {
    match library::book(storage, path).and_then(|b| b.progress) {
        Some(100) => FINISHED,
        Some(p) if p > 0 => READING,
        _ => NEW,
    }
}

/// A line of the catalog: path, author, title, series and number (the
/// tags after them are not needed here).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) path: String,
    pub(crate) author: String,
    pub(crate) title: String,
    pub(crate) series: String,
    pub(crate) number: String,
}

impl Record {
    fn parse(text: &str) -> Option<Record> {
        let mut fields = text.trim_end().split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_string();
        let mut next = || fields.next().unwrap_or("").to_string();
        Some(Record {
            path,
            author: next(),
            title: next(),
            series: next(),
            number: next(),
        })
    }
}

/// The book whose line of the catalog starts at `offset`.
pub(crate) fn record_at(storage: &dyn Storage, offset: u32) -> Option<Record> {
    let mut buf = [0u8; CHUNK];
    let n = storage
        .read_at(&catalog_path(), u64::from(offset), &mut buf)
        .ok()?;
    let line = &buf[..n];
    let end = line.iter().position(|b| *b == b'\n').unwrap_or(n);
    Record::parse(&String::from_utf8_lossy(&line[..end]))
}

/// Making the [`Index`]: the catalog read a piece at a time, the details
/// of a few books a step.
#[derive(Debug, Default)]
pub(crate) struct Build {
    pub(crate) index: Index,
    /// Where `rest` starts in the catalog.
    offset: u64,
    /// Read but not yet taken.
    rest: Vec<u8>,
    /// The catalog was read to its end.
    end: bool,
}

impl Build {
    pub(crate) fn start(storage: &dyn Storage) -> Build {
        let mut build = Build::default();
        build.index.rerank(storage);
        build
    }

    /// Books found so far.
    pub(crate) fn found(&self) -> usize {
        self.index.lines.len()
    }

    /// One step; `true` when the index is complete.
    pub(crate) fn step(&mut self, env: &Env<'_>) -> bool {
        let mut taken = 0;
        while taken < STEP_BOOKS {
            if let Some(end) = self.rest.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.rest.drain(..=end).collect();
                let offset = self.offset;
                self.offset += line.len() as u64;
                taken += usize::from(self.add(env, offset, &line));
                continue;
            }
            if self.end {
                let line = core::mem::take(&mut self.rest);
                let offset = self.offset;
                self.add(env, offset, &line);
                return true;
            }
            let mut buf = [0u8; CHUNK];
            let at = self.offset + self.rest.len() as u64;
            match env.storage.read_at(&catalog_path(), at, &mut buf) {
                Ok(n) if n > 0 => self.rest.extend_from_slice(&buf[..n]),
                _ => self.end = true,
            }
        }
        false
    }

    /// Takes the book of one line of the catalog; `false` if there is
    /// none (or its file is gone).
    fn add(&mut self, env: &Env<'_>, offset: u64, line: &[u8]) -> bool {
        let Some(record) = Record::parse(&String::from_utf8_lossy(line)) else {
            return false;
        };
        let storage = env.storage;
        let Ok(offset) = u32::try_from(offset) else {
            return false;
        };
        if storage.size(&record.path).is_err() {
            return false;
        }
        let index = &mut self.index;
        let key = key(&record.path);
        let dir = index.dir(files::parent(&record.path));
        let series = if record.series.is_empty() {
            NONE
        } else {
            index_of(&mut index.series, &record.series)
        };
        let number = record
            .number
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|n| *n >= 0.0)
            .map_or(NONE, |n| {
                ((n * 10.0) as u32).min(u32::from(NONE) - 1) as u16
            });
        let mut state = state_of(storage, &record.path);
        if index.favourites.contains(&key) {
            state |= FAVOURITE;
        }
        index.lines.push(Line {
            key,
            offset,
            sort: sort_key(&record.title),
            dir,
            series,
            number,
            rank: rank_of(&index.opened, key),
            state,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_sort_without_articles() {
        assert_eq!(initial(sort_key("The Hound of the Baskervilles")), b'H');
        assert_eq!(initial(sort_key("Die Verwandlung")), b'V');
        assert_eq!(initial(sort_key("A")), b'A', "an article alone is a title");
        assert_eq!(initial(sort_key("1984")), b'#');
        assert!(sort_key("Emma") < sort_key("Frankenstein"));
        assert!(sort_key("Ozma of Oz") < sort_key("Persuasion"));
    }

    #[test]
    fn lines_are_read_back() {
        let record = Record::parse("b/a.epub\tAnn\tFirst\tTales\t2\tKids\n").unwrap();
        assert_eq!(record.title, "First");
        assert_eq!(record.number, "2");
        assert!(Record::parse("\tno path").is_none());
    }
}
