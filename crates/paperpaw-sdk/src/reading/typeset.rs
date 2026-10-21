//! The type of book pages as the settings make it: the page's box and
//! spacing ([`PageType`]), the reading faces with the book's own fonts and
//! small capitals ([`ReadingFaces`], a layout
//! [`Typesetter`](crate::layout::Typesetter)), the boxes of a page's words
//! ([`word_boxes`]) and the page drawn ([`draw_lines`], [`draw_image`]).
//! A reader of one's own lays pages out and draws them as the shipped one
//! does; what it puts around them (status lines) is its own.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{fonts, settings};
use crate::document::{Document, Locator};
use crate::layout::{Face, Page, PageImage, PageLine, Style, Typesetter};
use crate::{Canvas, Color, Env, FontHandle, FontSet, Id, Rect};

/// Relative sizes of headings 1 to 6.
const HEADING_SCALE: [f32; 6] = [1.6, 1.4, 1.2, 1.1, 1.0, 1.0];

/// Glyphs of a book's embedded fonts kept at most.
const EMBEDDED_CACHE: usize = 256 * 1024;

/// Largest total size of a book's embedded fonts loaded.
const EMBEDDED_LIMIT: usize = 8 * 1024 * 1024;

/// Small capitals are this much of the text size.
const SMALL_CAPS: f32 = 0.78;

/// Whether a book's own fonts are used: the font chosen is the book's
/// (the default choice), and they render on this device (not on small
/// devices: they take hundreds of kilobytes).
pub fn uses_book_fonts(env: &Env<'_>) -> bool {
    crate::text::OUTLINES
        && env.device.memory != crate::MemoryClass::Small
        && env.settings.read(&settings::READING_FONT) == settings::DEFAULT_FONT
}

/// The fonts a book embeds, loaded with the book.
pub struct EmbeddedFonts {
    set: FontSet,
    /// Family, bold, italic and index in `set` of each font.
    faces: Vec<(u8, bool, bool, usize)>,
}

impl core::fmt::Debug for EmbeddedFonts {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EmbeddedFonts")
            .field("faces", &self.faces.len())
            .finish_non_exhaustive()
    }
}

impl EmbeddedFonts {
    /// The book's fonts, if they are used ([`uses_book_fonts`]) and it
    /// has any that load.
    pub fn load(env: &Env<'_>, document: &dyn Document) -> Option<EmbeddedFonts> {
        if !uses_book_fonts(env) {
            return None;
        }
        let fonts = document.fonts(env.storage);
        if fonts.is_empty() {
            return None;
        }
        let mut set = FontSet::new(EMBEDDED_CACHE);
        let mut faces = Vec::new();
        let mut total = 0;
        for (index, font) in fonts.iter().enumerate() {
            let Ok(data) = document.font_data(env.storage, index) else {
                continue;
            };
            total += data.len();
            if total > EMBEDDED_LIMIT {
                break;
            }
            let Ok(loaded) = crate::Font::from_data(data, &[]) else {
                continue;
            };
            let id = Id::parse(&format!("core.book-font-{index}")).ok()?;
            let at = set.add(id, loaded);
            faces.push((font.family, font.bold, font.italic, at));
        }
        (!faces.is_empty()).then_some(EmbeddedFonts { set, faces })
    }

    /// The face of `family` closest to `bold` and `italic`.
    pub fn face(&self, family: u8, bold: bool, italic: bool) -> Option<FontHandle<'_>> {
        let of_family = || self.faces.iter().filter(|f| f.0 == family);
        let exact = of_family().find(|f| f.1 == bold && f.2 == italic);
        let regular = || of_family().find(|f| !f.1 && !f.2);
        exact
            .or_else(regular)
            .or_else(|| of_family().next())
            .and_then(|f| self.set.get(f.3))
    }
}

/// Room for status lines above and below a book's text: where they are
/// shown, how high they are and how far from the edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct StatusRoom {
    pub top: bool,
    pub bottom: bool,
    /// A line's height, in pixels.
    pub height: f32,
    /// From the screen's edge, in pixels.
    pub edge: f32,
}

impl StatusRoom {
    /// No status lines.
    pub const NONE: StatusRoom = StatusRoom {
        top: false,
        bottom: false,
        height: 0.0,
        edge: 0.0,
    };

    /// Lines `height` high, `edge` from the screen's edges, at the top and
    /// the bottom as `top` and `bottom` say.
    pub fn new(top: bool, bottom: bool, height: f32, edge: f32) -> Self {
        Self {
            top,
            bottom,
            height,
            edge,
        }
    }
}

