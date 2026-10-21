//! Keyboard layouts and the variants of letters (`é` on a long press on
//! `e`).
//!
//! The built-in layouts are static tables, so they cost no memory until
//! one is shown. The *automatic* layout is the built-in one of the UI
//! language, or else what its translation brings under `keyboard.letters`,
//! so a language pack works without code.
//!
//! Other plugins add layouts to the point [`POINT`] (`keyboard.layouts`),
//! a line each: `id; code; letter rows; variants`, e.g.
//! `dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; e=éèë i=ïí`. The id is
//! also the translation key of its name; the code has at most
//! [`MAX_CODE`] characters; the rows are separated by spaces, as the
//! built-in ones, at most [`MAX_ROWS`] of at most [`MAX_KEYS`] letters;
//! the variants (optional) are a letter, `=` and its variants (at most
//! [`MAX_VARIANTS`]), shown before the built-in ones of the letter.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::Env;

/// The setting value (and list entry) for the UI language's layout.
pub(crate) const AUTO: &str = "keyboard.layout.auto";

/// The point other plugins add layouts to (see the module's
/// documentation).
pub const POINT: &str = "keyboard.layouts";

/// Letter rows of an added layout, and letters in a row, at most: the
/// keyboard stays on the screen.
pub const MAX_ROWS: usize = 6;
pub const MAX_KEYS: usize = 16;
/// Characters of a layout's code, and variants of a letter, at most.
pub const MAX_CODE: usize = 4;
pub const MAX_VARIANTS: usize = 16;

/// Whether `c` shows nothing or turns text around (controls, marks of
/// direction, joiners): never a key.
fn invisible(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}

/// A layout chosen: the code on the key that switches layouts, the letter
/// rows, and the variants of letters it adds.
pub(crate) type Chosen = (String, Cow<'static, str>, Vec<(char, String)>);

/// A layout another plugin added (see [`POINT`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Added {
    pub id: String,
    pub code: String,
    pub letters: String,
    pub variants: Vec<(char, String)>,
}

/// The layouts other plugins added, in the order of the plugins; lines
/// that say too little are left out.
pub(crate) fn added(env: &Env<'_>) -> Vec<Added> {
    let mut added: Vec<Added> = Vec::new();
    for mut layout in env
        .services
        .contributions(POINT)
        .flat_map(|(_, text)| text.lines())
        .filter_map(parse)
    {
        // Not one of the built-in ids, nor one added before.
        let id = layout.id.as_str();
        if id == AUTO || LAYOUTS.iter().any(|l| l.id == id) || added.iter().any(|l| l.id == id) {
            continue;
        }
        // Variants the fonts draw.
        for (_, variants) in &mut layout.variants {
            variants.retain(|c| drawable(env, c.encode_utf8(&mut [0; 4])));
        }
        layout.variants.retain(|(_, variants)| !variants.is_empty());
        added.push(layout);
    }
    added
}

/// A line of [`POINT`].
fn parse(line: &str) -> Option<Added> {
    let mut fields = line.split(';').map(str::trim);
    let id = fields.next().filter(|id| !id.is_empty())?;
    let code: String = fields
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !invisible(*c))
        .take(MAX_CODE)
        .collect();
    // At most so many rows of so many visible letters.
    let rows: Vec<String> = fields
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .take(MAX_ROWS)
        .map(|row| {
            row.chars()
                .filter(|c| !invisible(*c))
                .take(MAX_KEYS)
                .collect::<String>()
        })
        .filter(|row| !row.is_empty())
        .collect();
    if code.is_empty() || rows.is_empty() {
        return None;
    }
    let variants = fields
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|pair| {
            let (base, variants) = pair.split_once('=')?;
            let mut chars = base.chars();
            let base = chars.next().filter(|_| chars.next().is_none())?;
            // As lower case letters are looked up.
            let base = base.to_lowercase().next().unwrap_or(base);
            let variants: String = variants
                .chars()
                .filter(|c| !invisible(*c))
                .take(MAX_VARIANTS)
                .collect();
            (!variants.is_empty()).then_some((base, variants))
        })
        .collect();
    Some(Added {
        id: id.to_string(),
        code,
        letters: rows.join(" "),
        variants,
    })
}

