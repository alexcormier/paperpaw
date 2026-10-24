//! The first-run setup: on the first start, a short guide through the
//! settings that matter, one a page, each with a sensible default already
//! chosen, so *Next* is always right.
//!
//! - The steps come from the plugins ([`paperpaw_sdk::setup`]): the
//!   language, the buttons, the text size, the time zone, Wi-Fi, the
//!   keyboard, the screen lock, and any other plugin's. Those this device
//!   does not need are left out (e.g. Wi-Fi where the system runs it).
//! - Skipped as a whole on its first page (by *Back* after a question),
//!   never shown again once done or skipped; *Set up PaperPaw* in the
//!   settings opens it again.
//! - The last page tells where books come from.
//! - It is the screen of the slot [`slots::SETUP`], so another plugin can
//!   replace it.
//!
//! Not shown on a device that was used before it came (a book was
//! opened already).

#![no_std]

extern crate alloc;

mod guide;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;

use paperpaw_sdk::builtin::{events, menus, slots};
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Id, Manifest, MenuEntry, Params, Plugin, Priority,
    RegisterError, Registrar, ScreenSpec, Version, library,
};

pub const PLUGIN: Id = Id::new_static("setup");
/// Opens the guide (from the settings).
pub const OPEN: Id = Id::new_static("setup.open");
/// The guide.
pub const SCREEN: Id = Id::new_static("setup.guide");

/// The file whose being there says the setup was done or skipped.
fn done_file() -> alloc::string::String {
    format!("{}/done", Env::plugin_dir(PLUGIN.as_str()))
}

/// Notes that the setup was done or skipped: it does not come again by
/// itself.
pub(crate) fn mark_done(env: &Env<'_>) {
    let _ = env.storage.write(&done_file(), b"");
}

/// The first-run setup.
#[derive(Debug, Default)]
pub struct Setup;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Setup {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Setup", Version::new(0, 1, 0))
            .description("A short guide through the settings that matter, on the first start.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "setup.title",
            Box::new(|env, _| Box::new(guide::Guide::new(env))),
        ))?;
        r.bind_slot(slots::SETUP, SCREEN, Priority::DEFAULT)?;
        r.add_action(ActionSpec::new(OPEN, "setup.open").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::OpenSlot(slots::SETUP, Params::new()));
            },
        ))?;
        // In the device's part of the settings, before *About*.
        r.add_menu_entry(MenuEntry::new(menus::SETTINGS, OPEN).order(97))?;
        r.add_icon(OPEN, Cow::Borrowed("wand"))?;
        // The first start: the guide, once.
        r.on(events::STARTED, |env, _, out| {
            if env.storage.exists(&done_file()) {
                return;
            }
            if library::opened_books(env.storage).is_empty() {
                out.push(Command::OpenSlot(slots::SETUP, Params::new()));
            } else {
                // Used before the guide came: not a first start.
                mark_done(env);
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
