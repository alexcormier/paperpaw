//! Making the catalog of a large library takes a few kilobytes, whatever
//! its size: a walk reads the old catalog along with it instead of
//! holding it, lists folders in pieces, and writes a few books a step
//! (the X4 has about 192 KiB of heap in all, the library's index of 2000
//! books 48 KB of it).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_fs::DirStorage;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::library::catalog::{self, Rewrite, Walk};
use paperpaw_sdk::{Env, FormatSpec, Formats, Id, Storage};

/// Counts the bytes allocated now, and the most since the peak was reset.
struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes every call on to the system's allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as the caller asked.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most the heap grows above where it is while `run` runs.
fn growth(run: impl FnOnce()) -> usize {
    let before = NOW.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    run();
    PEAK.load(Ordering::Relaxed) - before
}

/// Books in the library, in a few folders.
const BOOKS: usize = 2400;
/// The most making a catalog may take beside what is there.
const BOUND: usize = 12 * 1024;

fn walk(env: &Env<'_>) {
    let mut walk = Walk::start(env, Vec::new()).unwrap();
    while !walk.step(env).unwrap() {}
}

#[test]
fn making_the_catalog_of_a_large_library_takes_a_few_kilobytes() {
    let mut h = common::Harness::new();
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))
        .unwrap();
    h.formats = formats;
    // On disk: what the files hold is not counted.
    let root = std::env::temp_dir().join(format!("paperpaw-catalog-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let storage = DirStorage::new(&root).unwrap();
    storage.create_dir("books").unwrap();
    for shelf in 0..6 {
        storage.create_dir(&format!("books/Shelf {shelf}")).unwrap();
    }
    for n in 0..BOOKS {
        let path = format!("books/Shelf {}/A rather long book title {n:04}.txt", n % 6);
        storage.write(&path, b"text").unwrap();
    }
    let env = h.env_on(&storage);
    // The first walk reads every book's details (none were known).
    let first = growth(|| walk(&env));
    let mut count = 0;
    catalog::each(&storage, &mut |_| count += 1);
    assert_eq!(count, BOOKS);
    // Again: every book from the old catalog, read along.
    let again = growth(|| walk(&env));
    // Caught up: the old catalog copied a few lines a step.
    let caught_up = growth(|| {
        let fresh = vec![String::from(
            "books/Shelf 0/A rather long book title 0000.txt",
        )];
        let mut rewrite = Rewrite::start(&env, fresh).unwrap();
        while !rewrite.step(&env).unwrap() {}
    });
    for (what, took) in [
        ("first walk", first),
        ("walk", again),
        ("catch-up", caught_up),
    ] {
        assert!(took <= BOUND, "{what} took {took} bytes");
    }
    let _ = std::fs::remove_dir_all(&root);
}
