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
/// Books indexed at most on devices of little memory (the X4: 40 KB),
/// and on others; those past it are counted, not shown.
const MAX_BOOKS_SMALL: usize = 2000;
const MAX_BOOKS: usize = 20_000;
/// Where the index is kept while the library is covered (by a book).
const KEPT: &str = ".paperpaw/plugins/files/index.bin";
/// The kept index's first bytes: what it is, and its version.
const MAGIC: &[u8; 4] = b"PPI2";
/// Letters of a title kept to sort by.
pub(crate) const KEY: usize = 8;
/// Bytes of an item kept.
const ITEM_BYTES: usize = 22;
/// Bytes of the kept index's head: what it is, the catalog's size, the
/// archive (hashed), how many books are in it, in the catalog and
/// indexed, how many series.
const HEAD: usize = 30;

/// A book of the catalog, as the index keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Item {
    /// Where its line starts in the catalog.
    pub(crate) offset: u32,
    /// A hash of its path, to find it again ([`hash`]).
    pub(crate) hash: u32,
    /// Its title's first letters, to sort by ([`sort_key`]).
    pub(crate) key: [u8; KEY],
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

    /// The filter `steps` pills on, round at the ends (from Next in your
    /// series as from the shelves, where it is).
    pub(crate) fn step(self, steps: isize) -> Filter {
        let pill = if self == Filter::Next {
            Filter::Shelves
        } else {
            self
        };
        let at = Self::ALL.iter().position(|f| *f == pill).unwrap_or(0) as isize;
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

/// How many lines the catalog has, counted without reading them.
fn lines(storage: &dyn Storage) -> usize {
    let path = catalog_path();
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

/// The first letters of `title` to sort by ([`KEY`]), without a leading
/// article; titles with the same first letters are kept in the catalog's
/// order.
pub(crate) fn sort_key(title: &str) -> [u8; KEY] {
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
    /// Books in the catalog (but the archive's): more than
    /// [`Index::items`] where some were left out (more than the device
    /// holds).
    pub(crate) total: usize,
    /// Books in the archive folder, kept out of the list (they are on the
    /// shelves, in the archive's folder).
    pub(crate) archived: usize,
    /// The names of the series, as [`Item::series`] counts them.
    pub(crate) series: Vec<String>,
    /// Items whose progress is still to be read, the next one last.
    pending: Vec<u16>,
    /// Changed since it was kept in a file ([`Index::save`]).
    pub(crate) dirty: bool,
}

impl Index {
    /// The most books indexed on a device of `memory`.
    pub(crate) fn cap(memory: paperpaw_sdk::MemoryClass) -> usize {
        if memory == paperpaw_sdk::MemoryClass::Small {
            MAX_BOOKS_SMALL
        } else {
            MAX_BOOKS
        }
    }

    /// Whether there are books at all (in the archive too).
    pub(crate) fn has_books(&self) -> bool {
        !self.items.is_empty() || self.archived > 0
    }

    /// Makes the index from the catalog, of at most `cap` books, those in
    /// the folder `archive` left out (progress still unknown).
    pub(crate) fn build(storage: &dyn Storage, cap: usize, archive: &str) -> Index {
        let archive = if archive.is_empty() {
            String::new()
        } else {
            format!("{archive}/")
        };
        let mut archived = 0;
        let opened: Vec<u32> = library::opened_books(storage)
            .iter()
            .map(|p| hash(p))
            .collect();
        // As many as there are lines: a list grown by doubling would hold
        // twice that for a moment (40 KB for 1000 books on the X4).
        let total = lines(storage);
        let mut items = Vec::with_capacity(total.min(cap));
        let mut series: Vec<String> = Vec::new();
        scan(storage, &mut |offset, line| {
            if !archive.is_empty() && line.path.starts_with(&archive) {
                archived += 1;
                return;
            }
            if items.len() >= cap {
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
                progress: UNKNOWN,
                recent: rank(&opened, hash),
                series: of,
                number: number(&line.number),
            });
        });
        let mut index = Index {
            total: total.saturating_sub(archived).max(items.len()),
            archived,
            items,
            series,
            pending: Vec::new(),
            dirty: true,
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

    /// Reads the progress of the books read last (`recent` places) and of
    /// those whose paths hash to `hashes` again, in the background: what
    /// may have changed while the library was covered.
    pub(crate) fn read_some_again(&mut self, recent: u8, hashes: &[u32]) {
        for (i, item) in self.items.iter_mut().enumerate() {
            if item.recent < recent || hashes.contains(&item.hash) {
                item.progress = UNKNOWN;
                if !self.pending.contains(&(i as u16)) {
                    self.pending.push(i as u16);
                }
            }
        }
    }

    /// Keeps the index in a file, to be read again ([`Index::load`]) when
    /// the library shows again, so it holds no memory meanwhile. Written
    /// a piece at a time; `false` where it could not be.
    /// `archive` is the archive folder it was made without.
    pub(crate) fn save(&self, storage: &dyn Storage, archive: &str) -> bool {
        let Ok(stamp) = storage.size(&catalog_path()) else {
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
        if storage.write(KEPT, &head).is_err() {
            return false;
        }
        let mut piece: Vec<u8> = Vec::with_capacity(64 * ITEM_BYTES);
        for chunk in self.items.chunks(64) {
            piece.clear();
            for item in chunk {
                piece.extend_from_slice(&item.offset.to_le_bytes());
                piece.extend_from_slice(&item.hash.to_le_bytes());
                piece.extend_from_slice(&item.key);
                piece.push(item.progress);
                piece.push(item.recent);
                piece.extend_from_slice(&item.series.to_le_bytes());
                piece.extend_from_slice(&item.number.to_le_bytes());
            }
            if storage.append(KEPT, &piece).is_err() {
                return false;
            }
        }
        for name in &self.series {
            piece.clear();
            let name = &name.as_bytes()[..name.len().min(usize::from(u16::MAX))];
            piece.extend_from_slice(&(name.len() as u16).to_le_bytes());
            piece.extend_from_slice(name);
            if storage.append(KEPT, &piece).is_err() {
                return false;
            }
        }
        true
    }

    /// The index kept by [`Index::save`], if it is of the catalog there is
    /// now and without the archive folder `archive`; progress of books not
    /// yet read stays to be read.
    pub(crate) fn load(storage: &dyn Storage, archive: &str) -> Option<Index> {
        let stamp = storage.size(&catalog_path()).ok()?;
        let mut head = [0u8; HEAD];
        if storage.read_at(KEPT, 0, &mut head).ok()? != head.len() || &head[..4] != MAGIC {
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
            if storage.read_at(KEPT, at, want).ok()? != want.len() {
                return None;
            }
            at += want.len() as u64;
            for b in want.as_chunks::<ITEM_BYTES>().0 {
                let word = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                items.push(Item {
                    offset: word(0),
                    hash: word(4),
                    key: [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
                    progress: b[16],
                    recent: b[17],
                    series: u16::from_le_bytes([b[18], b[19]]),
                    number: u16::from_le_bytes([b[20], b[21]]),
                });
            }
        }
        let mut names = Vec::with_capacity(series);
        for _ in 0..series {
            let mut len = [0u8; 2];
            if storage.read_at(KEPT, at, &mut len).ok()? != 2 {
                return None;
            }
            let len = usize::from(u16::from_le_bytes(len));
            let mut name = alloc::vec![0u8; len];
            if storage.read_at(KEPT, at + 2, &mut name).ok()? != len {
                return None;
            }
            at += 2 + len as u64;
            names.push(String::from_utf8(name).ok()?);
        }
        // Progress not read before it was kept is read now.
        let pending = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.progress == UNKNOWN)
            .map(|(i, _)| i as u16)
            .collect();
        Some(Index {
            items,
            total,
            archived,
            series: names,
            pending,
            dirty: false,
        })
    }

    /// Takes the order of the books read last anew (after reading).
    pub(crate) fn rerank(&mut self, storage: &dyn Storage) {
        let opened: Vec<u32> = library::opened_books(storage)
            .iter()
            .map(|p| hash(p))
            .collect();
        for item in &mut self.items {
            let recent = rank(&opened, item.hash);
            self.dirty |= item.recent != recent;
            item.recent = recent;
        }
    }

    /// Reads the progress of up to `count` books; `true` when every book's
    /// is known.
    pub(crate) fn step(&mut self, storage: &dyn Storage, count: usize) -> bool {
        for _ in 0..count {
            let Some(i) = self.pending.pop() else {
                break;
            };
            self.dirty = true;
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
        if let Some(item) = self.items.iter_mut().find(|i| i.hash == hash)
            && item.progress != progress
        {
            item.progress = progress;
            self.dirty = true;
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
        assert_eq!(&sort_key("The Hound of the Baskervilles"), b"hound of");
        assert_eq!(&sort_key("Émile Zola"), b"emile zo");
        assert_eq!(&sort_key("A"), b"a\0\0\0\0\0\0\0");
        assert!(sort_key("Book 000") < sort_key("Book 019"));
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
    fn series_numbers() {
        assert_eq!(number("2"), 20);
        assert_eq!(number(" 2.5 "), 25);
        assert_eq!(number("11"), 110);
        assert_eq!(number(""), u16::MAX);
        assert_eq!(number("II"), u16::MAX);
    }

    /// An item of series `series` numbered `n`, read `progress` percent,
    /// `recent` among the books read last.
    fn item(series: u16, n: u16, progress: u8, recent: u8) -> Item {
        Item {
            offset: u32::from(series) * 100 + u32::from(n),
            hash: 0,
            key: [b'a' + n as u8, 0, 0, 0, 0, 0, 0, 0],
            progress,
            recent,
            series,
            number: n * 10,
        }
    }

    #[test]
    fn the_next_book_of_each_series_begun() {
        let index = Index {
            items: alloc::vec![
                // Series 0: 1 finished, 2 being read, 3 and 4 unread.
                item(0, 4, 0, NEVER),
                item(0, 1, 100, 3),
                item(0, 3, 0, NEVER),
                item(0, 2, 42, 0),
                // Series 1: never begun.
                item(1, 1, 0, NEVER),
                // Series 2: all read.
                item(2, 1, 100, 5),
                // No series.
                item(NO_SERIES, 1, 0, NEVER),
            ],
            series: alloc::vec!["A".into(), "B".into(), "C".into()],
            total: 7,
            archived: 0,
            pending: Vec::new(),
            dirty: false,
        };
        assert_eq!(index.order(Filter::Next, true), [2]);
    }

    #[test]
    fn series_stack_but_books_being_read_stay_apart() {
        let index = Index {
            items: alloc::vec![
                item(0, 2, 42, 0),
                item(0, 1, 100, 1),
                item(NO_SERIES, 1, 0, NEVER),
                item(0, 3, 0, NEVER),
                item(1, 1, 0, NEVER),
                item(1, 2, 0, NEVER),
            ],
            series: alloc::vec!["A".into(), "B".into()],
            total: 6,
            archived: 0,
            pending: Vec::new(),
            dirty: false,
        };
        let order = [0, 1, 2, 3, 4, 5];
        let (entries, counts) = index.stacked(&order);
        assert_eq!(entries, [0, 1, 2, 4]);
        assert_eq!(counts, [1, 2, 1, 2]);
    }

    /// A catalog of `count` books, `books/Book 0001.txt` and on, a few
    /// of a series and the last two in the archive folder.
    fn catalog(count: usize) -> paperpaw_sdk::MemoryStorage {
        let storage = paperpaw_sdk::MemoryStorage::new();
        let records: Vec<library::catalog::Record> = (1..=count)
            .map(|n| {
                let folder = if n + 2 > count {
                    "books/Archive"
                } else {
                    "books"
                };
                let series = (n % 3 == 0).then(|| String::from("Tales"));
                let book = library::Book::default()
                    .title(format!("Book {n:04}"))
                    .author(String::from("Ann"))
                    .series_index(series.as_ref().map(|_| format!("{n}")))
                    .series(series);
                library::catalog::Record::of(&format!("{folder}/Book {n:04}.txt"), &book)
            })
            .collect();
        library::catalog::write(&storage, &records);
        storage
    }

    #[test]
    fn an_index_holds_at_most_its_cap_and_says_how_many_it_left_out() {
        // A book costs 24 bytes; the X4's 2000 at most 48 KB.
        assert!(core::mem::size_of::<Item>() <= 24);
        assert!(
            Index::cap(paperpaw_sdk::MemoryClass::Small) * core::mem::size_of::<Item>()
                <= 48 * 1024
        );
        let storage = catalog(12);
        let index = Index::build(&storage, 5, "books/Archive");
        assert_eq!(index.items.len(), 5);
        assert!(index.items.capacity() <= 5, "made no larger than it holds");
        // The archive's two are apart; of the others five shown, ten in all.
        assert_eq!(index.archived, 2);
        assert_eq!(index.total, 10);
    }

    #[test]
    fn a_kept_index_is_read_back_while_its_catalog_is_the_same() {
        let storage = catalog(9);
        let mut index = Index::build(&storage, 100, "books/Archive");
        while !index.step(&storage, 8) {}
        index.set_progress(index.items[2].hash, 42);
        assert!(index.save(&storage, "books/Archive"));
        let kept = Index::load(&storage, "books/Archive").unwrap();
        assert_eq!(kept.items, index.items);
        assert_eq!(kept.series, index.series);
        assert_eq!((kept.total, kept.archived), (index.total, index.archived));
        assert_eq!(kept.unknown(), 0, "progress kept, not read again");
        // Another archive folder, or another catalog: made anew.
        assert!(Index::load(&storage, "books/Old").is_none());
        let other = catalog(10);
        assert!(index.save(&other, "books/Archive"));
        library::catalog::write(&other, &[]);
        assert!(Index::load(&other, "books/Archive").is_none());
    }

    #[test]
    fn filters_go_round() {
        assert_eq!(Filter::All.step(-1), Filter::Shelves);
        assert_eq!(Filter::Shelves.step(1), Filter::All);
        assert!(Filter::Reading.shows(42));
        assert!(!Filter::Unread.shows(100));
    }
}
