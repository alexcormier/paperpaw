//! Quotes, notes, to-dos and pictures.

use alloc::borrow::Cow;

use quireink_sdk::{RegisterError, Registrar};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/words.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/words.de.ini")))?;
    Ok(())
}
