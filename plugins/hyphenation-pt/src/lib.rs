//! Hyphenation patterns for Portuguese.
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM (see `patterns/README.md` for their source and
//! licence).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation-pt");
/// The patterns for the language tag `pt`.
pub const PT: Id = Id::new_static("hyphenation-pt.pt");

/// The compiled table, read in place.
const TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pt.pphp"));

/// The Portuguese hyphenation plugin.
#[derive(Debug, Default)]
pub struct Portuguese;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Portuguese {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (Portuguese)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for Portuguese, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        r.add_hyphenation(HyphenationSpec::new(PT, "pt", TABLE))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
