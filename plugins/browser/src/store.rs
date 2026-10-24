//! Bookmarks and the history: lists of pages in the plugin's folder, one
//! `title<TAB>address` line each.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{Env, Storage, StorageError};

use crate::PLUGIN;

/// Pages kept in the history; older ones are forgotten, with their files.
pub(crate) const HISTORY: usize = 30;

/// A page in a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub title: String,
    pub url: String,
}

impl Entry {
    pub(crate) fn new(title: &str, url: &str) -> Self {
        Self {
            title: title.to_string(),
            url: url.to_string(),
        }
    }
}

pub(crate) fn dir() -> String {
    Env::plugin_dir(PLUGIN.as_str())
}

pub(crate) fn bookmarks_path() -> String {
    format!("{}/bookmarks.txt", dir())
}

pub(crate) fn history_path() -> String {
    format!("{}/history.txt", dir())
}

/// Where pages are kept.
pub(crate) fn page_path(url: &str) -> String {
    format!("{}/pages/{}", dir(), crate::web::page_name(url))
}

/// A list, or `None` if it was never saved; an error where it is there
/// but cannot be read, so it is not written over.
pub(crate) fn load(storage: &dyn Storage, path: &str) -> Result<Option<Vec<Entry>>, StorageError> {
    let Some(text) = paperpaw_sdk::files::load_text_for_update(storage, path, 256 * 1024)? else {
        return Ok(None);
    };
    Ok(Some(
        text.lines()
            .filter_map(|line| line.split_once('\t'))
            .filter(|(_, url)| !url.is_empty())
            .map(|(title, url)| Entry::new(title, url))
            .collect(),
    ))
}

pub(crate) fn save(storage: &dyn Storage, path: &str, entries: &[Entry]) {
    let mut text = String::new();
    for entry in entries {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        text.push_str(&format!("{}\t{}\n", clean(&entry.title), clean(&entry.url)));
    }
    let _ = storage.write(path, text.as_bytes());
}

/// The bookmarks; until the user changes them, a few places to start
/// from, in the language of the device. `None` where they cannot be read:
/// then none are shown, and none saved over them.
pub(crate) fn bookmarks(env: &Env<'_>) -> Option<Vec<Entry>> {
    let saved = load(env.storage(), &bookmarks_path()).ok()?;
    Some(saved.unwrap_or_else(|| {
        let language = env.i18n().language();
        let language = language.split(['-', '_']).next().unwrap_or("en");
        Vec::from([
            Entry::new("Wikipedia", &format!("https://{language}.m.wikipedia.org/")),
            Entry::new("Project Gutenberg", "https://www.gutenberg.org/"),
            Entry::new("Standard Ebooks", "https://standardebooks.org/ebooks"),
        ])
    }))
}

/// Puts `entry` first in the history; pages that drop out of it are
/// deleted. Where the history cannot be read, nothing changes: `None`.
pub(crate) fn remember(storage: &dyn Storage, entry: Entry) -> Option<Vec<Entry>> {
    let mut history = load(storage, &history_path()).ok()?.unwrap_or_default();
    history.retain(|e| e.url != entry.url);
    history.insert(0, entry);
    for old in history.drain(HISTORY.min(history.len())..) {
        let _ = storage.remove(&page_path(&old.url));
    }
    save(storage, &history_path(), &history);
    Some(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn the_history_keeps_the_latest_pages() {
        let storage = MemoryStorage::new();
        for n in 0..HISTORY + 2 {
            let url = format!("https://a.org/{n}");
            storage.write(&page_path(&url), b"page").unwrap();
            remember(&storage, Entry::new("A\tB", &url)).unwrap();
        }
        // Visited again: first, once.
        let history = remember(&storage, Entry::new("Again", "https://a.org/5")).unwrap();
        assert_eq!(history.len(), HISTORY);
        assert_eq!(history[0], Entry::new("Again", "https://a.org/5"));
        assert_eq!(
            history
                .iter()
                .filter(|e| e.url == "https://a.org/5")
                .count(),
            1
        );
        // The oldest pages are gone.
        assert!(!storage.exists(&page_path("https://a.org/0")));
        assert!(storage.exists(&page_path("https://a.org/31")));
        let loaded = load(&storage, &history_path()).unwrap().unwrap();
        assert_eq!(loaded, history);
        assert_eq!(loaded[1].title, "A B");
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn lists_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::bookmarks_path(),
            include_str!("../testdata/bookmarks.txt.v1"),
        );
        let entries = super::load(&storage, &super::bookmarks_path())
            .unwrap()
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Wikipedia");
        assert_eq!(entries[1].url, "https://www.gutenberg.org/");
    }
}

#[cfg(test)]
mod format_tests;
