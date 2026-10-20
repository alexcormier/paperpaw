//! Decoded pictures in the caches: gray (`.gray`) for e-ink, colour
//! (`.rgb`) for colour screens (ADR 0015). Chapters keep a picture's
//! cache name without its kind; it is decoded in the kind the book was
//! opened with, and read in that kind, else in the other one there is.
//!
//! A book's pictures are decoded when a page shows them
//! (`Document::prepare_image`), not when their chapter is converted: then
//! they are noted ([`note_all`]) in `images.txt` of the book's cache, a line
//! each with the cache name, the size they are decoded to and their path
//! in the archive, for pages to be laid out meanwhile.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{DocumentError, ImageRef};
use paperpaw_sdk::image::ImageFile;

/// The cache file of picture `base` in colour or gray.
pub(crate) fn file(base: &str, color: bool) -> String {
    format!("{base}.{}", if color { "rgb" } else { "gray" })
}

/// The decoded picture `base`, the kind wanted first, else the other; a
/// picture of no size is none.
pub(crate) fn open(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    [color, !color].into_iter().find_map(|color| {
        ImageFile::open(storage, &file(base, color))
            .filter(|image| image.width > 0 && image.height > 0)
            .map(|image| ImageRef {
                path: image.path,
                width: image.width,
                height: image.height,
            })
    })
}

/// The mark of picture `base` that could not be decoded.
fn mark(base: &str) -> String {
    format!("{base}.bad")
}

/// Whether picture `base` was found not to be decodable: it is not tried
/// again each time its page shows (decoding a large one takes seconds on
/// small devices), until its book's cache goes.
pub(crate) fn failed(storage: &dyn Storage, base: &str) -> bool {
    storage.exists(&mark(base))
}

/// Notes that picture `base` cannot be decoded, where `error` says the
/// picture is at fault: short memory or a storage error may pass.
pub(crate) fn note_failure(storage: &dyn Storage, base: &str, error: &DocumentError) {
    if matches!(error, DocumentError::Malformed(_)) {
        let _ = storage.write(&mark(base), &[]);
    }
}

/// The list of pictures noted in cache folder `dir`.
fn list(dir: &str) -> String {
    format!("{dir}/images.txt")
}

/// A picture to be noted: its cache name (see [`file`]), the size it is
/// decoded to and its path in the archive.
#[derive(Debug)]
pub(crate) struct Note {
    pub base: String,
    pub size: (u32, u32),
    pub source: String,
}

/// Notes pictures to be decoded when a page shows them, those of one
/// cache folder each, and empties `notes`. The list is read once for all
/// of them, to leave out those noted before.
pub(crate) fn note_all(storage: &dyn Storage, notes: &mut Vec<Note>) {
    let Some(dir) = notes
        .first()
        .and_then(|n| n.base.rsplit_once('/'))
        .map(|(d, _)| d)
    else {
        return;
    };
    let path = list(dir);
    let names: Vec<&str> = notes
        .iter()
        .map(|note| note.base.rsplit_once('/').map_or("", |(_, name)| name))
        .collect();
    // A picture of no size is never noted: it could not be laid out.
    let mut fresh: Vec<bool> = notes
        .iter()
        .map(|note| note.size.0 > 0 && note.size.1 > 0)
        .collect();
    crate::cached::lines(storage, &path, |line| {
        let noted = line.split('\t').next().unwrap_or_default();
        for (name, fresh) in names.iter().zip(fresh.iter_mut()) {
            *fresh &= *name != noted;
        }
        fresh.contains(&true)
    });
    let mut lines = String::new();
    for ((note, name), _) in notes.iter().zip(&names).zip(&fresh).filter(|(_, f)| **f) {
        let (width, height) = note.size;
        lines.push_str(&format!("{name}\t{width}\t{height}\t{}\n", note.source));
    }
    drop(names);
    let _ = storage.append(&path, lines.as_bytes());
    notes.clear();
}

/// Picture `base` as noted ([`note_all`]): the size it is decoded to and
/// its path in the archive. The list is read a little at a time.
pub(crate) fn noted(storage: &dyn Storage, base: &str) -> Option<((u32, u32), String)> {
    let (dir, name) = base.rsplit_once('/')?;
    let mut found = None;
    crate::cached::lines(storage, &list(dir), |line| {
        found = parse(line, name);
        found.is_none()
    });
    found
}

/// A line of the list, if it notes picture `name` at a size it can have.
fn parse(line: &str, name: &str) -> Option<((u32, u32), String)> {
    let mut fields = line.split('\t');
    if fields.next()? != name {
        return None;
    }
    let width = fields.next()?.parse().ok().filter(|&w| w > 0)?;
    let height = fields.next()?.parse().ok().filter(|&h| h > 0)?;
    Some(((width, height), String::from(fields.next()?)))
}

/// Picture `base`: decoded, else as noted, to be decoded in the kind
/// wanted, unless it was found not to be decodable.
pub(crate) fn open_or_noted(storage: &dyn Storage, base: &str, color: bool) -> Option<ImageRef> {
    open(storage, base, color).or_else(|| {
        if failed(storage, base) {
            return None;
        }
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
