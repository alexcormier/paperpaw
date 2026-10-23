//! Where each book was left, shared like its annotations: any reader
//! keeps it, and plugins read it (a sync, a list of books) without
//! opening the book or knowing the reader. A file per book in
//! `.paperpaw/positions`, moved and removed with the book.
//!
//! ```ini
//! [position]
//! section = 7
//! offset = 1234
//! percent = 41.5
//! conversion = 00000000000000ff
//! xpointer = "/body/DocFragment[8]/body/p[3]/text()[1].12"
//! updated = 1760000000
//! ```

use alloc::format;
use alloc::string::{String, ToString};

use crate::document::Locator;
use crate::{Storage, StorageError};

/// The folder of the positions (registered as book data by the kernel).
pub const DIR: &str = crate::data_path!("positions");

/// The largest position file read.
const LIMIT: usize = 4096;

/// Where a book was left.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Position {
    /// The start of the page shown.
    pub at: Locator,
    /// How far into the book, in percent.
    pub percent: f32,
    /// Which conversion of the book `at` is in, as the reader names it
    /// (its options, see [`reading::open_options`](crate::reading::open_options)):
    /// offsets of one conversion need not fit another.
    pub conversion: Option<u64>,
    /// The place as an XPointer, which fits every conversion (and
    /// KOReader), where the reader kept one: when the book was closed.
    pub xpointer: Option<String>,
    /// When it was left, in seconds since 1970 (0 where the clock was not
    /// known).
    pub updated: i64,
}

impl Position {
    pub fn new(at: Locator, percent: f32) -> Self {
        Self {
            at,
            percent,
            conversion: None,
            xpointer: None,
            updated: 0,
        }
    }

    pub fn conversion(mut self, conversion: u64) -> Self {
        self.conversion = Some(conversion);
        self
    }

    pub fn xpointer(mut self, xpointer: impl Into<Option<String>>) -> Self {
        self.xpointer = xpointer.into();
        self
    }

    pub fn updated(mut self, updated: i64) -> Self {
        self.updated = updated;
        self
    }

    /// Read from a position file's text (also a reader's `progress.ini`
    /// of before, whose section is `[progress]`).
    pub fn parse(text: &str) -> Option<Position> {
        let (mut section, mut offset, mut percent) = (None, None, 0.0);
        let mut position = Position::new(Locator::START, 0.0);
        for entry in crate::config::parse(text).filter_map(Result::ok) {
            if entry.section != "position" && entry.section != "progress" {
                continue;
            }
            match entry.key {
                "section" => section = entry.value.as_u32(),
                "offset" => offset = entry.value.as_u32(),
                "percent" => percent = entry.value.as_str().parse().unwrap_or(0.0),
                "conversion" => {
                    position.conversion = u64::from_str_radix(&entry.value.as_str(), 16).ok();
                }
                "xpointer" => position.xpointer = Some(entry.value.as_str().to_string()),
                "updated" => position.updated = entry.value.as_i64().unwrap_or(0),
                _ => {}
            }
        }
        position.at = Locator::new(section?, offset?);
        position.percent = percent;
        Some(position)
    }

    /// As a position file's text.
    pub fn text(&self) -> String {
        let mut text = format!(
            "[position]\nsection = {}\noffset = {}\npercent = {:.1}\n",
            self.at.section, self.at.offset, self.percent
        );
        if let Some(conversion) = self.conversion {
            text.push_str(&format!("conversion = {conversion:016x}\n"));
        }
        if let Some(xpointer) = &self.xpointer {
            text.push_str(&format!("xpointer = {}\n", crate::config::quote(xpointer)));
        }
        if self.updated != 0 {
            text.push_str(&format!("updated = {}\n", self.updated));
        }
        text
    }
}

/// The position file of the book at `book`.
pub fn file(book: &str) -> String {
    format!("{DIR}/{}.txt", crate::library::key(book))
}

/// Where the book at `book` was left, if it was kept and can be read.
pub fn load(storage: &dyn Storage, book: &str) -> Option<Position> {
    load_for_update(storage, book).ok().flatten()
}

/// Where the book at `book` was left, `Ok(None)` if none was kept: an
/// error where the file is there but cannot be read (or is no position),
/// so it is not written over before the reader moves on.
pub fn load_for_update(
    storage: &dyn Storage,
    book: &str,
) -> Result<Option<Position>, StorageError> {
    let data = match crate::read_file(storage, &file(book), LIMIT) {
        Ok(data) => data,
        Err(StorageError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let text = core::str::from_utf8(crate::files::without_bom(&data))
        .map_err(|_| StorageError::InvalidData)?;
    Position::parse(text)
        .map(Some)
        .ok_or(StorageError::InvalidData)
}

/// Keeps where the book at `book` was left.
pub fn save(storage: &dyn Storage, book: &str, position: &Position) -> Result<(), StorageError> {
    let _ = storage.create_dir(DIR);
    storage.write(&file(book), position.text().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn positions_are_kept_and_read_back() {
        let storage = MemoryStorage::new();
        assert_eq!(load(&storage, "books/a.epub"), None);
        let position = Position::new(Locator::new(7, 1234), 41.5)
            .conversion(255)
            .xpointer(String::from("/body/DocFragment[8]/body/p[3]/text()[1].12"))
            .updated(1_760_000_000);
        save(&storage, "books/a.epub", &position).unwrap();
        assert_eq!(load(&storage, "books/a.epub"), Some(position));
        assert!(file("books/a.epub").starts_with(".paperpaw/positions/"));
    }

    /// A reader's `progress.ini` of version 1 reads as a position.
    #[test]
    fn a_progress_file_of_before_is_read() {
        let old = "[progress]\nsection = 7\noffset = 1234\npercent = 41.5\n";
        let position = Position::parse(old).unwrap();
        assert_eq!(position, Position::new(Locator::new(7, 1234), 41.5));
        assert_eq!(Position::parse("[position]\nsection = 1\n"), None);
    }

    /// A position file that is there but cannot be read is no missing
    /// one: it is not to be written over.
    #[test]
    fn a_position_that_cannot_be_read_is_told_from_none() {
        let book = "books/a.epub";
        let storage = MemoryStorage::new();
        assert_eq!(load_for_update(&storage, book), Ok(None));
        storage
            .write(&file(book), b"[position]\nsection = 1\noff")
            .unwrap();
        assert_eq!(
            load_for_update(&storage, book),
            Err(StorageError::InvalidData)
        );
        assert_eq!(load(&storage, book), None);
        let failing = crate::memory_storage::Unreadable(
            MemoryStorage::new().with(&file(book), "[position]\nsection = 1\noffset = 2\n"),
        );
        assert!(load_for_update(&failing, book).is_err());
    }
}
