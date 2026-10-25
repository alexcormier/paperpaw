use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_config::ParseError;

use crate::plural::PluralRule;

/// The messages of one language.
///
/// Messages are not copied: the catalog keeps its source (usually text
/// built into the program) and an index of where each message's line
/// starts in it, 2 bytes per message (4 in a source of 32 KiB or more);
/// the key and value are read from the
/// line when looked up. Messages the line does not give plainly (quoted
/// values, escape sequences) are kept as byte ranges instead, and only
/// values with escape sequences are stored decoded. A catalog of an
/// unused language can drop its index ([`Catalog::unload`]) and rebuild it
/// when needed.
#[derive(Debug, Clone)]
pub struct Catalog {
    source: Cow<'static, str>,
    language: Span,
    name: Span,
    plural: PluralRule,
    /// Decoded values that contained escape sequences; the meta values
    /// first (`meta_len` bytes), which stay when the catalog is unloaded.
    /// Boxed, as it changes only when the catalog is scanned: two words
    /// rather than three, in each catalog of every plugin.
    escaped: Box<str>,
    meta_len: u32,
    /// The messages, sorted by key; empty while unloaded. Each is where
    /// its line's key starts in the source or, with [`OTHER`] set, the
    /// place of its key and value in `others`.
    index: Index,
    /// Messages whose line does not give them plainly.
    others: Box<[(Span, Span)]>,
    loaded: bool,
    /// The index holds wide entries, each in two of its places (see
    /// [`Index`]); kept here, beside the other small fields, rather than
    /// in the index, where it would take a word.
    wide: bool,
    /// When it was added to a translator, of those added: later ones win.
    pub(crate) added: u32,
}

/// A catalog of a language not in use, kept small: its source and where
/// its language and name are in it, nothing more (see
/// [`Translator`](crate::Translator)). Made a catalog again when its
/// language comes into use.
#[derive(Debug, Clone)]
pub(crate) struct Parked {
    source: Cow<'static, str>,
    language: Span,
    name: Span,
    pub(crate) added: u32,
}

impl Parked {
    pub(crate) fn language(&self) -> &str {
        self.source.get(self.language.range()).unwrap_or_default()
    }

    pub(crate) fn name(&self) -> &str {
        self.source.get(self.name.range()).unwrap_or_default()
    }

    /// The catalog again, unloaded (its index made when it loads).
    pub(crate) fn unpark(self) -> Catalog {
        let added = self.added;
        // Read once already: it parses.
        let mut catalog = Catalog::parse(self.source).unwrap_or_else(|_| Catalog::empty());
        catalog.unload();
        catalog.added = added;
        catalog
    }
}

/// A byte range in the source, or in `escaped` if the top bit is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: u32,
    len: u32,
}

impl Span {
    /// Its bytes in the source (none for one in `escaped`).
    fn range(self) -> core::ops::Range<usize> {
        match self.start & ESCAPED {
            0 => self.start as usize..(self.start + self.len) as usize,
            _ => 0..0,
        }
    }
}

const ESCAPED: u32 = 1 << 31;
/// In the index: a message of `others`.
const OTHER: u32 = 1 << 31;

/// The index of a catalog: its entries (see [`OTHER`]) in two bytes each
/// where they fit, as in most catalogs (of a source under 32 KiB); else
/// (a wide index, see `Catalog::wide`) each in four, the low half first.
/// Made at run time, or kept in the program for catalogs built in
/// ([`FixedIndexes`]).
#[derive(Debug, Clone, Default)]
struct Index(Cow<'static, [u16]>);

/// The indexes of catalogs built into the program, made when it was built
/// (`paperpaw-app`'s build script): each with the [`fingerprint`] of its
/// source. A catalog whose source has one uses it rather than an index
/// on the heap ([`Translator::fixed_indexes`](crate::Translator::fixed_indexes)).
pub type FixedIndexes = &'static [(u64, &'static [u16])];

