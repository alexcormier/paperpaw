//! Page layout: turns a [`Document`] into pages.
//!
//! Layout is deterministic and starts at any [`Locator`] that begins a line,
//! so a reader only needs to remember where pages start, never the pages
//! themselves. Text is broken greedily per paragraph; because greedy breaking
//! is stable, laying out from the middle of a paragraph yields exactly the
//! same lines as laying out the whole paragraph.
//!
//! The engine knows nothing about fonts or drawing: a [`Typesetter`]
//! measures characters in each [`Face`] (character style, body text or
//! heading level) and kerns pairs of them, and the engine returns
//! positioned lines with their runs of faces. Pairs are kerned within a
//! word and a face only: a line's width is that of its words drawn one by
//! one, each run of a face at once.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

pub use paperpaw_document::ImageRef;
pub use paperpaw_document::TextStyle;
use paperpaw_document::{
    Align, AnnotationKind, Block, BlockKind, Document, DocumentError, Locator, MAX_LEADING_ABOVE,
    Storage,
};
pub use paperpaw_text::Hyphenate;
use paperpaw_text::{Line, wrap_lines_balanced_kerned, wrap_lines_kerned};

/// Largest extra space per word gap for justification, relative to the
/// line height.
const MAX_JUSTIFY_GAP: f32 = 0.6;
/// Extra space above a heading, relative to a body line.
const HEADING_SPACE: f32 = 0.6;
/// The least height of a line, in pixels, so a page holds a bounded
/// number of them whatever a [`Typesetter`] says.
const MIN_LINE_HEIGHT: f32 = 1.0;

/// How text is set: its character style and whether it is body text or a
/// heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Face {
    pub style: TextStyle,
    /// 0 for body text, 1 (most important) to 6 for headings.
    pub heading: u8,
    /// The book's text size in percent of body text; 0 for the reader's
    /// (body text, or by heading level).
    pub size: u16,
}

/// Changes whenever the same input is laid out differently, so readers
/// rebuild page indexes they cached.
pub const VERSION: u32 = 7;

/// Measures text for the layout engine; readers implement it with their
/// fonts.
pub trait Typesetter {
    /// Advance width of `c` in `face`.
    fn advance(&self, face: Face, c: char) -> f32;

    /// How much `right` moves when it follows `left` in `face`, in
    /// pixels (negative: closer): the font's kerning, as drawing text of
    /// that face applies it. Asked only for two characters of one word
    /// and one face (also for a hyphen after a letter); 0 by default.
    fn kern(&self, face: Face, left: char, right: char) -> f32 {
        let _ = (face, left, right);
        0.0
    }

    /// Lines in `face`: `(ascent, line height)`, i.e. the distance from
    /// the top of a line box to its baseline and between baselines.
    fn line(&self, face: Face) -> (f32, f32);
}

/// Which of a book's own styles are followed (see [`Style::book`]): each
/// can be left to the reader alone, e.g. a book's alignment but not its
/// margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BookStyles(u16);

impl BookStyles {
    /// None: the reader's settings alone.
    pub const NONE: BookStyles = BookStyles(0);
    /// A block's alignment ([`Block::align`]).
    pub const ALIGN: BookStyles = BookStyles(1);
    /// A first line's indent ([`Block::indent`]).
    pub const INDENT: BookStyles = BookStyles(1 << 1);
    /// Text sizes ([`Block::size`]).
    pub const SIZE: BookStyles = BookStyles(1 << 2);
    /// The space above blocks ([`Block::space`]).
    pub const SPACING: BookStyles = BookStyles(1 << 3);
    /// Font families the book embeds ([`TextStyle::family`]).
    pub const FONTS: BookStyles = BookStyles(1 << 4);
    /// Margins at the sides of blocks ([`Block::margins`]).
    pub const MARGINS: BookStyles = BookStyles(1 << 5);
    /// The distance between lines ([`Block::line_height`]).
    pub const LINE_HEIGHT: BookStyles = BookStyles(1 << 6);
    /// All of them.
    pub const ALL: BookStyles = BookStyles(0x7F);

    pub const fn has(self, other: BookStyles) -> bool {
        self.0 & other.0 == other.0
    }

    /// These and `other`'s.
    #[must_use]
    pub const fn with(self, other: BookStyles) -> BookStyles {
        BookStyles(self.0 | other.0)
    }

    /// These without `other`'s.
    #[must_use]
    pub const fn without(self, other: BookStyles) -> BookStyles {
        BookStyles(self.0 & !other.0)
    }

    /// For a layout's key.
    pub const fn bits(self) -> u16 {
        self.0
    }
}

/// Typographic settings for a page, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Style {
    /// Extra space between paragraphs.
    pub paragraph_spacing: f32,
    /// Indent of the first line of a paragraph.
    pub indent: f32,
    /// The body text size, for a book's indents and spaces given in ems.
    pub em: f32,
    /// Stretch spaces so lines fill the width (except a paragraph's last).
    /// The reader's choice wins over a book's justified text; centred and
    /// right-aligned blocks keep their alignment.
    pub justify: bool,
    /// The fewest lines of a paragraph left alone at the top of a page (a
    /// widow): lines that would leave fewer take more along to the next
    /// page. 1 lets lines fall where they fall.
    pub widows: u8,
    /// The fewest lines of a paragraph left alone at the bottom of a page
    /// (an orphan): a paragraph that would leave fewer goes to the next
    /// page whole, with a heading just before it. 1 lets lines fall where
    /// they fall.
    pub orphans: u8,
    /// Which of the book's own styles are followed; all by default.
    pub book: BookStyles,
    /// Paragraphs broken over all their lines, for lines that end more
    /// evenly ([`paperpaw_text::wrap_lines_balanced`]), not line by line;
    /// off by default.
    pub balanced: bool,
}

impl Style {
    /// Made of what it needs; the rest as set with its methods.
    pub const fn new(paragraph_spacing: f32, indent: f32, em: f32, justify: bool) -> Self {
        Self {
            paragraph_spacing,
            indent,
            em,
            justify,
            widows: 1,
            orphans: 1,
            book: BookStyles::ALL,
            balanced: false,
        }
    }

    /// The same, breaking paragraphs over all their lines, or line by line.
    pub const fn balanced(mut self, balanced: bool) -> Self {
        self.balanced = balanced;
        self
    }

    /// The same, following only `book` of the book's own styles.
    pub const fn book(mut self, book: BookStyles) -> Self {
        self.book = book;
        self
    }

    /// The same, keeping at least `widows` lines of a paragraph at the top
    /// of a page and `orphans` at the bottom (see [`Style::widows`]).
    pub const fn lines_kept(mut self, widows: u8, orphans: u8) -> Self {
        self.widows = widows;
        self.orphans = orphans;
        self
    }
}

/// Text of a line set in one face: byte range of [`PageLine::text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRun {
    pub start: usize,
    pub end: usize,
    pub face: Face,
}

