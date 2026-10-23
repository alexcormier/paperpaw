//! A storage that fails on purpose, for the tests of what happens then:
//! a card that fills up, a file that cannot be read from some point on,
//! a rename that takes the file it replaces away and then fails (as FAT
//! can), reads that return less than asked for.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

#[derive(Default)]
pub(crate) struct Faulty {
    pub inner: MemoryStorage,
    /// Writes and appends that still succeed; `None`: all of them.
    room: Cell<Option<u32>>,
    /// Files whose reads fail from an offset on.
    broken: RefCell<Vec<(String, u64)>>,
    /// Renames remove the file they replace, then fail.
    rename_drops_target: Cell<bool>,
    /// Reads return at most this many bytes.
    short_reads: Cell<Option<usize>>,
}

impl Faulty {
    pub(crate) fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            ..Self::default()
        }
    }

    /// Takes `writes` more writes and appends, then is full.
    pub(crate) fn fill_after(&self, writes: u32) {
        self.room.set(Some(writes));
    }

    /// Reads of `path` fail from byte `at` on.
    pub(crate) fn break_at(&self, path: &str, at: u64) {
        self.broken.borrow_mut().push((path.to_string(), at));
    }

    /// Renames from now on remove the file they replace, then fail.
    pub(crate) fn drop_target_on_rename(&self) {
        self.rename_drops_target.set(true);
    }

    /// Reads from now on return at most `bytes` bytes.
    pub(crate) fn read_short(&self, bytes: usize) {
        self.short_reads.set(Some(bytes));
    }

    fn take_room(&self) -> Result<(), StorageError> {
        match self.room.get() {
            Some(0) => Err(StorageError::Full),
            Some(n) => {
                self.room.set(Some(n - 1));
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl Storage for Faulty {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let broken = self.broken.borrow();
        match broken.iter().find(|(p, _)| p == path) {
            Some((_, at)) if offset + buf.len() as u64 > *at => Err(StorageError::Io),
            _ => {
                let len = self
                    .short_reads
                    .get()
                    .map_or(buf.len(), |n| n.min(buf.len()));
                self.inner.read_at(path, offset, &mut buf[..len])
            }
        }
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.take_room()?;
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.take_room()?;
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if self.rename_drops_target.get() {
            match self.inner.remove(to) {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(e) => return Err(e),
            }
            return Err(StorageError::Io);
        }
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
}
