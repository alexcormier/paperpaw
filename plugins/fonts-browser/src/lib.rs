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

use paperpaw_sdk::builtin::{events, groups, settings};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Command, FixedAction, FixedScreen, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("fonts-browser");
/// Opens the list of fonts.
pub const OPEN: Id = Id::new_static("fonts-browser.open");
const LIST_SCREEN: Id = Id::new_static("fonts-browser.list");
const FONT_SCREEN: Id = Id::new_static("fonts-browser.font");

/// Where the catalog of fonts comes from.
pub const CATALOG: Setting<paperpaw_sdk::setting::Text> = Setting::new("fonts-browser.catalog");

/// The font library's address, for developers and libraries of one's own.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::text(
    CATALOG.static_str(),
    "fonts-browser.setting.catalog",
    "https://paperpaw.org/api/v1/fonts.json",
    groups::DEVELOPER,
)
.description("fonts-browser.setting.catalog.help")];

/// The fonts browser.
#[derive(Debug, Default)]
pub struct FontsBrowser;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [FixedAction; 1] = [FixedAction::new(OPEN, "fonts-browser.more")
    .handler(|_, _, out| out.push(Command::OpenScreen(LIST_SCREEN, Params::new())))];
/// After the fonts to choose from, where they are chosen.
static ENTRIES: [MenuEntry; 2] = [
    MenuEntry::new(Id::new_static(settings::READING_FONT.static_str()), OPEN),
    MenuEntry::new(Id::new_static(settings::STATUS_FONT.static_str()), OPEN),
];
static ICONS: [(Id, &str); 1] = [(OPEN, "typography")];

static SCREENS: [FixedScreen; 2] = [
    FixedScreen::new(FONT_SCREEN, "fonts-browser.title", |env, params| {
        Box::new(font::FontScreen::new(env, params))
    }),
    FixedScreen::new(LIST_SCREEN, "fonts-browser.title", |env, _| {
        Box::new(list::ListScreen::new(env))
    })
    .instances(paperpaw_sdk::Instances::SINGLE),
];

impl Plugin for FontsBrowser {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "More fonts", paperpaw_sdk::package_version!()).description(
                "Shows fonts from the internet, each with a sample, and downloads them to read with.",
            ).how(&HOW).uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screens(&SCREENS)?;
        r.add_actions(&ACTIONS)?;
        r.add_menu_entries(&ENTRIES)?;
        r.add_icons(&ICONS)?;
        // Downloads go on while no screen of the plugin is open; one cut
        // off by a restart is begun anew from the font's page.
        r.on(install::DOWNLOADED, install::received)?;
        r.on(events::STARTED, |env, _, _| {
            install::INSTALLER.cancel(env.storage())
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