/// A built-in layout: its id (also the translation key of its name), the
/// short code on the key that switches layouts, and the letter rows
/// separated by spaces.
pub(crate) struct Layout {
    pub id: &'static str,
    pub code: &'static str,
    pub letters: &'static str,
}

pub(crate) const LAYOUTS: [Layout; 12] = [
    Layout {
        id: "keyboard.layout.en",
        code: "EN",
        letters: "qwertyuiop asdfghjkl zxcvbnm",
    },
    Layout {
        id: "keyboard.layout.de",
        code: "DE",
        letters: "qwertzuiopü asdfghjklöä yxcvbnmß",
    },
    Layout {
        id: "keyboard.layout.fr",
        code: "FR",
        letters: "azertyuiop qsdfghjklm wxcvbn",
    },
    Layout {
        id: "keyboard.layout.es",
        code: "ES",
        letters: "qwertyuiop asdfghjklñ zxcvbnm",
    },
    Layout {
        id: "keyboard.layout.pt",
        code: "PT",
        letters: "qwertyuiop asdfghjklç zxcvbnm",
    },
    Layout {
        id: "keyboard.layout.sv",
        code: "SV",
        letters: "qwertyuiopå asdfghjklöä zxcvbnm",
    },
    Layout {
        id: "keyboard.layout.da",
        code: "DA",
        letters: "qwertyuiopå asdfghjklæø zxcvbnm",
    },
    Layout {
        id: "keyboard.layout.cs",
        code: "CS",
        letters: "qwertzuiopú asdfghjklů yxcvbnm",
    },
    Layout {
        id: "keyboard.layout.tr",
        code: "TR",
        letters: "qwertyuıopğü asdfghjklşi zxcvbnmöç",
    },
    Layout {
        id: "keyboard.layout.ru",
        code: "RU",
        letters: "йцукенгшщзх фывапролджэ ячсмитьбю",
    },
    Layout {
        id: "keyboard.layout.uk",
        code: "UK",
        letters: "йцукенгшщзхї фівапролджє ячсмитьбю",
    },
    Layout {
        id: "keyboard.layout.el",
        code: "EL",
        letters: "ςερτυθιοπ ασδφγηξκλ ζχψωβνμ",
    },
];

/// Letters and signs with variants, and the variants (for lower case;
/// capitals are their upper case).
const VARIANTS: [(char, &str); 30] = [
    ('a', "àáâäãåāæą"),
    ('c', "çćč"),
    ('d', "ďđð"),
    ('e', "èéêëēėęě"),
    ('g', "ğ"),
    ('i', "îïíīįìı"),
    ('l', "łľ"),
    ('n', "ñńň"),
    ('o', "ôöòóõøōœő"),
    ('r', "ř"),
    ('s', "ßśšş"),
    ('t', "ťþ"),
    ('u', "ûüùúūůű"),
    ('y', "ÿý"),
    ('z', "žźż"),
    ('е', "ё"),
    ('ь', "ъ"),
    ('г', "ґ"),
    ('и', "ії"),
    ('α', "ά"),
    ('ε', "έ"),
    ('η', "ή"),
    ('ι', "ίϊΐ"),
    ('ο', "ό"),
    ('υ', "ύϋΰ"),
    ('ω', "ώ"),
    ('-', "–—"),
    ('"', "„“”«»"),
    ('\'', "‚‘’‹›"),
    ('.', "…"),
];

/// The variants of `c` (lower case), if it has any.
pub(crate) fn variants(c: char) -> Option<&'static str> {
    VARIANTS
        .iter()
        .find(|(base, _)| *base == c)
        .map(|(_, v)| *v)
}

/// Whether the device's fonts draw every letter of `letters` and their
/// capitals: a layout in a script they lack (e.g. Cyrillic on the X4's
/// packs) is not offered, rather than shown as question marks.
pub(crate) fn drawable(env: &Env<'_>, letters: &str) -> bool {
    let Some(font) = paperpaw_sdk::ui::UiStyle::new(env).font(env) else {
        return true;
    };
    letters
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| core::iter::once(c).chain(c.to_uppercase()))
        .all(|c| font.has_char(c))
}

