//! An index of the library for views of every book (a list, a grid): each
//! book of the catalog with what it is sorted and filtered by, in 24
//! bytes, made in one pass over the catalog ([`Index::build`]). Its
//! books' lines are read again by where they start ([`Index::record`])
//! for the page a view shows, never while drawing.
//!
//! On devices of little memory it holds [`cap`] books at most and counts
//! the others ([`Index::total`]). A view covered by a book can keep it in
//! a file ([`Index::save`]) and let it go, and read it back when it shows
//! again ([`Index::load`]).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::catalog::{self, Record, Show};
use crate::{MemoryClass, Storage};

/// Not among the books read last ([`Item::recent`]).
pub const NEVER: u8 = u8::MAX;
/// Never opened ([`Item::progress`]).
pub const NONE: u8 = u8::MAX;
/// No series ([`Item::series`]).
pub const NO_SERIES: u16 = u16::MAX;
/// Letters of a title kept to sort by ([`sort_key`]).
pub const KEY: usize = 8;
/// Books indexed at most on devices of little memory (the X4: 48 KB),
/// and on others.
const MAX_BOOKS_SMALL: usize = 2000;
const MAX_BOOKS: usize = 20_000;
/// Series told apart at most; books of others count as of none.
const MAX_SERIES: usize = 500;
/// The kept index's first bytes: what it is, and its version.
const MAGIC: &[u8; 4] = b"PPI3";
/// Bytes of an item kept.
const ITEM_BYTES: usize = 23;
/// Bytes of the kept index's head: what it is, the catalog's state, the
/// archive (hashed), how many books are in it, in the catalog and
/// indexed, how many series.
const HEAD: usize = 30;

/// The most books an index holds on a device of `memory`.
pub fn cap(memory: MemoryClass) -> usize {
    if memory == MemoryClass::Small {
        MAX_BOOKS_SMALL
    } else {
        MAX_BOOKS
    }
}

/// A book of the catalog, as the index keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    /// Where its line starts in the catalog ([`catalog::read_at`]).
    pub offset: u32,
    /// A hash of its path, to find it again ([`hash`]).
    pub hash: u32,
    /// Its title's first letters, to sort by ([`sort_key`]).
    pub key: [u8; KEY],
    /// Its series, as an index of [`Index::series`], or [`NO_SERIES`].
    pub series: u16,
    /// Its number in the series in tenths ([`number`]), to order by.
    pub number: u16,
    /// How far it is read in percent, or [`NONE`] never opened.
    pub progress: u8,
    /// Its place among the books read last (0 the last), or [`NEVER`].
    pub recent: u8,
    /// [`FAVOURITE`], [`OPENED`].
    pub flags: u8,
}

/// A favourite ([`Item::flags`]).
pub const FAVOURITE: u8 = 1;
/// Opened once ([`Item::flags`]).
pub const OPENED: u8 = 2;

impl Item {
    /// Whether `show` shows it.
    pub fn shown(&self, show: Show) -> bool {
        let opened = self.flags & OPENED != 0 || self.progress != NONE;
        match show {
            Show::Unread => !opened,
            Show::Reading => opened && self.progress != 100,
            Show::Finished => self.progress == 100,
            Show::Favourites => self.flags & FAVOURITE != 0,
            _ => true,
        }
    }
}

/// A book's number in its series (`"2"`, `"2.5"`) in tenths, to order
/// by; those without one last.
pub fn number(text: &str) -> u16 {
    match catalog::series_order(text) {
        u32::MAX => u16::MAX,
        thousandths => (thousandths / 100).min(u32::from(u16::MAX) - 1) as u16,
    }
}

/// A hash of `path`, to find a book of the index by its path: the low
/// bits of the one the catalog's log keys changes by
/// ([`catalog::changes`]).
pub fn hash(path: &str) -> u32 {
    paperpaw_digest::fnv1a64(path) as u32
}

