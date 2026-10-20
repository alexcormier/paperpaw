//! Any bytes as a plain text book, read block by block.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::read_document;
use paperpaw_plugin_format_txt::TxtDocument;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.txt", data.to_vec());
    if let Ok(doc) = TxtDocument::open(&storage, "book.txt") {
        read_document(&doc, &storage);
    }
});
