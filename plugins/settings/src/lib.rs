//! The settings screens, generated from the settings every plugin declares
//! (`Registrar::add_setting`), so no plugin needs its own settings UI.
//!
//! - The main page lists the setting groups and pages other plugins add
//!   to the `core.settings-pages` menu (e.g. the plugin manager) with
//!   their icons, under the headings of the sections in that menu (by
//!   order), and a switch for advanced settings.
//! - A group page lists its settings with their values and units, each
//!   with its description below it, so nothing has to be tapped (and
//!   changed) to find out what it does; settings that name a section
//!   are listed under its heading.
//! - Choices and languages open a list to pick from.
//!
//! Changes apply immediately and are saved to `quireink.ini` by the
//! kernel.
//!
//! Buttons: move to an item, *confirm* opens a group, toggles a switch,
//! opens a choice or starts editing a number; while editing, left/right
//! (or up/down, page buttons) change it and *confirm*/*back* finish.
//! Touch: tap an item to open, toggle or choose it; numbers show their −
//! and + buttons all the time, a tap on one changes the number at once.
//!
//! The main page is bound to the `core.settings` slot.

#![no_std]

extern crate alloc;

mod chooser;
mod group_list;
mod page;
mod search;
mod secrets;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::ask;
use quireink_sdk::builtin::{actions, events, groups, menus, params, setting_sections, slots};
use quireink_sdk::event::Event;
use quireink_sdk::replies::Typed;
use quireink_sdk::setting::Setting;
use quireink_sdk::ui::{self, ListChange, ListView, UiStyle};
use quireink_sdk::{ActionSpec, MenuEntry};
use quireink_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Level, Manifest, Params,
    Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec,
    SettingKind, SettingSpec, SettingValue, Version,
};

use self::chooser::*;
use self::group_list::*;
use self::page::*;

pub const PLUGIN: Id = Id::new_static("settings");
/// The main page: setting groups.
pub const SCREEN: Id = Id::new_static("settings.list");
/// The settings of one group; parameter `id`.
pub const GROUP_SCREEN: Id = Id::new_static("settings.group");
/// Picks a value of a choice or language setting; parameter `id`.
pub const CHOOSE_SCREEN: Id = Id::new_static("settings.choose");
/// Show advanced settings too.
pub const ADVANCED: Setting<bool> = Setting::new("settings.advanced");
/// Emitted by the keyboard with a text setting's new value (parameters
/// `id` and `text`).
const TEXT_ENTERED: Event<Typed> = Event::new("settings.text-entered");

const ID: &str = "id";
/// A setting to focus on its group's page (parameter of [`GROUP_SCREEN`]).
const FOCUS: &str = "focus";

