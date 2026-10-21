//! Cleaning up what is kept for books no longer on the storage
//! (`core.clean-up`).

use super::*;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::library::key;

/// What is kept for a file still on the storage stays, also where no
/// plugin opens its format now (switched off); what is kept for a file
/// gone goes.
#[test]
fn cleaning_up_keeps_what_belongs_to_files_still_there() {
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let storage = MemoryStorage::new()
        .with("Books/a.txt", "a book no plugin reads now")
        .with(&annotations("Books/a.txt"), "a note")
        .with(&annotations("Books/gone.txt"), "a note");
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(NoticePlugin { log: log() })
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(10), actions::CLEAN_UP);
    let storage = kernel.storage();
    assert!(storage.exists(&annotations("Books/a.txt")), "kept");
    assert!(!storage.exists(&annotations("Books/gone.txt")), "left over");
}

/// On a memory card, which ignores case, what is kept for a book stays
/// whether the library folder is spelt as its setting says (`books`) or
/// as the card has it (`Books`).
#[test]
fn cleaning_up_keeps_books_by_every_spelling_of_their_folder() {
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let storage = MemoryStorage::new()
        .with("Books/a.txt", "a book")
        .with(&annotations("books/a.txt"), "a note")
        .with(&annotations("Books/a.txt"), "a note");
    // The kernel's library folder, `books` by default.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(super::storage::Fat(storage))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(10), actions::CLEAN_UP);
    let storage = kernel.storage();
    assert!(
        storage.exists(&annotations("books/a.txt")),
        "as the setting"
    );
    assert!(storage.exists(&annotations("Books/a.txt")), "as the card");
}
