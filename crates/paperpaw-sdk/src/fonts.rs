use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;
use core::ops::Range;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Storage};
use paperpaw_text::sfnt::FontInfo;
use paperpaw_text::{Font, GlyphCache, LineMetrics, PackSource, StoredPack};

use crate::builtin::fonts as roles;
use crate::collections::VecMap;
use crate::id::Id;
use crate::plugin::{Priority, RegisterError};

/// A font contribution.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FontSpec {
    /// `<plugin>.<name>`, e.g. `fonts-default.literata`.
    pub id: Id,
    pub font: Font,
    /// The family it belongs to, e.g. `Literata`: families are what users
    /// choose (with their bold and italic faces). `None` keeps a font out
    /// of the choice.
    pub family: Option<Cow<'static, str>>,
    pub bold: bool,
    pub italic: bool,
}

impl FontSpec {
    /// A font outside any family.
    pub fn new(id: Id, font: Font) -> Self {
        Self {
            id,
            font,
            family: None,
            bold: false,
            italic: false,
        }
    }

    /// Puts the font into `family` as its regular, bold, italic or bold
    /// italic face.
    pub fn family(
        mut self,
        family: impl Into<Cow<'static, str>>,
        bold: bool,
        italic: bool,
    ) -> Self {
        self.family = Some(family.into());
        self.bold = bold;
        self.italic = italic;
        self
    }
}

/// A face of a family in a file on the storage, as a data plugin names
/// it (`[family]` in its `plugin.ini`): a font pack (`.ppfp`), read a
/// glyph at a time and drawn on every device, or an OpenType/TrueType
/// file, drawn where scalable fonts are, at its style's weight (400 or
/// 700) where it is a variable font. Loaded only while its family is
/// chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontFace {
    /// `<plugin>.<name>`, e.g. `font-fredoka.bold`.
    pub id: Id,
    /// The file on the storage.
    pub path: String,
    /// The family it belongs to, e.g. `Fredoka`.
    pub family: Cow<'static, str>,
    pub bold: bool,
    pub italic: bool,
}

impl FontFace {
    pub fn new(
        id: Id,
        path: impl Into<String>,
        family: impl Into<Cow<'static, str>>,
        bold: bool,
        italic: bool,
    ) -> Self {
        Self {
            id,
            path: path.into(),
            family: family.into(),
            bold,
            italic,
        }
    }

    /// Whether its file is a font pack, by its name.
    pub fn is_pack(&self) -> bool {
        is_pack(&self.path)
    }
}

/// Whether `path` names a font pack (`.ppfp`).
fn is_pack(path: &str) -> bool {
    path.len() > 5
        && path.is_char_boundary(path.len() - 5)
        && path[path.len() - 5..].eq_ignore_ascii_case(".ppfp")
}

/// One registered font.
#[derive(Debug)]
struct Entry {
    id: Id,
    /// `None` for a font file not loaded (yet).
    font: Option<Font>,
    family: Option<Cow<'static, str>>,
    bold: bool,
    italic: bool,
    /// 400 regular, 700 bold; picks among faces of the same style.
    weight: u16,
    /// A font file on the storage, loaded only while its family is chosen.
    file: Option<String>,
    /// For a pack that adds characters to another font (e.g. Cyrillic to
    /// a font for Latin): that font.
    extends: Option<Id>,
    /// A face named by its plugin ([`FontFace`]): a pack, or a scalable
    /// font drawn at its style's weight.
    face: bool,
}

/// Fonts asked in turn for a character a font lacks: its extensions, the
/// fallback font, the fallback's extensions.
const OTHERS: usize = 4;

/// Largest font file loaded.
const MAX_FONT_FILE: u64 = 48 * 1024 * 1024;

/// The lines of a heading or title, as many times its size apart
/// ([`FontHandle::heading_line_height`]).
const HEADING_LEADING: f32 = 1.2;

/// All registered fonts, the role bindings and the glyph cache.
///
/// Fonts are bound to *roles* such as
/// [`builtin::fonts::UI`](crate::builtin::fonts::UI), like screens to slots.
/// Plugins propose fonts for roles; a chosen reading family
/// (`Fonts::set_family`) fills the reading roles, and the user's
/// `[fonts]` configuration overrides everything.
pub struct Fonts {
    entries: Vec<Entry>,
    candidates: Vec<(Id, Id, Priority)>,
    user: VecMap<Id, Id>,
    /// The family chosen for reading.
    family: Option<String>,
    /// What the status lines are in: a value of
    /// [`STATUS_FONT`](crate::builtin::settings::STATUS_FONT).
    status: Option<String>,
    cache: RefCell<GlyphCache>,
    /// Only described, not drawn (see [`Fonts::set_describing`]): no
    /// glyph is made.
    describing: core::cell::Cell<bool>,
}