/// A line of text at its position on the page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageLine {
    /// Left edge relative to the content box.
    pub x: f32,
    /// Baseline relative to the content box's top.
    pub baseline: f32,
    pub text: String,
    /// Extra width to add to every space (justification).
    pub word_spacing: f32,
    /// The faces of `text`, in order and covering all of it.
    pub runs: Vec<LineRun>,
    /// The section the line is from.
    pub section: u32,
    /// Where each word of `text` (separated by spaces) starts and ends in
    /// the section's source, for positions that outlast the layout (e.g.
    /// highlights).
    pub words: Vec<(u32, u32)>,
    /// The room above the line's letters its block asked for
    /// ([`Block::leading_above`]), in pixels: part of the line's height,
    /// between the line before and this one's ascent, where a page layer
    /// draws (an interlinear reading). 0 for none.
    pub leading: f32,
    /// The line is of a block a text filter inserted
    /// ([`Block::inserted`]): its words have no places (`words` is
    /// empty), nothing on it is chosen, kept or read as the book's.
    pub inserted: bool,
    /// Annotations set above the line's words (ruby, glosses). Reserved:
    /// empty until the layout sets them (ADR 0032).
    pub annotations: Vec<LineAnnotation>,
}

/// An annotation set above a line ([`Block::annotations`]), where the
/// layout put it. Reserved: none are set yet (ADR 0032).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LineAnnotation {
    /// Left edge relative to the content box, and its width.
    pub x: f32,
    pub width: f32,
    /// Its baseline relative to the content box's top.
    pub baseline: f32,
    pub text: String,
    pub kind: AnnotationKind,
    /// Its text size in pixels.
    pub size: f32,
}

/// An image at its position and size on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    /// Left and top edges relative to the content box.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub image: ImageRef,
}

/// One laid-out page.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Page {
    /// Position of the first character on the page.
    pub start: Locator,
    /// Position where the next page starts; equal to `start` past the end.
    pub end: Locator,
    pub lines: Vec<PageLine>,
    pub images: Vec<PageImage>,
}

impl Page {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(start: Locator, end: Locator, lines: Vec<PageLine>, images: Vec<PageImage>) -> Self {
        Self {
            start,
            end,
            lines,
            images,
        }
    }
}

impl Page {
    /// Whether this page is past the end of the document.
    pub fn is_past_end(&self) -> bool {
        self.lines.is_empty() && self.images.is_empty()
    }
}

/// Source offsets of the words in `range` of a block's text.
fn word_sources(block: &Block, range: core::ops::Range<usize>) -> Vec<(u32, u32)> {
    let text = &block.text[range.clone()];
    let mut words = Vec::new();
    let mut at = range.start;
    for word in text.split(' ') {
        // The end after the last character: the offset of the text after
        // the word may lie past the block's end.
        if let Some((last, c)) = word.char_indices().last() {
            let end = block.source_offset(at + last) + c.len_utf8() as u32;
            words.push((block.source_offset(at), end));
        }
        at += word.len() + 1;
    }
    words
}

/// The face of a block's text at byte `index`; the book's own fonts only
/// where `book` follows them.
fn face_at(block: &Block, base: Face, index: usize, book: BookStyles) -> Face {
    let style = base.style.union(block.style_at(index));
    Face {
        style: if book.has(BookStyles::FONTS) {
            style
        } else {
            style.with_family(0)
        },
        heading: base.heading,
        size: base.size,
    }
}

/// The runs of faces for `range` of the block's text, as ranges of a line
/// text starting at `range.start`, plus `extra` bytes at the end (a hyphen).
fn line_runs(
    block: &Block,
    base: Face,
    range: core::ops::Range<usize>,
    extra: usize,
    book: BookStyles,
) -> Vec<LineRun> {
    let mut runs: Vec<LineRun> = Vec::new();
    for (i, _) in block.text[range.clone()].char_indices() {
        let face = face_at(block, base, range.start + i, book);
        match runs.last_mut() {
            Some(run) if run.face == face => run.end = i,
            _ => runs.push(LineRun {
                start: i,
                end: i,
                face,
            }),
        }
    }
    // Ends are exclusive: every run reaches the next one or the line end.
    let len = range.len() + extra;
    for k in 0..runs.len() {
        runs[k].end = runs.get(k + 1).map_or(len, |next| next.start);
    }
    if runs.is_empty() && len > 0 {
        runs.push(LineRun {
            start: 0,
            end: len,
            face: base,
        });
    }
    runs
}

/// The heading level of a block (0: none) and the face of its text; the
/// book's size only where `style` follows it.
fn block_face(block: &Block, style: &Style) -> (u8, Face) {
    let heading = match block.kind {
        BlockKind::Heading(level) => level.clamp(1, 6),
        _ => 0,
    };
    let face = Face {
        style: if heading > 0 {
            TextStyle::BOLD
        } else {
            TextStyle::REGULAR
        },
        heading,
        size: book_size(block, style).unwrap_or(0),
    };
    (heading, face)
}

/// The book's text size of a block, where `style` follows it.
fn book_size(block: &Block, style: &Style) -> Option<u16> {
    block.size.filter(|_| style.book.has(BookStyles::SIZE))
}

/// The ems of a block's text, in pixels: the book's are those of the
/// block's own text.
fn block_em(block: &Block, style: &Style) -> f32 {
    style.em * f32::from(book_size(block, style).unwrap_or(100)) / 100.0
}

/// Where a block's lines start and how wide they are: the first line
/// (indented where the block starts, by half a line at most, perhaps
/// hanging out into the book's margin) and the others, within the book's
/// margins, each kept to a quarter of the width.
#[derive(Debug, Clone, Copy)]
struct Measure {
    first: (f32, f32),
    rest: (f32, f32),
}

impl Measure {
    fn of(block: &Block, style: &Style, width: f32, indent: f32) -> Measure {
        let (start, end) = match style.book.has(BookStyles::MARGINS) {
            true => {
                let em = block_em(block, style);
                let side = |m: u16| (f32::from(m) / 100.0 * em).clamp(0.0, width / 4.0);
                (side(block.margins.0), side(block.margins.1))
            }
            false => (0.0, 0.0),
        };
        let rest = (start, (width - start - end).max(1.0));
        // Indented by half a line at most, so a narrow page keeps room.
        let first_x = (start + indent).clamp(0.0, start + rest.1 / 2.0);
        Measure {
            first: (first_x, (width - end - first_x).max(1.0)),
            rest,
        }
    }

    /// Where line `n` starts and how wide it is.
    fn line(&self, n: usize) -> (f32, f32) {
        if n == 0 { self.first } else { self.rest }
    }
}

