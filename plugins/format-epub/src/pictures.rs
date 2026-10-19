//! Decoded pictures in the caches: gray (`.gray`) for e-ink, colour
//! (`.rgb`) for colour screens (ADR 0015). Chapters keep a picture's
//! cache name without its kind; it is decoded in the kind the book was
//! opened with, and read in that kind, else in the other one there is.
//!
//! A book's pictures are decoded when a page shows them
//! (`Document::prepare_image`), not when their chapter is converted: then
//! they are noted ([`note`]) in `images.txt` of the book's cache, a line
//! each with the cache name, the size they are decoded to and their path
//! in the archive, for pages to be laid out meanwhile.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::image::ImageFile;

/// The cache file of picture `base` in colour or gray.
pub(crate) fn file(base: &str, color: bool) -> String {
    format!("{base}.{}", if color { "rgb" } else { "gray" })
}

/// The decoded picture `base`, the kind wanted first, else the other.
pub(crate) fn open(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    [color, !color].into_iter().find_map(|color| {
        ImageFile::open(storage, &file(base, color)).map(|image| ImageRef {
            path: image.path,
            width: image.width,
            height: image.height,
        })
    })
}

/// The list of pictures noted in cache folder `dir`.
fn list(dir: &str) -> String {
    format!("{dir}/images.txt")
}

/// Notes picture `base`, from `source` in the archive, to be decoded
/// `size` large when a page shows it.
pub(crate) fn note(storage: &dyn Storage, base: &str, size: (u32, u32), source: &str) {
    let Some((dir, name)) = base.rsplit_once('/') else {
        return;
    };
    if noted(storage, base).is_none() {
        let line = format!("{name}\t{}\t{}\t{source}\n", size.0, size.1);
        let _ = storage.append(&list(dir), line.as_bytes());
    }
}

/// Picture `base` as noted ([`note`]): the size it is decoded to and its
/// path in the archive. The list is read a little at a time.
pub(crate) fn noted(storage: &dyn Storage, base: &str) -> Option<((u32, u32), String)> {
    let (dir, name) = base.rsplit_once('/')?;
    let path = list(dir);
    let mut chunk = [0u8; 512];
    let mut line: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    let mut at = 0u64;
    loop {
        let n = storage.read_at(&path, at, &mut chunk).ok()?;
        at += n as u64;
        for &byte in &chunk[..n] {
            if byte != b'\n' {
                line.push(byte);
                continue;
            }
            if let Some(found) = parse(&line, name) {
                return Some(found);
            }
            line.clear();
        }
        if n == 0 {
            return parse(&line, name);
        }
    }
}

/// A line of the list, if it notes picture `name`.
fn parse(line: &[u8], name: &str) -> Option<((u32, u32), String)> {
    let line = core::str::from_utf8(line).ok()?;
    let mut fields = line.split('\t');
    if fields.next()? != name {
        return None;
    }
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    Some(((width, height), String::from(fields.next()?)))
}

/// Picture `base`: decoded, else as noted, to be decoded in the kind
/// wanted.
pub(crate) fn open_or_noted(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    open(storage, base, color).or_else(|| {
        let ((width, height), _) = noted(storage, base)?;
        Some(ImageRef {
            path: file(base, color),
            width,
            height,
        })
    })
}

/// Whether picture `base` is decoded in the kind wanted.
pub(crate) fn has(storage: &dyn Storage, base: &str, color: bool) -> bool {
    ImageFile::open(storage, &file(base, color)).is_some()
}