impl Fonts {
    /// While on, screens are only described, not drawn (a blind
    /// framebuffer): no glyph is made, also not to measure ink, which is
    /// taken from the line's measures. The kernel sets it around
    /// describing a screen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_describing(&self, on: bool) {
        self.describing.set(on);
    }
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fonts")
            .field(
                "fonts",
                &self.entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            )
            .field("family", &self.family)
            .finish_non_exhaustive()
    }
}

impl Fonts {
    /// `cache_bytes` bounds the memory for glyphs rasterized at runtime.
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            candidates: Vec::new(),
            user: VecMap::new(),
            family: None,
            status: None,
            cache: RefCell::new(GlyphCache::new(cache_bytes)),
            describing: core::cell::Cell::new(false),
        }
    }

    fn check_new(&self, id: &Id) -> Result<(), RegisterError> {
        if self.entries.iter().any(|e| e.id == *id) {
            return Err(RegisterError::Duplicate { id: id.clone() });
        }
        Ok(())
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, spec: FontSpec) -> Result<(), RegisterError> {
        self.check_new(&spec.id)?;
        self.entries.push(Entry {
            id: spec.id,
            font: Some(spec.font),
            family: spec.family,
            weight: if spec.bold { 700 } else { 400 },
            bold: spec.bold,
            italic: spec.italic,
            file: None,
            extends: None,
            face: false,
        });
        Ok(())
    }

    /// Adds a font pack on the storage (`.ppfp`) that adds characters to
    /// font `extends`, e.g. Cyrillic and Greek to `fonts-default.literata`:
    /// read a glyph at a time when drawn, so it takes almost no memory, and
    /// asked for what that font lacks.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_extension(
        &mut self,
        id: Id,
        path: String,
        extends: Id,
    ) -> Result<(), RegisterError> {
        self.check_new(&id)?;
        self.entries.push(Entry {
            id,
            font: None,
            family: None,
            bold: false,
            italic: false,
            weight: 400,
            file: Some(path),
            extends: Some(extends),
            face: false,
        });
        Ok(())
    }

    /// Adds a font file on the storage, described by its header (see
    /// [`sfnt::read_info`](paperpaw_text::sfnt::read_info)); it is loaded
    /// only when its family is chosen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_file(&mut self, id: Id, path: String, info: FontInfo) -> Result<(), RegisterError> {
        self.check_new(&id)?;
        self.entries.push(Entry {
            id,
            font: None,
            family: Some(Cow::Owned(info.family)),
            bold: info.bold,
            italic: info.italic,
            weight: info.weight,
            file: Some(path),
            extends: None,
            face: false,
        });
        Ok(())
    }

    /// Adds a face of a family in a file on the storage (see
    /// [`FontFace`]); it is loaded only when its family is chosen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_face(&mut self, face: FontFace) -> Result<(), RegisterError> {
        self.check_new(&face.id)?;
        self.entries.push(Entry {
            id: face.id,
            font: None,
            family: Some(face.family),
            weight: if face.bold { 700 } else { 400 },
            bold: face.bold,
            italic: face.italic,
            file: Some(face.path),
            extends: None,
            face: true,
        });
        Ok(())
    }

    /// Forgets every font of plugin `plugin` (ids `<plugin>.…`), e.g. of a
    /// data plugin removed while running; returns how many. Glyphs drawn
    /// so far are forgotten too.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn remove_plugin(&mut self, plugin: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.id.namespace() != plugin);
        self.candidates
            .retain(|(_, font, _)| font.namespace() != plugin);
        let removed = before - self.entries.len();
        if removed > 0 {
            // The cache knows fonts by their place in the list.
            self.cache.borrow_mut().clear();
        }
        removed
    }

    /// Proposes `font` for `role`; the highest priority wins.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn propose(&mut self, role: Id, font: Id, priority: Priority) {
        self.candidates.push((role, font, priority));
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.entries.shrink_to_fit();
        self.candidates.shrink_to_fit();
        self.user.shrink_to_fit();
    }

    /// The user's explicit choice for a role.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn bind_user(&mut self, role: Id, font: Id) {
        self.user.insert(role, font);
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id.as_str() == id)
    }

    /// Ids of all fonts, loaded or not.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.entries.iter().map(|e| &e.id)
    }

    /// The families to choose from, in registration order.
    pub fn families(&self) -> Vec<&str> {
        let mut families: Vec<&str> = Vec::new();
        for family in self.entries.iter().filter_map(|e| e.family.as_deref()) {
            if !families.contains(&family) {
                families.push(family);
            }
        }
        families
    }

    /// The family chosen for reading, if any.
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    /// Chooses the family for reading (`None`: the plugins' proposals).
    /// Call [`Fonts::load`] afterwards for font files.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_family(&mut self, family: Option<&str>) {
        self.family = family.map(String::from);
    }

    /// Chooses what the status lines are in ([`roles::STATUS`]): a value
    /// of [`STATUS_FONT`](crate::builtin::settings::STATUS_FONT), as the
    /// menus, as the book or a family's name. Call [`Fonts::load`]
    /// afterwards for font files.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_status(&mut self, choice: &str) {
        self.status = Some(String::from(choice));
    }

    /// The family of its own the status lines are in, if one is chosen.
    fn status_family(&self) -> Option<&str> {
        use crate::builtin::settings::{STATUS_FONT_BOOK, STATUS_FONT_UI};
        self.status
            .as_deref()
            .filter(|s| *s != STATUS_FONT_UI && *s != STATUS_FONT_BOOK)
    }

    /// The face for the status lines: the user's binding, else as chosen
    /// ([`Fonts::set_status`]), else the menus'.
    fn status_face(&self) -> Option<FontHandle<'_>> {
        use crate::builtin::settings::STATUS_FONT_BOOK;
        if let Some(font) = self.user.get(roles::STATUS.as_str()) {
            return self.get(font.as_str());
        }
        let own = self
            .status_family()
            .and_then(|family| self.best_face(family, false, false, true))
            .map(|index| self.handle(index));
        let book = (self.status.as_deref() == Some(STATUS_FONT_BOOK))
            .then(|| self.role(roles::READING.as_str()))
            .flatten();
        own.or(book).or_else(|| self.role(roles::UI.as_str()))
    }

    /// The entry of `family` best for a regular, bold, italic or bold
    /// italic face: that style, the weight closest to 400 or 700; among
    /// equals the first.
    fn best_face(&self, family: &str, bold: bool, italic: bool, loaded: bool) -> Option<usize> {
        let target: i32 = if bold { 700 } else { 400 };
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.family.as_deref() == Some(family)
                    && e.bold == bold
                    && e.italic == italic
                    && (!loaded || e.font.is_some())
            })
            .min_by_key(|(_, e)| (i32::from(e.weight) - target).abs())
            .map(|(index, _)| index)
    }

    /// Loads the files of the chosen family's faces (one per style) and
    /// frees all others; returns the files that could not be loaded.
    /// Packs that extend fonts are opened too (they stay where they are).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn load(&mut self, storage: &Rc<dyn Storage>) -> Vec<String> {
        let mut failed = Vec::new();
        let chosen = self.family.clone();
        let mut wanted: Vec<usize> = chosen
            .as_deref()
            .map(|family| {
                [(false, false), (true, false), (false, true), (true, true)]
                    .iter()
                    .filter_map(|&(b, i)| self.best_face(family, b, i, false))
                    .collect()
            })
            .unwrap_or_default();
        // The status lines' family of their own: its regular face.
        if let Some(index) = self
            .status_family()
            .and_then(|family| self.best_face(family, false, false, false))
        {
            wanted.push(index);
        }
        for (index, entry) in self.entries.iter_mut().enumerate() {
            let Some(path) = &entry.file else {
                continue;
            };
            if entry.extends.is_some() {
                if entry.font.is_none() {
                    let source = Rc::new(Stored {
                        storage: storage.clone(),
                        path: path.clone(),
                    });
                    match StoredPack::open(source) {
                        Ok(pack) => entry.font = Some(Font::Stored(pack)),
                        Err(_) => failed.push(path.clone()),
                    }
                }
                continue;
            }
            if !wanted.contains(&index) {
                entry.font = None;
                continue;
            }
            if entry.font.is_some() {
                continue;
            }
            let font = if entry.face && is_pack(path) {
                let source = Rc::new(Stored {
                    storage: storage.clone(),
                    path: path.clone(),
                });
                StoredPack::open(source).ok().map(Font::Stored)
            } else {
                // A variable font at its style's weight.
                let weight = entry.face.then(|| f32::from(entry.weight));
                read_font(storage.as_ref(), path, weight)
            };
            match font {
                Some(font) => entry.font = Some(font),
                None => failed.push(path.clone()),
            }
        }
        failed
    }

    /// A font by id, if loaded.
    pub fn get(&self, id: &str) -> Option<FontHandle<'_>> {
        self.index_of(id).map(|index| self.handle(index))
    }

    /// The chosen family's face for a reading role: `None` if no family
    /// is chosen or the role is not a reading role; `Some(None)` if the
    /// family lacks that face.
    fn family_face(&self, role: &str) -> Option<Option<FontHandle<'_>>> {
        let family = self.family.as_deref()?;
        let (bold, italic) = match role {
            r if r == roles::READING.as_str() => (false, false),
            r if r == roles::READING_BOLD.as_str() => (true, false),
            r if r == roles::READING_ITALIC.as_str() => (false, true),
            r if r == roles::READING_BOLD_ITALIC.as_str() => (true, true),
            _ => return None,
        };
        let index = self.best_face(family, bold, italic, true);
        Some(index.map(|index| self.handle(index)))
    }

    fn proposed(&self, role: &str) -> Option<FontHandle<'_>> {
        self.candidates
            .iter()
            .filter(|(r, font, _)| r.as_str() == role && self.get(font.as_str()).is_some())
            .max_by_key(|(_, _, priority)| *priority)
            .and_then(|(_, font, _)| self.get(font.as_str()))
    }

    /// The font bound to `role`: the user's choice, else the chosen
    /// family's face, else the best proposal, else the first font.
    pub fn role(&self, role: &str) -> Option<FontHandle<'_>> {
        if role == roles::STATUS.as_str() {
            return self.status_face();
        }
        let user = self.user.get(role).and_then(|id| self.get(id.as_str()));
        let first = || {
            self.entries
                .iter()
                .position(|e| e.font.is_some())
                .map(|index| self.handle(index))
        };
        user.or_else(|| self.family_face(role).flatten())
            .or_else(|| self.proposed(role))
            .or_else(first)
    }

    /// The font bound to `role` by the user, a chosen family or a plugin;
    /// unlike [`Fonts::role`], no fallback, for optional roles such as bold
    /// faces (a chosen family without bold has none, so its regular face
    /// is used rather than another family's bold).
    pub fn bound(&self, role: &str) -> Option<FontHandle<'_>> {
        let user = self.user.get(role).and_then(|id| self.get(id.as_str()));
        user.or_else(|| match self.family_face(role) {
            Some(face) => face,
            None => self.proposed(role),
        })
    }

    /// Whether the user chose the font for `role` in the configuration.
    pub fn user_bound(&self, role: &str) -> bool {
        self.user.contains_key(role)
    }

    /// Bytes used by the glyph cache.
    pub fn cache_bytes(&self) -> usize {
        self.cache.borrow().bytes()
    }

    /// A handle of loaded entry `index`, with the fonts asked for the
    /// characters it lacks: its extensions, the fallback font and the
    /// fallback's extensions.
    fn handle(&self, index: usize) -> FontHandle<'_> {
        let entry = &self.entries[index];
        let mut others: [Option<(&Font, u16)>; OTHERS] = [None; OTHERS];
        let mut add = |i: usize| {
            let Some(font) = self.entries[i].font.as_ref() else {
                return;
            };
            if i != index
                && !others.iter().flatten().any(|(_, n)| usize::from(*n) == i)
                && let Some(slot) = others.iter_mut().find(|o| o.is_none())
            {
                *slot = Some((font, i as u16));
            }
        };
        for i in self.extensions_of(index) {
            add(i);
        }
        if let Some(fallback) = self.fallback_index() {
            add(fallback);
            for i in self.extensions_of(fallback) {
                add(i);
            }
        }
        FontHandle {
            id: &entry.id,
            // Handles are made for loaded fonts only.
            font: entry.font.as_ref().unwrap_or_else(|| unreachable!()),
            cache: &self.cache,
            describing: &self.describing,
            index: index as u16,
            others,
            display: false,
        }
    }

    /// The loaded packs that extend entry `index`.
    fn extensions_of(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let id = &self.entries[index].id;
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.extends.as_ref() == Some(id) && e.font.is_some())
            .map(|(i, _)| i)
    }

    /// The font for characters another one lacks (e.g. Cyrillic in a UI
    /// font for Latin): the one plugins propose for reading, as reading
    /// fonts cover the most scripts.
    fn fallback_index(&self) -> Option<usize> {
        let reading = roles::READING;
        let reading = reading.as_str();
        self.candidates
            .iter()
            .filter(|(r, font, _)| r.as_str() == reading && self.index_of(font.as_str()).is_some())
            .max_by_key(|(_, _, priority)| *priority)
            .and_then(|(_, font, _)| self.index_of(font.as_str()))
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.id.as_str() == id && e.font.is_some())
    }
}

