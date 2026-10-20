//! The shelves' summary: every book under the library folder with what
//! the shelves sort it by (title, author, series and number) and how far
//! it is read, a book a line in `.paperpaw/plugins/files/shelves.txt`.
//!
//! The catalog (`catalog.txt`) has no reading progress, which lives in
//! each book's details file: reading a thousand of those whenever the
//! library opens would take seconds on a card. So the summary is made in
//! the background ([`Walk`], a few books a step, as the catalog is) and
//! read in one pass when the library shows; until a new one is complete,
//! the old one is read. Drawing never reads it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::library::listing::{Entry, Listing};
use paperpaw_sdk::library::{self, Book};
use paperpaw_sdk::{Env, Storage, files};

/// Entries looked at, and books opened, in one step of a [`Walk`].
const STEP_ENTRIES: usize = 12;

fn summary_path() -> String {
    format!("{}/shelves.txt", Env::plugin_dir("files"))
}

fn building_path() -> String {
    format!("{}/shelves.tmp", Env::plugin_dir("files"))
}

/// How far a book is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Never opened.
    New,
    /// Opened, read this far (percent).
    Reading(u8),
    Finished,
}

/// A book as the shelves know it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) series: String,
    /// Its number in the series, as the book names it (`3`, `2.5`).
    pub(crate) number: String,
    /// Reading progress in percent, once opened.
    pub(crate) progress: Option<u8>,
}

impl Item {
    /// The item of the book at `path` with the details `book`.
    pub(crate) fn of(path: &str, book: &Book) -> Item {
        Item {
            path: path.to_string(),
            title: book.shown_title(path).into_owned(),
            author: book.author.clone().unwrap_or_default(),
            series: book.series.clone().unwrap_or_default(),
            number: book.series_index.clone().unwrap_or_default(),
            progress: book.progress,
        }
    }

    pub(crate) fn state(&self) -> State {
        match self.progress {
            None => State::New,
            Some(100) => State::Finished,
            Some(p) => State::Reading(p),
        }
    }

    /// Its number in the series as a number to sort by; books without
    /// one come last.
    pub(crate) fn series_number(&self) -> f32 {
        self.number.trim().parse().unwrap_or(f32::MAX)
    }

    /// What it is sorted by among titles: lower case, a leading article
    /// left out ("The Time Machine" under T for Time).
    pub(crate) fn title_key(&self) -> String {
        sort_key(&self.title)
    }

    fn parse(text: &str) -> Option<Item> {
        let mut fields = text.split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_string();
        let mut next = || fields.next().unwrap_or("").to_string();
        let progress = next().parse().ok();
        Some(Item {
            path,
            progress,
            title: next(),
            author: next(),
            series: next(),
            number: next(),
        })
    }

    fn text(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let progress = self.progress.map(|p| p.to_string()).unwrap_or_default();
        format!(
            "{}\t{progress}\t{}\t{}\t{}\t{}\n",
            clean(&self.path),
            clean(&self.title),
            clean(&self.author),
            clean(&self.series),
            clean(&self.number),
        )
    }

    /// Whether all of the words of `query` (lower case) are in its title,
    /// author, series or file name.
    pub(crate) fn matches(&self, query: &[String]) -> bool {
        let text = format!(
            "{} {} {} {}",
            self.title,
            self.author,
            self.series,
            files::name(&self.path)
        )
        .to_lowercase();
        query.iter().all(|word| text.contains(word.as_str()))
    }
}