/// The page's box and the type of its text, from the standard settings
/// (see [`settings::FONT_SIZE`] and those after it) and the screen.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PageType {
    /// The text's size, in pixels.
    pub size: f32,
    /// Font weight steps (0: as the font is).
    pub weight: u8,
    /// Spaces relative to the font's.
    pub word_spacing: f32,
    /// Body text lines: baseline from the top of a line, line height.
    pub ascent: f32,
    pub line_height: f32,
    pub style: Style,
    /// The content box.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The status lines above and below the content, where there are any.
    pub top_status: Option<Rect>,
    pub status: Option<Rect>,
    /// The language whose hyphenation patterns are used, if hyphenating.
    pub hyphenation: Option<String>,
    /// What the plugins' text filters did when it was made
    /// ([`TextFilters::signature`](crate::filters::TextFilters)).
    pub filters: String,
    /// Names this layout: what pages laid out with it look like depends
    /// on nothing else (the settings, the faces, the box, the filters, the
    /// plugins' stylesheets, the layout's version). A page index of the
    /// book has it in its name.
    pub key: String,
}

impl PageType {
    /// The type of a book in `language` read in `font` (the reading font),
    /// with `status` for the status lines; a book of `whole_pages` (comics)
    /// is shown edge to edge, without margins or status lines.
    pub fn new(
        env: &Env<'_>,
        font: &FontHandle<'_>,
        language: &str,
        whole_pages: bool,
        status: StatusRoom,
    ) -> Self {
        let s = env.settings;
        let m = env.metrics;
        let size = m.pt(s.read(&settings::FONT_SIZE) as f32);
        let weight = s.read(&settings::FONT_WEIGHT).clamp(0, 4) as u8;
        let word_spacing = s.read(&settings::WORD_SPACING).clamp(50, 200) as f32 / 100.0;
        let metrics = font.line_metrics(size);
        let natural = metrics.ascent + metrics.descent;
        // At least the font's own distance; below 100 % that much of it.
        let spacing = s.read(&settings::LINE_SPACING).clamp(80, 250) as f32 / 100.0;
        let line_height = (size * spacing).max(natural * spacing.min(1.0));
        let indent = s.read(&settings::INDENT);
        let spaced = s.read(&settings::PARAGRAPH_SPACING);
        let justify = s.read(&settings::JUSTIFY);
        let ascent = metrics.ascent + (line_height - natural) / 2.0;
        let widows = s.read(&settings::WIDOWS).clamp(1, 4) as u8;
        let orphans = s.read(&settings::ORPHANS).clamp(1, 4) as u8;
        let style = Style::new(
            if spaced { line_height * 0.6 } else { 0.0 },
            if indent { size * 1.5 } else { 0.0 },
            size,
            justify,
        )
        .lines_kept(widows, orphans);

        let display = env.device.display;
        let millimetres = |setting: &crate::setting::Setting<i64>| match whole_pages {
            true => 0.0,
            false => m.mm(s.read(setting).max(0) as u32) as f32,
        };
        // At the sides, and at the top and bottom.
        let (margin, ends) = (
            millimetres(&settings::MARGIN),
            millimetres(&settings::MARGIN_ENDS),
        );
        let status = if whole_pages {
            StatusRoom::NONE
        } else {
            status
        };
        let width = (f32::from(display.width) - 2.0 * margin).max(1.0);
        // Status lines sit close to the edge, in the text's margin; the
        // text keeps its margin, and never runs up to a status line, also
        // on pages it fills to the last pixel.
        let status_room = status.height + m.mm(2) as f32;
        let screen = f32::from(display.height);
        let y = if status.top {
            ends.max(status.edge + status_room)
        } else {
            ends
        };
        let end = if status.bottom {
            (screen - ends).min(screen - status.edge - status_room)
        } else {
            screen - ends
        };
        let height = (end - y).max(1.0);
        let line = |y: f32| Rect::new(margin as u16, y as u16, width as u16, status.height as u16);
        let top_status = status.top.then(|| line(status.edge));
        let bottom_status = status
            .bottom
            .then(|| line(screen - status.edge - status.height));
        let hyphenation = (s.read(&settings::HYPHENATE) && env.hyphenation.get(language).is_some())
            .then(|| language.to_string());
        // The plugins' stylesheets change what the book's text looks like.
        let stylesheets = env.stylesheets.iter().fold(0u64, |hash, sheet| {
            hash.rotate_left(5) ^ paperpaw_digest::fnv1a64(sheet.as_bytes())
        });
        let key = format!(
            "{:016x}",
            paperpaw_digest::fnv1a64(format!(
                "{}|{}|{}|{}|{}|{size}|{line_height}|{}|{}|{justify}|{width}x{height}|{}|{}|{}|{word_spacing}|{}|{whole_pages}|{stylesheets:x}|{widows}:{orphans}",
                crate::layout::VERSION,
                font.id(),
                face_id(env, &fonts::READING_BOLD),
                face_id(env, &fonts::READING_ITALIC),
                face_id(env, &fonts::READING_BOLD_ITALIC),
                style.indent,
                style.paragraph_spacing,
                hyphenation.as_deref().unwrap_or("-"),
                s.read(&settings::PUBLISHER_STYLES),
                uses_book_fonts(env),
                env.text_filters.signature(env),
            ))
        );
        Self {
            size,
            weight,
            word_spacing,
            ascent,
            line_height,
            style,
            x: margin,
            y,
            width,
            height,
            top_status,
            status: bottom_status,
            hyphenation,
            filters: env.text_filters.signature(env),
            key,
        }
    }