/// A pack on the storage, read where it is.
struct Stored {
    storage: Rc<dyn Storage>,
    path: String,
}

impl PackSource for Stored {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            match self
                .storage
                .read_at(&self.path, at + done as u64, &mut buf[done..])
            {
                Ok(0) | Err(_) => return false,
                Ok(n) => done += n,
            }
        }
        true
    }
}

/// Reads a font file into memory.
fn read_font(storage: &dyn Storage, path: &str, weight: Option<f32>) -> Option<Font> {
    let size = storage.size(path).ok()?;
    if size > MAX_FONT_FILE {
        return None;
    }
    let mut data = alloc::vec![0u8; size as usize];
    let mut at = 0;
    while at < data.len() {
        let n = storage.read_at(path, at as u64, &mut data[at..]).ok()?;
        if n == 0 {
            return None;
        }
        at += n;
    }
    match weight {
        Some(weight) => Font::from_data(data, &[("wght", weight)]).ok(),
        None => Font::from_data(data, &[]).ok(),
    }
}

/// Fonts a plugin loads itself, e.g. the fonts embedded in a book, with
/// a glyph cache of their own; freed when the set is dropped.
pub struct FontSet {
    fonts: Vec<(Id, Font)>,
    cache: RefCell<GlyphCache>,
    /// Only described, not drawn (see [`Fonts::set_describing`]): no
    /// glyph is made.
    describing: core::cell::Cell<bool>,
}

