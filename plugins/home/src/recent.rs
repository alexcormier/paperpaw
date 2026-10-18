//! The recently read books, from the library service
//! ([`quireink_sdk::library`]), which follows the standard reading
//! events, so any reader plugin works.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::document::ImageRef;
use quireink_sdk::{Storage, library};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Recent {
    pub path: String,
    pub title: String,
    /// Percent, 0 to 100.
    pub progress: f32,
    /// The library read the book itself (its title, its cover).
    pub indexed: bool,
    /// Its cover, where it has one and the library read it.
    pub cover: Option<ImageRef>,
}

/// The `limit` books read last that are still there, most recent first.
/// On a card, each book read costs looking two files up: only as many as
/// shown are read.
pub(crate) fn load(storage: &dyn Storage, limit: usize) -> Vec<Recent> {
    library::opened_books(storage)
        .into_iter()
        .filter_map(|path| {
            // The details belong to the file there (its size checked);
            // without them, a book gone is not shown.
            let book = match library::book(storage, &path) {
                Some(book) => book,
                None if storage.exists(&path) => library::Book::default(),
                None => return None,
            };
            Some(Recent {
                title: book.title_or(&path).to_string(),
                progress: f32::from(book.progress.unwrap_or(0)),
                indexed: book.indexed,
                cover: book.cover,
                path,
            })
        })
        .take(limit)
        .collect()
}
