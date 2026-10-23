//! A folder's entries a page at a time, folders first, then by name (case
//! ignored), as [`files::list`](paperpaw_sdk::files::list) orders them,
//! without ever holding more than a page: a folder of thousands of files
//! fits the X4's memory.
//!
//! Pages are found as the library finds them (keyset pagination): one pass
//! over the folder keeps only the `n` entries that follow (or precede) a
//! known one in that order.

use alloc::vec::Vec;
use core::cmp::Ordering;

use paperpaw_sdk::files::Entry;
use paperpaw_sdk::{Storage, StorageError};

/// Folders first, then names regardless of case, then exact names (so the
/// order is total).
fn order(a: (bool, &str), b: (bool, &str)) -> Ordering {
    b.0.cmp(&a.0)
        .then_with(|| {
            a.1.chars()
                .flat_map(char::to_lowercase)
                .cmp(b.1.chars().flat_map(char::to_lowercase))
        })
        .then_with(|| a.1.cmp(b.1))
}

fn cmp(entry: &Entry, is_dir: bool, name: &str) -> Ordering {
    order((entry.is_dir, &entry.name), (is_dir, name))
}

/// The entries of folder `dir`; names starting with `.` only with
/// `hidden`.
pub(crate) struct Folder<'a> {
    pub storage: &'a dyn Storage,
    pub dir: &'a str,
    pub hidden: bool,
}

impl Folder<'_> {
    fn each(&self, f: &mut dyn FnMut(bool, &str, u64)) -> Result<(), StorageError> {
        self.storage.list(self.dir, &mut |e| {
            if self.hidden || !e.name.starts_with('.') {
                f(e.is_dir, e.name, e.size);
            }
        })
    }

    /// How many entries there are.
    pub(crate) fn count(&self) -> Result<usize, StorageError> {
        let mut count = 0;
        self.each(&mut |_, _, _| count += 1)?;
        Ok(count)
    }

    /// How many entries come before `entry` (where it is, or would be).
    pub(crate) fn rank(&self, entry: &Entry) -> Result<usize, StorageError> {
        let mut rank = 0;
        self.each(&mut |is_dir, name, _| {
            if cmp(entry, is_dir, name) == Ordering::Greater {
                rank += 1;
            }
        })?;
        Ok(rank)
    }

    /// The first `n` entries after `after` (from the start if `None`).
    pub(crate) fn after(
        &self,
        after: Option<&Entry>,
        n: usize,
    ) -> Result<Vec<Entry>, StorageError> {
        let mut page: Vec<Entry> = Vec::with_capacity(n);
        self.each(&mut |is_dir, name, size| {
            if n == 0 || after.is_some_and(|a| cmp(a, is_dir, name) != Ordering::Less) {
                return;
            }
            if page.len() == n && cmp(&page[n - 1], is_dir, name) == Ordering::Less {
                return;
            }
            let at = page.partition_point(|e| cmp(e, is_dir, name) == Ordering::Less);
            page.insert(at, entry(is_dir, name, size));
            page.truncate(n);
        })?;
        Ok(page)
    }

    /// The last `n` entries before `before` (up to the end if `None`).
    pub(crate) fn before(
        &self,
        before: Option<&Entry>,
        n: usize,
    ) -> Result<Vec<Entry>, StorageError> {
        let mut page: Vec<Entry> = Vec::with_capacity(n + 1);
        self.each(&mut |is_dir, name, size| {
            if n == 0 || before.is_some_and(|b| cmp(b, is_dir, name) != Ordering::Greater) {
                return;
            }
            if page.len() == n && cmp(&page[0], is_dir, name) == Ordering::Greater {
                return;
            }
            let at = page.partition_point(|e| cmp(e, is_dir, name) == Ordering::Less);
            page.insert(at, entry(is_dir, name, size));
            if page.len() > n {
                page.remove(0);
            }
        })?;
        Ok(page)
    }

    /// Page number `page` of `rows` entries of the `len` there are,
    /// walking from the nearer end.
    pub(crate) fn page(
        &self,
        page: usize,
        rows: usize,
        len: usize,
    ) -> Result<Vec<Entry>, StorageError> {
        let rows = rows.max(1);
        let pages = len.div_ceil(rows);
        if len == 0 {
            return Ok(Vec::new());
        }
        if page * 2 < pages {
            let mut entries = self.after(None, rows)?;
            for _ in 0..page {
                entries = self.after(entries.last(), rows)?;
            }
            Ok(entries)
        } else {
            let last = len - (pages - 1) * rows;
            let mut entries = self.before(None, last)?;
            for _ in page + 1..pages {
                entries = self.before(entries.first(), rows)?;
            }
            Ok(entries)
        }
    }

    /// The page of `rows` entries where `entry` is (or would be), and the
    /// index of its first entry.
    pub(crate) fn page_of(
        &self,
        entry: &Entry,
        rows: usize,
    ) -> Result<(usize, Vec<Entry>), StorageError> {
        let rows = rows.max(1);
        let rank = self.rank(entry)?;
        let mut entries = self.before(Some(entry), rank % rows)?;
        let rest = self.after(entries.last(), rows - entries.len())?;
        entries.extend(rest);
        Ok((rank - rank % rows, entries))
    }
}

