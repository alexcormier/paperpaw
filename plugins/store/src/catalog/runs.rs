//! Plugins ordered by a key, more of them than memory holds at once:
//! sorted in runs on the storage, then merged.

use alloc::vec::Vec;

use paperpaw_sdk::Storage;

use super::WRITE_AT;

/// A plugin with the key it is ordered by: (key, its place in the index).
pub(super) type Keyed = (u32, u32);

/// Plugins ordered by a key in memory at once (8 bytes each): longer lists
/// are sorted in runs of this many on the storage.
pub(crate) const RUN: usize = 512;

/// Entries of a run read at once while runs are merged.
const MERGE_READ: usize = 16;

/// The order of plugins by a key: the highest (newest) first; of one key,
/// in the list's order.
pub(super) fn order_of(a: Keyed, b: Keyed) -> core::cmp::Ordering {
    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
}

pub(super) fn before(a: Keyed, b: Keyed) -> bool {
    order_of(a, b) == core::cmp::Ordering::Less
}

/// Where a sorted run on the storage is read while merging.
pub(super) struct Cursor {
    /// The byte of the next entry not read yet, and how many are left.
    at: u64,
    left: u32,
    read: [Keyed; MERGE_READ],
    /// The next of those read, and how many were.
    next: usize,
    len: usize,
}

impl Cursor {
    pub(super) fn new(at: u64, left: u32) -> Self {
        Self {
            at,
            left,
            read: [(0, 0); MERGE_READ],
            next: 0,
            len: 0,
        }
    }

    /// The run's first entry not taken yet.
    pub(super) fn head(&mut self, storage: &dyn Storage, path: &str) -> Option<Keyed> {
        if self.next == self.len {
            let want = (self.left as usize).min(MERGE_READ);
            if want == 0 {
                return None;
            }
            let mut bytes = [0u8; MERGE_READ * 8];
            let mut got = 0;
            while got < want * 8 {
                match storage.read_at(path, self.at + got as u64, &mut bytes[got..want * 8]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            let entries = got / 8;
            for (slot, chunk) in self.read.iter_mut().zip(bytes[..entries * 8].chunks(8)) {
                let word = |at: usize| {
                    u32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
                };
                *slot = (word(0), word(4));
            }
            self.at += (entries * 8) as u64;
            self.left = if entries == want {
                self.left - want as u32
            } else {
                0
            };
            self.next = 0;
            self.len = entries;
            if entries == 0 {
                return None;
            }
        }
        Some(self.read[self.next])
    }

    pub(super) fn next(&mut self) {
        self.next += 1;
    }
}

/// The plugins picked, written to the storage a piece at a time.
pub(super) struct Output<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    bytes: Vec<u8>,
    pub(super) count: usize,
    written: bool,
}

impl<'a> Output<'a> {
    pub(super) fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
        Self {
            written: storage.write(path, b"").is_ok(),
            storage,
            path,
            bytes: Vec::new(),
            count: 0,
        }
    }

    pub(super) fn push(&mut self, n: u32) {
        self.bytes.extend_from_slice(&n.to_le_bytes());
        self.count += 1;
        if self.bytes.len() >= WRITE_AT {
            self.written &= self.storage.append(self.path, &self.bytes).is_ok();
            self.bytes.clear();
        }
    }

    /// How many were picked; none where they could not be kept.
    pub(super) fn finish(mut self) -> usize {
        self.written &= self.storage.append(self.path, &self.bytes).is_ok();
        if self.written { self.count } else { 0 }
    }
}
