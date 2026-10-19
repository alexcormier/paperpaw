//! Any bytes as an EPUB: open, read the first chapters and the table of
//! contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::document::{Document, Locator};

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.epub", data.to_vec());
    let Ok(doc) = EpubDocument::open(&storage, "book.epub") else {
        return;
    };
    for section in 0..doc.sections().min(3) {
        let mut at = Locator::new(section, 0);
        for _ in 0..2000 {
            match doc.block_at(&storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset);
                    at = Locator::new(section, block.end);
                }
                _ => break,
            }
        }
    }
    for i in 0..doc.toc_len(&storage).min(20) {
        let _ = doc.toc_target(&storage, i);
    }
});
