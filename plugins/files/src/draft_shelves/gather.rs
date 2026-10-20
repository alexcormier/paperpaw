//! What goes on a shelf: the books (or the series, authors and
//! collections) of each shelf, gathered from the summary
//! ([`summary`](super::summary)) in one pass, each shelf keeping only its
//! first few in order. Nothing here grows with the library but the
//! names of the series being read.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::library::listing::Listing;
use paperpaw_sdk::{Env, library};

use super::summary::{self, Item, State, sort_key};

/// What a shelf holds; also what a shelf seen in full shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// Books being read, the last read first.
    Continue,
    /// For each series being read, its next unread book.
    Next,
    /// Books never opened, by title.
    Unread,
    /// The favourites, the newest first.
    Favourites,
    /// Books read to the end, the last read first.
    Finished,
    /// The books of a folder of the library folder (a collection).
    Folder(String),
    /// The books of a series, by their numbers.
    Series(String),
    /// The books of an author, by title.
    Author(String),
    /// Every book, by title.
    All,
    /// The series, by name.
    SeriesIndex,
    /// The authors, by name.
    AuthorIndex,
    /// The collections (the folders of the library folder).
    FolderIndex,
    /// The books matching words.
    Search(String),
}

impl Source {
    /// The source as a screen's parameters: a mode and a name.
    pub(crate) fn mode(&self) -> (&'static str, &str) {
        match self {
            Source::Continue => ("continue", ""),
            Source::Next => ("next", ""),
            Source::Unread => ("unread", ""),
            Source::Favourites => ("favourites", ""),
            Source::Finished => ("finished", ""),
            Source::Folder(name) => ("folder", name),
            Source::Series(name) => ("series", name),
            Source::Author(name) => ("author", name),
            Source::All => ("all", ""),
            Source::SeriesIndex => ("series-index", ""),
            Source::AuthorIndex => ("author-index", ""),
            Source::FolderIndex => ("folder-index", ""),
            Source::Search(words) => ("search", words),
        }
    }

    /// The source named by [`Source::mode`].
    pub(crate) fn parse(mode: &str, name: &str) -> Source {
        let name = name.to_string();
        match mode {
            "continue" => Source::Continue,
            "next" => Source::Next,
            "unread" => Source::Unread,
            "favourites" => Source::Favourites,
            "finished" => Source::Finished,
            "folder" => Source::Folder(name),
            "series" => Source::Series(name),
            "author" => Source::Author(name),
            "series-index" => Source::SeriesIndex,
            "author-index" => Source::AuthorIndex,
            "folder-index" => Source::FolderIndex,
            "search" => Source::Search(name),
            _ => Source::All,
        }
    }

    /// Whether it lists groups of books (series, authors, collections).
    pub(crate) fn groups(&self) -> bool {
        matches!(
            self,
            Source::SeriesIndex | Source::AuthorIndex | Source::FolderIndex
        )
    }

    /// Whether it is in alphabetical order, so that buttons can jump from
    /// letter to letter.
    pub(crate) fn alphabetical(&self) -> bool {
        matches!(
            self,
            Source::All
                | Source::Unread
                | Source::Author(_)
                | Source::Folder(_)
                | Source::Search(_)
                | Source::SeriesIndex
                | Source::AuthorIndex
                | Source::FolderIndex
        )
    }
}

/// Books grouped: a series, an author or a collection, how many books it
/// has and the book whose cover stands for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) name: String,
    pub(crate) count: usize,
    pub(crate) cover: Item,
}

/// One place on a shelf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tile {
    Book(Item),
    Group(Group),
}

impl Tile {
    /// The book whose cover it shows.
    pub(crate) fn item(&self) -> &Item {
        match self {
            Tile::Book(item) => item,
            Tile::Group(group) => &group.cover,
        }
    }

    /// The text it is sorted and found by.
    pub(crate) fn name(&self) -> &str {
        match self {
            Tile::Book(item) => &item.title,
            Tile::Group(group) => &group.name,
        }
    }
}

/// The first books of a source and how many it has in all (for groups:
/// how many it has at least, `more` telling there are others).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Gathered {
    pub(crate) tiles: Vec<Tile>,
    pub(crate) total: usize,
    pub(crate) more: bool,
}

