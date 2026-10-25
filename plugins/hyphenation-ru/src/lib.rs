//! Hyphenation patterns for Russian.
//!
//! The patterns are compiled at build time and read in place, so they cost
//! flash but almost no RAM (see `patterns/README.md` for their source and
//! licence).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::{HyphenationSpec, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("hyphenation-ru");
/// The patterns for the language tag `ru`.
pub const RU: Id = Id::new_static("hyphenation-ru.ru");

/// The compiled table, read in place.
const TABLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ru.pphp"));

/// The Russian hyphenation plugin.
#[derive(Debug, Default)]
pub struct Russian;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Russian {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "Hyphenation (Russian)",
            paperpaw_sdk::package_version!(),
        )
        .description("Hyphenation patterns for Russian, for evenly set text.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        r.add_hyphenation(HyphenationSpec::new(RU, "ru", TABLE))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