    /// The type of a sample of text `height` pixels high in `language`
    /// (a preview of the settings): as wide as book pages and with their
    /// margins at the sides, `pad` above and below, without status lines.
    pub fn sample(
        env: &Env<'_>,
        font: &FontHandle<'_>,
        language: &str,
        height: f32,
        pad: f32,
    ) -> Self {
        Self {
            y: pad,
            height: (height - 2.0 * pad).max(1.0),
            ..Self::new(env, font, language, false, StatusRoom::NONE)
        }
    }
}

/// The font bound to an optional reading face, for the layout key.
fn face_id(env: &Env<'_>, role: &Id) -> String {
    env.fonts
        .bound(role.as_str())
        .map_or_else(String::new, |font| font.id().to_string())
}

/// The reading fonts by face, for layout and drawing: the faces bound to
/// the reading roles (a font the user chose for book text is not mixed
/// with the default bold or italic), the book's own where it uses them,
/// headings larger, small capitals.
#[derive(Clone, Copy)]
pub struct ReadingFaces<'a> {
    /// Regular, bold, italic and bold italic.
    fonts: [FontHandle<'a>; 4],
    weight: u8,
    word_spacing: f32,
    embedded: Option<&'a EmbeddedFonts>,
    size: f32,
    ascent: f32,
    line_height: f32,
}

impl core::fmt::Debug for ReadingFaces<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReadingFaces")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl<'a> ReadingFaces<'a> {
    /// The faces for `page`; `None` without a reading font.
    pub fn new(
        env: &Env<'a>,
        page: &PageType,
        embedded: Option<&'a EmbeddedFonts>,
    ) -> Option<Self> {
        let all = env.fonts;
        let regular = all.role(fonts::READING.as_str())?;
        let custom = all.user_bound(fonts::READING.as_str());
        let face = |role: &Id| {
            if custom && !all.user_bound(role.as_str()) {
                None
            } else {
                all.bound(role.as_str())
            }
        };
        let bold = face(&fonts::READING_BOLD).unwrap_or(regular);
        let italic = face(&fonts::READING_ITALIC).unwrap_or(regular);
        let bold_italic = face(&fonts::READING_BOLD_ITALIC).unwrap_or(bold);
        Some(Self {
            fonts: [regular, bold, italic, bold_italic],
            weight: page.weight,
            word_spacing: page.word_spacing,
            embedded,
            size: page.size,
            ascent: page.ascent,
            line_height: page.line_height,
        })
    }

    /// The book's font for text in an embedded family, else the reading
    /// face.
    pub fn font(&self, face: Face) -> FontHandle<'a> {
        let style = face.style;
        if style.family() != 0
            && let Some(font) = self
                .embedded
                .and_then(|e| e.face(style.family(), style.bold(), style.italic()))
        {
            return font;
        }
        self.fonts[usize::from(style.bits() & 3)]
    }

    /// The size of text in `face`, in pixels.
    pub fn size(&self, face: Face) -> f32 {
        match (face.size, face.heading) {
            // The book's size wins.
            (percent, _) if percent > 0 => self.size * f32::from(percent) / 100.0,
            (_, 0) => self.size,
            (_, level) => self.size * HEADING_SCALE[usize::from(level.clamp(1, 6)) - 1],
        }
    }

    /// The width of `text` in `face`, small capitals included.
    pub fn measure(&self, face: Face, text: &str) -> f32 {
        let (font, size) = (self.font(face), self.size(face));
        if !face.style.small_caps() {
            return font.measure(size, text);
        }
        small_caps_parts(text)
            .map(|(part, lower)| match lower {
                true => font.measure(size * SMALL_CAPS, &part.to_uppercase()),
                false => font.measure(size, part),
            })
            .sum()
    }

    /// A space between words in `face`.
    pub fn space(&self, face: Face) -> f32 {
        self.font(face).advance(self.size(face), ' ') * self.word_spacing
    }

    /// Draws `text` in `face`, as heavy as the font weight setting asks;
    /// its width.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> f32 {
        self.draw_in(canvas, face, x, baseline, text, Color::BLACK)
    }

    /// Draws `text` as [`ReadingFaces::draw`] does, in `ink` (e.g. a
    /// mark's).
    pub fn draw_in(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
        ink: Color,
    ) -> f32 {
        let size = self.size(face);
        // About a pixel per step at 48 px; at least one.
        let extra = match self.weight {
            0 => 0,
            w => (f32::from(w) * size / 48.0 + 0.5).max(1.0) as u16,
        };
        let font = self.font(face);
        if !face.style.small_caps() {
            return font.draw_bolder(canvas, size, x, baseline, text, ink, extra);
        }
        // Lower case letters as smaller capitals.
        let mut width = 0.0;
        for (part, lower) in small_caps_parts(text) {
            width += match lower {
                true => {
                    let upper = part.to_uppercase();
                    let small = size * SMALL_CAPS;
                    font.draw_bolder(canvas, small, x + width, baseline, &upper, ink, extra)
                }
                false => font.draw_bolder(canvas, size, x + width, baseline, part, ink, extra),
            };
        }
        width
    }
}

