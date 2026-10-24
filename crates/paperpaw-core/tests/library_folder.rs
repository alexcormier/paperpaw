//! The library's folder is there from the first start on: made where it
//! is not, as the setting names it, and never removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{DirEntry, Instant, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

/// Files in memory, and the folders made (a memory storage has folders
/// only through their files).
#[derive(Clone, Default)]
struct Card {
    files: Rc<MemoryStorage>,
    folders: Rc<RefCell<BTreeSet<String>>>,
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.files.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        match self.files.list(dir, visit) {
            Err(_) if self.folders.borrow().contains(dir) => Ok(()),
            result => result,
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.files.exists(path)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.folders.borrow_mut().insert(path.to_string());
        Ok(())
    }
}

/// A kernel started on `card`, with `config`.
fn start(card: &Card, config: &str) {
    if !config.is_empty() {
        card.files.write("paperpaw.ini", config.as_bytes()).unwrap();
    }
    let mut kernel = Kernel::builder(common::DEVICE)
        .storage(card.clone())
        .build()
        .unwrap();
    let _ = kernel.start(Instant(0));
}

#[test]
fn the_library_folder_is_made_at_the_first_start() {
    let card = Card::default();
    start(&card, "");
    assert!(card.folders.borrow().contains("books"));
}

#[test]
fn a_library_folder_set_deeper_is_made_with_its_parents() {
    let card = Card::default();
    start(
        &card,
        "[core]
library-folder = /Reading/Books
",
    );
    let folders = card.folders.borrow();
    assert!(
        folders.contains("Reading") && folders.contains("Reading/Books"),
        "{folders:?}"
    );
}

#[test]
fn a_library_folder_there_is_left_as_it_is() {
    let card = Card::default();
    card.files.write("books/alice.epub", b"book").unwrap();
    start(&card, "");
    assert!(card.files.exists("books/alice.epub"));
    assert!(card.folders.borrow().is_empty());
}
