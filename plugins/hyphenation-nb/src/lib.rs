//! Hyphenation patterns for Norwegian Bokmål.
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM (see `patterns/README.md` for their source and
//! licence).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation-nb");
/// The patterns for the language tag `nb`.
pub const NB: Id = Id::new_static("hyphenation-nb.nb");
/// The patterns for the language tag `no`.
pub const NO: Id = Id::new_static("hyphenation-nb.no");

/// The compiled table, read in place.
const TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/nb.pphp"));

/// The Norwegian Bokmål hyphenation plugin.
#[derive(Debug, Default)]
pub struct Norwegian;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Norwegian {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (Norwegian Bokmål)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for Norwegian Bokmål, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        for (language, id) in [("nb", NB), ("no", NO)] {
            r.add_hyphenation(HyphenationSpec::new(id, language, TABLE))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
