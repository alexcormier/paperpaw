//! The calm list's index of the library: every book of the catalog with
//! what it is sorted and filtered by, in 16 bytes each.
//!
//! The catalog (`catalog.txt`, see [`paperpaw_sdk::library::catalog`])
//! names every book under the library folder, but not how far each is
//! read: that is in each book's own details. So the index is made in two
//! parts. When the library opens, one pass over the catalog notes where
//! each book's line starts, a key to sort its title by, the first
//! letters of its title and author, and its place among the books read
//! last (from `opened.txt`). Then, in the background, a few books a step
//! (`on_timer`), each book's progress is read from its details: the books
//! read last first, so "Reading" is right at once. Nothing of this is
//! read while drawing; the lines of the visible books are read again when
//! a page is shown.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::library;

/// Progress not read yet.
pub(crate) const UNKNOWN: u8 = u8::MAX;
/// Not among the books read last.
pub(crate) const NEVER: u8 = u8::MAX;
/// Books indexed at most (as many as a search of the catalog finds).
const MAX_BOOKS: usize = 2000;

/// A book of the catalog, as the index keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Item {
    /// Where its line starts in the catalog.
    pub(crate) offset: u32,
    /// A hash of its path, to find it again ([`hash`]).
    pub(crate) hash: u32,
    /// Its title's first letters, to sort by ([`sort_key`]).
    pub(crate) key: [u8; 4],
    /// The first letter of its title and of its author's surname, in
    /// capitals, `#` for anything else ([`letter`]).
    pub(crate) initials: [u8; 2],
    /// How far it is read in percent (0 never), or [`UNKNOWN`].
    pub(crate) progress: u8,
    /// Its place among the books read last (0 the last), or [`NEVER`].
    pub(crate) recent: u8,
    /// Its series, as an index of [`Index::series`], or [`NO_SERIES`].
    pub(crate) series: u16,
    /// Its number in the series in tenths ([`number`]), to order by.
    pub(crate) number: u16,
}

/// No series.
pub(crate) const NO_SERIES: u16 = u16::MAX;
/// Series told apart at most; books of others count as of none.
const MAX_SERIES: usize = 500;

/// A book's number in its series (`"2"`, `"2.5"`) in tenths, to order
/// by; those without one last.
pub(crate) fn number(text: &str) -> u16 {
    let text = text.trim();
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    let tenth = part
        .bytes()
        .next()
        .filter(u8::is_ascii_digit)
        .map_or(0, |d| d - b'0');
    match whole.parse::<u16>() {
        Ok(whole) => whole
            .saturating_mul(10)
            .saturating_add(u16::from(tenth))
            .min(u16::MAX - 1),
        Err(_) => u16::MAX,
    }
}

/// What the library shows: all books, those by how far they are read,
/// the next book of each series being read, or the shelves (favourites,
/// series, folders).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    Reading,
    Unread,
    Finished,
    /// For each series begun, its unread book of the lowest number.
    Next,
    Shelves,
}

impl Filter {
    /// The filters of the pills, in their order ([`Filter::Next`] is
    /// on the shelves there).
    pub(crate) const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Reading,
        Filter::Unread,
        Filter::Finished,
        Filter::Shelves,
    ];

    /// The translation key of its name.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Filter::All => "files.list.filter.all",
            Filter::Reading => "files.list.filter.reading",
            Filter::Unread => "files.list.filter.unread",
            Filter::Finished => "files.list.filter.finished",
            Filter::Next => "files.list.filter.next",
            Filter::Shelves => "files.list.filter.shelves",
        }
    }

    /// The filter `steps` pills on, round at the ends.
    pub(crate) fn step(self, steps: isize) -> Filter {
        let at = Self::ALL.iter().position(|f| *f == self).unwrap_or(0) as isize;
        let len = Self::ALL.len() as isize;
        Self::ALL[(at + steps).rem_euclid(len) as usize]
    }

    /// Whether a book read `progress` percent is shown.
    fn shows(self, progress: u8) -> bool {
        match self {
            Filter::All => true,
            Filter::Reading => (1..100).contains(&progress),
            Filter::Unread => progress == 0,
            Filter::Finished => progress == 100,
            Filter::Next | Filter::Shelves => false,
        }
    }
}

