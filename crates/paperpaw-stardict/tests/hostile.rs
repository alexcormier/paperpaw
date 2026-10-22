//! Damaged or hostile dictionaries never panic and never make a lookup
//! hold much more memory than its budget: on the X4 a failed allocation
//! stops the device. The tests measure the most memory each step holds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_stardict::{Dictionary, Error, LOOKUP_BUDGET};

/// The system allocator, noting how much each thread holds at most.
struct Tracking;

thread_local! {
    static HELD: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

fn grow(size: usize) {
    let _ = HELD.try_with(|held| {
        held.set(held.get() + size);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(held.get())));
    });
}

fn shrink(size: usize) {
    let _ = HELD.try_with(|held| held.set(held.get().saturating_sub(size)));
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grow(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        grow(new_size);
        shrink(layout.size());
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrink(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

/// The most memory `f` holds at once, beyond what was held before.
fn peak_memory<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = HELD.with(Cell::get);
    PEAK.with(|peak| peak.set(before));
    let result = f();
    (result, PEAK.with(Cell::get) - before)
}

/// The memory a dictzip lookup needs besides its definitions: the deflate
/// window, input buffer and decoder.
const INFLATE: usize = 56 * 1024;

fn ifo(words: usize, offset_bits: u8, types: &str) -> String {
    format!(
        "StarDict's dict ifo file\nversion=3.0.0\nbookname=Hostile\nwordcount={words}\nidxoffsetbits={offset_bits}\nsametypesequence={types}\n"
    )
}

/// An `.idx` with 32-bit offsets.
fn idx(entries: &[(&str, u32, u32)]) -> Vec<u8> {
    let mut idx = Vec::new();
    for (word, offset, size) in entries {
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        idx.extend_from_slice(&offset.to_be_bytes());
        idx.extend_from_slice(&size.to_be_bytes());
    }
    idx
}

/// A dictzip file with the given `RA` table and compressed chunks.
fn dictzip(table: &[u8], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut extra = b"RA".to_vec();
    extra.extend_from_slice(&(table.len() as u16).to_le_bytes());
    extra.extend_from_slice(table);
    let mut file = vec![0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 3];
    file.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    file.extend_from_slice(&extra);
    for part in parts {
        file.extend_from_slice(part);
    }
    file
}

/// The `RA` table: version, chunk length, chunk count, compressed sizes.
fn table(chunk_len: u16, count: u16, parts: &[Vec<u8>]) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_le_bytes());
    table.extend_from_slice(&chunk_len.to_le_bytes());
    table.extend_from_slice(&count.to_le_bytes());
    for part in parts {
        table.extend_from_slice(&(part.len() as u16).to_le_bytes());
    }
    table
}

fn compress(data: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    data.chunks(chunk)
        .map(|c| miniz_oxide::deflate::compress_to_vec(c, 6))
        .collect()
}

fn open(storage: &MemoryStorage) -> Result<Dictionary, Error> {
    let dictionary = Dictionary::open(storage, "d/x.ifo", "cache")?;
    for mut builder in dictionary.builders(storage) {
        while !builder.step(storage, 4096)? {}
    }
    Ok(dictionary)
}

#[test]
fn dictzip_without_chunks_is_rejected() {
    // A table that says there are no chunks, with a word whose definition
    // lies in the first one.
    let storage = MemoryStorage::new()
        .with("d/x.ifo", ifo(1, 32, "m"))
        .with("d/x.idx", idx(&[("hus", 0, 10)]))
        .with("d/x.dict.dz", dictzip(&table(100, 0, &[]), &[]));
    assert!(matches!(open(&storage), Err(Error::Malformed(_))));
}

#[test]
fn dictzip_table_shorter_than_its_count() {
    // 65535 chunks announced, none listed: refused before the chunk list
    // is made.
    let storage = MemoryStorage::new()
        .with("d/x.ifo", ifo(1, 32, "m"))
        .with("d/x.idx", idx(&[("hus", 0, 10)]))
        .with("d/x.dict.dz", dictzip(&table(100, u16::MAX, &[]), &[]));
    let (opened, peak) = peak_memory(|| Dictionary::open(&storage, "d/x.ifo", "cache"));
    assert!(matches!(opened, Err(Error::Malformed(_))));
    assert!(peak < 16 * 1024, "held {peak} bytes");
}

#[test]
fn dictzip_chunk_longer_than_announced_is_read_once() {
    // The table says chunks of 100 bytes, but each holds 1000: every chunk
    // overlapping the definition used to add it again.
    let text: Vec<u8> = (0..5000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let parts = compress(&text, 1000);
    let storage = MemoryStorage::new()
        .with("d/x.ifo", ifo(1, 32, "m"))
        .with("d/x.idx", idx(&[("hus", 0, 500)]))
        .with("d/x.dict.dz", dictzip(&table(100, 5, &parts), &parts));
    let dictionary = open(&storage).unwrap();
    let found = dictionary.lookup(&storage, "hus").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].text.len() <= 500, "{} bytes", found[0].text.len());
}

