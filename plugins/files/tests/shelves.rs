//! The shelves (by author, series, tag) with the real kernel: their
//! catalog is made once and again only after the library changed, and
//! making it goes on where another screen came on top meanwhile.
//!
//! One test only: the catalog's freshness is known for the whole program,
//! as on a device, so tests of it run one after the other.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

/// A card that counts how often the library folder is listed.
struct Counted {
    inner: Rc<MemoryStorage>,
    listings: Rc<Cell<usize>>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if dir == "books" {
            self.listings.set(self.listings.get() + 1);
        }
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
}

/// Runs the timers asked for, `ticks` of them at most.
fn run(kernel: &mut Kernel, mut effects: Effects, ticks: usize) {
    let mut now = 0;
    for _ in 0..ticks {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
}

fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    let labels: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    labels.join("\n")
}

/// The tests' turns with the library: when its catalog was made is kept
/// for the whole program, as on a device, so tests that make or read it
/// take turns rather than run beside each other.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Waits for this test's turn with the library.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn the_catalog_is_made_once_and_after_changes() {
    let _turn = turn();
    let storage = Rc::new((0..40).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("books/book {i:02}.txt"), "Some text.")
    }));
    let listings = Rc::new(Cell::new(0));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Counted {
            inner: Rc::clone(&storage),
            listings: Rc::clone(&listings),
        })
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_files::Files)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    run(&mut kernel, effects, 100);
    let by_author = paperpaw_plugin_files::BY_AUTHOR;

    // The library (the home screen here) made the catalog; by author
    // shows it.
    let effects = kernel.run_action(Instant(0), by_author.clone());
    run(&mut kernel, effects, 1000);
    let text = shown(&mut kernel);
    assert!(text.contains("40 books"), "{text}");
    let effects = kernel.run_action(Instant(0), actions::BACK);
    run(&mut kernel, effects, 100);

    // Without a catalog, the library covered: by author makes it. Its
    // first steps, then a full screen on top; back, it goes on to the end.
    let info = Params::new().with(params::PATH, "books/book 02.txt");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_files::INFO_SCREEN, info);
    run(&mut kernel, effects, 100);
    storage.remove(".paperpaw/library/catalog.txt").unwrap();
    let effects = kernel.send_event(
        Instant(0),
        events::LIBRARY_CHANGED.id().clone(),
        Params::new(),
    );
    run(&mut kernel, effects, 100);
    let effects = kernel.run_action(Instant(0), by_author.clone());
    run(&mut kernel, effects, 2);
    assert!(
        shown(&mut kernel).contains("Collecting"),
        "{}",
        shown(&mut kernel)
    );
    let info = Params::new().with(params::PATH, "books/book 01.txt");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_files::INFO_SCREEN, info);
    run(&mut kernel, effects, 100);
    let effects = kernel.run_action(Instant(0), actions::BACK);
    run(&mut kernel, effects, 1000);
    let text = shown(&mut kernel);
    assert!(text.contains("40 books"), "{text}");
    // Back to the library, past the information.
    let effects = kernel.run_action(Instant(0), actions::BACK);
    run(&mut kernel, effects, 100);

    // Shown again: the catalog made is taken as it is.
    let effects = kernel.run_action(Instant(0), actions::BACK);
    run(&mut kernel, effects, 100);
    listings.set(0);
    let effects = kernel.run_action(Instant(0), by_author.clone());
    run(&mut kernel, effects, 1000);
    assert!(shown(&mut kernel).contains("40 books"));
    assert_eq!(listings.get(), 0, "made anew without a change");

    // After a change it is made anew.
    let effects = kernel.run_action(Instant(0), actions::BACK);
    run(&mut kernel, effects, 100);
    let effects = kernel.send_event(
        Instant(0),
        events::LIBRARY_CHANGED.id().clone(),
        Params::new(),
    );
    run(&mut kernel, effects, 100);
    let effects = kernel.run_action(Instant(0), Id::new_static("files.by-series"));
    run(&mut kernel, effects, 1000);
    assert!(listings.get() > 0, "not made anew after a change");
}
