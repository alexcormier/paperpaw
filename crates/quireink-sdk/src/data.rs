//! Versioned formats of the files plugins keep their data in.
//!
//! What QuireInk keeps on the card stays readable by every later version:
//! a file's format has a version, and a plugin that changes it in a way
//! older readers would misread raises the version and adds the step from
//! the one before ([`Format::upgrades`]). Files are upgraded as they are
//! read, in memory, and written in the new format when they are saved
//! next. Adding a key to an ini file needs no new version: readers skip
//! keys they do not know.
//!
//! A text file says its version in its first line, `# format 2`; a file
//! without that line is version 1, so files from before versions came
//! need no change. A firmware or app older than a file (after going back
//! to an older update) does not read it and does not write over it: it
//! stays as the newer version left it ([`Error::Newer`]).
//!
//! Binary files change their name for a new version instead (e.g.
//! `pages.bin` to `pages2.bin`), caches their stamp: they are made again.
//! Each format's versions have files in the repository that the tests
//! read (see `docs/data-files.md`).

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;

use quireink_platform::{Storage, StorageError};

/// The first line of a file from version 2 on, before the number.
pub const MARK: &str = "# format ";

/// One step: a file's text in version `n`, made into version `n + 1`.
pub type Upgrade = fn(&str) -> String;

/// A format of a text file and how its older versions become this one.
#[derive(Debug, Clone, Copy)]
pub struct Format {
    /// The version written, from 1.
    pub version: u32,
    /// From version `n` to `n + 1` for `n` from 1: one less than
    /// [`Format::version`].
    pub upgrades: &'static [Upgrade],
}

/// Why a file is not read or not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Written by a newer QuireInk (format version this): left as it is.
    Newer(u32),
    /// Not text, or not as the format says.
    Unreadable,
    Storage(StorageError),
}

impl Format {
    /// Format `version` with the steps from version 1 to it.
    pub const fn new(version: u32, upgrades: &'static [Upgrade]) -> Self {
        assert!(
            version >= 1 && upgrades.len() as u32 == version - 1,
            "one upgrade per version after the first"
        );
        Self { version, upgrades }
    }

    /// The version of a file's `text` (see the [module documentation](self)).
    pub fn version_of(text: &str) -> u32 {
        text.lines()
            .next()
            .and_then(|line| line.trim().strip_prefix(MARK))
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(1)
    }

    /// A file's `text` in this version, without its first line of the
    /// version: as it is if it is this version, upgraded step by step if
    /// it is older.
    pub fn read<'a>(&self, text: &'a str) -> Result<Cow<'a, str>, Error> {
        let mut version = Self::version_of(text);
        if version > self.version {
            return Err(Error::Newer(version));
        }
        let body = if version > 1 {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            text
        };
        let mut body = Cow::Borrowed(body);
        while version < self.version {
            let step = self.upgrades[version as usize - 1];
            body = Cow::Owned(step(&body));
            version += 1;
        }
        Ok(body)
    }

    /// The file at `path` in this version (see [`Format::read`]);
    /// `Ok(None)` if there is none. Files larger than `limit` are refused.
    pub fn load(
        &self,
        storage: &dyn Storage,
        path: &str,
        limit: usize,
    ) -> Result<Option<String>, Error> {
        if !storage.exists(path) {
            return Ok(None);
        }
        let data = crate::read_file(storage, path, limit).map_err(Error::Storage)?;
        let text = String::from_utf8(data).map_err(|_| Error::Unreadable)?;
        Ok(Some(self.read(&text)?.into_owned()))
    }

    /// `body` as a file of this version: with its first line from version
    /// 2 on.
    pub fn text(&self, body: &str) -> String {
        if self.version > 1 {
            format!("{MARK}{}\n{body}", self.version)
        } else {
            String::from(body)
        }
    }

    /// Writes `body` to `path` in this version, unless a newer QuireInk
    /// wrote the file there ([`Error::Newer`]).
    pub fn save(&self, storage: &dyn Storage, path: &str, body: &str) -> Result<(), Error> {
        let mut head = [0u8; 32];
        if let Ok(n) = storage.read_at(path, 0, &mut head) {
            let head = core::str::from_utf8(&head[..n]).unwrap_or_default();
            // The first line may be cut: only a whole one counts.
            if head.contains('\n') {
                let version = Self::version_of(head);
                if version > self.version {
                    return Err(Error::Newer(version));
                }
            }
        }
        storage
            .write(path, self.text(body).as_bytes())
            .map_err(Error::Storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_storage::MemoryStorage;

    /// Version 2 renamed `colour` to `color`, version 3 added a section.
    const FORMAT: Format = Format::new(
        3,
        &[
            |v1| v1.replace("colour", "color"),
            |v2| format!("[look]\n{v2}"),
        ],
    );

    #[test]
    fn older_files_are_upgraded_step_by_step() {
        assert_eq!(
            FORMAT.read("colour = red\n").unwrap(),
            "[look]\ncolor = red\n"
        );
        assert_eq!(
            FORMAT.read("# format 2\ncolor = red\n").unwrap(),
            "[look]\ncolor = red\n"
        );
        let current = FORMAT.read("# format 3\n[look]\ncolor = red\n").unwrap();
        assert!(matches!(current, Cow::Borrowed("[look]\ncolor = red\n")));
        assert_eq!(FORMAT.read("# format 4\nx\n"), Err(Error::Newer(4)));
        // A comment that is no mark is version 1.
        assert_eq!(Format::version_of("# my colours\ncolour = red"), 1);
    }

    #[test]
    fn saved_with_its_version_and_never_over_a_newer_file() {
        let storage = MemoryStorage::new().with("new.ini", "# format 9\nwhatever\n");
        FORMAT
            .save(&storage, "a.ini", "[look]\ncolor = blue\n")
            .unwrap();
        assert_eq!(
            FORMAT.load(&storage, "a.ini", 1024).unwrap().unwrap(),
            "[look]\ncolor = blue\n"
        );
        assert_eq!(
            crate::read_file(&storage, "a.ini", 1024).unwrap(),
            b"# format 3\n[look]\ncolor = blue\n"
        );
        assert_eq!(FORMAT.save(&storage, "new.ini", "x"), Err(Error::Newer(9)));
        assert_eq!(FORMAT.load(&storage, "new.ini", 1024), Err(Error::Newer(9)));
        assert_eq!(FORMAT.load(&storage, "none.ini", 1024), Ok(None));
        // Version 1 is written as before versions came.
        let v1 = Format::new(1, &[]);
        v1.save(&storage, "b.txt", "a\nb\n").unwrap();
        assert_eq!(
            crate::read_file(&storage, "b.txt", 1024).unwrap(),
            b"a\nb\n"
        );
    }
}