impl Typesetter for ReadingFaces<'_> {
    fn advance(&self, face: Face, c: char) -> f32 {
        if c == ' ' {
            return self.space(face);
        }
        if face.style.small_caps() && c.is_lowercase() {
            let small = self.size(face) * SMALL_CAPS;
            let font = self.font(face);
            return c.to_uppercase().map(|u| font.advance(small, u)).sum();
        }
        self.font(face).advance(self.size(face), c)
    }

    fn line(&self, face: Face) -> (f32, f32) {
        if face.heading == 0 {
            // Body text the book sets larger or smaller keeps the line
            // spacing chosen.
            let scale = match face.size {
                0 => 1.0,
                percent => f32::from(percent) / 100.0,
            };
            return (self.ascent * scale, self.line_height * scale);
        }
        let size = self.size(face);
        let metrics = self.font(face).line_metrics(size);
        let natural = metrics.ascent + metrics.descent;
        let line_height = (size * 1.2).max(natural);
        (metrics.ascent + (line_height - natural) / 2.0, line_height)
    }
}

/// `text` in parts of lower case letters and of the rest, each with
/// whether it is lower case.
pub fn small_caps_parts(text: &str) -> impl Iterator<Item = (&str, bool)> {
    let mut rest = text;
    core::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let lower = first.is_lowercase();
        let end = rest
            .char_indices()
            .find(|(_, c)| c.is_lowercase() != lower)
            .map_or(rest.len(), |(i, _)| i);
        let (part, tail) = rest.split_at(end);
        rest = tail;
        Some((part, lower))
    })
}

/// A word on the page, with its box on the screen and its place in the
/// book.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WordBox {
    pub area: Rect,
    pub text: String,
    pub start: Locator,
    pub end: Locator,
}

impl WordBox {
    pub fn new(area: Rect, text: impl Into<String>, start: Locator, end: Locator) -> Self {
        Self {
            area,
            text: text.into(),
            start,
            end,
        }
    }

    /// The word without the punctuation around it, as it is looked up.
    pub fn lookup_text(&self) -> &str {
        self.text.trim_matches(|c: char| !c.is_alphanumeric())
    }
}

