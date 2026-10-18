//! Firmware updates, on devices whose firmware QuireInk is (the X4): a
//! page in the settings finds an update file in the folder `updates` on
//! the card, says what it is and who signed it, and has the shell install
//! it (see `quireink_sdk::firmware`). An update found at start is offered
//! once; after the restart the plugin says how it went and removes the
//! file.
//!
//! Updates signed by QuireInk install at once; one's own builds, signed
//! with one's own key, after the reader chose to trust that key.

#![no_std]

extern crate alloc;

mod found;
mod page;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use quireink_sdk::builtin::{events, menus};
use quireink_sdk::firmware::Firmware;
use quireink_sdk::{
    ActionSpec, Arg, Command, Env, Id, Manifest, MenuEntry, MenuNeed, Params, Plugin,
    RegisterError, Registrar, SDK_VERSION, ScreenSpec, Storage, Version,
};

pub const PLUGIN: Id = Id::new_static("update");
/// Opens the page.
pub const OPEN: Id = Id::new_static("update.open");
const PAGE: Id = Id::new_static("update.page");
/// The update being installed (its path and version), kept through the
/// restart to say how it went.
const INSTALLING: &str = "installing.txt";
/// The update offered at start last (its path and version): offered once.
const OFFERED: &str = "offered.txt";

fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// Two lines of a small file of the plugin's.
fn read_pair(storage: &dyn Storage, name: &str) -> Option<(String, String)> {
    let text = quireink_sdk::read_file(storage, &file(name), 4096).ok()?;
    let text = String::from_utf8(text).ok()?;
    let mut lines = text.lines();
    Some((String::from(lines.next()?), String::from(lines.next()?)))
}

pub(crate) fn write_pair(storage: &dyn Storage, name: &str, first: &str, second: &str) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&file(name), format!("{first}\n{second}\n").as_bytes());
}

/// At start: how the update installed before went, and an update to offer.
fn started(env: &Env<'_>, firmware: &Firmware, out: &mut Vec<Command>) {
    let storage = env.storage;
    if let Some((path, version)) = read_pair(storage, INSTALLING) {
        let _ = storage.remove(&file(INSTALLING));
        if !firmware.undone && version == firmware.version {
            let _ = storage.remove(&path);
            out.push(Command::Notify(
                env.i18n
                    .format("update.done", &[("version", Arg::Str(&version))]),
            ));
            return;
        }
    }
    if firmware.undone {
        out.push(Command::Notify(env.i18n.format(
            "update.undone",
            &[("version", Arg::Str(&firmware.version))],
        )));
        return;
    }
    // A newer update on the card, not offered before.
    if let Ok(update) = found::find(storage, firmware)
        && quireink_sdk::firmware::compare_versions(&update.info.version, &firmware.version).is_gt()
        && read_pair(storage, OFFERED) != Some((update.path.clone(), update.info.version.clone()))
    {
        write_pair(storage, OFFERED, &update.path, &update.info.version);
        out.push(Command::OpenScreen(PAGE, Params::new()));
    }
}

/// The update plugin.
#[derive(Debug, Default)]
pub struct Update;

impl Plugin for Update {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Update"),
            description: Cow::Borrowed("Installs firmware updates from the card."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut dyn Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: PAGE,
            title: Cow::Borrowed("update.title"),
            factory: Box::new(|env, _| Box::new(page::UpdatePage::new(env))),
        })?;
        r.add_action(ActionSpec {
            id: OPEN,
            title: Cow::Borrowed("update.title"),
            handler: Some(Box::new(|_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(PAGE, Params::new()));
            })),
        })?;
        // In the device's part of the settings, before *About*.
        r.add_menu_entry(MenuEntry {
            menu: menus::SETTINGS,
            item: OPEN,
            order: 98,
        })?;
        r.set_menu_need(OPEN, MenuNeed::Firmware)?;
        r.add_icon(OPEN, Cow::Borrowed("download"))?;
        r.subscribe(
            events::STARTED,
            Box::new(|env, _, out| {
                if let Some(firmware) = env.firmware {
                    started(env, firmware, out);
                }
            }),
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