/// The lines of `block` from byte `from` of its text on (a line's start),
/// their ranges in its text, and where they are set: broken line by line
/// from there, or (balanced) over the whole block, from where it starts
/// (`indent` is its first line's), and those from `from` on taken.
fn block_lines(
    block: &Block,
    from: usize,
    base: Face,
    cx: &LayoutContext<'_>,
    heading: u8,
    indent: f32,
) -> Vec<(Line, (f32, f32))> {
    let (style, width) = (cx.style, cx.width);
    let advance = |i: usize, c: char| {
        cx.typesetter
            .advance(face_at(block, base, i, style.book), c)
    };
    // A pair in one face: the character before `i` and `right`, the text's
    // own there or a hyphen set like the character before it.
    let kern = |i: usize, left: char, right: char| {
        let face = face_at(block, base, i - left.len_utf8(), style.book);
        let own = block.text[i..].starts_with(right);
        if own && face_at(block, base, i, style.book) != face {
            return 0.0;
        }
        cx.typesetter.kern(face, left, right)
    };
    let hyphenate = cx.hyphenate.filter(|_| heading == 0);
    if style.balanced {
        let measure = Measure::of(block, style, width, indent);
        let lines = wrap_lines_balanced_kerned(
            &block.text,
            |n| measure.line(n).1,
            advance,
            kern,
            hyphenate,
        );
        return lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| line.range.start >= from)
            .map(|(n, line)| (line, measure.line(n)))
            .collect();
    }
    // A page that goes on with a block sets its lines as wide as those
    // after a first line.
    let measure = match from == 0 && !block.continues {
        true => Measure::of(block, style, width, indent),
        false => {
            let rest = Measure::of(block, style, width, 0.0).rest;
            Measure { first: rest, rest }
        }
    };
    let lines = wrap_lines_kerned(
        &block.text[from..],
        |n| measure.line(n).1,
        |i, c| advance(from + i, c),
        |i, left, right| kern(from + i, left, right),
        hyphenate,
    );
    lines
        .into_iter()
        .enumerate()
        .map(|(n, mut line)| {
            line.range = from + line.range.start..from + line.range.end;
            (line, measure.line(n))
        })
        .collect()
}

/// How a block's lines are aligned.
fn block_align(block: &Block, style: &Style, heading: u8) -> Align {
    let align = match style.book.has(BookStyles::ALIGN) {
        true => block.align,
        false => Align::Auto,
    };
    match align {
        Align::Auto if heading > 0 => Align::Start,
        Align::Auto | Align::Justify if style.justify => Align::Justify,
        Align::Auto | Align::Justify => Align::Start,
        align => align,
    }
}

/// The indent of a block's first line, where it starts (not where a page
/// continues it).
fn first_indent(block: &Block, style: &Style, align: Align) -> f32 {
    let em = block_em(block, style);
    let book = block.indent.filter(|_| style.book.has(BookStyles::INDENT));
    if block.continues || block.kind != BlockKind::Paragraph {
        0.0
    } else if let Some(hanging) = book.filter(|&i| i < 0) {
        // A hanging indent, whatever the alignment.
        f32::from(hanging) / 100.0 * em
    } else if matches!(align, Align::Start | Align::Justify) {
        book.map_or(style.indent, |i| f32::from(i) / 100.0 * em)
    } else {
        0.0
    }
}

/// What laying out a book's pages needs, the same for each of them: the
/// document and the storage it is read from, how wide a line is, the
/// style, how text is measured, and how words are hyphenated, if they
/// are.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct LayoutContext<'a> {
    pub document: &'a dyn Document,
    pub storage: &'a dyn Storage,
    /// The width of a line.
    pub width: f32,
    pub style: &'a Style,
    pub typesetter: &'a dyn Typesetter,
    /// Words that do not fit at the end of a line are hyphenated with it.
    pub hyphenate: Option<Hyphenate<'a>>,
}

impl<'a> LayoutContext<'a> {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(
        document: &'a dyn Document,
        storage: &'a dyn Storage,
        width: f32,
        style: &'a Style,
        typesetter: &'a dyn Typesetter,
    ) -> Self {
        Self {
            document,
            storage,
            width,
            style,
            typesetter,
            hyphenate: None,
        }
    }

    pub fn hyphenate(mut self, hyphenate: impl Into<Option<Hyphenate<'a>>>) -> Self {
        self.hyphenate = hyphenate.into();
        self
    }
}

impl core::fmt::Debug for LayoutContext<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LayoutContext")
            .field("width", &self.width)
            .field("style", self.style)
            .field("hyphenate", &self.hyphenate.is_some())
            .finish_non_exhaustive()
    }
}

/// The start of the line `at` falls in, the block laid out from its
/// start as [`layout_page`] does: a page shown from a position kept with
/// another layout (other fonts, a turned screen) or found by a search
/// starts with that whole line, not inside it.
pub fn line_start(cx: &LayoutContext<'_>, at: Locator) -> Result<Locator, DocumentError> {
    let LayoutContext {
        document,
        storage,
        style,
        ..
    } = *cx;
    let Some(block) = document.block_at(storage, at)? else {
        return Ok(at);
    };
    if block.kind == BlockKind::Image || at.offset <= block.start {
        return Ok(at);
    }
    let from = block.text_index(at.offset);
    if from == 0 {
        return Ok(at);
    }
    let (heading, base) = block_face(&block, style);
    let indent = first_indent(&block, style, block_align(&block, style, heading));
    let lines = block_lines(&block, 0, base, cx, heading, indent);
    let start = lines
        .iter()
        .rev()
        .map(|(line, _)| line.range.start)
        .find(|start| *start <= from)
        .unwrap_or(0);
    Ok(Locator::new(at.section, block.source_offset(start)))
}

