//! The notes a reader exports (`core.notes-exported`): Markdown with
//! the book's title as `# Title`, its author on the next line, then the
//! highlights and bookmarks under `##` headings (see the `readwise`
//! plugin, which reads the same). Read in pieces, never whole.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

/// The start of a notes file read for its title and author.
const HEAD: usize = 1024;
/// The pieces a body is copied in.
const PIECE: usize = 1024;

/// A book's title and author, as its notes start.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Head {
    pub title: String,
    pub author: String,
}

/// The title and author at the start of the notes file `path`.
pub fn head(storage: &dyn Storage, path: &str) -> Result<Head, StorageError> {
    let mut start = [0u8; HEAD];
    let n = storage.read_at(path, 0, &mut start)?;
    Ok(parse_head(&String::from_utf8_lossy(&start[..n])))
}

/// The title (`# Title`) and the author, the next line that is not
/// blank, both before the first `##` heading.
pub fn parse_head(text: &str) -> Head {
    let mut head = Head::default();
    for line in text.lines().map(str::trim) {
        if line.starts_with("## ") {
            break;
        }
        if let Some(title) = line.strip_prefix("# ") {
            head.title = title.trim().to_string();
        } else if !line.is_empty() && !head.title.is_empty() {
            head.author = line.to_string();
            break;
        }
    }
    head
}

/// Hands on the body of the notes file `from` in pieces: all of it but
/// the title line and the blank lines around, which a Joplin note has as
/// its title. Returns whether there was any.
pub fn copy_body(
    storage: &dyn Storage,
    from: &str,
    out: &mut dyn FnMut(&[u8]) -> Result<(), StorageError>,
) -> Result<bool, StorageError> {
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    let mut body = Body::default();
    loop {
        let n = storage.read_at(from, at, &mut piece)?;
        if n == 0 {
            return Ok(body.written);
        }
        at += n as u64;
        let text = body.feed(&piece[..n]);
        if !text.is_empty() {
            out(&text)?;
        }
    }
}

/// Where a body being copied is.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Before its first text: the title line and blank lines are left
    /// out.
    #[default]
    Start,
    /// After a `#` at the start: a title if a blank follows.
    Hash,
    /// In the title line.
    Title,
    Text,
}

/// A body copied piece by piece: what comes before is left out, the line
/// breaks at the end are held back until text follows them.
#[derive(Debug, Default)]
struct Body {
    part: Part,
    held: Vec<u8>,
    /// A `#` that turned out to be text, still to write.
    hash: bool,
    written: bool,
}

impl Body {
    /// What of `piece` goes into the note.
    fn feed(&mut self, mut piece: &[u8]) -> Vec<u8> {
        while self.part != Part::Text {
            let Some((&first, rest)) = piece.split_first() else {
                return Vec::new();
            };
            match (self.part, first) {
                (Part::Start, b'\n' | b'\r' | b' ' | b'\t') => piece = rest,
                (Part::Start, b'#') => {
                    self.part = Part::Hash;
                    piece = rest;
                }
                (Part::Start, _) => self.part = Part::Text,
                (Part::Hash, b' ') => self.part = Part::Title,
                (Part::Hash, _) => {
                    // Text that starts with `#`, e.g. a `##` heading.
                    self.part = Part::Text;
                    self.hash = true;
                }
                (Part::Title, b'\n') => {
                    self.part = Part::Start;
                    piece = rest;
                }
                (_, _) => piece = rest,
            }
        }
        let text = piece
            .iter()
            .rposition(|b| !matches!(b, b'\n' | b'\r'))
            .map_or(0, |last| last + 1);
        let mut out = Vec::new();
        if core::mem::take(&mut self.hash) {
            out.push(b'#');
            self.written = true;
        }
        if text > 0 {
            out.append(&mut self.held);
            out.extend_from_slice(&piece[..text]);
            self.written = true;
        }
        self.held.extend_from_slice(&piece[text..]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const NOTES: &str = "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n\
        > Curiouser and curiouser!\n>\n> 12 %\n\nA note.\n\n\
        ## Bookmarks\n\n- 50 %: Down the rabbit hole\n\n";

    #[test]
    fn reads_the_title_and_author() {
        let head = parse_head(NOTES);
        assert_eq!(head.title, "Alice's Adventures");
        assert_eq!(head.author, "Lewis Carroll");
        let head = parse_head("# Title\n\n## Highlights\n\n> x\n>\n> 1 %\n");
        assert_eq!((head.title.as_str(), head.author.as_str()), ("Title", ""));
    }

    /// `text`'s body, fed in pieces of `size`.
    fn body_in_pieces(text: &str, size: usize) -> String {
        let mut body = Body::default();
        let mut out = Vec::new();
        for piece in text.as_bytes().chunks(size) {
            out.extend(body.feed(piece));
        }
        assert_eq!(body.written, !out.is_empty());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn copies_the_body_without_the_title_in_any_pieces() {
        let want = &NOTES[NOTES.find("Lewis").unwrap()..NOTES.len() - 2];
        for size in [1, 2, 3, 7, 64, 4096] {
            assert_eq!(body_in_pieces(NOTES, size), want, "pieces of {size}");
            assert_eq!(body_in_pieces("\n## H\n\n> x\n", size), "## H\n\n> x");
            assert_eq!(body_in_pieces("#tag\n", size), "#tag");
            assert_eq!(body_in_pieces("# T\n\n\n", size), "");
        }
        let storage = MemoryStorage::new()
            .with("a.md", NOTES)
            .with("empty.md", "# Only a title\n\n\n")
            .with("plain.md", "\n## Highlights\n\n> x\n");
        let copy = |from: &str| {
            let mut out = Vec::new();
            let any = copy_body(&storage, from, &mut |piece| {
                out.extend_from_slice(piece);
                Ok(())
            })
            .unwrap();
            (any, String::from_utf8(out).unwrap())
        };
        assert_eq!(copy("a.md"), (true, String::from(want)));
        assert_eq!(copy("empty.md"), (false, String::new()));
        // A `##` heading is no title.
        assert_eq!(
            copy("plain.md"),
            (true, String::from("## Highlights\n\n> x"))
        );
        assert_eq!(
            copy_body(&storage, "none.md", &mut |_| Ok(())),
            Err(StorageError::NotFound)
        );
    }
}
