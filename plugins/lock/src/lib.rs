//! The screen lock: a PIN, a pattern or a password asked for when
//! QuireInk starts and when the device wakes up, before anything else can
//! be used. Off by default.
//!
//! - Set up under Settings › System › *Screen lock*: the code in use is
//!   asked for first, then the kind (off, PIN, pattern, password) and the
//!   new code twice. Patterns are offered on touch screens.
//! - The code is kept as a salted hash among the passwords and keys,
//!   never as it is. Like those, it keeps the device from casual use, not
//!   from someone who takes its files.
//! - Locked, the lock screen takes every input: buttons, taps, swipes in
//!   from the edges. A screen that would open above it (a rule, a timer)
//!   closes it, and it opens again at once.
//! - After five wrong tries, the next one waits half a minute.
//! - *Lock now* (`lock.now`) locks at once, from any button, gesture,
//!   menu or rule.

#![no_std]

extern crate alloc;

mod code;
mod screen;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;

use quireink_sdk::builtin::{events, groups};
use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::{
    ActionSpec, Command, Env, Id, Manifest, Params, Plugin, RegisterError, Registrar, SDK_VERSION,
    ScreenSpec, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("lock");
/// Locks at once.
pub const NOW: Id = Id::new_static("lock.now");
/// The kind of lock: `lock.kind.off`, `…pin`, `…pattern` or `…password`
/// (ids, as choices are, translated to their names).
pub const KIND: Setting<Text> = Setting::new("lock.kind");
/// Whether the lock is asked for when the device wakes up.
pub const ON_WAKE: Setting<bool> = Setting::new("lock.on-wake");
const SCREEN: Id = Id::new_static("lock.screen");

/// Whether the device is locked: shared by the plugin's handlers and its
/// screens.
pub(crate) type Locked = Rc<Cell<bool>>;

/// The kinds; the setting stores them after [`KIND_PREFIX`].
pub(crate) const KINDS: [&str; 4] = ["off", "pin", "pattern", "password"];
pub(crate) const KIND_PREFIX: &str = "lock.kind.";
/// The kinds as the setting stores them: [`KIND_PREFIX`] and the kind.
const KIND_VALUES: [&str; KINDS.len()] = [
    "lock.kind.off",
    "lock.kind.pin",
    "lock.kind.pattern",
    "lock.kind.password",
];

/// Opens the lock screen, if a lock is set and it is not open yet.
fn lock(env: &Env<'_>, locked: &Locked, out: &mut Vec<Command>) {
    if locked.get() || code::active(env).is_none() {
        return;
    }
    locked.set(true);
    out.push(Command::OpenScreen(
        SCREEN,
        Params::new().with(screen::MODE, screen::UNLOCK),
    ));
}

/// The screen lock plugin.
#[derive(Debug, Default)]
pub struct Lock;

/// The kind, chosen (and its code set) on the lock's page.
static ON_PAGE: quireink_sdk::SettingExtra = quireink_sdk::SettingExtra::new().editor(SCREEN);

/// The kind of lock, and whether waking up asks for it.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::fixed_choice(
        KIND.static_str(),
        "lock.setting.kind",
        &KIND_VALUES,
        KIND_VALUES[0],
        groups::SYSTEM,
    )
    .described("lock.setting.kind.help")
    .with_extra(&ON_PAGE),
    SettingSpec::fixed_switch(
        ON_WAKE.static_str(),
        "lock.setting.on-wake",
        true,
        groups::SYSTEM,
    )
    .advanced()
    .described("lock.setting.on-wake.help"),
];

impl Plugin for Lock {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Screen lock"),
            description: Cow::Borrowed("A PIN, a pattern or a password when the device wakes up."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let locked: Locked = Rc::new(Cell::new(false));
        let shared = locked.clone();
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("lock.setting.kind"),
            factory: Box::new(move |env, params| {
                Box::new(screen::LockScreen::new(env, params, shared.clone()))
            }),
        })?;
        r.add_settings(&SETTINGS)?;
        let now = locked.clone();
        r.add_action(ActionSpec::new(NOW, "lock.now").handler(
            move |env: &Env<'_>, out: &mut Vec<Command>| {
                lock(env, &now, out);
            },
        ))?;
        r.add_icon(NOW, Cow::Borrowed("lock"))?;
        let start = locked.clone();
        r.on(events::STARTED, move |env, _, out| {
            lock(env, &start, out);
        })?;
        r.on(events::RESUME, move |env, _, out| {
            if env.settings.read(&ON_WAKE) {
                lock(env, &locked, out);
            }
        })?;
        // The first-run setup: a lock, off unless chosen.
        r.add_setup_step(quireink_sdk::setup::SetupStepSpec {
            id: Id::new_static("lock.setup"),
            title: Cow::Borrowed(""),
            help: Cow::Borrowed(""),
            order: 70,
            page: quireink_sdk::setup::SetupPage::Setting(KIND.into()),
            applies: None,
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    /// The setting's values are the kinds after the prefix.
    #[test]
    fn kind_values_follow_kinds() {
        for (value, kind) in super::KIND_VALUES.iter().zip(super::KINDS) {
            assert_eq!(*value, alloc::format!("{}{kind}", super::KIND_PREFIX));
        }
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
