//! PaperPaw's default fonts, contributed like any other plugin:
//!
//! - **Literata** for reading (`fonts-default.literata`), designed for long
//!   texts on screens, with bold, italic and bold italic faces.
//! - **Source Sans 3** for the UI (`fonts-default.source-sans`), a
//!   humanist sans serif: calm and clear, with a warmer, livelier hand
//!   than a technical face. Its font file is shipped unchanged (its
//!   licence reserves the name "Source" for the original) and used at
//!   weight 430 ([`UI_WEIGHT`]); the bitmap packs are drawn from it at
//!   build time, at that weight and for Latin text only.
//! - **PaperPaw Icons** (`fonts-default.icons`): the icons of the menus,
//!   taken from Tabler Icons (MIT licence; see `tools/icons/subset.py`).
//!
//! [`DefaultFonts`] ships them as scalable fonts when built with the
//! `outline` feature, else as bitmap packs. [`DefaultFontPacks`] always uses
//! bitmap packs (feature `packs`), e.g. to emulate small devices.
//!
//! The text fonts are under the SIL Open Font License 1.1. Users can bind
//! other fonts to the roles:
//!
//! ```ini
//! [fonts]
//! core.font-reading = my-fonts.garamond
//! ```

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::{
    Font, FontSpec, Id, Manifest, Plugin, Priority, RegisterError, Registrar, Version,
};

pub const PLUGIN: Id = Id::new_static("fonts-default");
pub const LITERATA: Id = Id::new_static("fonts-default.literata");
pub const LITERATA_BOLD: Id = Id::new_static("fonts-default.literata-bold");
pub const LITERATA_ITALIC: Id = Id::new_static("fonts-default.literata-italic");
pub const LITERATA_BOLD_ITALIC: Id = Id::new_static("fonts-default.literata-bold-italic");
pub const SOURCE_SANS: Id = Id::new_static("fonts-default.source-sans");
pub const ICONS: Id = Id::new_static("fonts-default.icons");

/// The weight of the UI font: a little above regular, so thin strokes
/// hold on e-ink panels with one bit a pixel. Also in `build.rs`.
pub const UI_WEIGHT: f32 = 430.0;

/// The faces: id, role, font data and weight (for variable fonts).
type Faces = [(Id, Id, &'static [u8], f32); 5];

#[cfg(feature = "outline")]
mod outlines {
    const LITERATA: &[u8] = include_bytes!("../fonts/Literata.ttf");
    const LITERATA_ITALIC: &[u8] = include_bytes!("../fonts/Literata-Italic.ttf");
    const SOURCE_SANS: &[u8] = include_bytes!("../fonts/SourceSans3.ttf");
    pub const ICONS: &[u8] = include_bytes!("../fonts/PaperPawIcons.ttf");

    /// Bold faces are instances of the variable fonts.
    pub fn faces() -> super::Faces {
        use super::*;
        [
            (LITERATA, fonts::READING, self::LITERATA, 400.0),
            (LITERATA_BOLD, fonts::READING_BOLD, self::LITERATA, 700.0),
            (
                LITERATA_ITALIC,
                fonts::READING_ITALIC,
                self::LITERATA_ITALIC,
                400.0,
            ),
            (
                LITERATA_BOLD_ITALIC,
                fonts::READING_BOLD_ITALIC,
                self::LITERATA_ITALIC,
                700.0,
            ),
            (SOURCE_SANS, fonts::UI, self::SOURCE_SANS, UI_WEIGHT),
        ]
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
mod packs {
    macro_rules! pack {
        ($name:literal) => {
            include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".ppfp"))
        };
    }

    pub const ICONS: &[u8] = pack!("icons");

    pub fn faces() -> super::Faces {
        use super::*;
        [
            (LITERATA, fonts::READING, pack!("literata"), 400.0),
            (
                LITERATA_BOLD,
                fonts::READING_BOLD,
                pack!("literata-bold"),
                700.0,
            ),
            (
                LITERATA_ITALIC,
                fonts::READING_ITALIC,
                pack!("literata-italic"),
                400.0,
            ),
            (
                LITERATA_BOLD_ITALIC,
                fonts::READING_BOLD_ITALIC,
                pack!("literata-bold-italic"),
                700.0,
            ),
            (SOURCE_SANS, fonts::UI, pack!("source-sans"), UI_WEIGHT),
        ]
    }
}

#[cfg(feature = "outline")]
use outlines as preferred;
#[cfg(not(feature = "outline"))]
use packs as preferred;

/// The default fonts plugin: scalable fonts if available, else bitmap packs.
#[derive(Debug, Default)]
pub struct DefaultFonts;

/// The default fonts as bitmap packs, read in place without a glyph cache.
#[cfg(any(feature = "packs", not(feature = "outline")))]
#[derive(Debug, Default)]
pub struct DefaultFontPacks;

fn manifest() -> Manifest {
    Manifest::new(PLUGIN, "Default fonts", Version::new(0, 1, 0))
        .description("Literata for reading and Source Sans for menus.")
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn register(r: &mut Registrar, faces: Faces, icons: &'static [u8]) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
    for (id, role, data, weight) in faces {
        let font =
            Font::from_data(data, &[("wght", weight)]).map_err(|e| RegisterError::Invalid {
                message: Cow::Owned(alloc::format!("{id}: {e}")),
            })?;
        let family = if role == fonts::UI {
            "Source Sans 3"
        } else {
            "Literata"
        };
        let italic = role == fonts::READING_ITALIC || role == fonts::READING_BOLD_ITALIC;
        r.add_font(FontSpec::new(id.clone(), font).family(family, weight >= 600.0, italic))?;
        r.bind_font(role, id, Priority::DEFAULT)?;
    }
    // Outside any family, so it is never offered for text.
    let font = Font::from_data(icons, &[]).map_err(|e| RegisterError::Invalid {
        message: Cow::Owned(alloc::format!("{ICONS}: {e}")),
    })?;
    r.add_font(FontSpec::new(ICONS, font))?;
    r.bind_font(fonts::ICONS, ICONS, Priority::DEFAULT)?;
    Ok(())
}

impl Plugin for DefaultFonts {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register(r, preferred::faces(), preferred::ICONS)
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
impl Plugin for DefaultFontPacks {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register(r, packs::faces(), packs::ICONS)
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
