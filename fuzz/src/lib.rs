//! What the fuzz targets share: reading a book the way the reader does.

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Document, Locator};

/// Blocks read per section at most, so a long valid book stays quick.
const MAX_BLOCKS: usize = 2000;

/// Reads the first sections block by block, then the table of contents
/// and page labels; every block must move forward.
pub fn read_document(doc: &dyn Document, storage: &dyn Storage) {
    for section in 0..doc.sections().min(3) {
        let mut at = Locator::new(section, 0);
        for _ in 0..MAX_BLOCKS {
            match doc.block_at(storage, at) {
                Ok(Some(block)) => {
                    assert!(block.end > at.offset, "a block must move forward");
                    let _ = doc.page_label(storage, at);
                    at = Locator::new(section, block.end);
                }
                _ => break,
            }
        }
    }
    for i in 0..doc.toc_len(storage).min(20) {
        let _ = doc.toc_entry(storage, i);
        let _ = doc.toc_target(storage, i);
    }
}

/// Splits `data` into `N` parts, each prefixed by its length as two bytes
/// (little-endian); the last part takes the rest.
pub fn split<const N: usize>(mut data: &[u8]) -> [&[u8]; N] {
    let mut parts = [&[][..]; N];
    for (i, part) in parts.iter_mut().enumerate() {
        if i + 1 == N {
            *part = data;
            break;
        }
        let Some((len, rest)) = data.split_first_chunk::<2>() else {
            break;
        };
        let len = usize::from(u16::from_le_bytes(*len)).min(rest.len());
        (*part, data) = rest.split_at(len);
    }
    parts
}