/// The words of `page`, laid out with `faces` in `page_type`, with their
/// boxes on the screen, in reading order (measured as they are drawn: a
/// word that goes on in another face is one word).
pub fn word_boxes(faces: &ReadingFaces<'_>, page_type: &PageType, page: &Page) -> Vec<WordBox> {
    let t = page_type;
    let mut words: Vec<WordBox> = Vec::new();
    for line in &page.lines {
        let mut x = t.x + line.x;
        // Inside a word that continues in the next run (e.g. bold).
        let mut joined = false;
        // The next of the line's words.
        let mut ordinal = 0;
        for run in &line.runs {
            let (ascent, height) = faces.line(run.face);
            let top = t.y + line.baseline - ascent;
            let space = faces.space(run.face) + line.word_spacing;
            for (i, part) in line.text[run.start..run.end].split(' ').enumerate() {
                if i > 0 {
                    x += space;
                    joined = false;
                }
                if part.is_empty() {
                    continue;
                }
                let width = faces.measure(run.face, part);
                match words.last_mut() {
                    Some(last) if joined => {
                        last.text.push_str(part);
                        last.area.width = (x + width) as u16 - last.area.x;
                    }
                    _ => {
                        let (start, end) = line.words.get(ordinal).copied().unwrap_or_default();
                        ordinal += 1;
                        words.push(WordBox::new(
                            Rect::new(x as u16, top as u16, width as u16, height as u16),
                            part,
                            Locator::new(line.section, start),
                            Locator::new(line.section, end),
                        ));
                    }
                }
                x += width;
                joined = true;
            }
        }
    }
    words
}

/// Draws `lines` laid out with `faces` in `page_type`, their content box
/// at `origin`, each word in its ink (`inks`, by the words' order as
/// [`word_boxes`] has them; black where none). Lines outside the part
/// being drawn (another band) are only measured.
pub fn draw_lines(
    canvas: &mut Canvas<'_>,
    faces: &ReadingFaces<'_>,
    page_type: &PageType,
    (left, top): (f32, f32),
    lines: &[PageLine],
    inks: &[Option<Color>],
) {
    let clip = canvas.clip();
    // The words counted as `word_boxes` counts them: a word that goes on
    // in the next run (e.g. bold) is one word.
    let mut ordinal = 0usize;
    for line in lines {
        let baseline = (top + line.baseline + 0.5) as i32;
        let above = baseline - (2.0 * page_type.line_height) as i32;
        let below = baseline + page_type.line_height as i32;
        let shown = below >= i32::from(clip.y) && above <= clip.bottom() as i32;
        let mut x = left + line.x;
        let mut joined = false;
        for run in &line.runs {
            let text = &line.text[run.start..run.end];
            let space = faces.space(run.face) + line.word_spacing;
            for (i, word) in text.split(' ').enumerate() {
                if i > 0 {
                    x += space;
                    joined = false;
                }
                if word.is_empty() {
                    continue;
                }
                if !joined {
                    ordinal += 1;
                }
                joined = true;
                if !shown {
                    x += faces.measure(run.face, word);
                    continue;
                }
                match inks.get(ordinal - 1).copied().flatten() {
                    Some(ink) => x += faces.draw_in(canvas, run.face, x, baseline, word, ink),
                    None => x += faces.draw(canvas, run.face, x, baseline, word),
                }
            }
        }
    }
}

/// Draws a picture of the page at its place in `page_type`'s box.
pub fn draw_image(canvas: &mut Canvas<'_>, env: &Env<'_>, page_type: &PageType, image: &PageImage) {
    let t = page_type;
    let left = (t.x + image.x + 0.5) as i32;
    let top = (t.y + image.y + 0.5) as i32;
    let (w, h) = ((image.width + 0.5) as i32, (image.height + 0.5) as i32);
    crate::ui::draw_image(
        canvas,
        env.storage,
        &image.image,
        Rect::new(left as u16, top as u16, w as u16, h as u16),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn small_caps_split_lower_case() {
        let parts: Vec<(&str, bool)> = small_caps_parts("Mr. Gregor Samsa").collect();
        assert_eq!(
            parts,
            [
                ("M", false),
                ("r", true),
                (". G", false),
                ("regor", true),
                (" S", false),
                ("amsa", true)
            ]
        );
        assert_eq!(small_caps_parts("").count(), 0);
        assert_eq!(
            small_caps_parts("straße").collect::<Vec<_>>(),
            [("straße", true)]
        );
    }

    #[test]
    fn words_are_looked_up_without_punctuation() {
        let word = |text| WordBox::new(Rect::new(0, 0, 1, 1), text, Locator::START, Locator::START);
        let words = ["\"Rabbit,\"", "(dear)", "well-known"].map(word);
        let looked: Vec<&str> = words.iter().map(WordBox::lookup_text).collect();
        assert_eq!(looked, ["Rabbit", "dear", "well-known"]);
    }
}