/// A line of the catalog: a book with what the library knows of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) path: String,
    pub(crate) author: String,
    pub(crate) title: String,
    pub(crate) series: String,
    /// Its number in the series.
    pub(crate) number: String,
    /// Its tags, separated by U+001F (as the catalog keeps them).
    pub(crate) tags: String,
}

impl Line {
    /// The line `text` (tab separated: path, author, title, series,
    /// number, tags).
    pub(crate) fn parse(text: &str) -> Option<Line> {
        let mut fields = text.split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_owned();
        let mut next = || fields.next().unwrap_or("").to_owned();
        Some(Line {
            path,
            author: next(),
            title: next(),
            series: next(),
            number: next(),
            tags: next(),
        })
    }
}

fn catalog_path() -> String {
    format!("{}/catalog.txt", library::DIR)
}

/// Whether there is a catalog at all (none before it was first made).
pub(crate) fn has_catalog(storage: &dyn Storage) -> bool {
    storage.exists(&catalog_path())
}

/// Calls `f` with each line of the catalog and where it starts, reading
/// it a kilobyte at a time.
pub(crate) fn scan(storage: &dyn Storage, f: &mut dyn FnMut(u32, Line)) {
    let path = catalog_path();
    let mut buf = [0u8; 1024];
    let mut rest: Vec<u8> = Vec::new();
    // Where `rest` starts in the file, and where the next read does.
    let (mut start, mut offset) = (0u64, 0u64);
    while let Ok(n) = storage.read_at(&path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        offset += n as u64;
        rest.extend_from_slice(&buf[..n]);
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            if let Some(line) = Line::parse(String::from_utf8_lossy(&rest[..end]).trim_end()) {
                f(start as u32, line);
            }
            rest.drain(..=end);
            start += end as u64 + 1;
        }
    }
    if let Some(line) = Line::parse(String::from_utf8_lossy(&rest).trim_end()) {
        f(start as u32, line);
    }
}

/// The line of the catalog starting at `offset`.
pub(crate) fn read_line(storage: &dyn Storage, offset: u32) -> Option<Line> {
    let path = catalog_path();
    let mut buf = [0u8; 256];
    let mut bytes = Vec::new();
    let mut at = u64::from(offset);
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
    Line::parse(String::from_utf8_lossy(&bytes).trim_end())
}

/// A hash of `path` (FNV-1a), to find a book of the index by its path.
pub(crate) fn hash(path: &str) -> u32 {
    path.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

/// `title` without a leading article, as libraries sort it ("The Hound
/// of the Baskervilles" under H).
fn without_article(title: &str) -> &str {
    let title = title.trim_start();
    for article in ["the ", "a ", "an ", "der ", "die ", "das "] {
        if title.len() > article.len()
            && title.is_char_boundary(article.len())
            && title[..article.len()].eq_ignore_ascii_case(article)
        {
            return title[article.len()..].trim_start();
        }
    }
    title
}

/// `c` as a small ASCII letter or digit to sort by: accents dropped,
/// other letters after `z`; `None` for what is not sorted by (marks).
fn fold(c: char) -> Option<u8> {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'a'..='z' | '0'..='9' => Some(c as u8),
        ' ' => Some(b' '),
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => Some(b'a'),
        'ç' => Some(b'c'),
        'è' | 'é' | 'ê' | 'ë' => Some(b'e'),
        'ì' | 'í' | 'î' | 'ï' => Some(b'i'),
        'ñ' => Some(b'n'),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => Some(b'o'),
        'ù' | 'ú' | 'û' | 'ü' => Some(b'u'),
        'ß' => Some(b's'),
        c if c.is_alphabetic() => Some(b'~'),
        _ => None,
    }
}

/// The first four letters of `title` to sort by, without a leading
/// article; titles with the same four are kept in the catalog's order.
pub(crate) fn sort_key(title: &str) -> [u8; 4] {
    let mut key = [0u8; 4];
    for (slot, byte) in key
        .iter_mut()
        .zip(without_article(title).chars().filter_map(fold))
    {
        *slot = byte;
    }
    key
}

/// The capital `text` is listed under: its first letter without accents,
/// `#` for a digit or anything else.
pub(crate) fn letter(text: &str) -> u8 {
    match text.chars().find_map(fold) {
        Some(b @ b'a'..=b'z') => b.to_ascii_uppercase(),
        _ => b'#',
    }
}

