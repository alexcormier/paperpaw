//! Bluetooth, where the kernel uses a radio (the X4 Pro; see
//! `paperpaw_sdk::bluetooth` and ADR 0027): the page of the devices
//! bonded with this one, adding one (a search, then pairing, which the
//! kernel asks the reader to confirm), forgetting one and giving one to a
//! plugin that uses Bluetooth; and when a remote's or a keyboard's keys
//! go to the key maps (Remotes: by hand, while reading or while awake).
//!
//! Another plugin may take its place: the kernel keeps the bonds, with
//! the passwords and keys, the same for any plugin.

#![no_std]

extern crate alloc;

mod page;
mod search;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::bluetooth::{self, BtEvent, DeviceId};
use paperpaw_sdk::builtin::{events, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    ActionSpec, Command, Commands, Env, Id, Manifest, MenuNeed, Params, Plugin, Priority,
    RegisterError, Registrar, ScreenSpec, SettingSpec, StatusItemSpec, StatusValue, Uses,
};

pub const PLUGIN: Id = Id::new_static("bluetooth");
/// The settings group, shown as the Bluetooth page.
pub const GROUP: Id = Id::new_static("bluetooth.group");
/// The Bluetooth page.
pub const PAGE: Id = Id::new_static("bluetooth.page");
/// Opens the Bluetooth page: what a tap on its status item runs.
pub const OPEN: Id = Id::new_static("bluetooth.open");
/// Adding a device: a search, then pairing.
pub const SEARCH: Id = Id::new_static("bluetooth.search");
/// The status item: the Bluetooth sign while a device is connected.
pub const STATUS: Id = Id::new_static("bluetooth.status");
/// The icon of the page and the status item.
const ICON: &str = "bluetooth";
/// What the search finds.
pub(crate) const FOUND: Event<BtEvent> = Event::new("bluetooth.found");
/// What happens with a device being added.
pub(crate) const LINK: Event<BtEvent> = Event::new("bluetooth.link");
/// The answers to forgetting, giving and switching a remote's keys.
pub(crate) const DONE: Event<BtEvent> = Event::new("bluetooth.done");

/// When a remote's or a keyboard's keys go to the key maps.
pub const REMOTES: Setting<Text> = Setting::new("bluetooth.remotes");
/// Only when chosen on the page.
pub const BY_HAND: &str = "bluetooth.remotes.hand";
/// While a book is open.
pub const WHILE_READING: &str = "bluetooth.remotes.reading";
/// Whenever the device is awake.
pub const WHILE_AWAKE: &str = "bluetooth.remotes.awake";
/// The choices of [`REMOTES`] (each its name's key).
pub(crate) const REMOTES_CHOICES: [&str; 3] = [BY_HAND, WHILE_READING, WHILE_AWAKE];

pub(crate) static SETTINGS: [SettingSpec; 1] = [SettingSpec::choice(
    REMOTES.static_str(),
    "bluetooth.remotes",
    &REMOTES_CHOICES,
    BY_HAND,
    GROUP,
)
.description("bluetooth.remotes.help")];

/// What the plugin keeps between its parts: whether a book is open.
#[derive(Debug, Default)]
pub(crate) struct State {
    reading: Cell<bool>,
}

pub(crate) type Shared = Rc<State>;

/// The bonded devices whose keys can go to the key maps.
pub(crate) fn remotes(env: &Env<'_>) -> Vec<DeviceId> {
    env.bluetooth()
        .map(|b| {
            b.bonded()
                .filter(|d| d.kind.can_type())
                .map(|d| d.id)
                .collect()
        })
        .unwrap_or_default()
}

/// Every remote's keys to the key maps (`on`), or no longer.
fn all_remotes(env: &Env<'_>, on: bool, out: &mut Commands) {
    for device in remotes(env) {
        out.push(if on {
            bluetooth::remote_keys(device, &DONE)
        } else {
            bluetooth::remote_keys_off(device, &DONE)
        });
    }
}

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

#[derive(Debug)]
pub struct Bluetooth;

impl Plugin for Bluetooth {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Bluetooth", paperpaw_sdk::package_version!())
            .description("Pairs remotes, keyboards and sensors, and connects them.")
            .how(&HOW)
            .uses(Uses::BLUETOOTH)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::default();
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "bluetooth.title")
                    .order(86)
                    .page(PAGE);
            &SPEC
        })?;
        r.add_icon(GROUP, Cow::Borrowed(ICON))?;
        r.set_menu_need(GROUP, MenuNeed::Bluetooth)?;
        r.add_settings(&SETTINGS)?;
        for spec in &SETTINGS {
            r.set_menu_need(spec.id.clone(), MenuNeed::Bluetooth)?;
        }
        r.add_status_item(
            StatusItemSpec::new(
                STATUS,
                "bluetooth.status.item",
                Box::new(|env, _| {
                    env.bluetooth()
                        .is_some_and(|b| b.bonded().any(|d| d.connected))
                        .then_some(StatusValue::Icon(Cow::Borrowed(ICON)))
                }),
            )
            .tap(OPEN),
        )?;
        r.add_action(ActionSpec::new(OPEN, "bluetooth.title").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                out.push(Command::OpenScreen(PAGE, Params::new()));
            },
        ))?;
        r.add_icon(OPEN, Cow::Borrowed(ICON))?;
        r.add_screen(ScreenSpec::new(
            PAGE,
            "bluetooth.title",
            Box::new(|env, _| Box::new(page::BluetoothPage::new(env))),
        ))?;
        // The Bluetooth manager: it sees every bond and connects remotes.
        r.bind_slot(slots::BLUETOOTH, PAGE, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            SEARCH,
            "bluetooth.add",
            Box::new(|env, _| Box::new(search::Search::new(env))),
        ))?;
        // When remotes connect.
        let s = state.clone();
        r.on(events::BOOK_OPENED, move |env, _, out| {
            s.reading.set(true);
            if env.settings().read(&REMOTES) == WHILE_READING {
                all_remotes(env, true, out);
            }
        })?;
        let s = state.clone();
        r.on(events::BOOK_CLOSED, move |env, _, out| {
            s.reading.set(false);
            if env.settings().read(&REMOTES) == WHILE_READING {
                all_remotes(env, false, out);
            }
        })?;
        for event in [events::STARTED, events::RESUME] {
            let s = state.clone();
            r.on(event, move |env, (), out| {
                let remotes = env.settings().read(&REMOTES);
                let wanted = match remotes {
                    WHILE_AWAKE => true,
                    WHILE_READING => s.reading.get(),
                    _ => false,
                };
                if wanted {
                    all_remotes(env, true, out);
                }
            })?;
        }
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
