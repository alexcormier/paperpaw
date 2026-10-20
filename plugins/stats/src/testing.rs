//! A storage that fails on purpose, for the tests of what happens then:
//! a card that fills up, a file that cannot be read from some point on.

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
            _ => self.inner.read_at(path, offset, buf),
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
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
}