/// What a catalog's source is known by in [`FixedIndexes`]: FNV-1a of its
/// bytes.
pub fn fingerprint(source: &str) -> u64 {
    source.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// In a narrow index: a message of `others`.
const NARROW_OTHER: u16 = 1 << 15;

impl Index {
    /// `entries` (sorted), narrow where every one fits; and whether it is
    /// wide.
    fn of(entries: Vec<u32>) -> (Self, bool) {
        let narrow = |at: u32| -> Option<u16> {
            let place = at & !OTHER;
            let place = u16::try_from(place)
                .ok()
                .filter(|p| p & NARROW_OTHER == 0)?;
            Some(if at & OTHER != 0 {
                place | NARROW_OTHER
            } else {
                place
            })
        };
        match entries
            .iter()
            .map(|at| narrow(*at))
            .collect::<Option<Vec<u16>>>()
        {
            Some(narrow) => (
                Index(Cow::Owned(narrow.into_boxed_slice().into_vec())),
                false,
            ),
            None => {
                let halves = entries
                    .iter()
                    .flat_map(|at| [*at as u16, (at >> 16) as u16])
                    .collect::<Vec<u16>>();
                (
                    Index(Cow::Owned(halves.into_boxed_slice().into_vec())),
                    true,
                )
            }
        }
    }

    fn len(&self, wide: bool) -> usize {
        if wide { self.0.len() / 2 } else { self.0.len() }
    }

    /// Entry `i`, as a wide index has it.
    fn at(&self, i: usize, wide: bool) -> u32 {
        if wide {
            return u32::from(self.0[2 * i]) | u32::from(self.0[2 * i + 1]) << 16;
        }
        let at = self.0[i];
        let place = u32::from(at & !NARROW_OTHER);
        if at & NARROW_OTHER != 0 {
            place | OTHER
        } else {
            place
        }
    }
}

/// The key of the message line starting at `start`: its characters (see
/// [`plain`]). Only the key is read, as looking a message up compares
/// many keys and reads one value: the catalog may lie in flash, where
/// every byte read costs.
fn plain_key(source: &str, start: u32) -> &str {
    let line = source.get(start as usize..).unwrap_or_default();
    let len = line
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')))
        .unwrap_or(line.len());
    &line[..len]
}

/// The value of the message line starting at `start` (see [`plain`]).
fn plain_value(source: &str, start: u32) -> Option<&str> {
    let after = start as usize + plain_key(source, start).len();
    let rest = source.get(after..)?;
    let rest = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let value = rest.trim_start().strip_prefix('=')?.trim();
    (!value.starts_with('"')).then_some(value)
}

/// The key and value of the message line starting at `start`, as the
/// parser reads them when the value is plain: the key's characters, `=`,
/// then the rest of the line, trimmed. `None` for anything else (a quoted
/// value), which the index keeps in `others`.
fn plain(source: &str, start: u32) -> Option<(&str, &str)> {
    let key = plain_key(source, start);
    let value = plain_value(source, start)?;
    (!key.is_empty()).then_some((key, value))
}

/// Why a catalog could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CatalogError {
    Syntax(ParseError),
    /// `[meta] language` is missing.
    MissingLanguage,
    /// `[meta] plural` names an unknown rule.
    UnknownPluralRule {
        line: u32,
    },
    /// A key outside the `[meta]` and `[messages]` sections.
    UnexpectedSection {
        line: u32,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Syntax(error) => error.fmt(f),
            CatalogError::MissingLanguage => f.write_str("missing `language` in [meta]"),
            CatalogError::UnknownPluralRule { line } => {
                write!(f, "line {line}: unknown plural rule")
            }
            CatalogError::UnexpectedSection { line } => {
                write!(f, "line {line}: only [meta] and [messages] are allowed")
            }
        }
    }
}

impl core::error::Error for CatalogError {}

impl Catalog {
    /// Parses a catalog file. Built-in catalogs (`&'static str`) are not
    /// copied.
    pub fn parse(source: impl Into<Cow<'static, str>>) -> Result<Catalog, CatalogError> {
        let mut catalog = Catalog {
            source: source.into(),
            language: Span { start: 0, len: 0 },
            name: Span { start: 0, len: 0 },
            plural: PluralRule::OneOther,
            escaped: Box::default(),
            meta_len: 0,
            index: Index::default(),
            others: Box::default(),
            loaded: false,
            wide: false,
            added: 0,
        };
        catalog.scan(true)?;
        Ok(catalog)
    }

