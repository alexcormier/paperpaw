//! Hyphenation patterns for French.
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM (see `patterns/README.md` for their source and
//! licence).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation-fr");
/// The patterns for the language tag `fr`.
pub const FR: Id = Id::new_static("hyphenation-fr.fr");

/// The compiled table, read in place.
const TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fr.pphp"));

/// The French hyphenation plugin.
#[derive(Debug, Default)]
pub struct French;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for French {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (French)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for French, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        r.add_hyphenation(HyphenationSpec::new(FR, "fr", TABLE))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