impl fmt::Debug for FontSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSet")
            .field(
                "fonts",
                &self.fonts.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl FontSet {
    /// `cache_bytes` bounds the memory for the set's glyphs.
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            fonts: Vec::new(),
            cache: RefCell::new(GlyphCache::new(cache_bytes)),
            describing: core::cell::Cell::new(false),
        }
    }

    /// Adds a font; returns its index.
    pub fn add(&mut self, id: Id, font: Font) -> usize {
        self.fonts.push((id, font));
        self.fonts.len() - 1
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// Font `index`, ready to measure and draw.
    pub fn get(&self, index: usize) -> Option<FontHandle<'_>> {
        let (id, font) = self.fonts.get(index)?;
        Some(FontHandle {
            id,
            font,
            cache: &self.cache,
            describing: &self.describing,
            index: index as u16,
            others: [None; OTHERS],
            display: false,
        })
    }
}

/// A font ready to measure and draw text.
#[derive(Clone, Copy)]
pub struct FontHandle<'a> {
    id: &'a Id,
    font: &'a Font,
    /// The glyph cache of the registry or set the font is in.
    cache: &'a RefCell<GlyphCache>,
    /// Its registry only describes what is drawn: no glyph is made.
    describing: &'a core::cell::Cell<bool>,
    /// The font's number in that cache.
    index: u16,
    /// Fonts for the characters this one lacks, asked in turn, and their
    /// numbers in the same cache.
    others: [Option<(&'a Font, u16)>; OTHERS],
    /// Draws at a pack's large sizes made for a few characters too (see
    /// [`FontHandle::display`]).
    display: bool,
}

impl fmt::Debug for FontHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FontHandle").field(self.id()).finish()
    }
}