    /// A catalog of no messages, of the fallback language.
    fn empty() -> Catalog {
        Catalog::parse("[meta]\nlanguage = en\n").unwrap_or_else(|_| unreachable!())
    }

    /// Kept small while its language is not in use ([`Parked`]); itself
    /// where its language or name is escaped (they would be lost).
    pub(crate) fn park(self) -> Result<Parked, Catalog> {
        if self.meta_len > 0 {
            return Err(self);
        }
        Ok(Parked {
            source: self.source,
            language: self.language,
            name: self.name,
            added: self.added,
        })
    }

    /// Reads the source. With `meta`, also reads and validates `[meta]`
    /// (otherwise only the messages are indexed again).
    fn scan(&mut self, meta: bool) -> Result<(), CatalogError> {
        let source: &str = &self.source;
        let base = source.as_ptr() as usize;
        let mut escaped = if meta {
            String::new()
        } else {
            let mut kept = String::from(core::mem::take(&mut self.escaped));
            kept.truncate(self.meta_len as usize);
            kept
        };
        let span = |escaped: &mut String, value: Cow<'_, str>| match value {
            Cow::Borrowed(text) => Span {
                start: (text.as_ptr() as usize - base) as u32,
                len: text.len() as u32,
            },
            Cow::Owned(text) => {
                let start = escaped.len() as u32 | ESCAPED;
                escaped.push_str(&text);
                Span {
                    start,
                    len: text.len() as u32,
                }
            }
        };
        let mut index = Vec::new();
        let mut others = Vec::new();
        let (mut language, mut name, mut plural) = (None, None, PluralRule::OneOther);
        for entry in paperpaw_config::parse(source) {
            let entry = entry.map_err(CatalogError::Syntax)?;
            match (entry.section, entry.key) {
                ("meta", _) if !meta => {}
                ("meta", "language") => language = Some(span(&mut escaped, entry.value.as_str())),
                ("meta", "name") => name = Some(span(&mut escaped, entry.value.as_str())),
                ("meta", "plural") => {
                    plural = PluralRule::from_id(&entry.value.as_str())
                        .ok_or(CatalogError::UnknownPluralRule { line: entry.line })?;
                }
                ("meta", _) => {}
                ("messages", key) => {
                    let value = entry.value.as_str();
                    // Plainly on its line, as the parser read it: only
                    // where the line starts.
                    let start = (key.as_ptr() as usize - base) as u32;
                    let read = plain(source, start);
                    let same = |a: &str, b: &str| a.as_ptr() == b.as_ptr() && a.len() == b.len();
                    match (&value, read) {
                        (Cow::Borrowed(v), Some((k, w))) if same(k, key) && same(w, v) => {
                            index.push(start);
                        }
                        _ => {
                            index.push(others.len() as u32 | OTHER);
                            let key = span(&mut escaped, Cow::Borrowed(key));
                            others.push((key, span(&mut escaped, value)));
                        }
                    }
                }
                _ => return Err(CatalogError::UnexpectedSection { line: entry.line }),
            }
        }
        if meta {
            // Meta values are read before any message, so an escaped one
            // is at the start of `escaped`.
            self.language = language.ok_or(CatalogError::MissingLanguage)?;
            self.name = name.unwrap_or(self.language);
            self.plural = plural;
            self.meta_len = [self.language, self.name]
                .iter()
                .filter(|s| s.start & ESCAPED != 0)
                .map(|s| (s.start & !ESCAPED) + s.len)
                .max()
                .unwrap_or(0);
        }
        // Stable sort keeps file order for duplicates; the last one wins.
        // By cached keys: its scratch lies on the heap, where `sort_by`
        // takes 4 KiB of stack (too much on the X4).
        let key = |at: u32| key_of(source, &escaped, &others, at);
        index.sort_by_cached_key(|at| key(*at));
        index.dedup_by(|later, earlier| {
            let duplicate = key(*later) == key(*earlier);
            if duplicate {
                *earlier = *later;
            }
            duplicate
        });
        self.escaped = escaped.into_boxed_str();
        // Boxed: they no longer change, and a box is smaller than a
        // vector (one of each per catalog, of every plugin).
        (self.index, self.wide) = Index::of(index);
        self.others = others.into_boxed_slice();
        self.loaded = true;
        Ok(())
    }

    /// Drops the message index (keeps language, name and plural rule).
    pub fn unload(&mut self) {
        self.index = Index::default();
        self.wide = false;
        self.others = Box::default();
        self.escaped = Box::from(&self.escaped[..self.meta_len as usize]);
        self.loaded = false;
    }

    /// Rebuilds the message index after [`Catalog::unload`].
    pub fn load(&mut self) {
        if !self.loaded {
            // The source was validated when it was parsed.
            let _ = self.scan(false);
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn text(&self, span: Span) -> &str {
        text_of(&self.source, &self.escaped, span)
    }

    /// Language tag, e.g. `de` or `pt-BR`.
    pub fn language(&self) -> &str {
        self.text(self.language)
    }

    /// Human-readable language name in that language, e.g. `Deutsch`.
    pub fn name(&self) -> &str {
        self.text(self.name)
    }

    pub fn plural_rule(&self) -> PluralRule {
        self.plural
    }

    fn key(&self, at: u32) -> &str {
        key_of(&self.source, &self.escaped, &self.others, at)
    }

    /// The message for `key`, if this catalog has one and is loaded.
    pub fn get(&self, key: &str) -> Option<&str> {
        let (mut low, mut high) = (0, self.index.len(self.wide));
        let at = loop {
            if low >= high {
                return None;
            }
            let middle = low + (high - low) / 2;
            let at = self.index.at(middle, self.wide);
            match self.key(at).cmp(key) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => break at,
            }
        };
        if at & OTHER != 0 {
            let (_, value) = self.others.get((at & !OTHER) as usize)?;
            Some(self.text(*value))
        } else {
            plain_value(&self.source, at)
        }
    }

    /// All message keys, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        (0..self.index.len(self.wide)).map(|i| self.key(self.index.at(i, self.wide)))
    }

    pub fn len(&self) -> usize {
        self.index.len(self.wide)
    }

    pub fn is_empty(&self) -> bool {
        self.index.0.is_empty()
    }

    /// Its index, two bytes an entry, for a build script that keeps it in
    /// the program ([`FixedIndexes`]); `None` while unloaded or where the
    /// index is wide.
    pub fn narrow_index(&self) -> Option<&[u16]> {
        (self.loaded && !self.wide).then_some(&*self.index.0)
    }

    /// The bytes its index takes on the heap: none where it is kept in
    /// the program ([`FixedIndexes`]) or unloaded.
    pub fn index_on_heap(&self) -> usize {
        match &self.index.0 {
            Cow::Owned(index) => index.len() * 2,
            Cow::Borrowed(_) => 0,
        }
    }

    /// Takes its index from `fixed` where that has the same one for its
    /// source: the one on the heap goes.
    pub fn use_fixed(&mut self, fixed: FixedIndexes) {
        if fixed.is_empty() || !matches!(self.index.0, Cow::Owned(_)) {
            return;
        }
        let Some(own) = self.narrow_index() else {
            return;
        };
        let print = fingerprint(&self.source);
        if let Some(&(_, index)) = fixed.iter().find(|(p, index)| *p == print && *index == own) {
            self.index = Index(Cow::Borrowed(index));
        }
    }
}

