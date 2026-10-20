//! Dictionaries built for the tests: `.ifo`, `.idx` (32 or 64-bit
//! offsets), `.syn` and the definitions plain or as dictzip; and a
//! storage that counts its reads and stops answering after a budget, so
//! a lookup caught in a loop fails instead of hanging.

// Not every test file uses every helper.
#![allow(dead_code)]

use std::cell::Cell;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use paperpaw_stardict::Dictionary;

/// How a dictionary is built.
#[derive(Debug, Clone)]
pub struct Spec {
    /// Headwords and definitions, in any order (sorted when built).
    pub entries: Vec<(String, String)>,
    /// Other forms and the headword they point to.
    pub synonyms: Vec<(String, String)>,
    /// Definitions in dictzip chunks of this many bytes.
    pub chunk: Option<usize>,
    pub offset_bits: u8,
    pub types: Option<String>,
}

impl Spec {
    pub fn new(entries: &[(&str, &str)]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|(w, d)| (w.to_string(), d.to_string()))
                .collect(),
            synonyms: Vec::new(),
            chunk: None,
            offset_bits: 32,
            types: Some("m".to_string()),
        }
    }
}

/// StarDict's order: ASCII letters without case, then the bytes.
pub fn stardict_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_ascii_lowercase()
        .cmp(&b.to_ascii_lowercase())
        .then_with(|| a.cmp(b))
}

/// The files of `spec` below `d/test.*`.
pub fn build(spec: &Spec) -> MemoryStorage {
    let mut entries = spec.entries.clone();
    entries.sort_by(|a, b| stardict_order(&a.0, &b.0));
    let mut dict = Vec::new();
    let mut idx = Vec::new();
    for (word, definition) in &entries {
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        if spec.offset_bits == 64 {
            idx.extend_from_slice(&(dict.len() as u64).to_be_bytes());
        } else {
            idx.extend_from_slice(&(dict.len() as u32).to_be_bytes());
        }
        idx.extend_from_slice(&(definition.len() as u32).to_be_bytes());
        dict.extend_from_slice(definition.as_bytes());
    }
    let mut synonyms = spec.synonyms.clone();
    synonyms.sort_by(|a, b| stardict_order(&a.0, &b.0));
    let mut syn = Vec::new();
    for (form, word) in &synonyms {
        let number = entries.iter().position(|(e, _)| e == word).unwrap() as u32;
        syn.extend_from_slice(form.as_bytes());
        syn.push(0);
        syn.extend_from_slice(&number.to_be_bytes());
    }
    let mut ifo = format!(
        "StarDict's dict ifo file\nversion=3.0.0\nbookname=Test\nwordcount={}\nidxfilesize={}\n",
        entries.len(),
        idx.len()
    );
    if !synonyms.is_empty() {
        ifo.push_str(&format!("synwordcount={}\n", synonyms.len()));
    }
    if spec.offset_bits == 64 {
        ifo.push_str("idxoffsetbits=64\n");
    }
    if let Some(types) = &spec.types {
        ifo.push_str(&format!("sametypesequence={types}\n"));
    }
    let mut storage = MemoryStorage::new()
        .with("d/test.ifo", ifo)
        .with("d/test.idx", idx);
    if !synonyms.is_empty() {
        storage = storage.with("d/test.syn", syn);
    }
    match spec.chunk {
        None => storage.with("d/test.dict", dict),
        Some(chunk) => storage.with("d/test.dict.dz", dictzip(&dict, chunk, 4 | 8)),
    }
}

/// A dictzip file: a gzip header with flags `flags` (at least FEXTRA,
/// 4), the `RA` chunk table, then each chunk compressed on its own.
pub fn dictzip(data: &[u8], chunk: usize, flags: u8) -> Vec<u8> {
    let parts: Vec<Vec<u8>> = data
        .chunks(chunk)
        .map(|c| miniz_oxide::deflate::compress_to_vec(c, 6))
        .collect();
    let mut table = Vec::new();
    table.extend_from_slice(&1u16.to_le_bytes());
    table.extend_from_slice(&(chunk as u16).to_le_bytes());
    table.extend_from_slice(&(parts.len() as u16).to_le_bytes());
    for part in &parts {
        table.extend_from_slice(&(part.len() as u16).to_le_bytes());
    }
    // Another subfield first: readers look for `RA` among them.
    let mut extra = b"XY".to_vec();
    extra.extend_from_slice(&3u16.to_le_bytes());
    extra.extend_from_slice(b"abc");
    extra.extend_from_slice(b"RA");
    extra.extend_from_slice(&(table.len() as u16).to_le_bytes());
    extra.extend_from_slice(&table);
    let mut file = vec![0x1f, 0x8b, 8, flags, 0, 0, 0, 0, 0, 3];
    file.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    file.extend_from_slice(&extra);
    if flags & 8 != 0 {
        file.extend_from_slice(b"test.dict\0");
    }
    if flags & 16 != 0 {
        file.extend_from_slice(b"a comment\0");
    }
    if flags & 2 != 0 {
        file.extend_from_slice(&[0xab, 0xcd]);
    }
    for part in parts {
        file.extend_from_slice(&part);
    }
    file
}

/// Opens `d/test.ifo` with its block indexes in `cache`, built.
pub fn open(storage: &dyn Storage) -> Dictionary {
    let dictionary = Dictionary::open(storage, "d/test.ifo", "cache").unwrap();
    prepare(storage, &dictionary);
    dictionary
}

pub fn prepare(storage: &dyn Storage, dictionary: &Dictionary) {
    for mut builder in dictionary.builders(storage) {
        while !builder.step(storage, 4096).unwrap() {}
    }
}

/// The words found for `word` with their definitions.
pub fn found(dictionary: &Dictionary, storage: &dyn Storage, word: &str) -> Vec<(String, String)> {
    dictionary
        .lookup(storage, word)
        .unwrap()
        .into_iter()
        .map(|d| (d.word, d.text))
        .collect()
}

/// A storage that counts reads and fails them after a budget.
pub struct Counted<'s> {
    pub inner: &'s dyn Storage,
    pub reads: Cell<usize>,
    pub budget: usize,
}

impl<'s> Counted<'s> {
    pub fn new(inner: &'s dyn Storage, budget: usize) -> Self {
        Self {
            inner,
            reads: Cell::new(0),
            budget,
        }
    }
}

impl Storage for Counted<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.reads.set(self.reads.get() + 1);
        if self.reads.get() > self.budget {
            return Err(StorageError::Io);
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