impl<'a> FontHandle<'a> {
    pub fn id(&self) -> &'a Id {
        self.id
    }

    fn font(&self) -> &'a Font {
        self.font
    }

    /// Ascent, descent and line height at `size` pixels: of the size text
    /// is drawn at (see [`FontHandle::size_for`]; for a display, the size
    /// asked).
    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        self.font()
            .line_metrics(self.font().size_for(size, "", self.display))
    }

    /// How far apart the lines of a heading or title set on more lines
    /// are, in pixels: closer than those of text (its line height, about
    /// 1.4 times the size with the default font), 1.2 times the size, so
    /// that a title on two lines reads as one; the highest letters of a
    /// line (capitals with accents) still stay clear of the lowest of the
    /// line above (descenders). Text keeps its line height.
    pub fn heading_line_height(&self, size: f32) -> f32 {
        let drawn = self.font().size_for(size, "", self.display);
        // In whole pixels, rounded up (no float rounding in `no_std`).
        let apart = (drawn * HEADING_LEADING + 0.999) as u32 as f32;
        apart.min(self.line_metrics(size).line_height)
    }

    /// The font that draws `ch` and its number in the cache: this one,
    /// or the fallback if only that has it.
    fn pick(&self, ch: char) -> (&'a Font, u16) {
        if self.font.has_char(ch) {
            return (self.font, self.index);
        }
        self.others
            .iter()
            .flatten()
            .find(|(font, _)| font.has_char(ch))
            .copied()
            .unwrap_or((self.font, self.index))
    }

    /// Whether this font (or its fallback) draws `ch`; without it, the
    /// character would show as a question mark or not at all.
    pub fn has_char(&self, ch: char) -> bool {
        self.font.has_char(ch)
            || self
                .others
                .iter()
                .flatten()
                .any(|(font, _)| font.has_char(ch))
    }

    /// Horizontal advance of one character in pixels, at the size it is
    /// drawn at alone (see [`FontHandle::size_for`]).
    pub fn advance(&self, size: f32, ch: char) -> f32 {
        let mut buffer = [0u8; 4];
        let size = self.size_for(size, ch.encode_utf8(&mut buffer));
        self.pick(ch).0.advance(ch, size)
    }

    /// This font for a text of a few characters drawn large alone, a
    /// clock's time or a number: where its pack keeps large sizes only for
    /// some characters (the digits and the colon, on devices that draw
    /// from packs), a text of those is drawn at them; any other text at
    /// the sizes that have every character, as by the font itself.
    pub fn display(self) -> Self {
        Self {
            display: true,
            ..self
        }
    }

    /// The size `text` is drawn at when asked for `size`: the nearest size
    /// of a pack that has all of its characters (see
    /// [`FontHandle::display`]), one for the whole text; `size` where the
    /// font is scalable. Measuring, inking and drawing use it.
    pub fn size_for(&self, size: f32, text: &str) -> f32 {
        self.font.size_for(size, text, self.display)
    }

    /// Width of a single line of text in pixels.
    pub fn measure(&self, size: f32, text: &str) -> f32 {
        let size = self.size_for(size, text);
        if self.others[0].is_none() {
            paperpaw_text::measure(self.font(), size, text)
        } else {
            text.chars().map(|ch| self.advance(size, ch)).sum()
        }
    }

    /// How far the ink of one line of `text` reaches above and below its
    /// baseline, in pixels (above negative), counting pixels at least half
    /// covered, as panels of black and white show them: what centres it
    /// by its ink rather than by its line ([`ui::draw_header`]). `None`
    /// for text without ink (spaces).
    ///
    /// [`ui::draw_header`]: crate::ui::draw_header
    pub fn ink(&self, size: f32, text: &str) -> Option<(i32, i32)> {
        // Only described: as a line would be, no glyph made.
        if self.describing.get() {
            let metrics = self.line_metrics(size);
            return Some((-(metrics.ascent as i32), -(metrics.descent as i32)));
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut ink: Option<(i32, i32)> = None;
        for ch in text.chars() {
            let (font, index) = self.pick(ch);
            let glyph = cache.glyph(index, font, ch, size);
            let inked = |y: u16| (0..glyph.width).any(|x| glyph.coverage(x, y) >= 128);
            let (Some(first), Some(last)) = (
                (0..glyph.height).find(|y| inked(*y)),
                (0..glyph.height).rev().find(|y| inked(*y)),
            ) else {
                continue;
            };
            let top = i32::from(glyph.top) + i32::from(first);
            let bottom = i32::from(glyph.top) + i32::from(last) + 1;
            ink = Some(ink.map_or((top, bottom), |(t, b)| (t.min(top), b.max(bottom))));
        }
        ink
    }

    /// Where the ink of `text` at `size` begins and ends across, from the
    /// pen's start, in pixels (counting pixels at least half covered, as
    /// [`FontHandle::ink`] does): what centres it by its ink rather than by
    /// its advance (e.g. an icon in its column). `None` for text without
    /// ink.
    pub fn ink_across(&self, size: f32, text: &str) -> Option<(i32, i32)> {
        // Only described: as far as its advances go, no glyph made.
        if self.describing.get() {
            return Some((0, self.measure(size, text) as i32));
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut ink: Option<(i32, i32)> = None;
        let mut pen = 0.0f32;
        let mut based = false;
        for ch in text.chars() {
            let (font, index) = self.pick(ch);
            let glyph = cache.glyph(index, font, ch, size);
            let mark = paperpaw_text::is_mark(ch);
            // Where it is drawn ([`FontHandle::draw`]).
            let at = paperpaw_text::glyph_x(pen, glyph.left, mark && !based);
            based |= !mark;
            let inked = |x: u16| (0..glyph.height).any(|y| glyph.coverage(x, y) >= 128);
            if let (Some(first), Some(last)) = (
                (0..glyph.width).find(|x| inked(*x)),
                (0..glyph.width).rev().find(|x| inked(*x)),
            ) {
                let (left, right) = (at + i32::from(first), at + i32::from(last) + 1);
                ink = Some(ink.map_or((left, right), |(l, r)| (l.min(left), r.max(right))));
            }
            pen += glyph.advance;
        }
        ink
    }

    /// Breaks `text` into lines that fit `max_width`.
    pub fn wrap(&self, size: f32, text: &str, max_width: f32) -> Vec<Range<usize>> {
        paperpaw_text::wrap(text, max_width, |s| self.measure(size, s))
    }

    /// Draws one line with its baseline at `baseline`; returns its width.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
    ) -> f32 {
        self.draw_bolder(canvas, size, x, baseline, text, color, 0)
    }

    /// Like [`FontHandle::draw`], with glyphs widened by `extra` pixels
    /// (e.g. a reader's font weight setting).
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw_bolder(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
        extra: u16,
    ) -> f32 {
        // Only described, as a screen reader asks: no glyph made.
        if canvas.is_blind() || self.describing.get() {
            return self.measure(size, text);
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut pen = x;
        // Runs of characters drawn by the same font; marks at the start of
        // one sit on the character before it, if there is one.
        let mut rest = text;
        let mut based = false;
        while let Some(first) = rest.chars().next() {
            let (font, index) = self.pick(first);
            let end = rest
                .char_indices()
                .find(|(_, ch)| self.pick(*ch).1 != index)
                .map_or(rest.len(), |(at, _)| at);
            let style = paperpaw_text::TextStyle::new(font, index, size, color)
                .bolder(extra)
                .based(based);
            let run = &rest[..end];
            pen += paperpaw_text::draw_text(canvas, &mut cache, &style, pen, baseline, run);
            based |= run.chars().any(|ch| !paperpaw_text::is_mark(ch));
            rest = &rest[end..];
        }
        pen - x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use paperpaw_text::{Glyph, PackBuilder};

    /// A pack holding `chars`, with empty glyphs.
    fn pack(chars: &str) -> alloc::vec::Vec<u8> {
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 12.0,
        };
        let mut builder = PackBuilder::new();
        let glyphs = chars.chars().map(|c| (c, Glyph::empty(5.0))).collect();
        builder.add_strike(10.0, metrics, glyphs);
        builder.build()
    }

    /// Any Tabler icon has a name; one the icon font lacks is drawn from
    /// an icon pack that extends it, others stay without a drawing.
    #[test]
    fn icon_packs_add_icons_the_font_lacks() {
        let zoom = crate::icons::char("zoom-in").expect("every Tabler name is known");
        let star = crate::icons::char("star").unwrap();
        let other = crate::icons::char("zoom-out").unwrap();
        let mut packed = String::new();
        packed.push(zoom);
        let storage: Rc<dyn Storage> = Rc::new(
            MemoryStorage::new().with("plugins/icons-zoom/fonts/icons.ppfp", pack(&packed)),
        );
        let mut fonts = Fonts::new(4096);
        let mut shipped = String::new();
        shipped.push(star);
        let icons = Font::from_data(pack(&shipped), &[]).unwrap();
        fonts
            .add(FontSpec::new(Id::new_static("a.icons"), icons))
            .unwrap();
        fonts.propose(roles::ICONS, Id::new_static("a.icons"), Priority::DEFAULT);
        fonts
            .add_extension(
                Id::new_static("icons-zoom.icons"),
                String::from("plugins/icons-zoom/fonts/icons.ppfp"),
                Id::new_static("a.icons"),
            )
            .unwrap();
        assert!(fonts.load(&storage).is_empty());
        let bound = fonts.bound(roles::ICONS.as_str()).unwrap();
        assert!(bound.has_char(star) && bound.has_char(zoom));
        assert!(!bound.has_char(other));
    }

    #[test]
    fn extensions_add_characters_read_from_storage() {
        let storage: Rc<dyn Storage> =
            Rc::new(MemoryStorage::new().with("plugins/cyr/fonts/latin-cyr.ppfp", pack("жщ")));
        let mut fonts = Fonts::new(4096);
        let latin = Font::from_data(pack("ab"), &[]).unwrap();
        fonts
            .add(FontSpec::new(Id::new_static("a.latin"), latin))
            .unwrap();
        let ui = Font::from_data(pack("ab"), &[]).unwrap();
        fonts
            .add(FontSpec::new(Id::new_static("a.ui"), ui))
            .unwrap();
        fonts.propose(roles::READING, Id::new_static("a.latin"), Priority::DEFAULT);
        fonts
            .add_extension(
                Id::new_static("cyr.latin-cyr"),
                String::from("plugins/cyr/fonts/latin-cyr.ppfp"),
                Id::new_static("a.latin"),
            )
            .unwrap();
        assert!(
            !fonts.get("a.latin").unwrap().has_char('ж'),
            "not before it is loaded"
        );
        assert!(fonts.load(&storage).is_empty());
        // The font it extends, and others by way of their fallback.
        assert!(fonts.get("a.latin").unwrap().has_char('ж'));
        assert!(fonts.get("a.ui").unwrap().has_char('щ'));
        assert_eq!(fonts.get("a.ui").unwrap().measure(10.0, "aж"), 10.0);
        assert!(!fonts.get("a.ui").unwrap().has_char('ю'));
    }

    /// A family of packs on the storage: its faces read a glyph at a time
    /// while it is chosen, freed when another is, forgotten with their
    /// plugin.
    #[test]
    fn families_of_packs_are_chosen_and_removed() {
        let storage: Rc<dyn Storage> = Rc::new(
            MemoryStorage::new()
                .with("plugins/round/fonts/round.ppfp", pack("ab"))
                .with("plugins/round/fonts/round-bold.ppfp", pack("abc")),
        );
        let mut fonts = Fonts::new(4096);
        let builtin = Font::from_data(pack("a"), &[]).unwrap();
        fonts
            .add(FontSpec::new(Id::new_static("a.serif"), builtin).family("Serif", false, false))
            .unwrap();
        fonts.propose(roles::READING, Id::new_static("a.serif"), Priority::DEFAULT);
        for (name, bold) in [("round", false), ("round-bold", true)] {
            let face = FontFace::new(
                Id::parse(&alloc::format!("round.{name}")).unwrap(),
                alloc::format!("plugins/round/fonts/{name}.ppfp"),
                "Round",
                bold,
                false,
            );
            assert!(face.is_pack());
            fonts.add_face(face).unwrap();
        }
        assert_eq!(fonts.families(), ["Serif", "Round"]);
        assert!(
            fonts.get("round.round").is_none(),
            "not before it is chosen"
        );

        fonts.set_family(Some("Round"));
        assert!(fonts.load(&storage).is_empty());
        let reading = fonts.role(roles::READING.as_str()).unwrap();
        assert_eq!(reading.id().as_str(), "round.round");
        assert!(reading.has_char('b') && !reading.has_char('c'));
        let bold = fonts.bound(roles::READING_BOLD.as_str()).unwrap();
        assert!(bold.has_char('c'));
        // No italic: the regular face stands in, not another family's.
        assert!(fonts.bound(roles::READING_ITALIC.as_str()).is_none());

        fonts.set_family(None);
        assert!(fonts.load(&storage).is_empty());
        assert!(fonts.get("round.round").is_none(), "freed");

        fonts.set_family(Some("Round"));
        assert!(fonts.load(&storage).is_empty());
        assert_eq!(fonts.remove_plugin("round"), 2);
        assert_eq!(fonts.families(), ["Serif"]);
        assert_eq!(
            fonts.role(roles::READING.as_str()).unwrap().id().as_str(),
            "a.serif"
        );
    }
}