/// The key of index entry `at` (see the field `index` of `Catalog`).
fn key_of<'a>(source: &'a str, escaped: &'a str, others: &[(Span, Span)], at: u32) -> &'a str {
    if at & OTHER != 0 {
        others
            .get((at & !OTHER) as usize)
            .map_or("", |(key, _)| text_of(source, escaped, *key))
    } else {
        plain_key(source, at)
    }
}

fn text_of<'a>(source: &'a str, escaped: &'a str, span: Span) -> &'a str {
    let (text, start) = if span.start & ESCAPED != 0 {
        (escaped, span.start & !ESCAPED)
    } else {
        (source, span.start)
    };
    text.get(start as usize..(start + span.len) as usize)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A catalog of a source past 32 KiB has a wide index: every message
    /// is found, plain or quoted, before and after unloading.
    #[test]
    fn a_large_catalog_finds_every_message() {
        let mut source = String::from(
            "[meta]
language = en
[messages]
",
        );
        for n in 0..3000 {
            source.push_str(&alloc::format!(
                "message.{n:04} = The text of message {n}.
"
            ));
        }
        source.push_str(
            "quoted.last = \"  far \"
",
        );
        assert!(source.len() > 64 * 1024);
        let mut catalog = Catalog::parse(source).unwrap();
        assert!(catalog.wide);
        for _ in 0..2 {
            assert_eq!(catalog.len(), 3001);
            assert_eq!(catalog.get("message.0000"), Some("The text of message 0."));
            assert_eq!(
                catalog.get("message.2999"),
                Some("The text of message 2999.")
            );
            assert_eq!(catalog.get("quoted.last"), Some("  far "));
            assert_eq!(catalog.get("message.3000"), None);
            assert_eq!(catalog.keys().next(), Some("message.0000"));
            catalog.unload();
            assert!(catalog.is_empty());
            catalog.load();
        }
    }

    #[test]
    fn parses_meta_and_messages() {
        let catalog = Catalog::parse(
            "[meta]\nlanguage = de\nname = Deutsch\n[messages]\nb = Zwei\na = Eins\n",
        )
        .unwrap();
        assert_eq!(catalog.language(), "de");
        assert_eq!(catalog.name(), "Deutsch");
        assert_eq!(catalog.get("a"), Some("Eins"));
        assert_eq!(catalog.get("b"), Some("Zwei"));
        assert_eq!(catalog.get("c"), None);
        assert_eq!(catalog.keys().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn escaped_values_survive_unloading() {
        // Owned sources (e.g. loaded at runtime) work the same way.
        let mut catalog = Catalog::parse(String::from(
            r#"[meta]
language = "d\\e"
[messages]
b = plain
a = "two\nlines"
"#,
        ))
        .unwrap();
        assert_eq!(catalog.language(), r"d\e");
        assert_eq!(catalog.get("a"), Some("two\nlines"));
        catalog.unload();
        assert_eq!(catalog.get("a"), None);
        assert_eq!(catalog.language(), r"d\e");
        catalog.load();
        assert_eq!(catalog.get("a"), Some("two\nlines"));
        assert_eq!(catalog.get("b"), Some("plain"));
    }

    #[test]
    fn plain_and_quoted_messages() {
        let catalog = Catalog::parse(
            "[meta]\nlanguage = en\n[messages]\n  spaced   =   value with = and # in it  \r\n\
             quoted = \"  kept spaces  \"\nescaped = \"a\\tb\"\nempty =\nlast = end",
        )
        .unwrap();
        assert_eq!(catalog.get("spaced"), Some("value with = and # in it"));
        assert_eq!(catalog.get("quoted"), Some("  kept spaces  "));
        assert_eq!(catalog.get("escaped"), Some("a\tb"));
        assert_eq!(catalog.get("empty"), Some(""));
        assert_eq!(catalog.get("last"), Some("end"));
        // Only the quoted ones need more than where their line starts.
        assert_eq!(catalog.others.len(), 2);
        assert_eq!(
            catalog.keys().collect::<Vec<_>>(),
            ["empty", "escaped", "last", "quoted", "spaced"]
        );
    }

    #[test]
    fn last_duplicate_wins() {
        let catalog =
            Catalog::parse("[meta]\nlanguage = en\n[messages]\na = first\na = second\n").unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog.get("a"), Some("second"));
    }

    #[test]
    fn rejects_invalid_catalogs() {
        assert_eq!(
            Catalog::parse("[messages]\na = b\n").unwrap_err(),
            CatalogError::MissingLanguage
        );
        assert_eq!(
            Catalog::parse("[meta]\nlanguage = en\nplural = klingon\n").unwrap_err(),
            CatalogError::UnknownPluralRule { line: 3 }
        );
        assert_eq!(
            Catalog::parse("[meta]\nlanguage = en\n[other]\na = b\n").unwrap_err(),
            CatalogError::UnexpectedSection { line: 4 }
        );
    }
}
