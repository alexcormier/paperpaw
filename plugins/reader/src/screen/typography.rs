//! The page's type: the faces, sizes and spacing chosen in the settings,
//! the book's own fonts, small capitals.

use super::*;

/// Relative sizes of headings 1 to 6.
pub(super) const HEADING_SCALE: [f32; 6] = [1.6, 1.4, 1.2, 1.1, 1.0, 1.0];

/// Glyphs of a book's embedded fonts kept at most.
pub(super) const EMBEDDED_CACHE: usize = 256 * 1024;

/// Largest total size of a book's embedded fonts loaded.
pub(super) const EMBEDDED_LIMIT: usize = 8 * 1024 * 1024;

/// The fonts a book embeds, loaded with the book.
pub(super) struct Embedded {
    pub(super) set: FontSet,
    /// Family, bold, italic and index in `set` of each font.
    pub(super) faces: Vec<(u8, bool, bool, usize)>,
}

/// Whether a book's own fonts are used: the font chosen is the book's
/// (the default choice), and they render on this device.
pub(super) fn uses_book_fonts(env: &Env<'_>) -> bool {
    use quireink_sdk::builtin::settings::{DEFAULT_FONT, READING_FONT};
    // Embedded fonts take hundreds of kilobytes: not on small devices.
    quireink_sdk::text::OUTLINES
        && env.device.memory != quireink_sdk::MemoryClass::Small
        && env.settings.read(&READING_FONT) == DEFAULT_FONT
}

impl Embedded {
    /// The book's fonts, if the reader uses them (the font chosen is the
    /// book's own) and they render here.
    pub(super) fn load(env: &Env<'_>, document: &dyn Document) -> Option<Embedded> {
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
            let Ok(loaded) = quireink_sdk::Font::from_data(data, &[]) else {
                continue;
            };
            let id = Id::parse(&format!("reader.book-font-{index}")).ok()?;
            let at = set.add(id, loaded);
            faces.push((font.family, font.bold, font.italic, at));
        }
        (!faces.is_empty()).then_some(Embedded { set, faces })
    }

    /// The face of `family` closest to `bold` and `italic`.
    pub(super) fn face(&self, family: u8, bold: bool, italic: bool) -> Option<FontHandle<'_>> {
        let of_family = || self.faces.iter().filter(|f| f.0 == family);
        let exact = of_family().find(|f| f.1 == bold && f.2 == italic);
        let regular = || of_family().find(|f| !f.1 && !f.2);
        exact
            .or_else(regular)
            .or_else(|| of_family().next())
            .and_then(|f| self.set.get(f.3))
    }
}

/// The reading fonts by face, for layout and drawing.
pub(super) struct Faces<'a> {
    /// Regular, bold, italic and bold italic.
    pub(super) fonts: [FontHandle<'a>; 4],
    pub(super) weight: u8,
    pub(super) word_spacing: f32,
    /// The book's own fonts.
    pub(super) embedded: Option<&'a Embedded>,
    pub(super) size: f32,
    pub(super) ascent: f32,
    pub(super) line_height: f32,
}

impl<'a> Faces<'a> {
    /// The reading faces; missing ones fall back to the nearest face, and
    /// a font the user chose for book text is not mixed with the default
    /// bold or italic faces.
    pub(super) fn new(
        env: &Env<'a>,
        t: &Typography,
        embedded: Option<&'a Embedded>,
    ) -> Option<Self> {
        let fonts_ = env.fonts;
        let regular = fonts_.role(fonts::READING.as_str())?;
        let custom = fonts_.user_bound(fonts::READING.as_str());
        let face = |role: &quireink_sdk::Id| {
            if custom && !fonts_.user_bound(role.as_str()) {
                None
            } else {
                fonts_.bound(role.as_str())
            }
        };
        let bold = face(&fonts::READING_BOLD).unwrap_or(regular);
        let italic = face(&fonts::READING_ITALIC).unwrap_or(regular);
        let bold_italic = face(&fonts::READING_BOLD_ITALIC).unwrap_or(bold);
        Some(Self {
            fonts: [regular, bold, italic, bold_italic],
            weight: t.weight,
            word_spacing: t.word_spacing,
            embedded,
            size: t.size,
            ascent: t.ascent,
            line_height: t.line_height,
        })
    }

    /// The book's font for text in an embedded family, else the reader's.
    pub(super) fn font(&self, face: Face) -> FontHandle<'a> {
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

    pub(super) fn size(&self, face: Face) -> f32 {
        match (face.size, face.heading) {
            // The book's size wins.
            (percent, _) if percent > 0 => self.size * f32::from(percent) / 100.0,
            (_, 0) => self.size,
            (_, level) => self.size * HEADING_SCALE[usize::from(level.clamp(1, 6)) - 1],
        }
    }
}

/// Small capitals are this much of the text size.
pub(super) const SMALL_CAPS: f32 = 0.78;

