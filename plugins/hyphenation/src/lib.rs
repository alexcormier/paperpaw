//! Hyphenation patterns for English (American spelling).
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM. Other languages, German among them, are
//! plugins of the store that add their own patterns the same way: the
//! firmware stays small. A later registration for a language replaces an
//! earlier one.

#![no_std]

use alloc::borrow::Cow;

extern crate alloc;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation");
pub const ENGLISH: Id = Id::new_static("hyphenation.en");

const EN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/en.pphp"));

/// The hyphenation plugin.
#[derive(Debug, Default)]
pub struct Hyphenation;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Hyphenation {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (English)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for English, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        r.add_hyphenation(HyphenationSpec::new(ENGLISH, "en", EN))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;
    use paperpaw_sdk::hyphen::Patterns;

    fn hyphenated(table: &[u8], word: &str) -> String {
        let patterns = Patterns::new(table).unwrap();
        let mut breaks = Vec::new();
        patterns.hyphenate(word, &mut |at| breaks.push(at));
        let mut out = String::new();
        for (i, c) in word.char_indices() {
            if breaks.contains(&i) {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn hyphenates_english() {
        assert_eq!(hyphenated(EN, "hyphenation"), "hy-phen-ation");
        assert_eq!(hyphenated(EN, "Wonderland"), "Won-der-land");
        // From the exception list.
        assert_eq!(hyphenated(EN, "table"), "ta-ble");
    }
}

#[cfg(test)]
mod words_tests;
