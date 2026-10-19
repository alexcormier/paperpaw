//! Reading a book with the real kernel, the text and EPUB formats and the
//! default fonts: pages turn both ways, the position is
//! kept and the book continues there, a larger font keeps the place,
//! bookmarks are kept, and plugins hear of it all, the chapter too.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_reader::{Reader, actions};
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::builtin::{actions as core_actions, events, params, slots};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass,
    MemoryStorage, Params, PixelFormat, Plugin, RegisterError, Registrar, SDK_VERSION, Storage,
    StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

const BOOK: &str = "books/alice.txt";
/// The same book with a table of contents.
const EPUB: &str = "books/alice.epub";

/// A storage the test looks into while kernels have it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// What a plugin hears of the reading: the event and what it carries.
type Heard = Rc<RefCell<Vec<(&'static str, events::Reading)>>>;

/// A plugin that listens to the reading events.
struct Listener(Heard);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("listener"),
            name: "Listener".into(),
            description: "Hears the reading events.".into(),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (event, name) in [
            (events::BOOK_OPENED, "opened"),
            (events::PAGE_TURNED, "turned"),
            (events::BOOK_CLOSED, "closed"),
        ] {
            let heard = self.0.clone();
            r.on(event, move |_, reading, _| {
                heard.borrow_mut().push((name, reading.clone()));
            })?;
        }
        Ok(())
    }
}

struct Reading {
    kernel: Kernel,
    storage: Shared,
    heard: Heard,
    now: u64,
    /// The book read.
    book: &'static str,
}

impl Reading {
    /// A kernel over `storage`, with the book on it.
    fn new(storage: Shared) -> Self {
        let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
        Self::with_book(storage, BOOK, text.as_bytes())
    }

    /// A kernel over `storage`, with book `data` at `path`.
    fn with_book(storage: Shared, path: &'static str, data: &[u8]) -> Self {
        if !storage.exists(path) {
            storage.create_dir("books").unwrap();
            storage.write(path, data).unwrap();
        }
        let heard = Heard::default();
        let kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_format_txt::TxtFormat)
            .plugin(paperpaw_plugin_format_epub::EpubFormat)
            .plugin(Reader)
            .plugin(Listener(heard.clone()))
            .build()
            .unwrap();
        let mut reading = Self {
            kernel,
            storage,
            heard,
            now: 0,
            book: path,
        };
        let effects = reading.kernel.start(Instant(0));
        reading.settle(effects);
        reading
    }

    /// Lets the kernel's timers run (the page index, a first opening)
    /// until nothing is due within a second.
    fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn open(&mut self) {
        self.now += 10;
        let params = Params::new().with(params::PATH, self.book);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
        assert_eq!(
            self.kernel.active_screen().map(Id::as_str),
            Some("reader.screen")
        );
    }

    fn run(&mut self, action: Id) {
        // Past the time positions are written at most this often.
        self.now += 6000;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// The position the reader kept: section and offset; the start
    /// while it kept none (a book opened and not read on).
    fn kept(&mut self) -> (u32, u32) {
        self.now += 10;
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
        let dir = format!(
            ".paperpaw/plugins/reader/books/{:016x}",
            paperpaw_digest::fnv1a64(self.book)
        );
        let Some(data) = self.storage.0.get(&format!("{dir}/progress.ini")) else {
            return (0, 0);
        };
        let text = String::from_utf8(data).unwrap();
        let value = |key: &str| -> u32 {
            let line = text.lines().find(|l| l.starts_with(key)).unwrap();
            line.rsplit(' ').next().unwrap().parse().unwrap()
        };
        (value("section"), value("offset"))
    }

    fn heard(&self) -> Vec<&'static str> {
        self.heard.borrow().iter().map(|(name, _)| *name).collect()
    }
}

#[test]
fn turns_pages_both_ways() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let second = reading.kept();
    reading.run(core_actions::PREV_PAGE);
    // The first page: where the text begins (a Project Gutenberg header
    // before it is left out).
    let first = reading.kept();
    assert!(first < second, "{first:?} before {second:?}");
    reading.run(core_actions::NEXT_PAGE);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::NEXT_PAGE);
    let third = reading.kept();
    assert!(third > second, "{third:?} after {second:?}");
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), second);
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), first);
    // Not before the first page.
    reading.run(core_actions::PREV_PAGE);
    assert_eq!(reading.kept(), first);
}