#[test]
fn definition_offset_near_the_end_of_u64() {
    // 64-bit offsets that overflow when the size is added, and offsets
    // past the end of the definitions.
    let mut index = Vec::new();
    for (word, offset) in [("a", u64::MAX - 5), ("b", 1 << 40), ("c", 0)] {
        index.extend_from_slice(word.as_bytes());
        index.push(0);
        index.extend_from_slice(&offset.to_be_bytes());
        index.extend_from_slice(&100u32.to_be_bytes());
    }
    let text = b"Bedeutung".repeat(20);
    let parts = compress(&text, 64);
    for dict in [
        ("d/x.dict", text.clone()),
        (
            "d/x.dict.dz",
            dictzip(&table(64, parts.len() as u16, &parts), &parts),
        ),
    ] {
        let storage = MemoryStorage::new()
            .with("d/x.ifo", ifo(3, 64, "m"))
            .with("d/x.idx", index.clone())
            .with(dict.0, dict.1);
        let dictionary = open(&storage).unwrap();
        for word in ["a", "b"] {
            let found = dictionary.lookup(&storage, word).unwrap();
            assert!(found.iter().all(|d| d.text.is_empty()), "{word} {}", dict.0);
        }
        let found = dictionary.lookup(&storage, "c").unwrap();
        assert_eq!(found[0].text.as_bytes(), &text[..100], "{}", dict.0);
    }
}

/// Eight headwords "hus", each with the largest definition there is.
fn eight_large_definitions(dict: &str) -> MemoryStorage {
    let text: Vec<u8> = (0..600_000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let entries: Vec<(&str, u32, u32)> = (0..8).map(|i| ("hus", i * 70_000, u32::MAX)).collect();
    let data = if dict.ends_with(".dz") {
        let parts = compress(&text, 60_000);
        dictzip(&table(60_000, parts.len() as u16, &parts), &parts)
    } else {
        text
    };
    MemoryStorage::new()
        .with("d/x.ifo", ifo(8, 32, "m"))
        .with("d/x.idx", idx(&entries))
        .with(dict, data)
}

#[test]
fn a_lookup_keeps_to_its_budget() {
    for dict in ["d/x.dict", "d/x.dict.dz"] {
        let storage = eight_large_definitions(dict);
        let dictionary = open(&storage).unwrap();
        let (found, peak) = peak_memory(|| dictionary.lookup(&storage, "hus").unwrap());
        let text: usize = found.iter().map(|d| d.text.len()).sum();
        assert!(text <= LOOKUP_BUDGET, "{dict}: {text} bytes of text");
        assert!(
            peak <= 2 * LOOKUP_BUDGET + INFLATE,
            "{dict}: held {peak} bytes"
        );

        // A larger budget, shared with other dictionaries, gives more.
        let mut budget = 200_000;
        let found = dictionary
            .lookup_within(&storage, "hus", &mut budget)
            .unwrap();
        let text: usize = found.iter().map(|d| d.text.len()).sum();
        assert_eq!((found.len(), text + budget), (4, 200_000), "{dict}");
        assert_eq!(budget, 0, "{dict}");
        assert!(
            dictionary
                .lookup_within(&storage, "hus", &mut budget)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn many_empty_fields_need_little_memory() {
    // Each "m\0" is a field of its own; they used to be collected first.
    let data = b"m\0".repeat(30_000);
    let storage = MemoryStorage::new()
        .with(
            "d/x.ifo",
            "StarDict's dict ifo file\nbookname=x\nwordcount=1\n",
        )
        .with("d/x.idx", idx(&[("hus", 0, data.len() as u32)]))
        .with("d/x.dict", data);
    let dictionary = open(&storage).unwrap();
    let (found, peak) = peak_memory(|| dictionary.lookup(&storage, "hus").unwrap());
    assert_eq!(found[0].text, "");
    assert!(peak < 2 * 60_000 + 4096, "held {peak} bytes");
}

#[test]
fn invalid_utf8_stays_within_the_budget() {
    // Every byte becomes a three-byte replacement character.
    let data = vec![0xffu8; 60_000];
    let storage = MemoryStorage::new()
        .with("d/x.ifo", ifo(1, 32, "m"))
        .with("d/x.idx", idx(&[("hus", 0, data.len() as u32)]))
        .with("d/x.dict", data);
    let dictionary = open(&storage).unwrap();
    let mut budget = 10_000;
    let found = dictionary
        .lookup_within(&storage, "hus", &mut budget)
        .unwrap();
    assert!(found[0].text.len() <= 10_000);
    assert!(found[0].text.starts_with('\u{fffd}'));
}

/// Storage counting the reads of the `.idx` file.
struct CountingReads {
    inner: MemoryStorage,
    reads: Cell<usize>,
}

impl Storage for CountingReads {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path.ends_with(".idx") {
            self.reads.set(self.reads.get() + 1);
        }
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
        self.inner.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

#[test]
fn an_unsorted_index_is_not_walked_to_its_end() {
    // 128 blocks of 32 words, all "a" but the first word of the blocks
    // the binary search for "b" looks at: the walk after the first match
    // used to go on through every "a".
    let count = 128 * 32;
    let looked_at = [1, 2, 4, 8, 16, 32, 64];
    let entries: Vec<(&str, u32, u32)> = (0..count)
        .map(|i| {
            let first_of = (i % 32 == 0).then_some(i / 32);
            let word = if first_of.is_some_and(|b| looked_at.contains(&b)) {
                "b"
            } else {
                "a"
            };
            (word, 0, 1)
        })
        .collect();
    let storage = CountingReads {
        inner: MemoryStorage::new()
            .with("d/x.ifo", ifo(count as usize, 32, "m"))
            .with("d/x.idx", idx(&entries))
            .with("d/x.dict", "x"),
        reads: Cell::new(0),
    };
    let dictionary = Dictionary::open(&storage, "d/x.ifo", "cache").unwrap();
    for mut builder in dictionary.builders(&storage) {
        while !builder.step(&storage, 4096).unwrap() {}
    }
    storage.reads.set(0);
    dictionary.lookup(&storage, "b").unwrap();
    let reads = storage.reads.get();
    assert!(reads < 200, "{reads} reads");
}