/// `text` as it is sorted: lower case, without a leading English or
/// German article.
pub(crate) fn sort_key(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    for article in ["the ", "a ", "an ", "der ", "die ", "das "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

/// The letter `text` is sorted under ([`sort_key`]): its first letter in
/// capitals, `#` for digits and signs.
pub(crate) fn initial(text: &str) -> char {
    sort_key(text)
        .chars()
        .next()
        .filter(|c| c.is_alphabetic())
        .and_then(|c| c.to_uppercase().next())
        .unwrap_or('#')
}

/// Whether there is a summary to read.
pub(crate) fn exists(storage: &dyn Storage) -> bool {
    storage.exists(&summary_path())
}

/// Calls `f` with each book of the summary, reading it in pieces.
pub(crate) fn each(storage: &dyn Storage, f: &mut dyn FnMut(Item)) {
    let mut buf = [0u8; 2048];
    let mut rest: Vec<u8> = Vec::new();
    let mut offset = 0u64;
    let path = summary_path();
    while let Ok(n) = storage.read_at(&path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        offset += n as u64;
        rest.extend_from_slice(&buf[..n]);
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = rest.drain(..=end).collect();
            if let Some(item) = Item::parse(String::from_utf8_lossy(&line).trim_end()) {
                f(item);
            }
        }
    }
    if let Some(item) = Item::parse(String::from_utf8_lossy(&rest).trim_end()) {
        f(item);
    }
}

/// Writes `items` as the summary, e.g. for tests.
#[cfg(test)]
pub(crate) fn write(storage: &dyn Storage, items: &[Item]) {
    let text: String = items.iter().map(Item::text).collect();
    let _ = storage.write(&summary_path(), text.as_bytes());
}

/// Making the summary anew: the folders still to walk, and where the
/// current one is.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    dirs: Vec<String>,
    dir: Option<String>,
    last: Option<Entry>,
    found: usize,
}

impl Walk {
    /// Starts anew with the books under `root`.
    pub(crate) fn start(storage: &dyn Storage, root: &str) -> Walk {
        let _ = storage.create_dir(&Env::plugin_dir("files"));
        let _ = storage.write(&building_path(), &[]);
        Walk {
            dirs: Vec::from([root.trim_matches('/').to_string()]),
            ..Walk::default()
        }
    }

    /// Books found so far.
    pub(crate) fn found(&self) -> usize {
        self.found
    }

    /// One step: a few entries of a folder, at most one book opened
    /// (a book read for the first time); `true` when the summary is
    /// complete and has replaced the old one.
    pub(crate) fn step(&mut self, env: &Env<'_>) -> bool {
        let Some(dir) = self.dir.clone().or_else(|| self.dirs.pop()) else {
            let _ = files::move_to(env.storage, &building_path(), &summary_path());
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
        let mut lines = String::new();
        for entry in entries {
            let path = files::join(&dir, &entry.name);
            let mut opened = false;
            if entry.is_dir {
                // Not into hidden folders (PaperPaw's own).
                if !entry.name.starts_with('.') {
                    self.dirs.push(path);
                }
            } else {
                let known = library::book_sized(env.storage, &path, entry.size);
                let book = match known {
                    Some(book) if book.indexed => book,
                    _ => {
                        opened = true;
                        library::index(env, &path)
                    }
                };
                lines.push_str(&Item::of(&path, &book).text());
                self.found += 1;
            }
            self.last = Some(entry);
            // Opening a book takes a moment: the rest in the next step.
            if opened {
                break;
            }
        }
        let _ = env.storage.append(&building_path(), lines.as_bytes());
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn written_and_read_back() {
        let storage = MemoryStorage::new();
        let item = Item {
            path: "books/b.epub".into(),
            title: "The Sign\tof the Four".into(),
            author: "Doyle".into(),
            series: "Sherlock Holmes".into(),
            number: "2".into(),
            progress: Some(100),
        };
        let new = Item {
            path: "books/a.txt".into(),
            title: "A".into(),
            ..Item::default()
        };
        write(&storage, &[item.clone(), new.clone()]);
        let mut all = Vec::new();
        each(&storage, &mut |i| all.push(i));
        assert_eq!(all[0].title, "The Sign of the Four");
        assert_eq!(all[0].state(), State::Finished);
        assert_eq!(all[1], new);
        assert_eq!(all[1].state(), State::New);
        assert_eq!(all[0].series_number(), 2.0);
        assert_eq!(all[0].title_key(), "sign of the four");
        assert_eq!(initial("The Time Machine"), 'T');
        assert_eq!(initial("émile"), 'É');
        assert_eq!(initial("1984"), '#');
        let words = ["doyle".to_string(), "sign".to_string()];
        assert!(all[0].matches(&words));
        assert!(!all[1].matches(&words));
    }
}
