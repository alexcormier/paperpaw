//! A library of thousands of plugins takes little memory to browse: the
//! newest and a group sorted by date are picked without holding every
//! plugin at once (the X4 has about 192 KiB of heap in all).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Shopping;
use paperpaw_sdk::builtin::actions;

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

/// Plugins enough that holding each would take 40 KB.
const PLUGINS: usize = 5000;

fn list() -> String {
    let plugins: Vec<String> = (0..PLUGINS)
        .map(|i| {
            let day = 1 + (i * 7919) % 28;
            format!(
                r#"{{"id": "p{i:05}", "name": "Plugin {i:05}", "summary": "Does {i}.",
  "group": "reading", "author": "A", "licence": "MIT", "kind": "data",
  "added": "2026-02-{day:02}",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 10, "date": "2026-03-{day:02}"}}}}"#
            )
        })
        .collect();
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, plugins.join(", "))
}

/// Moves the focus to the entry that begins with `label`.
fn focus(shopping: &mut Shopping, label: &str) {
    for _ in 0..50 {
        shopping.kernel.render(&mut |_, _| {});
        let on = shopping
            .kernel
            .describe()
            .into_iter()
            .any(|p| p.focused && p.label.starts_with(label));
        if on {
            return;
        }
        shopping.run(actions::DOWN);
    }
    panic!("{label}: {}", shopping.shown());
}

/// Far below what holding every plugin's key would take (40 KB, and twice
/// that while a vector grows).
const MOST: usize = 24 * 1024;

#[test]
fn thousands_of_plugins_are_sorted_in_little_memory() {
    // The store's files on the disk, as on a card: only what the store
    // holds counts.
    let storage = common::Shared::on_disk("memory");
    let mut shopping = Shopping::with(Some("x4"), None, &list(), storage);
    // The newest: the 30 best kept while reading.
    focus(&mut shopping, "New");
    let grew = growth(|| shopping.run(actions::CONFIRM));
    assert!(grew < MOST, "the newest: {grew} bytes");
    let shown = shopping.shown();
    assert!(shown.starts_with("New"), "{shown}");
    shopping.run(actions::BACK);
    // A group of all of them, newest first: sorted in runs on the storage.
    shopping.choose("Reading");
    shopping.choose("Sort by");
    focus(&mut shopping, "Newest");
    let grew = growth(|| shopping.run(actions::CONFIRM));
    assert!(grew < MOST, "a group by date: {grew} bytes");
    let shown = shopping.shown();
    assert!(shown.contains("Sort by, Newest"), "{shown}");
    drop(shopping);
    let root = std::env::temp_dir().join(format!("paperpaw-store-memory-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(root);
}
