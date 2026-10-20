//! What the storage is asked frame by frame (`--trace`), below the kernel
//! as the X4's `trace` sees the card: the calls each frame's drawing made,
//! band by band, how long the drawing took, and the calls made since the
//! frame before (opening a screen, handling a press). Drawn in bands, a
//! screen that reads while it draws reads again in every band.
//!
//! The times are the computer's, many times faster than an X4's; the
//! calls are the same on both.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use paperpaw_platform::{DirEntry, Storage, StorageError};

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static COUNTS: RefCell<Counts> = RefCell::new(Counts::default());
}

/// Calls by what was done, and bytes read.
#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    reads: u32,
    bytes: u64,
    sizes: u32,
    lists: u32,
    writes: u32,
    other: u32,
}

impl Counts {
    fn calls(&self) -> u32 {
        self.reads + self.sizes + self.lists + self.writes + self.other
    }

    fn since(&self, before: &Counts) -> Counts {
        Counts {
            reads: self.reads - before.reads,
            bytes: self.bytes - before.bytes,
            sizes: self.sizes - before.sizes,
            lists: self.lists - before.lists,
            writes: self.writes - before.writes,
            other: self.other - before.other,
        }
    }
}

impl std::fmt::Display for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} calls ({} reads of {} KB, {} sizes, {} lists, {} writes, {} other)",
            self.calls(),
            self.reads,
            self.bytes.div_ceil(1024),
            self.sizes,
            self.lists,
            self.writes,
            self.other
        )
    }
}

/// Prints a line for every frame from now on.
pub fn start() {
    ON.with(|on| on.set(true));
}

fn on() -> bool {
    ON.with(Cell::get)
}

fn count(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|counts| f(&mut counts.borrow_mut()));
}

fn now() -> Counts {
    COUNTS.with(|counts| *counts.borrow())
}

/// A frame being drawn, from [`Frame::start`] until [`Frame::done`].
pub struct Frame {
    started: Instant,
    before: Counts,
    band: Counts,
    bands: Vec<u32>,
}

thread_local! {
    /// The counts when the last frame was done.
    static LAST: RefCell<Counts> = RefCell::new(Counts::default());
}

impl Frame {
    /// A frame begins; `None` while not tracing.
    pub fn start() -> Option<Frame> {
        on().then(|| Frame {
            started: Instant::now(),
            before: now(),
            band: now(),
            bands: Vec::new(),
        })
    }

    /// A band was drawn.
    pub fn band(&mut self) {
        let counts = now();
        self.bands.push(counts.since(&self.band).calls());
        self.band = counts;
    }

    /// The frame is drawn: its line, the screen shown being `screen`.
    pub fn done(self, screen: &str) {
        let took = self.started.elapsed();
        let counts = now();
        let drawing = counts.since(&self.before);
        let before = LAST.with(|last| self.before.since(&last.replace(counts)));
        println!(
            "frame {screen}: drawn in {:.1} ms, {} bands {:?}, {drawing}; before it {before}",
            took.as_secs_f64() * 1000.0,
            self.bands.len(),
            self.bands,
        );
    }
}

/// The storage, its calls counted while tracing.
#[derive(Debug)]
pub struct Traced<S>(pub S);

impl<S: Storage> Storage for Traced<S> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let read = self.0.read_at(path, offset, buf);
        let n = *read.as_ref().unwrap_or(&0) as u64;
        count(|c| {
            c.reads += 1;
            c.bytes += n;
        });
        read
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        count(|c| c.sizes += 1);
        self.0.size(path)
    }

    fn exists(&self, path: &str) -> bool {
        count(|c| c.sizes += 1);
        self.0.exists(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        count(|c| c.writes += 1);
        self.0.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        count(|c| c.writes += 1);
        self.0.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        count(|c| c.lists += 1);
        self.0.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        self.0.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        self.0.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        self.0.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        self.0.remove_dir_all(path)
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.0.local_path(path, with);
    }
}