/// The settings plugin.
#[derive(Debug, Default)]
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Settings"),
            description: Cow::Borrowed(
                "The settings pages, built from the settings of every plugin.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("settings.title"),
            // With a group, e.g. a widget's own settings: that group.
            factory: Box::new(|env, params| {
                match params.get(ID).and_then(|id| Id::parse(id).ok()) {
                    Some(group) => Box::new(GroupPage::new(env, group)) as Box<dyn Screen>,
                    None => Box::new(GroupList::new(env)),
                }
            }),
        })?;
        r.add_screen(ScreenSpec {
            id: GROUP_SCREEN,
            title: Cow::Borrowed("settings.title"),
            factory: Box::new(|env, params| {
                let group = params.get(ID).and_then(|id| Id::parse(id).ok());
                let mut page = GroupPage::new(env, group.unwrap_or(groups::OTHER));
                if let Some(focus) = params.get(FOCUS) {
                    page.focus_on(env, focus);
                }
                Box::new(page)
            }),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOOSE_SCREEN,
            title: Cow::Borrowed("settings.title"),
            factory: Box::new(|env, params| {
                let id = params.get(ID).unwrap_or_default();
                Box::new(Chooser::new(env, id))
            }),
        })?;
        r.add_screen(ScreenSpec {
            id: search::SCREEN,
            title: Cow::Borrowed("settings.search"),
            factory: Box::new(|env, params| Box::new(search::SearchScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: secrets::SCREEN,
            title: Cow::Borrowed("settings.secrets"),
            factory: Box::new(|env, _| Box::new(secrets::SecretList::new(env))),
        })?;
        r.add_screen(ScreenSpec {
            id: secrets::SECRET_SCREEN,
            title: Cow::Borrowed("settings.secrets"),
            factory: Box::new(|env, params| Box::new(secrets::SecretPage::new(env, params))),
        })?;
        r.add_action(ActionSpec::new(secrets::OPEN, "settings.secrets").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(secrets::SCREEN, Params::new()));
            },
        ))?;
        // In the device's part, after the system.
        r.add_menu_entry(MenuEntry {
            menu: menus::SETTINGS,
            item: secrets::OPEN,
            order: 95,
        })?;
        r.add_icon(secrets::OPEN, Cow::Borrowed("lock"))?;
        r.on(secrets::ENTERED, secrets::entered)?;
        r.bind_slot(slots::SETTINGS, SCREEN, Priority::DEFAULT)?;
        r.add_setting(
            SettingSpec::switch(&ADVANCED, "settings.advanced", false)
                .group(groups::SYSTEM)
                .description("settings.advanced.help"),
        )?;
        r.on(TEXT_ENTERED, |env, typed, out| {
            let id = typed.context.get(ID).and_then(|id| Id::parse(id).ok());
            if let Some(id) = id
                && env.settings.get(id.as_str()).is_some()
            {
                out.push(Command::SetSetting(
                    id,
                    SettingValue::Text(Cow::Owned(typed.text.trim().to_string())),
                ));
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Whether a setting is shown with the current level.
fn visible(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // "Only this book" and the like mean nothing outside a scope.
    let scope_only = spec.id == quireink_sdk::builtin::settings::SCOPE_ONLY;
    if scope_only && env.settings.scope().is_none() {
        return false;
    }
    // Not for this device (e.g. reading aloud without a speaker).
    if !env.menus.available(spec.id.as_str()) {
        return false;
    }
    // For developers, once they asked for it; then without the advanced
    // switch.
    if env.settings.group_of(spec) == groups::DEVELOPER {
        return env
            .settings
            .bool(quireink_sdk::builtin::settings::DEVELOPER.as_str());
    }
    spec.level == Level::Basic || env.settings.read(&ADVANCED)
}

/// The header, list area and list for a screen; rows have notes if
/// `noted`.
fn frame(env: &Env<'_>, noted: bool) -> (UiStyle, ui::Header, ListView) {
    let (style, header, area) = page(env);
    let row = if noted { style.noted_row() } else { style.row };
    (style, header, ListView::new(area, row).focus_on_keys(env))
}

/// The style, header and the area below it for the list of a page.
fn page(env: &Env<'_>) -> (UiStyle, ui::Header, Rect) {
    let style = UiStyle::new(env);
    let display = env.device.display;
    let header = ui::Header::new(env, &style, true);
    let top = header.area.height + style.gap as u16;
    let height = display.height.saturating_sub(top);
    (style, header, Rect::new(0, top, display.width, height))
}

/// Moves the focus of `rows` (lists with headings) by the navigation
/// actions; `None` if `action` is another. The first press on a touch
/// screen only shows the focus (`keys`), which taps hide.
fn navigate_rows(rows: &mut ui::Rows, keys: &mut bool, action: &Id) -> Option<ListChange> {
    let forward =
        if *action == actions::UP || *action == actions::LEFT || *action == actions::PREVIOUS {
            Some(false)
        } else if *action == actions::DOWN || *action == actions::RIGHT || *action == actions::NEXT
        {
            Some(true)
        } else {
            None
        };
    let page = if *action == actions::NEXT_PAGE {
        Some(true)
    } else if *action == actions::PREV_PAGE {
        Some(false)
    } else {
        None
    };
    if let Some(forward) = page {
        // Pages turn whether the focus shows or not.
        return Some(rows.turn(forward));
    }
    let forward = forward?;
    if !*keys {
        *keys = true;
        return Some(ListChange::Focus);
    }
    Some(rows.step(forward))
}

/// The page shown in a header: its number and the number of pages.
fn paging(rows: &ui::Rows) -> Option<(usize, usize)> {
    Some(rows.paging())
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
