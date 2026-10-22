//! A book through the plugins' block filters (`reading::FilteredDocument`):
//! blocks left out, where filters are told a block is, runs left out read
//! once, too many left out an error, and a footnote whose block is left
//! out.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_sdk::Id;
use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::filters::{TextFilterSpec, Verdict};
use paperpaw_sdk::reading::{DroppedRuns, FilteredDocument, note_text};

/// Sections of paragraphs, each a block from its first byte to after
/// its line end; links `#<section>.<paragraph>` lead to paragraphs.
struct Book {
    sections: Vec<Vec<String>>,
    metadata: Metadata,
}

impl Book {
    fn new(sections: &[&[&str]]) -> Self {
        Self {
            sections: sections
                .iter()
                .map(|s| s.iter().map(|p| p.to_string()).collect())
                .collect(),
            metadata: Metadata::default(),
        }
    }

    /// Where paragraph `index` of `section` starts.
    fn start(&self, section: u32, index: usize) -> u32 {
        self.sections[section as usize][..index]
            .iter()
            .map(|p| p.len() as u32 + 1)
            .sum()
    }
}

impl Document for Book {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        self.sections.len() as u32
    }

    fn section_len(&self, section: u32) -> u32 {
        let paragraphs = &self.sections[section as usize];
        self.start(section, paragraphs.len())
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let Some(paragraphs) = self.sections.get(at.section as usize) else {
            return Ok(None);
        };
        let mut start = 0;
        for text in paragraphs {
            let end = start + text.len() as u32 + 1;
            if at.offset < end {
                return Ok(Some(Block::new(
                    BlockKind::Paragraph,
                    start,
                    end,
                    text.clone(),
                )));
            }
            start = end;
        }
        Ok(None)
    }

    fn link_target(&self, _: &dyn Storage, href: &str) -> Option<Locator> {
        let (section, index) = href.strip_prefix('#')?.split_once('.')?;
        let section: u32 = section.parse().ok()?;
        Some(Locator::new(
            section,
            self.start(section, index.parse().ok()?),
        ))
    }
}

/// What the filter was told: where each block it got starts.
type Told = Rc<RefCell<Vec<Locator>>>;

/// A harness whose filter leaves out every paragraph that begins with
/// `-`, telling `told` where each block it got is.
fn harness(told: &Told) -> common::Harness {
    let mut harness = common::Harness::new();
    let told = told.clone();
    harness.text_filters.add(TextFilterSpec::block(
        Id::new_static("test.dashes"),
        "dashes",
        Box::new(move |_, cx, block| {
            told.borrow_mut().push(cx.at);
            match block.text.starts_with('-') {
                true => Verdict::Drop,
                false => Verdict::Keep,
            }
        }),
        Box::new(|_| String::from("on")),
    ));
    harness
}

fn text(block: Result<Option<Block>, DocumentError>) -> Option<String> {
    block.unwrap().map(|b| b.text)
}

#[test]
fn a_block_begun_in_the_middle_is_told_where_it_starts() {
    let book = Book::new(&[&["one two three", "- out", "four five"]]);
    let told = Told::default();
    let harness = harness(&told);
    let env = harness.env();
    let filtered = FilteredDocument::new(&book, &env);
    let storage = &harness.storage;
    // In the middle of the first paragraph: that one, told its start.
    assert_eq!(
        text(filtered.block_at(storage, Locator::new(0, 5))).as_deref(),
        Some("one two three")
    );
    assert_eq!(*told.borrow(), [Locator::new(0, 0)]);
    // In the middle of the one left out: the next.
    told.borrow_mut().clear();
    assert_eq!(
        text(filtered.block_at(storage, Locator::new(0, 16))).as_deref(),
        Some("four five")
    );
    assert_eq!(*told.borrow(), [Locator::new(0, 14), Locator::new(0, 20)]);
}

#[test]
fn blocks_left_out_at_a_sections_end_end_it() {
    let book = Book::new(&[
        &["kept", "- out", "- out too"],
        &["- all", "- of it"],
        &["after"],
    ]);
    let told = Told::default();
    let harness = harness(&told);
    let env = harness.env();
    let filtered = FilteredDocument::new(&book, &env);
    let storage = &harness.storage;
    assert_eq!(text(filtered.block_at(storage, Locator::new(0, 5))), None);
    // A section left out whole has nothing; the next goes on.
    assert_eq!(text(filtered.block_at(storage, Locator::new(1, 0))), None);
    assert_eq!(
        text(filtered.block_at(storage, Locator::new(2, 0))).as_deref(),
        Some("after")
    );
}

#[test]
fn a_run_left_out_is_read_once() {
    let book = Book::new(&[&["- a", "- b", "- c", "kept"]]);
    let told = Told::default();
    let harness = harness(&told);
    let env = harness.env();
    let runs = DroppedRuns::new();
    let filtered = FilteredDocument::new(&book, &env).remembering(&runs);
    let storage = &harness.storage;
    assert_eq!(
        text(filtered.block_at(storage, Locator::new(0, 0))).as_deref(),
        Some("kept")
    );
    assert_eq!(told.borrow().len(), 4);
    // Again, from inside the run: straight to what is kept.
    told.borrow_mut().clear();
    assert_eq!(
        text(filtered.block_at(storage, Locator::new(0, 5))).as_deref(),
        Some("kept")
    );
    assert_eq!(*told.borrow(), [Locator::new(0, 12)]);
    // Forgotten, it is read again.
    runs.clear();
    told.borrow_mut().clear();
    filtered.block_at(storage, Locator::new(0, 0)).unwrap();
    assert_eq!(told.borrow().len(), 4);
}

#[test]
fn too_many_blocks_left_out_are_an_error_not_an_end() {
    let dashes: Vec<String> = (0..10_050).map(|_| String::from("-")).collect();
    let mut paragraphs: Vec<&str> = dashes.iter().map(String::as_str).collect();
    paragraphs.push("kept");
    let book = Book::new(&[&paragraphs]);
    let told = Told::default();
    let harness = harness(&told);
    let env = harness.env();
    let runs = DroppedRuns::new();
    let filtered = FilteredDocument::new(&book, &env).remembering(&runs);
    let storage = &harness.storage;
    let first = filtered.block_at(storage, Locator::START);
    assert!(
        matches!(first, Err(DocumentError::Malformed(_))),
        "{first:?}"
    );
    // What was read is remembered: asked again, it goes on from there.
    assert_eq!(
        text(filtered.block_at(storage, Locator::START)).as_deref(),
        Some("kept")
    );
}

#[test]
fn a_note_whose_block_is_left_out_has_no_text() {
    let book = Book::new(&[&["Text with notes.", "- an advert", "A note of some length."]]);
    let told = Told::default();
    let harness = harness(&told);
    let env = harness.env();
    let filtered = FilteredDocument::new(&book, &env);
    let storage = &harness.storage;
    assert_eq!(note_text(&filtered, storage, "#0.1"), None);
    assert_eq!(
        note_text(&filtered, storage, "#0.2").as_deref(),
        Some("A note of some length.")
    );
    // The block left out is not the next one kept.
    assert_eq!(
        filtered
            .kept_block_at(storage, Locator::new(0, 17))
            .unwrap(),
        None
    );
}
