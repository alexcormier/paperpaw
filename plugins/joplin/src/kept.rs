//! What the plugin keeps in its folder, a line each (see
//! `docs/data-files.md`), read line by line so their length does not
//! matter:
//!
//! - `sent.txt`: the books whose notes are in Joplin: the notes file,
//!   the note's id and the MD5 of its text as written last
//!   (`path<TAB>id<TAB>md5`).
//! - `waiting.txt`: the notes files still to send.
//! - `notebook.txt`: the notebook made for the notes: its name and id.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::files::for_each_line;
use paperpaw_sdk::{Env, Storage, StorageError};

use crate::PLUGIN;

/// A file in the plugin's folder.
pub fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

const SENT: &str = "sent.txt";
const WAITING: &str = "waiting.txt";
const NOTEBOOK: &str = "notebook.txt";

/// A book's note in Joplin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    /// The MD5 of its text as written (see `item::text_hash`).
    pub text: String,
}

/// The first line of `name` whose first field is `key`, its fields.
fn find(storage: &dyn Storage, name: &str, key: &str) -> Option<Vec<String>> {
    let mut found = None;
    let _ = for_each_line(storage, &file(name), &mut |line| {
        let mut fields = line.split('\t');
        if fields.next() != Some(key) {
            return ControlFlow::Continue(());
        }
        found = Some(fields.map(String::from).collect());
        ControlFlow::Break(())
    });
    found
}

/// Writes `name` anew without the lines whose first field is `key`, with
/// `line` at the end if given: through a copy, line by line.
fn rewrite(
    storage: &dyn Storage,
    name: &str,
    key: &str,
    line: Option<&str>,
) -> Result<(), StorageError> {
    let (path, copy) = (file(name), file(&format!("{name}.new")));
    storage.write(&copy, &[])?;
    let mut result = Ok(());
    let read = for_each_line(storage, &path, &mut |old| {
        if old.is_empty() || old.split('\t').next() == Some(key) {
            return ControlFlow::Continue(());
        }
        result = storage.append(&copy, format!("{old}\n").as_bytes());
        if result.is_ok() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    });
    match read {
        Ok(()) | Err(StorageError::NotFound) => {}
        Err(e) => result = Err(e),
    }
    if let Some(line) = line {
        result = result.and_then(|()| storage.append(&copy, format!("{line}\n").as_bytes()));
    }
    match result {
        Ok(()) => storage.rename(&copy, &path),
        Err(e) => {
            let _ = storage.remove(&copy);
            Err(e)
        }
    }
}

/// The note of the notes file `path`, if it was sent.
pub fn sent(storage: &dyn Storage, path: &str) -> Option<Sent> {
    let fields = find(storage, SENT, path)?;
    let [id, text] = <[String; 2]>::try_from(fields).ok()?;
    Some(Sent { id, text })
}

/// Keeps that the notes file `path` was sent as `note`.
pub fn keep_sent(storage: &dyn Storage, path: &str, note: &Sent) -> Result<(), StorageError> {
    let line = format!("{path}\t{}\t{}", note.id, note.text);
    rewrite(storage, SENT, path, Some(&line))
}

/// The notes files waiting to be sent, the oldest first.
pub fn waiting(storage: &dyn Storage) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let _ = for_each_line(storage, &file(WAITING), &mut |line| {
        if !line.is_empty() && !paths.iter().any(|p| p == line) {
            paths.push(line.to_string());
        }
        ControlFlow::Continue(())
    });
    paths
}

/// Adds the notes file `path` to those waiting, at the end.
pub fn add_waiting(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    rewrite(storage, WAITING, path, Some(path))
}

/// The notes file `path` waits no more.
pub fn remove_waiting(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    rewrite(storage, WAITING, path, None)
}

/// The notebook made for the notes: its name and id.
pub fn notebook(storage: &dyn Storage) -> Option<(String, String)> {
    let mut kept = None;
    let _ = for_each_line(storage, &file(NOTEBOOK), &mut |line| {
        kept = line
            .split_once('\t')
            .map(|(name, id)| (name.to_string(), id.to_string()));
        ControlFlow::Break(())
    });
    kept
}

/// Keeps the notebook `name` made as `id`.
pub fn keep_notebook(storage: &dyn Storage, name: &str, id: &str) -> Result<(), StorageError> {
    storage.write(&file(NOTEBOOK), format!("{name}\t{id}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn keeps_what_was_sent() {
        let s = MemoryStorage::new();
        assert_eq!(sent(&s, "Notes/a.md"), None);
        let note = |id: &str, text: &str| Sent {
            id: id.to_string(),
            text: text.to_string(),
        };
        keep_sent(&s, "Notes/a.md", &note("1", "t1")).unwrap();
        keep_sent(&s, "Notes/b.md", &note("2", "t2")).unwrap();
        keep_sent(&s, "Notes/a.md", &note("1", "t3")).unwrap();
        assert_eq!(sent(&s, "Notes/a.md"), Some(note("1", "t3")));
        assert_eq!(sent(&s, "Notes/b.md"), Some(note("2", "t2")));
        assert_eq!(
            s.get(&file(SENT)).unwrap(),
            b"Notes/b.md\t2\tt2\nNotes/a.md\t1\tt3\n"
        );
        assert!(s.get(&file("sent.txt.new")).is_none());
    }

    #[test]
    fn keeps_what_waits() {
        let s = MemoryStorage::new();
        assert!(waiting(&s).is_empty());
        add_waiting(&s, "Notes/a.md").unwrap();
        add_waiting(&s, "Notes/b.md").unwrap();
        add_waiting(&s, "Notes/a.md").unwrap();
        assert_eq!(waiting(&s), ["Notes/b.md", "Notes/a.md"]);
        remove_waiting(&s, "Notes/b.md").unwrap();
        remove_waiting(&s, "Notes/c.md").unwrap();
        assert_eq!(waiting(&s), ["Notes/a.md"]);
    }

    #[test]
    fn keeps_the_notebook() {
        let s = MemoryStorage::new();
        assert_eq!(notebook(&s), None);
        keep_notebook(&s, "Books", "abc").unwrap();
        assert_eq!(
            notebook(&s),
            Some((String::from("Books"), String::from("abc")))
        );
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn files_of_version_1_are_read() {
        let s = MemoryStorage::new()
            .with(&file(SENT), include_str!("../testdata/sent.txt.v1"))
            .with(&file(WAITING), include_str!("../testdata/waiting.txt.v1"))
            .with(&file(NOTEBOOK), include_str!("../testdata/notebook.txt.v1"));
        assert_eq!(
            sent(&s, "Notes/Lewis Carroll - Alice.md"),
            Some(Sent {
                id: String::from("1ae79351dd744c2794eebb334b1629a7"),
                text: String::from("9e107d9d372bb6826bd81d3542a419d6"),
            })
        );
        assert_eq!(waiting(&s), ["Notes/Jane Austen - Emma.md"]);
        assert_eq!(
            notebook(&s),
            Some((
                String::from("PaperPaw"),
                String::from("7db793874c624138b3b4520ae5cdf20c")
            ))
        );
    }
}

#[cfg(test)]
mod roundtrip_tests;