/// What a book is sorted by on a shelf: a rank first (recently read,
/// a series' number), then a text.
type Key = (u32, String);

/// What the sources share: the library folder, the order of the
/// recently read and of the favourites, progress read anew, the words
/// searched for.
struct Context {
    /// The library folder followed by `/` (empty for the storage's top).
    prefix: String,
    opened: Vec<(String, u32)>,
    favourites: Vec<(String, u32)>,
    /// Progress of the books read last, read from their details: newer
    /// than the summary until it is made again.
    fresh: Vec<(String, Option<u8>)>,
}

fn ranked(paths: Vec<String>) -> Vec<(String, u32)> {
    let mut ranked: Vec<(String, u32)> = paths
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p, i as u32))
        .collect();
    ranked.sort_unstable();
    ranked
}

fn rank_in(list: &[(String, u32)], path: &str) -> Option<u32> {
    list.binary_search_by(|(p, _)| p.as_str().cmp(path))
        .ok()
        .map(|i| list[i].1)
}

/// Books whose progress is read from their details rather than taken
/// from the summary: those read last, whose progress changes as they are
/// read.
const FRESH: usize = 12;

impl Context {
    fn new(env: &Env<'_>, root: &str) -> Self {
        let opened = library::opened_books(env.storage);
        let fresh = opened
            .iter()
            .take(FRESH)
            .map(|path| {
                let progress = library::book(env.storage, path).and_then(|b| b.progress);
                (path.clone(), progress)
            })
            .collect();
        let root = root.trim_matches('/');
        Self {
            prefix: if root.is_empty() {
                String::new()
            } else {
                alloc::format!("{root}/")
            },
            opened: ranked(opened),
            favourites: ranked(library::favourites(env.storage)),
            fresh,
        }
    }

    fn opened_rank(&self, item: &Item) -> u32 {
        rank_in(&self.opened, &item.path).unwrap_or(u32::MAX)
    }

    /// The collection (folder of the library folder) `item` is in.
    fn folder<'i>(&self, item: &'i Item) -> Option<&'i str> {
        let inside = item.path.strip_prefix(&self.prefix)?;
        inside.split_once('/').map(|(folder, _)| folder)
    }

    /// `item` with its progress read anew where it was read last.
    fn refresh(&self, mut item: Item) -> Item {
        if let Some((_, progress)) = self.fresh.iter().find(|(p, _)| *p == item.path) {
            item.progress = *progress;
        }
        item
    }
}

/// A book's number in its series as a rank.
fn number_rank(item: &Item) -> u32 {
    let n = item.series_number();
    if n == f32::MAX {
        u32::MAX
    } else {
        (n.max(0.0) * 1000.0) as u32
    }
}

/// Where `item` goes on a shelf of books of `source` and by what it is
/// sorted there; `None` if it is not one of them.
fn book_key(source: &Source, item: &Item, cx: &Context, words: &[String]) -> Option<Key> {
    let title = || item.title_key();
    match source {
        Source::Continue => {
            matches!(item.state(), State::Reading(_)).then(|| (cx.opened_rank(item), title()))
        }
        Source::Finished => {
            (item.state() == State::Finished).then(|| (cx.opened_rank(item), title()))
        }
        Source::Unread => (item.state() == State::New).then(|| (0, title())),
        Source::Favourites => rank_in(&cx.favourites, &item.path).map(|r| (r, String::new())),
        Source::Folder(name) => (cx.folder(item) == Some(name)).then(|| (0, title())),
        Source::Series(name) => (item.series == *name).then(|| (number_rank(item), title())),
        Source::Author(name) => (item.author == *name).then(|| (0, title())),
        Source::All => Some((0, title())),
        Source::Search(_) => item.matches(words).then(|| (0, title())),
        _ => None,
    }
}

/// The name of the group `item` is in for `source`, if any.
fn group_of<'i>(source: &Source, item: &'i Item, cx: &Context) -> Option<&'i str> {
    let name = match source {
        Source::SeriesIndex => item.series.as_str(),
        Source::AuthorIndex => item.author.as_str(),
        Source::FolderIndex => cx.folder(item)?,
        _ => return None,
    };
    (!name.trim().is_empty()).then_some(name)
}