#[test]
fn continues_where_it_was_left() {
    let storage = Shared::default();
    let mut reading = Reading::new(storage.clone());
    reading.open();
    for _ in 0..3 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let left = reading.kept();
    assert!(left.1 > 0);
    // Another start of the device, the same card.
    let mut again = Reading::new(storage);
    again.open();
    assert_eq!(again.kept(), left);
}

#[test]
fn a_larger_font_keeps_the_place() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    for _ in 0..4 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let before = reading.kept();
    reading.run(actions::FONT_LARGER);
    let after = reading.kept();
    // The page starts where a line starts, at the place read or just
    // before it.
    assert_eq!(after.0, before.0);
    assert!(
        after.1 <= before.1 && after.1 + 400 > before.1,
        "{after:?} for {before:?}"
    );
}

#[test]
fn bookmarks_are_kept() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(actions::TOGGLE_BOOKMARK);
    let marks = |reading: &Reading| {
        let marks = paperpaw_sdk::annotations::marks(&reading.storage, BOOK);
        let bookmarks = marks.iter().filter(|m| m.kind == Kind::Bookmark);
        bookmarks.count()
    };
    assert_eq!(marks(&reading), 1);
    // Again on the same page: taken away.
    reading.run(actions::TOGGLE_BOOKMARK);
    assert_eq!(marks(&reading), 0);
}

#[test]
fn plugins_hear_the_reading() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(core_actions::NEXT_PAGE);
    reading.now += 10;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), core_actions::BACK);
    reading.settle(effects);
    let heard = reading.heard();
    assert_eq!(heard.first(), Some(&"opened"));
    assert_eq!(heard.iter().filter(|e| **e == "turned").count(), 2);
    let events = reading.heard.borrow();
    assert!(events.iter().all(|(_, r)| r.path == BOOK));
    let progress: Vec<f32> = events.iter().map(|(_, r)| r.progress).collect();
    assert!(progress.windows(2).all(|p| p[0] <= p[1]), "{progress:?}");
}

#[test]
fn plugins_hear_the_chapter() {
    let epub = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    // Until every page is counted.
    let counted = |reading: &Reading| {
        let heard = reading.heard.borrow();
        heard.last().is_some_and(|(_, r)| r.pages.is_some())
    };
    let mut turned = 0;
    while !counted(&reading) {
        reading.run(core_actions::NEXT_PAGE);
        turned += 1;
        assert!(turned < 400, "the pages are never counted");
    }
    // Then through the chapters: each page in one, as many pages left in
    // it as come before the next.
    let from = reading.heard.borrow().len() - 1;
    for _ in 0..60 {
        reading.run(core_actions::NEXT_PAGE);
    }
    let heard = reading.heard.borrow();
    let pages: Vec<&events::Reading> = heard[from..].iter().map(|(_, r)| r).collect();
    assert!(pages.iter().all(|r| r.path == EPUB && r.page.is_some()));
    let titles: std::collections::BTreeSet<&str> =
        pages.iter().map(|r| r.chapter.as_str()).collect();
    for chapter in ["Down the Rabbit-Hole", "The Pool of Tears"] {
        assert!(titles.iter().any(|t| t.ends_with(chapter)), "{titles:?}");
    }
    for pair in pages.windows(2) {
        let (this, next) = (pair[0], pair[1]);
        assert_eq!(next.page, this.page.map(|p| p + 1));
        // Before the first chapter (the title page): none.
        if this.chapter.is_empty() {
            assert_eq!(this.chapter_left, None);
            continue;
        }
        let left = this.chapter_left.expect("pages left once counted");
        if next.chapter == this.chapter {
            assert_eq!(next.chapter_left, Some(left - 1), "{this:?} {next:?}");
        } else {
            assert_eq!(left, 0, "{this:?} {next:?}");
        }
    }
}
