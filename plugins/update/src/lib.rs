//! Firmware updates, on devices whose firmware PaperPaw is (the X4): a
//! page in the settings finds an update file in the folder `updates` on
//! the card, says what it is and who signed it, and has the shell install
//! it (see `paperpaw_sdk::firmware`). An update found at start is offered
//! once; after the restart the plugin says how it went and removes the
//! file. The page also asks the update server for a newer firmware and
//! downloads it into `updates` ([`online`]), and then the code plugins
//! built for it (`fetch`), which the updated firmware installs at its
//! first start.
//!
//! Updates signed by PaperPaw install at once; one's own builds, signed
//! with one's own key, once the reader accepted that key, shown by its
//! fingerprint, at each install. An older version installs only when the
//! reader chooses it, told it is older. An update without some of the
//! plugins built in now says which, and the reader chooses to update
//! without them or to wait.

#![no_std]

extern crate alloc;

mod fetch;
mod found;
pub mod online;
mod page;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, groups, menus};
use paperpaw_sdk::firmware::Firmware;
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Env, Id, Manifest, MenuEntry, MenuNeed, Params, Plugin,
    RegisterError, Registrar, ScreenSpec, SettingSpec, Storage, Version,
};

pub const PLUGIN: Id = Id::new_static("update");
/// Opens the page.
pub const OPEN: Id = Id::new_static("update.open");
/// Asks the update server for an update and offers one it names (e.g.
/// from a rule); says nothing otherwise.
pub const CHECK: Id = Id::new_static("update.check");
const PAGE: Id = Id::new_static("update.page");
/// Declared work: a check no reader waits for (a rule's).
const CHECK_DUE: paperpaw_sdk::event::Event = paperpaw_sdk::event::Event::new("update.check-due");
/// The update being installed (its path and version), kept through the
/// restart to say how it went.
const INSTALLING: &str = "installing.txt";
/// The update offered at start last (its path and version; `online` and
/// the version for one named by the update server): offered once.
const OFFERED: &str = "offered.txt";

/// The update server's address, for developers and servers of one's own.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_text(
    online::SERVER.static_str(),
    "update.setting.server",
    online::DEFAULT_SERVER,
    groups::DEVELOPER,
)
.described("update.setting.server.help")];

fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// Two lines of a small file of the plugin's.
fn read_pair(storage: &dyn Storage, name: &str) -> Option<(String, String)> {
    let text = paperpaw_sdk::read_file(storage, &file(name), 4096).ok()?;
    let text = String::from_utf8(text).ok()?;
    let mut lines = text.lines();
    Some((String::from(lines.next()?), String::from(lines.next()?)))
}

pub(crate) fn write_pair(storage: &dyn Storage, name: &str, first: &str, second: &str) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&file(name), format!("{first}\n{second}\n").as_bytes());
}

/// At start: how the update installed before went, and an update to
/// offer. A download the restart cut off is let go.
fn started(env: &Env<'_>, firmware: &Firmware, out: &mut Vec<Command>) {
    let storage = env.storage;
    online::clear_download(storage);
    fetch::interrupted(storage, firmware);
    if let Some((path, version)) = read_pair(storage, INSTALLING) {
        let _ = storage.remove(&file(INSTALLING));
        if !firmware.undone && version == firmware.version {
            let _ = storage.remove(&path);
            out.push(Command::Announce(
                env.i18n
                    .format("update.done", &[("version", Arg::Str(&version))]),
            ));
            return;
        }
    }
    if firmware.undone {
        out.push(Command::Announce(env.i18n.format(
            "update.undone",
            &[("version", Arg::Str(&firmware.version))],
        )));
        return;
    }
    // A newer update on the card, not offered before.
    if let Ok(update) = found::find(storage, firmware)
        && update.is_newer(firmware)
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
        Manifest::new(PLUGIN, "Update", Version::new(0, 1, 0))
            .description(
                "Installs firmware updates, from the card or downloaded from the internet.",
            )
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        // Only where PaperPaw updates the firmware.
        r.set_menu_need(online::SERVER.id().clone(), MenuNeed::Firmware)?;
        r.add_screen(ScreenSpec::new(
            PAGE,
            "update.title",
            Box::new(|env, _| Box::new(page::UpdatePage::new(env))),
        ))?;
        r.add_action(ActionSpec::new(OPEN, "update.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(PAGE, Params::new()));
            },
        ))?;
        r.add_action(ActionSpec::new(CHECK, "update.check").handler(
            |env: &Env<'_>, out: &mut Vec<Command>| {
                if env.reader {
                    online::check(env, true, out);
                } else {
                    let due = CHECK_DUE.id().clone();
                    out.push(Command::Work(paperpaw_sdk::work::Work::when_cheap(
                        due,
                        core::time::Duration::from_secs(3600),
                    )));
                }
            },
        ))?;
        r.on(CHECK_DUE, |env, (), out| online::check(env, true, out))?;
        r.set_menu_need(CHECK, MenuNeed::Firmware)?;
        r.add_icon(CHECK, Cow::Borrowed("cloud-download"))?;
        // Answers and downloads go on while the page is closed.
        r.on(online::CHECKED, online::checked)?;
        r.on(online::DOWNLOADED, online::downloaded)?;
        r.on(fetch::FETCHED, fetch::received)?;
        // In the device's part of the settings, before *About*.
        r.add_menu_entry(MenuEntry::new(menus::SETTINGS, OPEN).order(98))?;
        r.set_menu_need(OPEN, MenuNeed::Firmware)?;
        r.add_icon(OPEN, Cow::Borrowed("circle-arrow-up"))?;
        r.on(events::STARTED, |env, _, out| {
            if let Some(firmware) = env.firmware {
                started(env, firmware, out);
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
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

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn pairs_of_version_1_are_read() {
        let storage = paperpaw_sdk::MemoryStorage::new().with(
            &super::file(super::INSTALLING),
            include_str!("../testdata/installing.txt.v1"),
        );
        assert_eq!(
            super::read_pair(&storage, super::INSTALLING),
            Some((
                alloc::string::String::from("updates/paperpaw-x4-0.4.0.bin"),
                alloc::string::String::from("0.4.0")
            ))
        );
    }
}
