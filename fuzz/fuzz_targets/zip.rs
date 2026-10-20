//! Any bytes as a ZIP archive: list it, look up every entry by name and
//! read each one whole.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_sdk::MemoryStorage;
use paperpaw_zip::ZipArchive;

/// Entries read at most, so a large valid archive stays quick.
const MAX_ENTRIES: usize = 64;

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("a.zip", data.to_vec());
    let Ok(archive) = ZipArchive::open(&storage, "a.zip") else {
        return;
    };
    let names: Vec<String> = archive
        .entries(&storage)
        .take(MAX_ENTRIES)
        .filter_map(|entry| entry.ok().map(|entry| entry.name))
        .collect();
    let mut buf = [0u8; 4096];
    for name in &names {
        let Ok(Some(entry)) = archive.entry(&storage, name) else {
            continue;
        };
        let Ok(mut reader) = archive.reader(&storage, &entry) else {
            continue;
        };
        while matches!(reader.read(&storage, &mut buf), Ok(n) if n > 0) {}
    }
});