/// Lays out the page starting at `start`, `height` high, as `cx` says
/// (see [`LayoutContext`]).
///
/// Headings are set in their own face, never indented, justified or
/// hyphenated, and never left alone at the bottom of a page.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn layout_page(
    cx: &LayoutContext<'_>,
    start: Locator,
    height: f32,
) -> Result<Page, DocumentError> {
    let LayoutContext {
        document,
        storage,
        width,
        style,
        typesetter,
        ..
    } = *cx;
    let mut page = Page {
        start,
        end: start,
        lines: Vec::new(),
        images: Vec::new(),
    };
    let mut at = start;
    let mut y = 0.0f32;
    let body_line = typesetter.line(Face::default()).1.max(MIN_LINE_HEIGHT);
    // The heading laid out last, while nothing else followed it on the
    // page: the lines before it, where it starts, and whether anything
    // came before it. A paragraph that goes on whole takes it along.
    let mut heading_before: Option<(usize, Locator, bool)> = None;

    loop {
        let Some(block) = document.block_at(storage, at)? else {
            // End of the section: chapters start on a new page.
            let next = at
                .section
                .checked_add(1)
                .filter(|&n| n < document.sections());
            if let Some(next) = next {
                at = Locator::new(next, 0);
                if page.is_past_end() {
                    page.start = at;
                    continue;
                }
            }
            break;
        };
        // Every block takes the layout further, or it would never end.
        if block.end <= at.offset {
            return Err(malformed());
        }
        let from = if at.offset > block.start {
            block.text_index(at.offset)
        } else {
            0
        };
        if page.is_past_end() && from == 0 && at.offset < block.start {
            page.start = Locator::new(at.section, block.start);
        }
        // What follows a heading, if this block does.
        let after_heading = heading_before.take();
        if block.kind == BlockKind::Image
            && let Some(image) = &block.image
        {
            // A picture without size has nothing to show (and no scale).
            if image.width == 0 || image.height == 0 {
                at = Locator::new(at.section, block.end);
                continue;
            }
            // Scaled to fit the content box, keeping its proportions.
            let scale = (width / image.width as f32).min(height / image.height as f32);
            let (w, h) = (image.width as f32 * scale, image.height as f32 * scale);
            let spacing = if page.is_past_end() {
                0.0
            } else {
                style.paragraph_spacing.max(body_line * 0.5)
            };
            if !page.is_past_end() && y + spacing + h > height {
                return ended(page, Locator::new(at.section, block.start));
            }
            if page.is_past_end() {
                page.start = Locator::new(at.section, block.start);
            }
            y += spacing;
            page.images.push(PageImage {
                x: (width - w) / 2.0,
                y,
                width: w,
                height: h,
                image: image.clone(),
            });
            y += h + spacing;
            at = Locator::new(at.section, block.end);
            continue;
        }
        let (heading, base) = block_face(&block, style);
        let em = block_em(&block, style);
        let (ascent, line_height) = block_line(&block, style, typesetter.line(base), em);
        // Room a plugin asked for above every line: it is part of each
        // line's height, so the page breaks with it.
        let leading = block_leading(&block, em);
        let letters = line_height;
        let (ascent, line_height) = (ascent + leading, line_height + leading);
        let lines_before = page.lines.len();
        let had_content = !page.is_past_end();
        let at_block_start = from == 0 && !block.continues;
        // The book's space where it sets one; at the top of a page only
        // where a chapter starts (e.g. a title set lower).
        let book_space = block
            .space
            .filter(|_| style.book.has(BookStyles::SPACING))
            .map(|s| f32::from(s) / 100.0 * em);
        if page.is_past_end() && at_block_start && book_space.is_some() {
            let first = document.block_at(storage, Locator::new(at.section, 0))?;
            if first.is_some_and(|f| f.start == block.start) {
                y += book_space.unwrap_or(0.0).min(height / 3.0);
            }
        }
        if !page.is_past_end() && at_block_start {
            y += book_space.unwrap_or(style.paragraph_spacing);
            if heading > 0 {
                y += body_line * HEADING_SPACE;
            }
        }
        let align = block_align(&block, style, heading);
        let indent = if at_block_start {
            first_indent(&block, style, align)
        } else {
            0.0
        };
        // Balanced, the whole block is broken from its start, its first
        // line indented as there.
        let block_indent = match style.balanced {
            true => first_indent(&block, style, align),
            false => indent,
        };
        let lines = block_lines(&block, from, base, cx, heading, block_indent);

        for (n, (line, (first_x, available))) in lines.iter().enumerate() {
            let (first_x, available) = (*first_x, *available);
            let fits = y + line_height <= height;
            if !fits && !page.is_past_end() {
                // Widows and orphans: as many lines go on as the next page
                // needs, or the paragraph goes whole where too few would
                // stay here.
                let left = lines.len() - n;
                let back = usize::from(style.widows).saturating_sub(left).min(n);
                let stay = n - back;
                let orphans = usize::from(style.orphans.max(1));
                let alone = stay < orphans;
                let line_start = |i: usize| {
                    Locator::new(at.section, block.source_offset(lines[i].0.range.start))
                };
                if alone && had_content {
                    match after_heading {
                        // The heading just before goes along.
                        Some((kept, start, true)) => {
                            page.lines.truncate(kept);
                            return ended(page, start);
                        }
                        // The paragraph goes whole.
                        None => {
                            page.lines.truncate(page.lines.len() - n);
                            return ended(page, line_start(0));
                        }
                        // Under a heading that starts the page nothing can
                        // go: it stays as it falls.
                        Some(_) => {}
                    }
                } else if back > 0 && !alone {
                    page.lines.truncate(page.lines.len() - back);
                    return ended(page, line_start(stay));
                }
                // Starting the page, it cannot go whole: then it stays as it
                // falls rather than leaving an orphan here.
                return ended(page, line_start(n));
            }
            let free = (available - line.width).max(0.0);
            let (x, word_spacing) = match align {
                Align::Center => (first_x + free / 2.0, 0.0),
                Align::End => (first_x + free, 0.0),
                Align::Justify if !line.last && line.spaces > 0 => {
                    let extra = free / line.spaces as f32;
                    // Huge gaps look worse than a ragged line: set those
                    // lines flush left (e.g. a few words before a very long
                    // word).
                    if extra > letters * MAX_JUSTIFY_GAP {
                        (first_x, 0.0)
                    } else {
                        (first_x, extra)
                    }
                }
                _ => (first_x, 0.0),
            };
            let mut line_text = String::from(&block.text[line.range.clone()]);
            if line.hyphen {
                line_text.push('-');
            }
            let range = line.range.clone();
            let words = word_sources(&block, range.clone());
            page.lines.push(PageLine {
                x,
                baseline: y + ascent,
                text: line_text,
                word_spacing,
                runs: line_runs(&block, base, range, usize::from(line.hyphen), style.book),
                section: at.section,
                words,
                leading,
                inserted: false,
                annotations: Vec::new(),
            });
            y += line_height;
        }
        // What a filter inserted after the block, below its last line and
        // never apart from it.
        if !block.inserted.is_empty() {
            let (placed, below, whole) = inserted_lines(&block, cx, at.section, y, height);
            if !whole {
                // Not all of it fits: the page ends before the block's last
                // line, which goes on the next page with it, where all of
                // it fits there. Where it would not fit on any page (or
                // that line starts the page), what fits stays here and the
                // rest goes: what is inserted has no place to go on from.
                let last = page.lines.len().saturating_sub(1);
                let (_, tall, _) = inserted_lines(&block, cx, at.section, 0.0, f32::INFINITY);
                if page.lines.len() > lines_before
                    && last > 0
                    && line_height + tall <= height
                    && let Some((line, _)) = lines.last()
                {
                    page.lines.truncate(last);
                    let start = Locator::new(at.section, block.source_offset(line.range.start));
                    return ended(page, start);
                }
                page.lines.extend(placed);
                page.end = Locator::new(at.section, block.end);
                return Ok(page);
            }
            page.lines.extend(placed);
            y = below;
        }
        // A heading needs at least one line of what follows on its page.
        let heading_start = Locator::new(at.section, block.source_offset(from));
        if heading > 0 && had_content && y + body_line > height {
            page.lines.truncate(lines_before);
            return ended(page, heading_start);
        }
        if heading > 0 {
            heading_before = Some((lines_before, heading_start, had_content));
        }
        at = Locator::new(at.section, block.end);
    }
    page.end = at;
    Ok(page)
}

/// A block's lines: `(ascent, line height)`, the reader's (`reader`, as
/// the typesetter says) or, where `style` follows it, the book's
/// distance between lines in percent of the block's text size (`em`),
/// kept between its size and three times it, the text centred in it.
fn block_line(block: &Block, style: &Style, reader: (f32, f32), em: f32) -> (f32, f32) {
    let (ascent, line_height) = (reader.0, reader.1.max(MIN_LINE_HEIGHT));
    match block
        .line_height
        .filter(|_| style.book.has(BookStyles::LINE_HEIGHT))
    {
        Some(percent) => {
            let book = (em * f32::from(percent.clamp(100, 300)) / 100.0).max(MIN_LINE_HEIGHT);
            (ascent + (book - line_height) / 2.0, book)
        }
        None => (ascent, line_height),
    }
}