/// Gathering the first `cap` tiles of one source.
struct Top {
    source: Source,
    cap: usize,
    /// Books with their keys, or groups (by [`sort_key`] of the name).
    books: Vec<(Key, Item)>,
    groups: Vec<(String, Group)>,
    total: usize,
    more: bool,
}

impl Top {
    fn new(source: Source, cap: usize) -> Self {
        Self {
            source,
            cap: cap.max(1),
            books: Vec::new(),
            groups: Vec::new(),
            total: 0,
            more: false,
        }
    }

    fn offer(&mut self, item: &Item, cx: &Context, words: &[String]) {
        if self.source.groups() {
            if let Some(name) = group_of(&self.source, item, cx) {
                self.offer_group(name, item);
            }
            return;
        }
        let Some(key) = book_key(&self.source, item, cx, words) else {
            return;
        };
        self.total += 1;
        if self.books.len() == self.cap && self.books.last().is_some_and(|(k, _)| key >= *k) {
            return;
        }
        let at = self.books.partition_point(|(k, _)| *k <= key);
        self.books.insert(at, (key, item.clone()));
        self.books.truncate(self.cap);
    }

    /// A book of group `name`: counted if the group is among the first
    /// (a group left out once never comes back: the last kept only gets
    /// earlier), taken in if it is.
    fn offer_group(&mut self, name: &str, item: &Item) {
        if let Some((_, group)) = self.groups.iter_mut().find(|(_, g)| g.name == name) {
            group.count += 1;
            // A series shows its first book; others the first by title.
            let better = if self.source == Source::SeriesIndex {
                item.series_number() < group.cover.series_number()
            } else {
                item.title_key() < group.cover.title_key()
            };
            if better {
                group.cover = item.clone();
            }
            return;
        }
        let key = sort_key(name);
        if self.groups.len() == self.cap && self.groups.last().is_some_and(|(k, _)| key >= *k) {
            self.more = true;
            return;
        }
        let at = self.groups.partition_point(|(k, _)| *k <= key);
        let group = Group {
            name: name.to_string(),
            count: 1,
            cover: item.clone(),
        };
        self.groups.insert(at, (key, group));
        if self.groups.len() > self.cap {
            self.groups.truncate(self.cap);
            self.more = true;
        }
    }

    fn done(self) -> Gathered {
        if self.source.groups() {
            return Gathered {
                total: self.groups.len(),
                more: self.more,
                tiles: self
                    .groups
                    .into_iter()
                    .map(|(_, g)| Tile::Group(g))
                    .collect(),
            };
        }
        Gathered {
            total: self.total,
            more: false,
            tiles: self.books.into_iter().map(|(_, i)| Tile::Book(i)).collect(),
        }
    }
}

/// Series being read: name, how recently one of its books was read.
const MAX_SERIES_READ: usize = 200;

/// The first `cap` tiles of each of `sources`, in the order of each,
/// from one pass through the summary (two if [`Source::Next`] is asked
/// for: the series being read are known only at the end of the first).
pub(crate) fn gather(env: &Env<'_>, root: &str, sources: &[Source], cap: usize) -> Vec<Gathered> {
    let cx = Context::new(env, root);
    let words: Vec<String> = sources
        .iter()
        .find_map(|s| match s {
            Source::Search(words) => Some(words.to_lowercase()),
            _ => None,
        })
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect();
    let mut tops: Vec<Top> = sources.iter().map(|s| Top::new(s.clone(), cap)).collect();
    let wants_next = sources.contains(&Source::Next);
    let mut reading: Vec<(String, u32)> = Vec::new();
    summary::each(env.storage, &mut |item| {
        let item = cx.refresh(item);
        for top in &mut tops {
            top.offer(&item, &cx, &words);
        }
        if wants_next && !item.series.is_empty() && item.state() != State::New {
            let rank = cx.opened_rank(&item);
            match reading.iter().position(|(s, _)| *s == item.series) {
                Some(at) => reading[at].1 = reading[at].1.min(rank),
                None if reading.len() < MAX_SERIES_READ => {
                    reading.push((item.series.clone(), rank));
                }
                None => {}
            }
        }
    });
    let mut gathered: Vec<Gathered> = tops.into_iter().map(Top::done).collect();
    if let Some(at) = sources.iter().position(|s| *s == Source::Next) {
        gathered[at] = next_in_series(env, &cx, &reading, cap);
    }
    gathered
}