/// The surname of the first of `authors`: before the comma of "Doyle,
/// Arthur Conan", else the last word.
pub(crate) fn surname(authors: &str) -> &str {
    let first = authors.split([';', '&']).next().unwrap_or("").trim();
    match first.split_once(',') {
        Some((surname, _)) => surname.trim(),
        None => first.rsplit(' ').next().unwrap_or(first),
    }
}

/// The index; see the [module documentation](self).
#[derive(Debug, Default)]
pub(crate) struct Index {
    pub(crate) items: Vec<Item>,
    /// The names of the series, as [`Item::series`] counts them.
    pub(crate) series: Vec<String>,
    /// Items whose progress is still to be read, the next one last.
    pending: Vec<u16>,
}

impl Index {
    /// Makes the index from the catalog (progress still unknown).
    pub(crate) fn build(storage: &dyn Storage) -> Index {
        let opened: Vec<u32> = library::opened_books(storage)
            .iter()
            .map(|p| hash(p))
            .collect();
        let mut items = Vec::new();
        let mut series: Vec<String> = Vec::new();
        scan(storage, &mut |offset, line| {
            if items.len() >= MAX_BOOKS {
                return;
            }
            let hash = hash(&line.path);
            let of = if line.series.is_empty() {
                NO_SERIES
            } else if let Some(at) = series.iter().position(|s| *s == line.series) {
                at as u16
            } else if series.len() < MAX_SERIES {
                series.push(line.series.clone());
                (series.len() - 1) as u16
            } else {
                NO_SERIES
            };
            items.push(Item {
                offset,
                hash,
                key: sort_key(&line.title),
                initials: [
                    letter(without_article(&line.title)),
                    letter(surname(&line.author)),
                ],
                progress: UNKNOWN,
                recent: rank(&opened, hash),
                series: of,
                number: number(&line.number),
            });
        });
        let mut index = Index {
            items,
            series,
            pending: Vec::new(),
        };
        index.read_again();
        index
    }

    /// Reads every book's progress again, in the background
    /// ([`Index::step`]): the books read last first.
    pub(crate) fn read_again(&mut self) {
        let mut pending: Vec<u16> = (0..self.items.len() as u16).collect();
        // Popped from the end: the last read last in the list.
        pending.sort_by_key(|&i| core::cmp::Reverse(self.items[usize::from(i)].recent));
        self.pending = pending;
    }

    /// Takes the order of the books read last anew (after reading).
    pub(crate) fn rerank(&mut self, storage: &dyn Storage) {
        let opened: Vec<u32> = library::opened_books(storage)
            .iter()
            .map(|p| hash(p))
            .collect();
        for item in &mut self.items {
            item.recent = rank(&opened, item.hash);
        }
    }

    /// Reads the progress of up to `count` books; `true` when every book's
    /// is known.
    pub(crate) fn step(&mut self, storage: &dyn Storage, count: usize) -> bool {
        for _ in 0..count {
            let Some(i) = self.pending.pop() else {
                break;
            };
            let item = &mut self.items[usize::from(i)];
            let progress = read_line(storage, item.offset)
                .and_then(|line| library::book(storage, &line.path))
                .and_then(|book| book.progress)
                .unwrap_or(0);
            item.progress = progress;
        }
        self.pending.is_empty()
    }

    /// How many books' progress is still to be read.
    pub(crate) fn unknown(&self) -> usize {
        self.pending.len()
    }

    /// Notes the progress of the book whose path hashes to `hash`, e.g.
    /// as a page of the list read it.
    pub(crate) fn set_progress(&mut self, hash: u32, progress: u8) {
        if let Some(item) = self.items.iter_mut().find(|i| i.hash == hash) {
            item.progress = progress;
        }
    }

    /// The books `filter` shows, as indices of [`Index::items`]: the books
    /// read last first (`by_recent`), the others by title. Books whose
    /// progress is not known yet are in "All" only.
    pub(crate) fn order(&self, filter: Filter, by_recent: bool) -> Vec<u16> {
        if filter == Filter::Next {
            return self.next_in_series(by_recent);
        }
        let mut order: Vec<u16> = (0..self.items.len() as u16)
            .filter(|&i| {
                let progress = self.items[usize::from(i)].progress;
                filter == Filter::All || (progress != UNKNOWN && filter.shows(progress))
            })
            .collect();
        order.sort_by_key(|&i| {
            let item = &self.items[usize::from(i)];
            let recent = if by_recent { item.recent } else { NEVER };
            (recent, item.key, item.offset)
        });
        order
    }
}

