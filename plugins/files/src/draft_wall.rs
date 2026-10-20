//! Draft of a redesigned library (experiment): a wall of covers with a strip to continue reading and filter pills.
//!
//! Shown in place of today's library when [`crate::DRAFT`] is
//! `files.draft.wall`.

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::{Env, Params, RegisterError, Registrar, Screen};

/// Registers what this draft adds (screens, actions, translations).
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-wall.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-wall.de.ini")))?;
    Ok(())
}

/// The library screen of this draft.
pub(crate) fn screen(env: &Env<'_>, params: &Params) -> Box<dyn Screen> {
    Box::new(crate::Browser::new(env, params))
}
