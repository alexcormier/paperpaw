//! Hostile books: made to exhaust memory, storage or time. Each must open
//! and read within small, fixed bounds. Memory is measured with an
//! allocator that counts this thread's heap; the files of the storage are
//! not counted, as on a device they are on the card.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_plugin_format_epub::EpubDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::zip::testing::ZipBuilder;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

/// The system's allocator, counting what this thread holds.
struct Counting;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
    /// Set while the storage works: its files are not counted.
    static PAUSED: Cell<bool> = const { Cell::new(false) };
}

fn count(delta: isize) {
    if PAUSED.try_with(Cell::get).unwrap_or(true) {
        return;
    }
    let _ = LIVE.try_with(|live| {
        let now = live.get() + delta;
        live.set(now);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let block = unsafe { System.alloc(layout) };
        if !block.is_null() {
            count(layout.size() as isize);
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        unsafe { System.dealloc(block, layout) };
        count(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(block, layout, size) };
        if !moved.is_null() {
            count(size as isize - layout.size() as isize);
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most heap `f` holds at once, beyond what was held before.
fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let out = f();
    let top = PEAK.with(Cell::get);
    (out, (top - base) as usize)
}

/// Runs `f` without counting what it allocates.
fn paused<T>(f: impl FnOnce() -> T) -> T {
    PAUSED.with(|paused| paused.set(true));
    let out = f();
    PAUSED.with(|paused| paused.set(false));
    out
}

/// A card: files in memory that do not count as heap, and how often they
/// are read and written.
struct Card {
    files: MemoryStorage,
    reads: Cell<usize>,
    writes: Cell<usize>,
}

impl Card {
    fn new(path: &str, book: Vec<u8>) -> Card {
        Card {
            files: MemoryStorage::new().with(path, book),
            reads: Cell::new(0),
            writes: Cell::new(0),
        }
    }

    /// The bytes of all files below `dir`.
    fn bytes_below(&self, dir: &str) -> u64 {
        let mut files = Vec::new();
        list_all(&self.files, dir, &mut files);
        files.iter().map(|f| self.files.size(f).unwrap_or(0)).sum()
    }
}

fn list_all(storage: &MemoryStorage, dir: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        entries.push((format!("{dir}/{}", entry.name), entry.is_dir))
    });
    for (path, is_dir) in entries {
        if is_dir {
            list_all(storage, &path, out);
        } else {
            out.push(path);
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        paused(|| {
            self.reads.set(self.reads.get() + 1);
            self.files.read_at(path, offset, buf)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        paused(|| self.files.size(path))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        paused(|| {
            self.writes.set(self.writes.get() + 1);
            self.files.write(path, data)
        })
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        paused(|| {
            self.writes.set(self.writes.get() + 1);
            self.files.append(path, data)
        })
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.remove(path))
    }

    fn exists(&self, path: &str) -> bool {
        paused(|| self.files.exists(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        paused(|| self.files.rename(from, to))
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.create_dir(path))
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        paused(|| self.files.remove_dir_all(path))
    }
}

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;

/// An EPUB of one chapter, `ch1.xhtml`, with `extra` files beside it.
fn epub_with(chapter: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let opf = "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" \
        version=\"3.0\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
        <dc:title>Hostile</dc:title></metadata><manifest>\
        <item id=\"c1\" href=\"ch1.xhtml\" media-type=\"application/xhtml+xml\"/>\
        </manifest><spine><itemref idref=\"c1\"/></spine></package>";
    let mut zip = ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", CONTAINER)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/ch1.xhtml", chapter);
    for (name, data) in extra {
        zip = zip.stored(name, data.to_vec());
    }
    zip.build()
}

fn epub(chapter: &str) -> Vec<u8> {
    epub_with(chapter, &[])
}

/// What reading a book gave: its blocks, words and last block's text
/// (the texts themselves are not kept, so as not to count them as heap).
#[derive(Debug, Default, PartialEq, Eq)]
struct Read {
    blocks: usize,
    words: usize,
    last: String,
}

/// Reads every block of every section.
fn read_all(doc: &dyn Document, storage: &dyn Storage) -> Read {
    let mut read = Read::default();
    for section in 0..doc.sections() {
        let mut at = Locator::new(section, 0);
        while let Ok(Some(block)) = doc.block_at(storage, at) {
            assert!(block.end > at.offset, "no progress at {at:?}");
            at = Locator::new(section, block.end);
            read.blocks += 1;
            read.words += block.text.split_whitespace().count();
            read.last = block.text;
        }
    }
    read
}

/// Opens and reads an EPUB on `card`; the most heap it took.
fn open_and_read(card: &Card) -> (Read, usize) {
    peak(|| {
        let doc = EpubDocument::open(card, "book.epub").expect("opens");
        read_all(&doc, card)
    })
}

/// The most heap a book may take to be opened and read. A plain book
/// takes about 55 KiB here (on 64 bits), mostly the archive's inflater;
/// the bound leaves room for what a hostile one fills up to its limits
/// (deep elements, the places of many text nodes), not for growth with
/// it.
const PLAIN_PEAK: usize = 128 * 1024;

#[test]
fn deeply_nested_elements_take_little_memory_and_storage() {
    // Unclosed elements, each with a little text, as careless markup has.
    let mut chapter = String::from("<html><body>");
    for i in 0..20_000 {
        chapter.push_str(&format!("<span class=\"c{}\">w{i} ", i % 7));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.last.ends_with("w19999"));
    // The places of its text nodes are listed briefly, so what is written
    // stays near what the chapter says.
    let written = card.bytes_below(".paperpaw");
    assert!(
        written < 16 * chapter.len() as u64,
        "{written} bytes written"
    );
}

#[test]
fn unclosed_paragraphs_are_ended_by_the_next() {
    let mut chapter = String::from("<html><body>");
    for i in 0..1500 {
        chapter.push_str(&format!("<p>Paragraph {i}."));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert_eq!(read.blocks, 1500);
    assert_eq!(read.last, "Paragraph 1499.");
}

#[test]
fn many_names_long_names_and_nested_links_take_little_memory() {
    let mut chapter = String::from("<html><body><div>");
    // Children of many names, and names as long as markup may be.
    for i in 0..20_000 {
        chapter.push_str(&format!("<x{i}>a</x{i}>"));
    }
    let long = "n".repeat(3000);
    for _ in 0..100 {
        chapter.push_str(&format!("<{long}>b"));
    }
    for _ in 0..100 {
        chapter.push_str(&format!("</{long}>"));
    }
    // Links nested in links, each with a long target.
    let target = format!("https://example.org/{}", "t".repeat(1900));
    for _ in 0..100 {
        chapter.push_str(&format!("<a href=\"{target}\">c "));
    }
    chapter.push_str("</div></body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.last.ends_with('c'));
}

#[test]
fn a_block_of_many_text_nodes_is_continued() {
    // One paragraph, its words alternately bold and not.
    let mut chapter = String::from("<html><body><p>");
    for i in 0..3000 {
        chapter.push_str(&format!("<b>{i}</b> {i} "));
    }
    chapter.push_str("</p></body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.blocks > 1);
    assert_eq!(read.words, 6000);
    assert!(read.last.ends_with(" 2999"));
}

#[test]
fn many_pictures_are_listed_on_the_card() {
    let mut chapter = String::from("<html><body>");
    for i in 0..5000 {
        chapter.push_str(&format!("<img src=\"p{i}.png\" alt=\"{i}\"/>"));
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    // None of them is in the book: each is its alternative text.
    assert_eq!(read.blocks, 5000);
    assert_eq!(read.last, "4999");
}

#[test]
fn what_a_chapter_writes_is_bounded_by_its_size() {
    // Little markup, many places to list: text nodes deep down with long
    // names, each of one character.
    let mut chapter = String::from("<html><body>");
    for depth in 0..8 {
        chapter.push_str(&format!("<{}{depth}>", "d".repeat(30)));
    }
    for _ in 0..200_000 {
        chapter.push_str("<b>x</b>");
    }
    chapter.push_str("</body></html>");
    let card = Card::new("book.epub", epub(&chapter));
    let (read, peak) = open_and_read(&card);
    assert!(peak < PLAIN_PEAK, "{peak} bytes at once");
    assert!(read.blocks > 0);
    let written = card.bytes_below(".paperpaw");
    assert!(
        written < 16 * chapter.len() as u64 + (2 << 20),
        "{written} bytes written"
    );
    // Opened again, it is read from the cache as it is.
    let writes = card.writes.get();
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    assert_eq!(read_all(&doc, &card), read);
    assert_eq!(card.writes.get(), writes);
}

/// `zip` with the data of entry `name` damaged, so it cannot be inflated.
fn damage(mut zip: Vec<u8>, name: &str) -> Vec<u8> {
    let at = zip
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"PK\x03\x04")
        .map(|(i, _)| i)
        .find(|&i| {
            let len = usize::from(u16::from_le_bytes([zip[i + 26], zip[i + 27]]));
            zip.get(i + 30..i + 30 + len) == Some(name.as_bytes())
        })
        .expect("entry");
    let name_len = usize::from(u16::from_le_bytes([zip[at + 26], zip[at + 27]]));
    let extra = usize::from(u16::from_le_bytes([zip[at + 28], zip[at + 29]]));
    let data = at + 30 + name_len + extra;
    // A deflate block of the reserved type.
    zip[data..data + 8].fill(0xff);
    zip
}

#[test]
fn a_chapter_that_cannot_be_read_is_not_converted_again_and_again() {
    let chapter = format!("<html><body><p>{}</p></body></html>", "word ".repeat(2000));
    let card = Card::new("book.epub", damage(epub(&chapter), "OEBPS/ch1.xhtml"));
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    assert!(doc.block_at(&card, Locator::START).is_err());
    let (reads, writes) = (card.reads.get(), card.writes.get());
    for _ in 0..10 {
        assert!(doc.block_at(&card, Locator::START).is_err());
    }
    assert_eq!(card.writes.get(), writes, "converted again");
    assert!(card.reads.get() - reads <= 10, "inflated again");
}
