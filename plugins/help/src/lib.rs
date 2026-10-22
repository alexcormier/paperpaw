//! First-time help: the hints plugins name for their screens (see
//! [`paperpaw_sdk::help`]), shown over a screen the first time it opens
//! on this device, once: its zones framed with what a tap there does, the
//! rest in a box. A tap or a button closes them; *No more hints* switches
//! them all off (`help.show`, on at first). *Show hints again*, in the
//! system settings, has each shown once more.
//!
//! The screens shown are kept in `seen.txt`, a screen's id a line
//! (version 1).

#![no_std]

extern crate alloc;

mod overlay;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, groups, params};
use paperpaw_sdk::help;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Command, Env, Id, Manifest, Params, Plugin, RegisterError, Registrar, ScreenSpec, SettingExtra,
    SettingSpec, Storage,
};

pub const PLUGIN: Id = Id::new_static("help");
/// Setting: hints show the first time a screen opens.
pub const SHOW: Setting<bool> = Setting::new("help.show");
/// The hints over a screen; parameter `id` names it.
pub const OVERLAY: Id = Id::new_static("help.overlay");
/// What *Show hints again* opens: it has them shown again, and says so.
const AGAIN: Id = Id::new_static("help.again");

const SECTION: &str = "help.section";
static IN_HELP: SettingExtra = SettingExtra::new().section(SECTION);
static AGAIN_EXTRA: SettingExtra = SettingExtra::new().section(SECTION).editor(AGAIN);

static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::fixed_switch(SHOW.static_str(), "help.setting.show", true, groups::SYSTEM)
        .with_extra(&IN_HELP)
        .described("help.setting.show.help"),
    SettingSpec::fixed_text("help.again", "help.setting.again", "", groups::SYSTEM)
        .with_extra(&AGAIN_EXTRA)
        .described("help.setting.again.help"),
];

fn seen_file() -> String {
    format!("{}/seen.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// Whether the hints of `screen` were shown on this device.
pub(crate) fn seen(storage: &dyn Storage, screen: &str) -> bool {
    paperpaw_sdk::read_file(storage, &seen_file(), 64 * 1024)
        .ok()
        .is_some_and(|text| {
            core::str::from_utf8(&text).is_ok_and(|t| t.lines().any(|l| l.trim() == screen))
        })
}

/// The hints of `screen` were shown.
pub(crate) fn mark_seen(storage: &dyn Storage, screen: &str) {
    if !seen(storage, screen) {
        let _ = storage.append(&seen_file(), format!("{screen}\n").as_bytes());
    }
}

/// The plugin.
#[derive(Debug, Default)]
pub struct Help;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Help {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Hints", paperpaw_sdk::package_version!())
            .description("Shows what a screen's controls do, the first time it opens.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.on(events::SCREEN_SHOWN, |env, shown, out| {
            if !help::waits(env, shown.screen.as_str()) {
                explain(env, &shown.screen, out);
            }
        })?;
        r.on(help::READY, |env, ready, out| {
            explain(env, &ready.screen, out)
        })?;
        r.add_screen(ScreenSpec::new(
            OVERLAY,
            "help.title",
            Box::new(|env, params| Box::new(overlay::Overlay::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            AGAIN,
            "help.setting.again",
            Box::new(|env, _| Box::new(overlay::Again::new(env))),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Opens the hints over `screen`, where it has some, they are on, and
/// they were not shown yet.
fn explain(env: &Env<'_>, screen: &Id, out: &mut Vec<Command>) {
    if !env.settings.read(&SHOW)
        || screen.namespace() == PLUGIN.as_str()
        || help::hints(env, screen.as_str()).is_empty()
        || seen(env.storage, screen.as_str())
    {
        return;
    }
    let params = Params::new().with(params::ID, screen.as_str());
    out.push(Command::OpenScreen(OVERLAY, params));
}

/// Has every screen's hints shown again.
pub(crate) fn again(storage: &dyn Storage) {
    let _ = storage.remove(&seen_file());
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
