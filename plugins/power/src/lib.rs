//! Power: which power mode is worn (ADR 0025). The reader chooses
//! Normal, Saving, Offline, or Automatic, which goes by the battery:
//! Saving below one level, Offline below another, Normal while charging.
//! The modes themselves are presets of the kind
//! [`MODE_KIND`](paperpaw_sdk::power::MODE_KIND), worn by the kernel
//! ([`Command::Wear`]); this plugin only decides which, silently.
//!
//! - *Power* in the quick panel: the four as segments, by buttons and by
//!   touch alike.
//! - Settings › System › Power: the mode, Automatic's two levels and
//!   *Normal while charging*.
//! - A status item, *Power mode*: a leaf while saving, Wi-Fi crossed out
//!   while offline; nothing in Normal, so the lines stay calm.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, menus};
use paperpaw_sdk::power::{self, PowerLevel, Wear};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    Command, Env, Id, Manifest, MenuEntry, Plugin, RegisterError, Registrar, SettingGroupSpec,
    SettingSpec, StatusItemSpec, StatusValue, Version,
};

pub const PLUGIN: Id = Id::new_static("power");
const GROUP: Id = Id::new_static("power.group");
/// Which mode: one of [`MODES`].
pub const MODE: Setting<Text> = Setting::new("power.mode");
/// Automatic: Saving below this percentage of charge.
pub const SAVING_BELOW: Setting<i64> = Setting::new("power.saving-below");
/// Automatic: Offline below this percentage.
pub const OFFLINE_BELOW: Setting<i64> = Setting::new("power.offline-below");
/// Automatic: Normal while charging.
pub const NORMAL_WHILE_CHARGING: Setting<bool> = Setting::new("power.normal-while-charging");
const STATUS: Id = Id::new_static("power.status");

/// The choices of [`MODE`], translation keys too.
const AUTOMATIC: &str = "power.mode.auto";
const MODES: [&str; 4] = [
    AUTOMATIC,
    "power.mode.normal",
    "power.mode.saving",
    "power.mode.offline",
];
/// Automatic changes back only this far above a level, so that it does
/// not go to and fro.
const MARGIN: i64 = 5;

static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        MODE.static_str(),
        "power.setting.mode",
        &MODES,
        AUTOMATIC,
        GROUP,
    )
    .described("power.setting.mode.help"),
    SettingSpec::fixed_number(
        SAVING_BELOW.static_str(),
        "power.setting.saving-below",
        5,
        50,
        20,
        GROUP,
    )
    .described("power.setting.saving-below.help"),
    SettingSpec::fixed_number(
        OFFLINE_BELOW.static_str(),
        "power.setting.offline-below",
        0,
        30,
        10,
        GROUP,
    )
    .described("power.setting.offline-below.help"),
    SettingSpec::fixed_switch(
        NORMAL_WHILE_CHARGING.static_str(),
        "power.setting.normal-while-charging",
        true,
        GROUP,
    )
    .described("power.setting.normal-while-charging.help"),
];

/// The mode to wear now, and whether the battery chose it.
pub fn choose(env: &Env<'_>) -> (Id, bool) {
    let built_in = |level: PowerLevel| match level {
        PowerLevel::Normal => power::NORMAL,
        PowerLevel::Saving => power::SAVING,
        PowerLevel::Offline => power::OFFLINE,
    };
    let chosen = env.settings.read(&MODE);
    let level = match chosen {
        "power.mode.normal" => Some(PowerLevel::Normal),
        "power.mode.saving" => Some(PowerLevel::Saving),
        "power.mode.offline" => Some(PowerLevel::Offline),
        _ => None,
    };
    if let Some(level) = level {
        return (built_in(level), false);
    }
    (built_in(automatic(env)), true)
}

/// What Automatic chooses by the battery: Normal without one.
fn automatic(env: &Env<'_>) -> PowerLevel {
    let Some(battery) = env.status.battery else {
        return PowerLevel::Normal;
    };
    if battery.charging && env.settings.read(&NORMAL_WHILE_CHARGING) {
        return PowerLevel::Normal;
    }
    let percent = i64::from(battery.percent);
    // Wearing one already: kept until the charge is clearly above it.
    let now = env.power.level;
    let below = |level: &Setting<i64>, current: PowerLevel| {
        let margin = if now >= current { MARGIN } else { 0 };
        percent < env.settings.read(level) + margin
    };
    if below(&OFFLINE_BELOW, PowerLevel::Offline) {
        PowerLevel::Offline
    } else if below(&SAVING_BELOW, PowerLevel::Saving) {
        PowerLevel::Saving
    } else {
        PowerLevel::Normal
    }
}

/// Wears the mode chosen now, if another is worn.
fn wear(env: &Env<'_>, out: &mut Vec<Command>) {
    let (mode, automatic) = choose(env);
    if env.power.mode == mode && env.power.automatic == automatic {
        return;
    }
    let wear = Wear::new(mode);
    out.push(Command::Wear(if automatic {
        wear.automatic()
    } else {
        wear
    }));
}

#[derive(Debug)]
pub struct Power;

impl Plugin for Power {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Power", Version::new(0, 1, 0))
            .description(
                "Chooses the power mode, by hand or by the battery: Normal, Saving, Offline.",
            )
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting_group(SettingGroupSpec::new(GROUP, "power.title").order(86))?;
        r.add_icon(GROUP, Cow::Borrowed("leaf"))?;
        r.add_settings(&SETTINGS)?;
        // The mode in the quick panel, as segments.
        r.add_menu_entry(MenuEntry::new(menus::QUICK, MODE.id().clone()).order(30))?;
        r.add_status_item(StatusItemSpec::new(
            STATUS,
            "power.status.item",
            Box::new(|env, _| {
                let icon = match env.power.level {
                    PowerLevel::Normal => return None,
                    PowerLevel::Saving => "leaf",
                    PowerLevel::Offline => "wifi-off",
                };
                Some(StatusValue::Icon(Cow::Borrowed(icon)))
            }),
        ))?;
        // Silently: the mode changes, only the status mark says so.
        for event in [
            events::STARTED.id(),
            events::STATUS_CHANGED.id(),
            events::RESUME.id(),
        ] {
            r.subscribe(event.clone(), Box::new(|env, _, out| wear(env, out)))?;
        }
        r.on(events::SETTING_CHANGED, |env, changed, out| {
            if changed.id.namespace() == PLUGIN.as_str() {
                wear(env, out);
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