/// The lines of the blocks inserted after `block` ([`Block::inserted`]),
/// set from `y` down on a page `height` high, as many as fit; where they
/// end, and whether all fit. They take the paragraph spacing before
/// each, their own face, size and alignment, and no places.
fn inserted_lines(
    block: &Block,
    cx: &LayoutContext<'_>,
    section: u32,
    mut y: f32,
    height: f32,
) -> (Vec<PageLine>, f32, bool) {
    let (style, typesetter) = (cx.style, cx.typesetter);
    let mut placed = Vec::new();
    for inserted in &block.inserted {
        if inserted.kind == BlockKind::Image || inserted.text.is_empty() {
            continue;
        }
        let (heading, base) = block_face(inserted, style);
        let em = block_em(inserted, style);
        let (ascent, line_height) = block_line(inserted, style, typesetter.line(base), em);
        let leading = block_leading(inserted, em);
        let (ascent, line_height) = (ascent + leading, line_height + leading);
        let align = block_align(inserted, style, heading);
        y += style.paragraph_spacing;
        let indent = first_indent(inserted, style, align);
        for (line, (first_x, available)) in block_lines(inserted, 0, base, cx, heading, indent) {
            if y + line_height > height {
                return (placed, y, false);
            }
            let free = (available - line.width).max(0.0);
            let x = match align {
                Align::Center => first_x + free / 2.0,
                Align::End => first_x + free,
                _ => first_x,
            };
            let mut text = String::from(&inserted.text[line.range.clone()]);
            if line.hyphen {
                text.push('-');
            }
            let runs = line_runs(
                inserted,
                base,
                line.range.clone(),
                usize::from(line.hyphen),
                style.book,
            );
            placed.push(PageLine {
                x,
                baseline: y + ascent,
                text,
                word_spacing: 0.0,
                runs,
                section,
                words: Vec::new(),
                leading,
                inserted: true,
                annotations: Vec::new(),
            });
            y += line_height;
        }
    }
    (placed, y, true)
}

/// The room above each of `block`'s lines that a plugin asked for
/// ([`Block::leading_above`]), in pixels for its text size `em`.
fn block_leading(block: &Block, em: f32) -> f32 {
    let hundredths = block.leading_above.unwrap_or(0).min(MAX_LEADING_ABOVE);
    em * f32::from(hundredths) / 100.0
}

/// `page`, ending at `end`. A page that ends where it starts, or before,
/// would be shown again and again.
fn ended(mut page: Page, end: Locator) -> Result<Page, DocumentError> {
    if end <= page.start {
        return Err(malformed());
    }
    page.end = end;
    Ok(page)
}