impl Index {
    /// For each series begun (a book of it opened or read), its unread
    /// book of the lowest number never opened: the series read last
    /// first (`by_recent`), else by title.
    fn next_in_series(&self, by_recent: bool) -> Vec<u16> {
        let count = self.series.len();
        // Per series: when a book of it was read last (or never), and
        // its next book.
        let mut last = alloc::vec![None::<u8>; count];
        let mut next: Vec<Option<u16>> = alloc::vec![None; count];
        for (i, item) in self.items.iter().enumerate() {
            let at = usize::from(item.series);
            if at >= count {
                continue;
            }
            let begun = item.recent != NEVER || (1..=100).contains(&item.progress);
            if begun {
                last[at] = Some(last[at].map_or(item.recent, |l| l.min(item.recent)));
            } else if item.progress == 0 {
                let better = next[at].is_none_or(|n| {
                    let other = &self.items[usize::from(n)];
                    (item.number, item.key) < (other.number, other.key)
                });
                if better {
                    next[at] = Some(i as u16);
                }
            }
        }
        let mut order: Vec<u16> = (0..count)
            .filter(|&s| last[s].is_some())
            .filter_map(|s| next[s])
            .collect();
        order.sort_by_key(|&i| {
            let item = &self.items[usize::from(i)];
            let recent = match last.get(usize::from(item.series)) {
                Some(Some(recent)) if by_recent => *recent,
                _ => NEVER,
            };
            (recent, item.key, item.offset)
        });
        order
    }

    /// `order` with the books of each series as one entry (a stack) at
    /// the place of its first book there: the entries, and how many books
    /// each stands for. Books being read stay on their own, to go on
    /// reading them at once.
    pub(crate) fn stacked(&self, order: &[u16]) -> (Vec<u16>, Vec<u16>) {
        let mut entries: Vec<u16> = Vec::new();
        let mut counts: Vec<u16> = Vec::new();
        // Where each series' stack is among the entries.
        let mut stacks: Vec<(u16, usize)> = Vec::new();
        for &i in order {
            let item = &self.items[usize::from(i)];
            let series = if (1..100).contains(&item.progress) {
                NO_SERIES
            } else {
                item.series
            };
            if series != NO_SERIES
                && let Some(&(_, at)) = stacks.iter().find(|(s, _)| *s == series)
            {
                counts[at] += 1;
                continue;
            }
            if series != NO_SERIES {
                stacks.push((series, entries.len()));
            }
            entries.push(i);
            counts.push(1);
        }
        (entries, counts)
    }
}

/// The place of the book hashing to `hash` among the books read last.
fn rank(opened: &[u32], hash: u32) -> u8 {
    opened
        .iter()
        .position(|o| *o == hash)
        .map_or(NEVER, |r| r.min(usize::from(NEVER) - 1) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_sort_without_articles() {
        assert_eq!(&sort_key("The Hound of the Baskervilles"), b"houn");
        assert_eq!(&sort_key("Émile"), b"emil");
        assert_eq!(&sort_key("A"), b"a\0\0\0");
        assert_eq!(letter(without_article("Die Verwandlung")), b'V');
        assert_eq!(letter("1984"), b'#');
    }

    #[test]
    fn surnames() {
        assert_eq!(surname("Arthur Conan Doyle"), "Doyle");
        assert_eq!(surname("Doyle, Arthur Conan"), "Doyle");
        assert_eq!(surname("Ann Smith & Bob Jones"), "Smith");
        assert_eq!(surname(""), "");
    }

    #[test]
    fn lines_parse() {
        let line = Line::parse("b/x.epub\tAnn\tTitle\tTales\t2\tKids").unwrap();
        assert_eq!(line.series, "Tales");
        assert_eq!(line.number, "2");
        assert!(Line::parse("").is_none());
    }

    #[test]
    fn filters_go_round() {
        assert_eq!(Filter::All.step(-1), Filter::Shelves);
        assert_eq!(Filter::Shelves.step(1), Filter::All);
        assert!(Filter::Reading.shows(42));
        assert!(!Filter::Unread.shows(100));
    }
}