fn entry(is_dir: bool, name: &str, size: u64) -> Entry {
    Entry::new(name, is_dir, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::String;
    use paperpaw_sdk::MemoryStorage;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// A folder with two folders, a hidden file and 25 files of both cases.
    fn card() -> MemoryStorage {
        let mut card = MemoryStorage::new()
            .with("d/Zeta/x", "")
            .with("d/alpha/y", "")
            .with("d/.hidden", "");
        for i in 0..25 {
            let name = format!("{}ile {i:02}.txt", if i % 2 == 0 { "F" } else { "f" });
            card = card.with(&format!("d/{name}"), "");
        }
        card
    }

    #[test]
    fn pages_follow_the_order_of_files_list() {
        let card = card();
        let folder = Folder {
            storage: &card,
            dir: "d",
            hidden: false,
        };
        let all: Vec<String> = paperpaw_sdk::files::list(&card, "d", false)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(&all[..2], ["alpha", "Zeta"]);
        let len = folder.count().unwrap();
        assert_eq!(len, all.len());
        let mut walked = Vec::new();
        for page in 0..len.div_ceil(4) {
            let entries = folder.page(page, 4, len).unwrap();
            walked.extend(names(&entries).into_iter().map(String::from));
        }
        assert_eq!(walked, all);
        // Page by page forwards and back, as the buttons go.
        let second = folder.after(None, 4).unwrap();
        let third = folder.after(second.last(), 4).unwrap();
        assert_eq!(folder.before(third.first(), 4).unwrap(), second);
    }

    #[test]
    fn the_page_of_an_entry() {
        let card = card();
        let folder = Folder {
            storage: &card,
            dir: "d",
            hidden: true,
        };
        let target = entry(false, "file 13.txt", 0);
        let (first, page) = folder.page_of(&target, 6).unwrap();
        // `.hidden`, two folders, then 13 files before it: index 16.
        assert_eq!(first, 12);
        assert_eq!(page[16 - 12].name, "file 13.txt");
        assert_eq!(page.len(), 6);
    }

    #[test]
    fn an_empty_or_missing_folder() {
        let card = MemoryStorage::new();
        let folder = Folder {
            storage: &card,
            dir: "",
            hidden: false,
        };
        assert_eq!(folder.count().unwrap(), 0);
        assert!(folder.page(0, 8, 0).unwrap().is_empty());
        let missing = Folder {
            storage: &card,
            dir: "gone",
            hidden: false,
        };
        assert!(missing.count().is_err());
    }
}