/// For each series being read, its unread book of the lowest number,
/// the series read last first.
fn next_in_series(env: &Env<'_>, cx: &Context, reading: &[(String, u32)], cap: usize) -> Gathered {
    let mut next: Vec<Option<Item>> = alloc::vec![None; reading.len()];
    summary::each(env.storage, &mut |item| {
        let item = cx.refresh(item);
        if item.state() != State::New {
            return;
        }
        if let Some(at) = reading.iter().position(|(s, _)| *s == item.series) {
            let slot = &mut next[at];
            if slot
                .as_ref()
                .is_none_or(|b| item.series_number() < b.series_number())
            {
                *slot = Some(item);
            }
        }
    });
    let mut found: Vec<(u32, Item)> = reading
        .iter()
        .zip(next)
        .filter_map(|((_, rank), item)| Some((*rank, item?)))
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    Gathered {
        total: found.len(),
        more: false,
        tiles: found
            .into_iter()
            .take(cap)
            .map(|(_, i)| Tile::Book(i))
            .collect(),
    }
}

/// The collections: the folders of the library folder, by name (at most
/// `max`).
pub(crate) fn collections(env: &Env<'_>, root: &str, max: usize) -> Vec<String> {
    let listing = Listing {
        storage: env.storage,
        formats: env.formats,
        dir: root,
        folders_only: true,
        rank: None,
    };
    listing
        .after(None, max)
        .into_iter()
        .filter(|e| e.is_dir && !e.name.starts_with('.'))
        .map(|e| e.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, series: &str, number: &str, progress: Option<u8>) -> Item {
        Item {
            path: path.into(),
            title: path.trim_start_matches("b/").into(),
            author: "Ann".into(),
            series: series.into(),
            number: number.into(),
            progress,
        }
    }

    #[test]
    fn shelves_keep_their_first_books_in_order() {
        let cx = Context {
            prefix: "b/".into(),
            opened: ranked(alloc::vec!["b/e".into(), "b/a".into()]),
            favourites: Vec::new(),
            fresh: Vec::new(),
        };
        let items = [
            item("b/a", "S", "1", Some(100)),
            item("b/b", "S", "3", None),
            item("b/c", "S", "2", None),
            item("b/d", "", "", None),
            item("b/e", "S", "5", Some(42)),
            item("b/x/f", "T", "1", None),
        ];
        let mut unread = Top::new(Source::Unread, 2);
        let mut series = Top::new(Source::Series("S".into()), 10);
        let mut index = Top::new(Source::SeriesIndex, 1);
        let mut folders = Top::new(Source::FolderIndex, 5);
        for i in &items {
            for top in [&mut unread, &mut series, &mut index, &mut folders] {
                top.offer(i, &cx, &[]);
            }
        }
        let unread = unread.done();
        assert_eq!(unread.total, 4);
        let titles: Vec<&str> = unread.tiles.iter().map(Tile::name).collect();
        assert_eq!(titles, ["b", "c"]);
        let numbers: Vec<&str> = series
            .done()
            .tiles
            .iter()
            .map(|t| t.item().number.as_str())
            .collect();
        assert_eq!(numbers, ["1", "2", "3", "5"]);
        let index = index.done();
        assert_eq!((index.tiles.len(), index.more), (1, true));
        match &index.tiles[0] {
            Tile::Group(g) => assert_eq!(
                (g.name.as_str(), g.count, g.cover.number.as_str()),
                ("S", 4, "1")
            ),
            Tile::Book(_) => panic!("a group"),
        }
        assert_eq!(folders.done().tiles[0].name(), "x");
        assert_eq!(cx.opened_rank(&items[4]), 0);
    }
}
