//! Hyphenation patterns for Finnish.
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM (see `patterns/README.md` for their source and
//! licence).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation-fi");
/// The patterns for the language tag `fi`.
pub const FI: Id = Id::new_static("hyphenation-fi.fi");

/// The compiled table, read in place.
const TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fi.pphp"));

/// The Finnish hyphenation plugin.
#[derive(Debug, Default)]
pub struct Finnish;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Finnish {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (Finnish)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for Finnish, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        r.add_hyphenation(HyphenationSpec::new(FI, "fi", TABLE))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
