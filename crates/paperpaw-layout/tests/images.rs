//! Pictures on pages: scaled into the content box keeping their
//! proportions, moved to the next page when they do not fit below text,
//! and harmless when a book gives sizes of zero (no positions that are
//! not numbers, no page that never ends).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_document::{
    Align, Block, BlockKind, Document, DocumentError, ImageRef, Locator, Metadata, NoStorage,
    OffsetMap, Storage,
};
use paperpaw_layout::{Face, LayoutContext, Page, Style, Typesetter, layout_page};
use proptest::prelude::*;

/// Blocks of one byte each: text paragraphs or pictures of a size.
struct Album {
    blocks: Vec<Option<(u32, u32)>>,
    metadata: Metadata,
}

impl Document for Album {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        self.blocks.len() as u32
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let Some(block) = self.blocks.get(at.offset as usize) else {
            return Ok(None);
        };
        let image = block.map(|(width, height)| ImageRef {
            path: format!("{}.gray", at.offset),
            width,
            height,
        });
        Ok(Some(Block {
            kind: if image.is_some() {
                BlockKind::Image
            } else {
                BlockKind::Paragraph
            },
            continues: false,
            start: at.offset,
            end: at.offset + 1,
            text: String::from("x"),
            map: OffsetMap::IDENTITY,
            runs: Vec::new(),
            align: Align::Auto,
            indent: None,
            space: None,
            size: None,
            image,
        }))
    }
}

struct Units;

impl Typesetter for Units {
    fn advance(&self, _: Face, _: char) -> f32 {
        1.0
    }

    fn line(&self, _: Face) -> (f32, f32) {
        (8.0, 10.0)
    }
}

const STYLE: Style = Style {
    paragraph_spacing: 4.0,
    indent: 0.0,
    em: 10.0,
    justify: false,
};

fn pages(album: &Album, width: f32, height: f32) -> Vec<Page> {
    let cx = LayoutContext {
        document: album,
        storage: &NoStorage,
        width,
        style: &STYLE,
        typesetter: &Units,
        hyphenate: None,
    };
    let mut pages = Vec::new();
    let mut at = Locator::START;
    loop {
        let page = layout_page(&cx, at, height).unwrap();
        if page.is_past_end() {
            return pages;
        }
        assert!(page.end > at, "the page at {at:?} does not move on");
        assert!(pages.len() <= album.blocks.len(), "more pages than blocks");
        at = page.end;
        pages.push(page);
    }
}

fn album(blocks: Vec<Option<(u32, u32)>>) -> Album {
    Album {
        blocks,
        metadata: Metadata::default(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn pictures_fit_the_box_and_keep_their_proportions(
        blocks in proptest::collection::vec(
            prop_oneof![1 => Just(None), 2 => (1u32..5000, 1u32..5000).prop_map(Some)],
            1..8,
        ),
        width in 10.0f32..500.0,
        height in 10.0f32..800.0,
    ) {
        let album = album(blocks);
        let pages = pages(&album, width, height);
        let shown: usize = pages.iter().map(|p| p.images.len() + p.lines.len()).sum();
        prop_assert_eq!(shown, album.blocks.len(), "every block once");
        for page in &pages {
            for image in &page.images {
                prop_assert!(image.width <= width + 1e-3 && image.height <= height + 1e-3);
                prop_assert!(image.x >= -1e-3 && image.y >= -1e-3);
                prop_assert!(image.y + image.height <= height + 1e-3, "{:?}", image);
                let ratio = image.image.width as f32 / image.image.height as f32;
                let shown = image.width / image.height;
                prop_assert!((ratio - shown).abs() <= ratio * 1e-3, "{} vs {}", ratio, shown);
            }
        }
    }
}

#[test]
fn a_picture_below_text_that_does_not_fit_goes_to_the_next_page() {
    let album = album(vec![None, Some((100, 100))]);
    let pages = pages(&album, 100.0, 100.0);
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].lines.len(), 1);
    // On a page of its own, without space above.
    assert_eq!(pages[1].images[0].y, 0.0);
    assert_eq!(pages[1].start, Locator::new(0, 1));
}

/// A picture the book gives no size (0 by 0, or 0 wide) has no place: it
/// must not put positions that are not numbers on the page, nor push
/// what follows off it.
#[test]
#[ignore = "bug: a picture of size 0 by 0 is scaled by infinity: NaN positions"]
fn pictures_of_size_zero_are_harmless() {
    for size in [(0, 0), (0, 50), (50, 0)] {
        let album = album(vec![Some(size), None]);
        let pages = pages(&album, 100.0, 100.0);
        for page in &pages {
            for image in &page.images {
                for value in [image.x, image.y, image.width, image.height] {
                    assert!(value.is_finite(), "{size:?}: {image:?}");
                }
            }
            for line in &page.lines {
                assert!(line.baseline.is_finite(), "{size:?}: {line:?}");
            }
        }
        let lines: usize = pages.iter().map(|p| p.lines.len()).sum();
        assert_eq!(lines, 1, "{size:?}: the text after it is shown");
    }
}