/// The layouts chosen in the setting `value` (ids separated by commas),
/// as their codes and letter rows; the UI language's layout if none.
/// Layouts the fonts cannot draw are left out.
pub(crate) fn chosen(env: &Env<'_>, value: &str) -> Vec<Chosen> {
    let mut layouts: Vec<Chosen> = Vec::new();
    let mut added: Option<Vec<Added>> = None;
    for id in value.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        if id == AUTO {
            let (code, letters) = automatic(env);
            layouts.push((code, letters, Vec::new()));
        } else if let Some(layout) = LAYOUTS
            .iter()
            .find(|l| l.id == id && drawable(env, l.letters))
        {
            layouts.push((
                layout.code.to_string(),
                Cow::Borrowed(layout.letters),
                Vec::new(),
            ));
        } else if let Some(layout) = added
            .get_or_insert_with(|| self::added(env))
            .iter()
            .find(|l| l.id == id && drawable(env, &l.letters))
        {
            let letters = Cow::Owned(layout.letters.clone());
            layouts.push((layout.code.clone(), letters, layout.variants.clone()));
        }
    }
    if layouts.is_empty() {
        let (code, letters) = automatic(env);
        layouts.push((code, letters, Vec::new()));
    }
    layouts
}

/// The UI language's layout: the built-in one for the language, or its
/// translation's (`keyboard.letters`, which falls back to English).
fn automatic(env: &Env<'_>) -> (String, Cow<'static, str>) {
    let language = env.i18n.language();
    let base = language.split('-').next().unwrap_or(language);
    let code = base.to_uppercase();
    let builtin = LAYOUTS
        .iter()
        .find(|l| l.id.rsplit('.').next() == Some(base));
    match (builtin, env.i18n.get("keyboard.letters")) {
        (Some(layout), _) => (code, Cow::Borrowed(layout.letters)),
        (None, Some(letters)) => (code, Cow::Owned(letters.to_string())),
        (None, None) => (code, Cow::Borrowed(LAYOUTS[0].letters)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_have_names_and_rows() {
        let en = include_str!("../i18n/en.ini");
        let de = include_str!("../i18n/de.ini");
        for layout in &LAYOUTS {
            let key = alloc::format!("{} =", layout.id);
            assert!(en.contains(&key) && de.contains(&key), "{}", layout.id);
            assert_eq!(layout.letters.split(' ').count(), 3, "{}", layout.id);
        }
        assert!(en.contains("keyboard.layout.auto ="));
    }

    #[test]
    fn added_layouts_are_read_from_their_lines() {
        let dutch = parse("dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; e=éèë ij=x i=ïí");
        let dutch = dutch.unwrap();
        assert_eq!(
            (dutch.id.as_str(), dutch.code.as_str()),
            ("dutch.layout", "NL")
        );
        assert_eq!(dutch.letters, "qwertyuiop asdfghjkl zxcvbnm");
        // Two letters before `=` are no letter.
        assert_eq!(
            dutch.variants,
            [('e', String::from("éèë")), ('i', String::from("ïí"))]
        );
        assert!(parse("plain.layout; PL; abc").unwrap().variants.is_empty());
        // Capitals are looked up in lower case; too much is cut; what
        // shows nothing goes.
        let big = parse(&alloc::format!(
            "big.layout; LONGCODE; {}; A=\u{202E}áà{}",
            ["abcdefghijklmnopqrstuvwxyz"; 30].join(" "),
            "x".repeat(40)
        ))
        .unwrap();
        assert_eq!(big.code, "LONG");
        assert_eq!(big.letters.split(' ').count(), MAX_ROWS);
        assert!(
            big.letters
                .split(' ')
                .all(|row| row.chars().count() == MAX_KEYS)
        );
        assert_eq!(big.variants[0].0, 'a');
        assert_eq!(big.variants[0].1.chars().count(), MAX_VARIANTS);
        assert!(!big.variants[0].1.contains('\u{202E}'));
        assert_eq!(parse("id; \u{202E}; abc"), None);
        for line in ["", "only.id", "id; CODE", "; X; abc", "id;; abc"] {
            assert_eq!(parse(line), None, "{line}");
        }
    }

    #[test]
    fn letters_have_variants() {
        assert_eq!(variants('e'), Some("èéêëēėęě"));
        assert!(variants('e').unwrap().contains('é'));
        assert_eq!(variants('q'), None);
        // Every variant is one character in upper case too, or kept.
        for (_, variants) in VARIANTS {
            assert!(!variants.is_empty());
        }
    }
}
