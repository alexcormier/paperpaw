//! Any bytes as an XTC book of prerendered pages: open, read the pages
//! and the table of contents.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::read_document;
use paperpaw_plugin_format_xtc::XtcDocument;
use paperpaw_sdk::MemoryStorage;

fuzz_target!(|data: &[u8]| {
    let storage = MemoryStorage::new().with("book.xtc", data.to_vec());
    if let Ok(doc) = XtcDocument::open(&storage, "book.xtc") {
        read_document(&doc, &storage);
    }
});
