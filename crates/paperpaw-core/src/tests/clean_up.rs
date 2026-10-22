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

/// With a clock, the storage is cleaned up by itself while charging, a
/// day after it was first seen and then weekly; not before.
#[test]
fn cleaning_up_runs_by_itself_while_charging_now_and_then() {
    use paperpaw_platform::{Battery, SystemStatus, WallClock};
    let annotations = |path: &str| alloc::format!(".paperpaw/annotations/{}", key(path));
    let storage = MemoryStorage::new().with(&annotations("Books/gone.txt"), "a note");
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    let status = |charging: bool| SystemStatus {
        clock: Some(WallClock {
            at: Instant(0),
            unix_millis: 1_800_000_000_000,
            utc_offset_minutes: None,
        }),
        battery: Some(Battery {
            percent: 50,
            charging,
            millivolts: None,
        }),
    };
    kernel.set_status(Instant(0), status(false));
    kernel.start(Instant(0));
    let gone = annotations("Books/gone.txt");
    // Charging at once: too early.
    kernel.set_status(Instant(1000), status(true));
    kernel.tick(Instant(2000));
    assert!(kernel.storage().exists(&gone));
    // A day later, charging: cleaned up, and the time noted.
    let day = 24 * 3600 * 1000;
    kernel.set_status(Instant(day + 1000), status(false));
    kernel.set_status(Instant(day + 2000), status(true));
    for at in [day + 3000, day + 4000, day + 5000] {
        kernel.tick(Instant(at));
    }
    assert!(!kernel.storage().exists(&gone));
    let noted = paperpaw_sdk::read_file(kernel.storage(), ".paperpaw/cleaned-up.txt", 32).unwrap();
    // At the first tick after charging began.
    assert_eq!(noted, b"1800086403\n");
}