fn malformed() -> DocumentError {
    DocumentError::Malformed(Cow::Borrowed("a block that does not advance"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use paperpaw_document::{MAX_INSERTED, MAX_INSERTED_TEXT, Metadata, OffsetMap, Run};

    /// Paragraphs separated by single newlines; offsets are bytes. Lines
    /// starting with `#` are headings; `*word*` is bold.
    struct Text {
        text: &'static str,
        metadata: Metadata,
    }

    impl Text {
        fn new(text: &'static str) -> Self {
            Self {
                text,
                metadata: Metadata::default(),
            }
        }
    }

    impl Document for Text {
        fn metadata(&self) -> &Metadata {
            &self.metadata
        }

        fn sections(&self) -> u32 {
            1
        }

        fn section_len(&self, _: u32) -> u32 {
            self.text.len() as u32
        }

        fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
            let offset = at.offset as usize;
            if offset >= self.text.len() {
                return Ok(None);
            }
            let start = self.text[..offset].rfind('\n').map_or(0, |i| i + 1);
            let end = self.text[offset..]
                .find('\n')
                .map_or(self.text.len(), |i| offset + i + 1);
            let line = self.text[start..end].trim_end_matches('\n');
            let runs = line
                .match_indices('*')
                .collect::<Vec<_>>()
                .chunks(2)
                .filter(|pair| pair.len() == 2)
                .map(|pair| Run {
                    start: pair[0].0 as u32,
                    end: pair[1].0 as u32 + 1,
                    style: TextStyle::BOLD,
                })
                .collect();
            let kind = if line.starts_with('#') {
                BlockKind::Heading(1)
            } else {
                BlockKind::Paragraph
            };
            let align = if line.starts_with('>') {
                Align::Center
            } else if line.starts_with('=') {
                Align::Justify
            } else {
                Align::Auto
            };
            let spaced = line.starts_with('~');
            Ok(Some(
                Block::new(kind, start as u32, end as u32, String::from(line))
                    .runs(runs)
                    .align(align)
                    .indent(spaced.then_some(0))
                    .space(spaced.then_some(200))
                    .size(spaced.then_some(150)),
            ))
        }
    }

    /// Body text: 1 unit per character (bold 2), lines of 10 with ascent 8;
    /// headings: 2 units, lines of 20.
    struct Units;

    impl Typesetter for Units {
        fn advance(&self, face: Face, _: char) -> f32 {
            if face.heading > 0 || face.style.bold() {
                2.0
            } else {
                1.0
            }
        }

        fn line(&self, face: Face) -> (f32, f32) {
            if face.heading > 0 {
                (16.0, 20.0)
            } else {
                (8.0, 10.0)
            }
        }
    }

    const STYLE: Style = Style {
        paragraph_spacing: 0.0,
        indent: 0.0,
        em: 10.0,
        justify: false,
        widows: 1,
        orphans: 1,
        book: BookStyles::ALL,
        balanced: false,
    };

    #[test]
    fn widows_and_orphans_move_lines_on() {
        // A line, then a paragraph of three; pages of three lines.
        let doc = Text::new("aaaa bbbb\ncccc dddd eeee ffff gggg hhhh\n");
        let page = |style: &Style| {
            let first = layout(&doc, 0, 30.0, style);
            let next = layout(&doc, first.end.offset, 30.0, style);
            (texts(&first).len(), texts(&next).len())
        };
        // As they fall: the paragraph's last line alone on the next page.
        assert_eq!(page(&STYLE), (3, 1));
        // Two at the top: a line more goes on.
        assert_eq!(page(&STYLE.lines_kept(2, 1)), (2, 2));
        // And two at the bottom: the paragraph goes whole.
        assert_eq!(page(&STYLE.lines_kept(2, 2)), (1, 3));
        // A paragraph that starts the page stays, whatever it leaves.
        let long = Text::new("aaaa bbbb cccc dddd eeee ffff gggg hhhh\n");
        let first = layout(&long, 0, 30.0, &STYLE.lines_kept(3, 3));
        assert_eq!(texts(&first).len(), 3);
    }

    #[test]
    fn a_heading_goes_on_with_the_paragraph_after_it() {
        // A line, a heading, a paragraph of two; pages 50 high.
        let doc = Text::new(
            "aaaa bbbb
#Hd
cccc dddd eeee ffff
",
        );
        let page = |style: &Style| {
            let first = layout(&doc, 0, 50.0, style);
            let next = layout(&doc, first.end.offset, 50.0, style);
            (texts(&first).len(), texts(&next).len())
        };
        // As they fall: the paragraph's last line on the next page.
        assert_eq!(page(&STYLE), (3, 1));
        // The paragraph goes whole, and its heading with it.
        assert_eq!(page(&STYLE.lines_kept(2, 1)), (1, 3));
        assert_eq!(page(&STYLE.lines_kept(2, 2)), (1, 3));
        // Under a heading that starts the page, it stays as it falls.
        let top = Text::new(
            "#Hd
cccc dddd eeee ffff
",
        );
        let first = layout(&top, 0, 35.0, &STYLE.lines_kept(2, 2));
        assert_eq!(texts(&first).len(), 2);
    }

    fn texts(page: &Page) -> Vec<&str> {
        page.lines.iter().map(|l| l.text.as_str()).collect()
    }

    fn layout(doc: &Text, start: u32, height: f32, style: &Style) -> Page {
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext {
            document: doc,
            storage: &storage,
            width: 10.0,
            style,
            typesetter: &Units,
            hyphenate: None,
        };
        layout_page(&cx, Locator::new(0, start), height).unwrap()
    }

    #[test]
    fn fills_pages_and_continues_mid_paragraph() {
        let doc = Text::new("aaaa bbbb cccc dddd\nee ff\n");
        let first = layout(&doc, 0, 30.0, &STYLE);
        assert_eq!(texts(&first), vec!["aaaa bbbb", "cccc dddd", "ee ff"]);
        assert_eq!(first.end, Locator::new(0, 26));

        let small = layout(&doc, 0, 10.0, &STYLE);
        assert_eq!(texts(&small), vec!["aaaa bbbb"]);
        assert_eq!(small.end.offset, 10);
        let next = layout(&doc, small.end.offset, 10.0, &STYLE);
        assert_eq!(texts(&next), vec!["cccc dddd"]);
        assert_eq!(next.lines[0].baseline, 8.0);
    }

    #[test]
    fn positions_inside_a_line_go_to_its_start() {
        let doc = Text::new(
            "aaaa bbbb cccc dddd
ee ff
",
        );
        let storage = paperpaw_document::NoStorage;
        let start = |offset, width| {
            let cx = LayoutContext {
                document: &doc,
                storage: &storage,
                width,
                style: &STYLE,
                typesetter: &Units,
                hyphenate: None,
            };
            line_start(&cx, Locator::new(0, offset)).unwrap().offset
        };
        // Lines of 10 units: "aaaa bbbb", "cccc dddd".
        assert_eq!(start(12, 10.0), 10, "inside \"cccc\"");
        assert_eq!(start(10, 10.0), 10, "a line's start stays");
        assert_eq!(start(3, 10.0), 0);
        // Wider lines (e.g. turned): "aaaa bbbb cccc", "dddd".
        assert_eq!(start(12, 15.0), 0);
        assert_eq!(start(16, 15.0), 15);
        assert_eq!(start(20, 15.0), 20, "another paragraph's start");
        assert_eq!(start(22, 15.0), 20);
    }

    #[test]
    fn indents_and_justifies() {
        let doc = Text::new("aa bb cc dd ee\n");
        let style = Style {
            indent: 3.0,
            em: 10.0,
            justify: true,
            ..STYLE
        };
        let page = layout(&doc, 0, 100.0, &style);
        assert_eq!(texts(&page), vec!["aa bb", "cc dd ee"]);
        assert_eq!(page.lines[0].x, 3.0);
        // 7 px available, 5 used, one space: +2 per space.
        assert_eq!(page.lines[0].word_spacing, 2.0);
        assert_eq!(
            page.lines[1].word_spacing, 0.0,
            "last line is not justified"
        );
    }

    #[test]
    fn the_books_indents_and_spaces_win() {
        let doc = Text::new(
            "aa
~bb
cc
",
        );
        let style = Style {
            indent: 3.0,
            em: 10.0,
            ..STYLE
        };
        let page = layout(&doc, 0, 100.0, &style);
        assert_eq!(texts(&page), vec!["aa", "~bb", "cc"]);
        assert_eq!(page.lines[0].x, 3.0);
        assert_eq!(page.lines[1].x, 0.0, "the book's indent");
        assert_eq!(page.lines[2].x, 3.0);
        // 2 em above "~bb", which is set at 150 % (15 px ems); the reader's
        // spacing elsewhere.
        assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 40.0);
        assert_eq!(page.lines[1].runs[0].face.size, 150);
        assert_eq!(page.lines[0].runs[0].face.size, 0);
        assert_eq!(page.lines[2].baseline - page.lines[1].baseline, 10.0);
        // At the top of a chapter's first page the space stays.
        let doc = Text::new(
            "~aa
",
        );
        let page = layout(&doc, 0, 100.0, &style);
        assert_eq!(page.lines[0].baseline, 38.0);
    }

    #[test]
    fn the_readers_choice_wins_over_justified_books() {
        // "=" marks a block the book justifies.
        let doc = Text::new(
            "=a bb cc dd ee
",
        );
        let page = layout(&doc, 0, 100.0, &STYLE);
        assert_eq!(texts(&page), vec!["=a bb cc", "dd ee"]);
        assert_eq!(page.lines[0].word_spacing, 0.0);
        let style = Style {
            justify: true,
            ..STYLE
        };
        let page = layout(&doc, 0, 100.0, &style);
        assert_eq!(page.lines[0].word_spacing, 1.0);
    }

    #[test]
    fn words_know_where_they_come_from() {
        // Second line of the second block: "cc dd" at 3..8 of "aa bb cc dd".
        let doc = Text::new(
            "x
aa bb cc dd
",
        );
        let page = layout(&doc, 0, 100.0, &STYLE);
        assert_eq!(texts(&page), vec!["x", "aa bb cc", "dd"]);
        assert_eq!(page.lines[1].words, [(2, 4), (5, 7), (8, 10)]);
        assert_eq!(page.lines[2].words, [(11, 13)]);
        assert_eq!(page.lines[2].section, 0);
    }

    #[test]
    fn past_the_end_is_empty() {
        let doc = Text::new("x\n");
        let page = layout(&doc, 2, 100.0, &STYLE);
        assert!(page.is_past_end());
        assert_eq!(page.end, page.start);
    }

    #[test]
    fn paragraph_spacing_between_blocks() {
        let doc = Text::new("a\nb\n");
        let style = Style {
            paragraph_spacing: 5.0,
            ..STYLE
        };
        let page = layout(&doc, 0, 100.0, &style);
        assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 15.0);
    }

    #[test]
    fn styled_runs_are_measured_and_reported() {
        // "*bb*" is bold: 4 characters of 2 units, so "a *bb*" just fits.
        let doc = Text::new("a *bb* c\n");
        let page = layout(&doc, 0, 100.0, &STYLE);
        assert_eq!(texts(&page), vec!["a *bb*", "c"]);
        let faces: Vec<(usize, usize, bool)> = page.lines[0]
            .runs
            .iter()
            .map(|r| (r.start, r.end, r.face.style.bold()))
            .collect();
        assert_eq!(faces, vec![(0, 2, false), (2, 6, true)]);
    }

    #[test]
    fn headings_use_their_face_and_centred_blocks_are_centred() {
        let doc = Text::new("#ab\n>xy\n");
        let style = Style {
            indent: 3.0,
            em: 10.0,
            justify: true,
            ..STYLE
        };
        let page = layout(&doc, 0, 100.0, &style);
        // Heading: 3 characters of 2 units, line box of 20 with ascent 16.
        assert_eq!(page.lines[0].baseline, 16.0);
        assert_eq!(page.lines[0].x, 0.0, "headings are not indented");
        assert_eq!(page.lines[0].runs[0].face.heading, 1);
        // ">xy" is 3 units in a 10-unit line, centred and not indented.
        assert_eq!(page.lines[1].x, 3.5);
        assert_eq!(page.lines[1].baseline, 20.0 + 8.0);
    }

    /// A document whose blocks come from a function, for documents that
    /// break the rules. Fails the test when laid out without end.
    struct Blocks<F> {
        block_at: F,
        calls: core::cell::Cell<u32>,
        metadata: Metadata,
    }

    impl<F: Fn(Locator) -> Option<Block>> Blocks<F> {
        fn new(block_at: F) -> Self {
            Self {
                block_at,
                calls: core::cell::Cell::new(0),
                metadata: Metadata::default(),
            }
        }
    }

    impl<F: Fn(Locator) -> Option<Block>> Document for Blocks<F> {
        fn metadata(&self) -> &Metadata {
            &self.metadata
        }

        fn sections(&self) -> u32 {
            1
        }

        fn section_len(&self, _: u32) -> u32 {
            u32::MAX
        }

        fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
            self.calls.set(self.calls.get() + 1);
            assert!(self.calls.get() < 10_000, "the layout does not end");
            Ok((self.block_at)(at))
        }
    }

    fn block(text: &str, start: u32, end: u32) -> Block {
        Block::new(BlockKind::Paragraph, start, end, String::from(text))
            .map(OffsetMap::IDENTITY)
            .align(Align::Auto)
    }

    /// Lines without height, as from a broken font.
    struct Flat;

    impl Typesetter for Flat {
        fn advance(&self, _: Face, _: char) -> f32 {
            1.0
        }

        fn line(&self, _: Face) -> (f32, f32) {
            (0.0, 0.0)
        }
    }

    fn layout_with(
        doc: &dyn Document,
        typesetter: &dyn Typesetter,
        start: Locator,
        height: f32,
    ) -> Result<Page, DocumentError> {
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext {
            document: doc,
            storage: &storage,
            width: 10.0,
            style: &STYLE,
            typesetter,
            hyphenate: None,
        };
        layout_page(&cx, start, height)
    }

    #[test]
    fn blocks_that_do_not_go_on_are_malformed() {
        // Each block ends where it was asked for, or before.
        for back in [0, 1] {
            let doc = Blocks::new(|at: Locator| Some(block("", 0, at.offset.saturating_sub(back))));
            for typesetter in [&Units as &dyn Typesetter, &Flat] {
                let page = layout_with(&doc, typesetter, Locator::new(0, 1), 100.0);
                assert!(matches!(page, Err(DocumentError::Malformed(_))));
            }
        }
        // A page with lines that ends where it starts would come again and
        // again.
        let mut map = OffsetMap::default();
        map.checkpoint(10, 0);
        let doc = Blocks::new(move |_| Some(block("aaaa bbbb cccc", 100, 200).map(map.clone())));
        let page = layout_with(&doc, &Units, Locator::new(0, 100), 10.0);
        assert!(matches!(page, Err(DocumentError::Malformed(_))));
    }

    #[test]
    fn pictures_without_size_are_left_out() {
        let doc = Blocks::new(|at: Locator| match at.offset {
            0..10 => {
                let mut picture = block("", 0, 10).image(ImageRef {
                    path: String::from("a.gray"),
                    width: 0,
                    height: 0,
                });
                picture.kind = BlockKind::Image;
                Some(picture)
            }
            10..20 => Some(block("aa", 10, 20)),
            _ => None,
        });
        let page = layout_with(&doc, &Units, Locator::START, 100.0).unwrap();
        assert!(page.images.is_empty());
        assert_eq!(texts(&page), vec!["aa"]);
        assert_eq!(page.lines[0].baseline, 8.0);
    }

    #[test]
    fn lines_without_height_still_fill_pages() {
        let doc = Blocks::new(|at: Locator| {
            (at.offset < 5000).then(|| block("a", at.offset, at.offset + 1))
        });
        let page = layout_with(&doc, &Flat, Locator::START, 100.0).unwrap();
        assert!(page.lines.len() <= 100, "{} lines", page.lines.len());
        assert!(page.end > page.start);
    }

    #[test]
    fn the_last_section_number_does_not_overflow() {
        let doc = Text::new("x\n");
        let page = layout_with(&doc, &Units, Locator::new(u32::MAX, 0), 100.0).unwrap();
        assert_eq!(texts(&page), vec!["x"]);
    }

    #[test]
    fn headings_are_not_left_alone_at_the_bottom() {
        let doc = Text::new("aa\n#h\nbb\n");
        // Room for "aa" (10) and the heading (20), but not for "bb".
        let page = layout(&doc, 0, 35.0, &STYLE);
        assert_eq!(texts(&page), vec!["aa"]);
        assert_eq!(page.end, Locator::new(0, 3));
        let next = layout(&doc, page.end.offset, 35.0, &STYLE);
        assert_eq!(texts(&next), vec!["#h", "bb"]);
    }

    /// One paragraph of 4-letter words, laid out 40 wide in `style`.
    fn styled_page(block: Block, style: &Style) -> (Page, Locator) {
        let doc = Blocks::new(move |at: Locator| (at.offset < 79).then(|| block.clone()));
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext::new(&doc, &storage, 40.0, style, &Units);
        let page = layout_page(&cx, Locator::new(0, 0), 100.0).unwrap();
        // Where the second line starts, as found from inside it.
        let second = Locator::new(0, page.lines[1].words[0].0);
        let inside = Locator::new(0, second.offset + 2);
        assert_eq!(
            line_start(&cx, inside).unwrap(),
            second,
            "line starts agree"
        );
        (page, second)
    }

    fn words() -> Block {
        let text =
            "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp";
        block(text, 0, 79)
    }

    #[test]
    fn book_margins_narrow_the_lines_within_a_quarter() {
        // 2 and 9 ems at 10 px: 20 px kept to a quarter of 40, 10 px.
        let book = words().margins(200, 900);
        let (page, _) = styled_page(book.clone(), &STYLE);
        for line in &page.lines {
            assert_eq!(line.x, 10.0);
            assert!(line.text.len() <= 20, "{:?}", line.text);
        }
        // Left to the reader: the whole width.
        let reader = STYLE.book(BookStyles::ALL.without(BookStyles::MARGINS));
        let (page, _) = styled_page(book, &reader);
        assert!(page.lines.iter().all(|l| l.x == 0.0));
        assert_eq!(
            page.lines[0].text,
            "aaaa bbbb cccc dddd eeee ffff gggg hhhh"
        );
    }

    #[test]
    fn a_negative_indent_hangs_into_the_margin() {
        let hanging = words().margins(100, 0).indent(-100);
        let (page, _) = styled_page(hanging.clone(), &STYLE);
        assert_eq!(page.lines[0].x, 0.0);
        assert!(page.lines[1..].iter().all(|l| l.x == 10.0));
        // Without a margin it cannot hang out of the page.
        let (page, _) = styled_page(words().indent(-100), &STYLE);
        assert!(page.lines.iter().all(|l| l.x == 0.0));
        // The book's indent left to the reader: the margin stays.
        let reader = STYLE.book(BookStyles::ALL.without(BookStyles::INDENT));
        let (page, _) = styled_page(hanging, &reader);
        assert!(page.lines.iter().all(|l| l.x == 10.0));
    }

    #[test]
    fn the_books_line_height_only_where_followed() {
        let tall = words().line_height(200);
        let (page, _) = styled_page(tall.clone(), &STYLE);
        assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 20.0);
        let reader = STYLE.book(BookStyles::ALL.without(BookStyles::LINE_HEIGHT));
        let (page, _) = styled_page(tall, &reader);
        assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 10.0);
    }

    #[test]
    fn room_above_the_lines_moves_them_down_and_breaks_the_page_sooner() {
        let (plain, _) = styled_page(words(), &STYLE);
        // Half an em at 10 px: 5 px above every line, part of its height,
        // whether the reader follows the book or not.
        for style in [STYLE, STYLE.book(BookStyles::NONE)] {
            let (spaced, _) = styled_page(words().leading_above(50), &style);
            assert_eq!(spaced.lines[0].baseline - plain.lines[0].baseline, 5.0);
            assert_eq!(spaced.lines[1].baseline - spaced.lines[0].baseline, 15.0);
            assert!(spaced.lines.iter().all(|l| l.leading == 5.0));
            assert_eq!(spaced.lines[0].text, plain.lines[0].text);
        }
        assert!(plain.lines.iter().all(|l| l.leading == 0.0));
        // Four ems at most: on a page 60 px high, a line of 50 px leaves
        // no room for the second, which comes on the next page.
        let page = |block: Block| {
            let doc = Blocks::new(move |at: Locator| (at.offset < 79).then(|| block.clone()));
            let storage = paperpaw_document::NoStorage;
            let cx = LayoutContext::new(&doc, &storage, 40.0, &STYLE, &Units);
            layout_page(&cx, Locator::new(0, 0), 60.0).unwrap()
        };
        let (plain, most) = (page(words()), page(words().leading_above(5000)));
        assert_eq!(plain.lines.len(), 2);
        assert_eq!(most.lines.len(), 1);
        assert_eq!(most.lines[0].leading, 40.0);
        assert!(most.end < plain.end, "the page breaks sooner");
        assert_eq!(most.end.offset, plain.lines[1].words[0].0);
    }

    /// A page `height` high from `start`, of the one block `block`.
    fn page_of(block: Block, start: u32, height: f32) -> Page {
        let end = block.end;
        let doc = Blocks::new(move |at: Locator| (at.offset < end).then(|| block.clone()));
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext::new(&doc, &storage, 40.0, &STYLE, &Units);
        layout_page(&cx, Locator::new(0, start), height).unwrap()
    }

    /// `words()` with a filter's translation inserted after it.
    fn translated() -> Block {
        let mut block = words();
        assert!(block.insert_after(block_of("xxxx yyyy")));
        block
    }

    fn block_of(text: &str) -> Block {
        Block::new(
            BlockKind::Paragraph,
            0,
            text.len() as u32,
            String::from(text),
        )
    }

    #[test]
    fn blocks_a_filter_inserts_follow_the_block_without_places() {
        let page = page_of(translated(), 0, 100.0);
        let (anchor, inserted): (Vec<_>, Vec<_>) = page.lines.iter().partition(|l| !l.inserted);
        assert_eq!(anchor.len(), 2);
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].text, "xxxx yyyy");
        assert!(inserted[0].words.is_empty(), "no places of its own");
        assert!(inserted[0].baseline > anchor[1].baseline);
        // The page ends where the book's block does.
        assert_eq!(page.end, Locator::new(0, 79));
        // Bounded: no more than four, nor more than 4 KiB of text.
        let mut block = words();
        for _ in 0..MAX_INSERTED {
            assert!(block.insert_after(block_of("z")));
        }
        assert!(!block.insert_after(block_of("z")));
        let mut big = words();
        assert!(!big.insert_after(block_of(&"z".repeat(MAX_INSERTED_TEXT + 1))));
    }

    #[test]
    fn the_last_line_of_a_block_goes_with_what_was_inserted_after_it() {
        // Two lines of 10 px fit in 25, the translation's third does not:
        // the second line goes on with it.
        let first = page_of(translated(), 0, 25.0);
        assert_eq!(first.lines.len(), 1);
        assert!(!first.lines[0].inserted);
        let second_start = page_of(translated(), 0, 100.0).lines[1].words[0].0;
        assert_eq!(first.end, Locator::new(0, second_start));
        let next = page_of(translated(), first.end.offset, 25.0);
        assert_eq!(next.lines.len(), 2);
        assert!(!next.lines[0].inserted && next.lines[1].inserted);
        assert_eq!(next.end, Locator::new(0, 79));
        // A block's only line at the top of a page with no room for what
        // follows it: what fits stays, the rest goes, the page goes on.
        let mut short = block_of("aaaa");
        short.end = 4;
        assert!(short.insert_after(block_of("xxxx yyyy")));
        let page = page_of(short, 0, 15.0);
        assert_eq!(page.lines.len(), 1);
        assert_eq!(page.end, Locator::new(0, 4));
        // Taller than any page: no line moves on (a page of nothing but
        // the last line would not show it whole either); what fits stays.
        let mut long = words();
        assert!(long.insert_after(block_of(&"xxxx ".repeat(100))));
        let page = page_of(long, 0, 50.0);
        assert_eq!(page.lines.iter().filter(|l| !l.inserted).count(), 2);
        assert_eq!(page.lines.len(), 5);
        assert_eq!(page.end, Locator::new(0, 79));
    }

    #[test]
    fn the_books_alignment_size_and_fonts_can_be_left_to_the_reader() {
        let centred = words().align(Align::Center).size(200).runs(vec![Run {
            start: 0,
            end: 4,
            style: TextStyle::REGULAR.with_family(3),
        }]);
        let (page, _) = styled_page(centred.clone(), &STYLE);
        assert!(page.lines[0].x > 0.0);
        assert_eq!(page.lines[0].runs[0].face.size, 200);
        assert_eq!(page.lines[0].runs[0].face.style.family(), 3);
        let (page, _) = styled_page(centred, &STYLE.book(BookStyles::NONE));
        assert_eq!(page.lines[0].x, 0.0);
        assert_eq!(page.lines[0].runs[0].face.size, 0);
        assert_eq!(page.lines[0].runs[0].face.style.family(), 0);
    }
}
