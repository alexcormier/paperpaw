//! Sorting files of fixed-size records bytewise with little memory: runs
//! that fit in memory are sorted in place and written to files, then
//! merged a few at a time. Records are big-endian, so bytewise order is
//! numeric order.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

/// Bytes sorted in memory at once.
const RUN: usize = 16 * 1024;
/// Runs merged at once: memory stays at [`RUN`] plus this many
/// [`AHEAD`]s, however long the file; longer files take more passes.
const FAN_IN: usize = 16;
/// Bytes read ahead from each run while merging.
const AHEAD: usize = 512;

/// Sorts the records of `size` bytes in `input` into `output` (which may
/// be `input`); `temp` names the files of the runs, which are gone
/// afterwards, also after an error.
pub(crate) fn sort(
    storage: &dyn Storage,
    input: &str,
    output: &str,
    size: usize,
    temp: &str,
) -> Result<(), StorageError> {
    let mut runs: Vec<String> = Vec::new();
    let result = sort_in_runs(storage, input, output, size, temp, &mut runs);
    for run in &runs {
        let _ = storage.remove(run);
    }
    result
}

fn sort_in_runs(
    storage: &dyn Storage,
    input: &str,
    output: &str,
    size: usize,
    temp: &str,
    runs: &mut Vec<String>,
) -> Result<(), StorageError> {
    let per_run = (RUN / size).max(1) * size;
    let mut buffer = vec![0u8; per_run];
    let mut at = 0u64;
    // Every run made, also those merged into others (to be removed).
    let mut made = 0;
    let mut run_name = |runs: &mut Vec<String>| {
        let run = format!("{temp}.{made}");
        made += 1;
        runs.push(run.clone());
        run
    };
    let mut pending: Vec<String> = Vec::new();
    loop {
        let mut filled = 0;
        while filled < per_run {
            let n = match storage.read_at(input, at + filled as u64, &mut buffer[filled..]) {
                Ok(n) => n,
                Err(StorageError::NotFound) if at == 0 && filled == 0 => 0,
                Err(e) => return Err(e),
            };
            if n == 0 {
                break;
            }
            filled += n;
        }
        let whole = filled - filled % size;
        if whole == 0 {
            break;
        }
        sort_records(&mut buffer[..whole], size);
        let run = run_name(runs);
        storage.write(&run, &buffer[..whole])?;
        pending.push(run);
        at += whole as u64;
        if filled < per_run {
            break;
        }
    }
    drop(buffer);
    // Merged a few at a time until few enough are left for the output.
    while pending.len() > FAN_IN {
        let mut merged = Vec::new();
        for group in pending.chunks(FAN_IN) {
            let run = run_name(runs);
            merge(storage, group, &run, size)?;
            for done in group {
                let _ = storage.remove(done);
            }
            merged.push(run);
        }
        pending = merged;
    }
    merge(storage, &pending, output, size)
}

/// Sorts the records of `size` bytes in `data` in place (a heapsort: no
/// memory beyond the records themselves).
fn sort_records(data: &mut [u8], size: usize) {
    fn record(data: &[u8], size: usize, i: usize) -> &[u8] {
        &data[i * size..(i + 1) * size]
    }
    fn swap(data: &mut [u8], size: usize, a: usize, b: usize) {
        let (a, b) = (a.min(b), a.max(b));
        let (low, high) = data.split_at_mut(b * size);
        low[a * size..(a + 1) * size].swap_with_slice(&mut high[..size]);
    }
    /// Lets record `root` sink until the heap below it holds again.
    fn sift(data: &mut [u8], size: usize, mut root: usize, end: usize) {
        loop {
            let mut child = 2 * root + 1;
            if child >= end {
                return;
            }
            if child + 1 < end && record(data, size, child) < record(data, size, child + 1) {
                child += 1;
            }
            if record(data, size, root) >= record(data, size, child) {
                return;
            }
            swap(data, size, root, child);
            root = child;
        }
    }
    let n = data.len() / size;
    for root in (0..n / 2).rev() {
        sift(data, size, root, n);
    }
    for end in (1..n).rev() {
        swap(data, size, 0, end);
        sift(data, size, 0, end);
    }
}

/// A run being read.
struct Reader<'a> {
    path: &'a str,
    at: u64,
    buffer: Vec<u8>,
    start: usize,
}

impl Reader<'_> {
    fn head(&mut self, storage: &dyn Storage, size: usize) -> Result<Option<&[u8]>, StorageError> {
        if self.start + size > self.buffer.len() {
            self.buffer.resize(AHEAD.max(size) / size * size, 0);
            let n = storage.read_at(self.path, self.at, &mut self.buffer)?;
            let whole = n - n % size;
            self.at += whole as u64;
            self.buffer.truncate(whole);
            self.start = 0;
            if whole == 0 {
                return Ok(None);
            }
        }
        Ok(Some(&self.buffer[self.start..self.start + size]))
    }
}

/// Merges the sorted `runs` into `output`.
fn merge(
    storage: &dyn Storage,
    runs: &[String],
    output: &str,
    size: usize,
) -> Result<(), StorageError> {
    storage.write(output, &[])?;
    let mut readers: Vec<Reader<'_>> = runs
        .iter()
        .map(|path| Reader {
            path,
            at: 0,
            buffer: Vec::new(),
            start: 0,
        })
        .collect();
    let mut out: Vec<u8> = Vec::with_capacity(AHEAD * 4);
    let mut record = vec![0u8; size];
    loop {
        // The run with the smallest head (few runs: a scan is fine).
        let mut best: Option<usize> = None;
        for (i, reader) in readers.iter_mut().enumerate() {
            if let Some(head) = reader.head(storage, size)?
                && best.is_none_or(|_| head < record.as_slice())
            {
                record.copy_from_slice(head);
                best = Some(i);
            }
        }
        let Some(i) = best else {
            break;
        };
        readers[i].start += size;
        out.extend_from_slice(&record);
        if out.len() >= AHEAD * 4 {
            storage.append(output, &out)?;
            out.clear();
        }
    }
    storage.append(output, &out)
}

