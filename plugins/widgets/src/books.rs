//! Books: covers, progress, what to read next, highlights.

use alloc::borrow::Cow;

use quireink_sdk::{RegisterError, Registrar};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/books.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/books.de.ini")))?;
    Ok(())
}
