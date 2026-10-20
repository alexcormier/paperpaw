//! Fonts from the internet: a catalog of fonts (paperpaw.org's by
//! default), each name shown in its own font; one is shown larger with a
//! sentence in it, downloaded as a data plugin and read with at once.
//!
//! Nothing of a font is built in: devices that draw scalable fonts get
//! its font files, the others (the X4) font packs made for them, read a
//! glyph at a time from the storage. *More fonts* is offered where the
//! reading font and the status lines' font are chosen.

#![no_std]

extern crate alloc;

mod catalog;
mod font;
mod install;
mod list;
mod samples;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, groups, settings};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::choice::Choice;
use paperpaw_sdk::{
    ActionSpec, Command, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError, Registrar,
    SDK_VERSION, ScreenSpec, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("fonts-browser");
/// Opens the list of fonts.
pub const OPEN: Id = Id::new_static("fonts-browser.open");
const LIST_SCREEN: Id = Id::new_static("fonts-browser.list");
const FONT_SCREEN: Id = Id::new_static("fonts-browser.font");
const CHOICE: Id = Id::new_static("fonts-browser.choice");

/// Where the catalog of fonts comes from.
pub const CATALOG: Setting<paperpaw_sdk::setting::Text> = Setting::new("fonts-browser.catalog");

/// The font library's address, for developers and libraries of one's own.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_text(
    CATALOG.static_str(),
    "fonts-browser.setting.catalog",
    "https://paperpaw.org/api/v1/fonts.json",
    groups::DEVELOPER,
)
.described("fonts-browser.setting.catalog.help")];

/// The fonts browser.
#[derive(Debug, Default)]
pub struct FontsBrowser;

impl Plugin for FontsBrowser {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("More fonts"),
            description: Cow::Borrowed(
                "Shows fonts from the internet, each with a sample, and downloads them to read with.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec {
            id: LIST_SCREEN,
            title: Cow::Borrowed("fonts-browser.title"),
            factory: Box::new(|env, _| Box::new(list::ListScreen::new(env))),
        })?;
        r.add_screen(ScreenSpec {
            id: FONT_SCREEN,
            title: Cow::Borrowed("fonts-browser.title"),
            factory: Box::new(|env, params| Box::new(font::FontScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOICE,
            title: Cow::Borrowed("fonts-browser.title"),
            factory: Box::new(|env, params| Box::new(Choice::new(env, params))),
        })?;
        r.add_action(ActionSpec::new(OPEN, "fonts-browser.more").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(LIST_SCREEN, Params::new()));
            },
        ))?;
        // After the fonts to choose from, where they are chosen.
        for setting in [settings::READING_FONT, settings::STATUS_FONT] {
            r.add_menu_entry(MenuEntry {
                menu: setting.id().clone(),
                item: OPEN,
                order: 0,
            })?;
        }
        r.add_icon(OPEN, Cow::Borrowed("typography"))?;
        // Downloads go on while no screen of the plugin is open; one cut
        // off by a restart is begun anew from the font's page.
        r.on(install::DOWNLOADED, install::received)?;
        r.on(events::STARTED, |env, _, _| {
            install::INSTALLER.cancel(env.storage)
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