impl Faces<'_> {
    /// The width of `text` in `face`, small capitals included.
    pub(super) fn measure(&self, face: Face, text: &str) -> f32 {
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
    pub(super) fn space(&self, face: Face) -> f32 {
        self.font(face).advance(self.size(face), ' ') * self.word_spacing
    }

    /// Draws `text` in `face`, as heavy as the font weight setting asks.
    pub(super) fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        face: Face,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> f32 {
        self.draw_in(canvas, face, x, baseline, text, Color::BLACK)
    }

    /// Draws `text` as [`Faces::draw`] does, in `ink` (e.g. a mark's).
    pub(super) fn draw_in(
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

/// `text` in parts of lower case letters and of the rest, each with
/// whether it is lower case.
pub(super) fn small_caps_parts(text: &str) -> impl Iterator<Item = (&str, bool)> {
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

impl Typesetter for Faces<'_> {
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

/// Page geometry and text style, derived from settings and the screen.
#[derive(Debug, Clone)]
pub(super) struct Typography {
    pub(super) size: f32,
    /// Font weight steps (0: as the font is).
    pub(super) weight: u8,
    /// Spaces relative to the font's.
    pub(super) word_spacing: f32,
    /// Body text lines: baseline from the top of a line, line height.
    pub(super) ascent: f32,
    pub(super) line_height: f32,
    pub(super) style: Style,
    /// Content box.
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    /// Status lines above and below the content, if they show anything.
    pub(super) top_status: Option<Rect>,
    pub(super) status: Option<Rect>,
    /// Language whose hyphenation patterns are used, if hyphenating.
    pub(super) hyphenation: Option<String>,
    /// What the text filters did when it was made.
    pub(super) filters: String,
    /// Identifies this layout; part of the page index file name.
    pub(super) key: String,
}

impl Typography {
    /// Typography for a book in `language`; a book of `whole_pages` is
    /// shown edge to edge, without margins or status lines.
    pub(super) fn new(
        env: &Env<'_>,
        font: &FontHandle<'_>,
        language: &str,
        whole_pages: bool,
    ) -> Self {
        let settings = env.settings;
        let m = env.metrics;
        let size = m.pt(settings.read(&FONT_SIZE) as f32);
        let weight = settings.read(&FONT_WEIGHT).clamp(0, 4) as u8;
        let word_spacing = settings.read(&WORD_SPACING).clamp(50, 200) as f32 / 100.0;
        let metrics = font.line_metrics(size);
        let natural = metrics.ascent + metrics.descent;
        let line_height = (size * settings.read(&LINE_SPACING) as f32 / 100.0).max(natural);
        let indent = settings.read(&INDENT);
        let justify = settings.read(&JUSTIFY);
        let ascent = metrics.ascent + (line_height - natural) / 2.0;
        let style = Style {
            paragraph_spacing: if indent { 0.0 } else { line_height * 0.6 },
            indent: if indent { size * 1.5 } else { 0.0 },
            em: size,
            justify,
        };

        let display = env.device.display;
        let margin = if whole_pages {
            0.0
        } else {
            m.mm(settings.read(&MARGIN).max(0) as u32) as f32
        };
        let status_height = status::height(env);
        let (top, bottom) = if whole_pages {
            (false, false)
        } else {
            (status::shown(env, true), status::shown(env, false))
        };
        let width = (f32::from(display.width) - 2.0 * margin).max(1.0);
        // Status lines sit close to the edge, in the text's margin; the
        // text keeps its margin, and never runs up to a status line, also
        // on pages it fills to the last pixel.
        let edge = m.tenth_mm(settings.read(&status::EDGE).clamp(0, 150) as u32) as f32;
        let status_gap = m.mm(2) as f32;
        let status_room = status_height + status_gap;
        let screen = f32::from(display.height);
        let y = if top {
            margin.max(edge + status_room)
        } else {
            margin
        };
        let end = if bottom {
            (screen - margin).min(screen - edge - status_room)
        } else {
            screen - margin
        };
        let height = (end - y).max(1.0);
        let line = |y: f32| Rect::new(margin as u16, y as u16, width as u16, status_height as u16);
        let top_status = top.then(|| line(edge));
        let status = bottom.then(|| line(screen - edge - status_height));
        let hyphenation = (settings.read(&HYPHENATE) && env.hyphenation.get(language).is_some())
            .then(|| language.to_string());
        let key = format!(
            "{:016x}",
            fnv1a64(format!(
                "{}|{}|{}|{}|{}|{size}|{line_height}|{}|{}|{justify}|{width}x{height}|{}|{}|{}|{word_spacing}|{}|{whole_pages}",
                quireink_sdk::layout::VERSION,
                font.id(),
                face_id(env, &fonts::READING_BOLD),
                face_id(env, &fonts::READING_ITALIC),
                face_id(env, &fonts::READING_BOLD_ITALIC),
                style.indent,
                style.paragraph_spacing,
                hyphenation.as_deref().unwrap_or("-"),
                settings.read(&crate::settings::PUBLISHER_STYLES),
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
            status,
            hyphenation,
            filters: env.text_filters.signature(env),
            key,
        }
    }
}

/// The font bound to an optional reading face, for the layout key.
pub(super) fn face_id(env: &Env<'_>, role: &quireink_sdk::Id) -> String {
    env.fonts
        .bound(role.as_str())
        .map_or_else(String::new, |font| font.id().to_string())
}