/// `title` without a leading article, as libraries sort it ("The Hound
/// of the Baskervilles" under H).
pub fn without_article(title: &str) -> &str {
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

/// The first letters of `title` to sort by ([`KEY`]), without a leading
/// article; titles with the same first letters are kept in the
/// catalog's order.
pub fn sort_key(title: &str) -> [u8; KEY] {
    let mut key = [0u8; KEY];
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
pub fn letter(text: &str) -> u8 {
    match text.chars().find_map(fold) {
        Some(b @ b'a'..=b'z') => b.to_ascii_uppercase(),
        _ => b'#',
    }
}

/// The surname of the first of `authors`: before the comma of "Doyle,
/// Arthur Conan", else the last word.
pub fn surname(authors: &str) -> &str {
    let first = authors.split([';', '&']).next().unwrap_or("").trim();
    match first.split_once(',') {
        Some((surname, _)) => surname.trim(),
        None => first.rsplit(' ').next().unwrap_or(first),
    }
}

/// The index; see the [module documentation](self).
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Index {
    pub items: Vec<Item>,
    /// The names of the series, as [`Item::series`] counts them.
    pub series: Vec<String>,
    /// Books in the catalog (but the archive's): more than
    /// [`Index::items`] where some were left out.
    pub total: usize,
    /// Books in the archive folder, left out.
    pub archived: usize,
    /// Changed since it was kept in a file ([`Index::save`]).
    pub dirty: bool,
}

impl Index {
    /// Whether there are books at all (in the archive too).
    pub fn has_books(&self) -> bool {
        !self.items.is_empty() || self.archived > 0
    }

    /// Makes the index from the catalog: at most `cap` books, those in the
    /// folder `archive` left out (counted).
    pub fn build(storage: &dyn Storage, cap: usize, archive: &str) -> Index {
        let archive = if archive.is_empty() {
            String::new()
        } else {
            format!("{archive}/")
        };
        let opened = opened(storage);
        // As many as there are books: a list grown by doubling would hold
        // twice that for a moment.
        let lines = count_lines(storage);
        let mut items = Vec::with_capacity(lines.min(cap));
        let mut series: Vec<String> = Vec::new();
        let mut archived = 0;
        catalog::each_at(storage, &mut |offset, record| {
            if !archive.is_empty() && record.path.starts_with(&archive) {
                archived += 1;
                return;
            }
            if items.len() >= cap {
                return;
            }
            let of = if record.series.is_empty() {
                NO_SERIES
            } else if let Some(at) = series.iter().position(|s| *s == record.series) {
                at as u16
            } else if series.len() < MAX_SERIES {
                series.push(record.series.clone());
                (series.len() - 1) as u16
            } else {
                NO_SERIES
            };
            items.push(item(offset, &record, of, &opened));
        });
        // The catalog's lines but its header.
        let total = lines.saturating_sub(1).saturating_sub(archived);
        Index {
            total: total.max(items.len()),
            archived,
            items,
            series,
            dirty: true,
        }
    }

    /// The book of item `i`, read from the catalog.
    pub fn record(&self, storage: &dyn Storage, i: usize) -> Option<Record> {
        catalog::read_at(storage, u64::from(self.items.get(i)?.offset))
    }

    /// Takes the order of the books read last, and the status changes
    /// logged beside the catalog (how far read, favourite), anew: after
    /// reading, a few short files read, not the catalog.
    pub fn refresh(&mut self, storage: &dyn Storage) {
        let opened = opened(storage);
        let changes = catalog::changes(storage);
        for item in &mut self.items {
            let recent = rank(&opened, item.hash);
            let (mut progress, mut flags) = (item.progress, item.flags);
            if let Some((_, status)) = changes.iter().find(|(h, _)| *h as u32 == item.hash) {
                progress = status.progress.unwrap_or(NONE);
                flags =
                    (u8::from(status.favourite) * FAVOURITE) | (u8::from(status.opened) * OPENED);
            }
            self.dirty |= (item.recent, item.progress, item.flags) != (recent, progress, flags);
            (item.recent, item.progress, item.flags) = (recent, progress, flags);
        }
    }

    /// The books `show` shows, as indices of [`Index::items`]: the books
    /// read last first (`by_recent`), the others by title.
    pub fn order(&self, show: Show, by_recent: bool) -> Vec<u16> {
        let mut order: Vec<u16> = (0..self.items.len() as u16)
            .filter(|&i| self.items[usize::from(i)].shown(show))
            .collect();
        order.sort_by_key(|&i| {
            let item = &self.items[usize::from(i)];
            let recent = if by_recent { item.recent } else { NEVER };
            (recent, item.key, item.offset)
        });
        order
    }

    /// For each series begun (a book of it opened or read), its unread
    /// book of the lowest number never opened: the series read last
    /// first (`by_recent`), else by title.
    pub fn next_in_series(&self, by_recent: bool) -> Vec<u16> {
        let count = self.series.len();
        // Per series: when a book of it was read last, and its next book.
        let mut last = alloc::vec![None::<u8>; count];
        let mut next: Vec<Option<u16>> = alloc::vec![None; count];
        for (i, item) in self.items.iter().enumerate() {
            let at = usize::from(item.series);
            if at >= count {
                continue;
            }
            if !item.shown(Show::Unread) {
                last[at] = Some(last[at].map_or(item.recent, |l| l.min(item.recent)));
            } else {
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
    /// each stands for. Books being read stay on their own.
    pub fn stacked(&self, order: &[u16]) -> (Vec<u16>, Vec<u16>) {
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

    /// Notes the progress of the book whose path hashes to `hash`, e.g.
    /// as a page of a view read it.
    pub fn set_progress(&mut self, hash: u32, progress: Option<u8>) {
        let progress = progress.unwrap_or(NONE);
        if let Some(item) = self.items.iter_mut().find(|i| i.hash == hash)
            && item.progress != progress
        {
            item.progress = progress;
            self.dirty = true;
        }
    }

    /// Keeps the index in the file `file`, to be read again
    /// ([`Index::load`]) instead of made anew, so a view holds no memory
    /// for it meanwhile; `archive` is the folder it was made without.
    /// Written a piece at a time; `false` where it could not be.
    pub fn save(&self, storage: &dyn Storage, file: &str, archive: &str) -> bool {
        let Some(stamp) = made(storage) else {
            return false;
        };
        let mut head = Vec::with_capacity(HEAD);
        head.extend_from_slice(MAGIC);
        head.extend_from_slice(&stamp.to_le_bytes());
        head.extend_from_slice(&hash(archive).to_le_bytes());
        head.extend_from_slice(&(self.archived as u32).to_le_bytes());
        head.extend_from_slice(&(self.total as u32).to_le_bytes());
        head.extend_from_slice(&(self.items.len() as u32).to_le_bytes());
        head.extend_from_slice(&(self.series.len() as u16).to_le_bytes());
        if storage.write(file, &head).is_err() {
            return false;
        }
        let mut piece: Vec<u8> = Vec::with_capacity(64 * ITEM_BYTES);
        for chunk in self.items.chunks(64) {
            piece.clear();
            for item in chunk {
                piece.extend_from_slice(&item.offset.to_le_bytes());
                piece.extend_from_slice(&item.hash.to_le_bytes());
                piece.extend_from_slice(&item.key);
                piece.extend_from_slice(&item.series.to_le_bytes());
                piece.extend_from_slice(&item.number.to_le_bytes());
                piece.push(item.progress);
                piece.push(item.recent);
                piece.push(item.flags);
            }
            if storage.append(file, &piece).is_err() {
                return false;
            }
        }
        for name in &self.series {
            piece.clear();
            let name = &name.as_bytes()[..name.len().min(usize::from(u16::MAX))];
            piece.extend_from_slice(&(name.len() as u16).to_le_bytes());
            piece.extend_from_slice(name);
            if storage.append(file, &piece).is_err() {
                return false;
            }
        }
        true
    }

    /// The index kept by [`Index::save`] in `file`, if it is of the
    /// catalog there is now and made without the folder `archive`.
    pub fn load(storage: &dyn Storage, file: &str, archive: &str) -> Option<Index> {
        let stamp = made(storage)?;
        let mut head = [0u8; HEAD];
        if storage.read_at(file, 0, &mut head).ok()? != head.len() || &head[..4] != MAGIC {
            return None;
        }
        let u32_at =
            |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let mut kept = [0u8; 8];
        kept.copy_from_slice(&head[4..12]);
        if u64::from_le_bytes(kept) != stamp || u32_at(12) != hash(archive) {
            return None;
        }
        let archived = u32_at(16) as usize;
        let total = u32_at(20) as usize;
        let count = u32_at(24) as usize;
        let series = usize::from(u16::from_le_bytes([head[28], head[29]]));
        let mut items = Vec::new();
        items.try_reserve_exact(count).ok()?;
        let mut at = head.len() as u64;
        let mut buf = [0u8; 64 * ITEM_BYTES];
        while items.len() < count {
            let n = (count - items.len()).min(64);
            let want = &mut buf[..n * ITEM_BYTES];
            if storage.read_at(file, at, want).ok()? != want.len() {
                return None;
            }
            at += want.len() as u64;
            for b in want.as_chunks::<ITEM_BYTES>().0 {
                let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                let mut key = [0u8; KEY];
                key.copy_from_slice(&b[8..16]);
                items.push(Item {
                    offset: word(0),
                    hash: word(4),
                    key,
                    series: u16::from_le_bytes([b[16], b[17]]),
                    number: u16::from_le_bytes([b[18], b[19]]),
                    progress: b[20],
                    recent: b[21],
                    flags: b[22],
                });
            }
        }
        let mut names = Vec::with_capacity(series);
        for _ in 0..series {
            let mut len = [0u8; 2];
            if storage.read_at(file, at, &mut len).ok()? != 2 {
                return None;
            }
            let len = usize::from(u16::from_le_bytes(len));
            let mut name = alloc::vec![0u8; len];
            if storage.read_at(file, at + 2, &mut name).ok()? != len {
                return None;
            }
            at += 2 + len as u64;
            names.push(String::from_utf8(name).ok()?);
        }
        Some(Index {
            items,
            series: names,
            total,
            archived,
            dirty: false,
        })
    }
}

/// The item of `record`, whose line starts at `offset`.
fn item(offset: u64, record: &Record, series: u16, opened: &[u32]) -> Item {
    let hash = hash(&record.path);
    let mut flags = 0;
    if record.status.favourite {
        flags |= FAVOURITE;
    }
    if record.status.opened {
        flags |= OPENED;
    }
    Item {
        offset: offset as u32,
        hash,
        key: sort_key(&record.title),
        series,
        number: number(&record.number),
        progress: record.status.progress.unwrap_or(NONE),
        recent: rank(opened, hash),
        flags,
    }
}

/// The books opened last, hashed, the last first.
fn opened(storage: &dyn Storage) -> Vec<u32> {
    super::opened_books(storage)
        .iter()
        .map(|p| hash(p))
        .collect()
}

/// The place of the book hashing to `hash` among the books read last.
fn rank(opened: &[u32], hash: u32) -> u8 {
    opened
        .iter()
        .position(|o| *o == hash)
        .map_or(NEVER, |r| r.min(usize::from(NEVER) - 1) as u8)
}

/// How many lines the catalog has, counted without reading them.
/// Which catalog an index is of: its generation and length (not its
/// log's, which [`Index::refresh`] reads).
fn made(storage: &dyn Storage) -> Option<u64> {
    Some((catalog::generation(storage)? << 40) ^ catalog::size(storage)?)
}

fn count_lines(storage: &dyn Storage) -> usize {
    let path = format!("{}/catalog.txt", super::DIR);
    let mut buf = [0u8; 1024];
    let (mut offset, mut lines, mut last) = (0u64, 0, b'\n');
    while let Ok(n) = storage.read_at(&path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        offset += n as u64;
        lines += buf[..n].iter().filter(|&&b| b == b'\n').count();
        last = buf[n - 1];
    }
    lines + usize::from(last != b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::library::Book;
    use crate::library::catalog::Status;

    #[test]
    fn titles_sort_without_articles() {
        assert_eq!(&sort_key("The Hound of the Baskervilles"), b"hound of");
        assert_eq!(&sort_key("Émile Zola"), b"emile zo");
        assert!(sort_key("Book 000") < sort_key("Book 019"));
        assert_eq!(letter(without_article("Die Verwandlung")), b'V');
        assert_eq!(letter("1984"), b'#');
        assert_eq!(surname("Doyle, Arthur Conan"), "Doyle");
        assert_eq!(surname("Ann Smith & Bob Jones"), "Smith");
        assert_eq!(number("2.5"), 25);
        assert_eq!(number("x"), u16::MAX);
    }

    /// A catalog of `count` books, `books/Book 0001.txt` and on, every
    /// third of a series, the last two in the archive folder.
    fn catalog(count: usize) -> MemoryStorage {
        let storage = MemoryStorage::new();
        let records: Vec<Record> = (1..=count)
            .map(|n| {
                let folder = if n + 2 > count {
                    "books/Archive"
                } else {
                    "books"
                };
                let book = Book::default()
                    .title(Some(format!("Book {n:04}")))
                    .author(Some(String::from("Ann")))
                    .series((n % 3 == 0).then(|| String::from("Tales")))
                    .series_index((n % 3 == 0).then(|| format!("{n}")));
                let status = Status::new((n % 4 == 1).then_some(40), n == 2, n % 4 == 1);
                Record::of(&format!("{folder}/Book {n:04}.txt"), &book).with_status(status)
            })
            .collect();
        catalog::write(&storage, &records);
        storage
    }

    #[test]
    fn an_index_holds_at_most_its_cap_and_counts_the_others() {
        // A book costs 24 bytes; the X4's 2000 at most 48 KB.
        assert!(core::mem::size_of::<Item>() <= 24);
        assert!(cap(MemoryClass::Small) * core::mem::size_of::<Item>() <= 48 * 1024);
        let storage = catalog(12);
        let index = Index::build(&storage, 5, "books/Archive");
        assert_eq!(index.items.len(), 5);
        assert!(index.items.capacity() <= 5, "made no larger than it holds");
        assert_eq!(index.archived, 2);
        assert_eq!(index.total, 10);
    }

    #[test]
    fn filters_come_from_the_catalog_at_once() {
        let storage = catalog(10);
        let index = Index::build(&storage, 100, "books/Archive");
        // Books 1 and 5 begun, book 2 a favourite.
        assert_eq!(index.order(Show::Reading, false).len(), 2);
        assert_eq!(index.order(Show::Favourites, false).len(), 1);
        assert_eq!(index.order(Show::Unread, false).len(), 6);
        assert_eq!(index.order(Show::All, false).len(), 8);
        let record = index.record(&storage, 0).unwrap();
        assert_eq!(record.title, "Book 0001");
        // Changed after: taken anew.
        catalog::note(
            &storage,
            "books/Book 0002.txt",
            Status::new(Some(100), true, true),
        );
        let mut index = index;
        index.refresh(&storage);
        assert_eq!(index.order(Show::Finished, false).len(), 1);
    }

    #[test]
    fn the_next_book_of_each_series_begun() {
        let storage = MemoryStorage::new();
        let book = |n: u32| {
            Book::default()
                .title(Some(format!("Tale {n}")))
                .series(Some(String::from("Tales")))
                .series_index(Some(format!("{n}")))
        };
        let status = |p: Option<u8>| Status::new(p, false, p.is_some());
        catalog::write(
            &storage,
            &[
                Record::of("b/4.txt", &book(4)).with_status(status(None)),
                Record::of("b/1.txt", &book(1)).with_status(status(Some(100))),
                Record::of("b/3.txt", &book(3)).with_status(status(None)),
                Record::of("b/2.txt", &book(2)).with_status(status(Some(42))),
                Record::of("b/x.txt", &Book::default().series(Some("Other".into())))
                    .with_status(status(None)),
            ],
        );
        let index = Index::build(&storage, 100, "");
        let next = index.next_in_series(false);
        assert_eq!(next.len(), 1, "the other series is not begun");
        assert_eq!(
            index.record(&storage, usize::from(next[0])).unwrap().path,
            "b/3.txt"
        );
        // As covers: the series a stack, the book being read apart.
        let all = index.order(Show::All, false);
        let (entries, counts) = index.stacked(&all);
        assert_eq!(entries.len(), 3);
        assert!(counts.contains(&3), "{counts:?}");
    }

    #[test]
    fn a_kept_index_is_read_back_while_its_catalog_is_the_same() {
        let storage = catalog(9);
        let mut index = Index::build(&storage, 100, "books/Archive");
        index.set_progress(index.items[2].hash, Some(42));
        assert!(index.save(&storage, "kept.bin", "books/Archive"));
        let kept = Index::load(&storage, "kept.bin", "books/Archive").unwrap();
        assert_eq!(kept.items, index.items);
        assert_eq!(kept.series, index.series);
        assert_eq!((kept.total, kept.archived), (index.total, index.archived));
        // Another archive folder: made anew.
        assert!(Index::load(&storage, "kept.bin", "books/Old").is_none());
        // A change noted beside the catalog: still of it, the change
        // taken by refreshing.
        catalog::note(
            &storage,
            "books/Book 0001.txt",
            Status::new(Some(100), false, true),
        );
        let mut kept = Index::load(&storage, "kept.bin", "books/Archive").unwrap();
        kept.refresh(&storage);
        assert_eq!(kept.order(Show::Finished, false).len(), 1);
        // The catalog written anew (folded, made), even as long as it
        // was: made anew, where its lines start may have moved.
        let records: Vec<Record> = {
            let mut all = Vec::new();
            catalog::each(&storage, &mut |r| all.push(r));
            all
        };
        assert!(index.save(&storage, "kept.bin", "books/Archive"));
        catalog::write(&storage, &records);
        assert!(Index::load(&storage, "kept.bin", "books/Archive").is_none());
    }
}