/// Calls `visit` with every record of `path`, in order.
pub(crate) fn each(
    storage: &dyn Storage,
    path: &str,
    size: usize,
    mut visit: impl FnMut(&[u8]),
) -> Result<(), StorageError> {
    let mut chunk = vec![0u8; (AHEAD * 4 / size).max(1) * size];
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(path, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if at == 0 => 0,
            Err(e) => return Err(e),
        };
        let whole = n - n % size;
        for record in chunk[..whole].chunks_exact(size) {
            visit(record);
        }
        if whole == 0 || n < chunk.len() {
            return Ok(());
        }
        at += whole as u64;
    }
}

/// The records of a file, read a chunk at a time. A read error ends them
/// early and is kept in [`Records::failed`].
pub(crate) struct Records<'s> {
    storage: &'s dyn Storage,
    path: String,
    size: usize,
    at: u64,
    buffer: Vec<u8>,
    start: usize,
    pub failed: Option<StorageError>,
}

impl<'s> Records<'s> {
    pub(crate) fn new(storage: &'s dyn Storage, path: &str, size: usize) -> Self {
        Self {
            storage,
            path: String::from(path),
            size,
            at: 0,
            buffer: Vec::new(),
            start: 0,
            failed: None,
        }
    }
}

impl Iterator for Records<'_> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Vec<u8>> {
        if self.start + self.size > self.buffer.len() {
            self.buffer
                .resize((AHEAD * 4 / self.size).max(1) * self.size, 0);
            let n = match self.storage.read_at(&self.path, self.at, &mut self.buffer) {
                Ok(n) => n,
                Err(StorageError::NotFound) if self.at == 0 => 0,
                Err(e) => {
                    self.failed = Some(e);
                    0
                }
            };
            let whole = n - n % self.size;
            self.buffer.truncate(whole);
            self.at += whole as u64;
            self.start = 0;
            if whole == 0 {
                return None;
            }
        }
        let record = self.buffer[self.start..self.start + self.size].to_vec();
        self.start += self.size;
        Some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// `count` records of 8 bytes in a jumbled order.
    fn jumbled(count: u32) -> Vec<u8> {
        let mut data = Vec::new();
        let mut x: u32 = 12_345;
        for i in 0..count {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            data.extend_from_slice(&(x % 5000).to_be_bytes());
            data.extend_from_slice(&i.to_be_bytes());
        }
        data
    }

    #[test]
    fn sorts_large_files_in_runs() {
        let storage = MemoryStorage::new();
        // 10 000 records of 8 bytes: five runs.
        storage.write("in", &jumbled(10_000)).unwrap();
        sort(&storage, "in", "in", 8, "tmp").unwrap();
        let mut records = Vec::new();
        each(&storage, "in", 8, |r| records.push(r.to_vec())).unwrap();
        assert_eq!(records.len(), 10_000);
        assert!(records.windows(2).all(|w| w[0] <= w[1]));
        assert!(storage.size("tmp.0").is_err(), "runs are removed");
        let streamed: Vec<Vec<u8>> = Records::new(&storage, "in", 8).collect();
        assert_eq!(streamed, records);
        // Nothing to sort.
        sort(&storage, "none", "out", 8, "tmp").unwrap();
        assert_eq!(storage.size("out").unwrap(), 0);
    }

    /// More runs than are merged at once: merged in passes, every run
    /// removed after.
    #[test]
    fn merges_many_runs_in_passes() {
        let storage = MemoryStorage::new();
        let count = (RUN / 8 * (FAN_IN * 2 + 3)) as u32;
        let mut expected: Vec<[u8; 8]> = jumbled(count).as_chunks::<8>().0.to_vec();
        storage.write("in", &jumbled(count)).unwrap();
        sort(&storage, "in", "out", 8, "tmp").unwrap();
        expected.sort_unstable();
        let mut records = Vec::new();
        each(&storage, "out", 8, |r| records.push(r.to_vec())).unwrap();
        assert_eq!(
            records,
            expected.iter().map(|r| r.to_vec()).collect::<Vec<_>>()
        );
        for i in 0..64 {
            assert!(!storage.exists(&format!("tmp.{i}")), "tmp.{i} is left");
        }
    }

    /// Runs are written to a card that fills up: the error comes back and
    /// no run is left behind.
    #[test]
    fn runs_go_after_an_error() {
        let storage = crate::testing::Faulty::new(MemoryStorage::new());
        storage.inner.write("in", &jumbled(10_000)).unwrap();
        storage.fill_after(3);
        assert_eq!(
            sort(&storage, "in", "out", 8, "tmp"),
            Err(StorageError::Full)
        );
        for i in 0..8 {
            assert!(!storage.exists(&format!("tmp.{i}")), "tmp.{i} is left");
        }
    }

    #[test]
    fn records_are_sorted_in_place() {
        let mut data = jumbled(1000);
        let mut expected: Vec<[u8; 8]> = data.as_chunks::<8>().0.to_vec();
        expected.sort_unstable();
        sort_records(&mut data, 8);
        assert_eq!(data, expected.concat());
        // Odd sizes, one record and none.
        let mut three = [5, 0, 1, 2, 9, 9, 1, 1, 1];
        sort_records(&mut three, 3);
        assert_eq!(three, [1, 1, 1, 2, 9, 9, 5, 0, 1]);
        sort_records(&mut three[..3], 3);
        sort_records(&mut [], 3);
    }
}
