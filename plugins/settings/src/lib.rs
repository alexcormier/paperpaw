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
//! - Each page ends with resetting its settings to their defaults, the
//!   main page with resetting all of them (but the language); both ask
//!   first.
//!
//! Changes apply immediately and are saved to `paperpaw.ini` by the
//! kernel.
//!
//! - On touch screens, numbers marked as sliders (`SettingExtra::slider`)
//!   show one below their title (`ui::slider`); with buttons alone they
//!   keep their compact row with − and +.
//!
//! Buttons: move to an item, *confirm* opens a group, toggles a switch,
//! opens a choice or starts changing a number; then left/right (or
//! up/down, page buttons) change it, shown at once but not kept yet,
//! *confirm* keeps it and *back* goes back to the value before.
//! Touch: tap an item to open, toggle or choose it; numbers show their −
//! and + buttons all the time, a tap on one changes the number at once;
//! a slider's knob is dragged, and kept where the finger is lifted.
//!
//! The main page is bound to the `core.settings` slot.

#![no_std]

extern crate alloc;

mod chooser;
mod group_list;
mod page;
mod search;
mod secrets;
mod themes;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, groups, menus, params, setting_sections, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::choice::{self, Choice};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::widgets::Widget;
use paperpaw_sdk::{ActionSpec, MenuEntry};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Level, Manifest,
    Params, Plugin, PointerEvent, PointerPhase, Priority, Rect, RefreshMode, RegisterError,
    Registrar, Screen, ScreenSpec, SettingKind, SettingSpec, SettingValue, Version,
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

/// Asks before settings are reset.
const CHOICE: Id = Id::new_static("settings.choice");
/// The answer: the group reset (its id; empty for all) and the option.
const CHOSEN: Event<choice::Picked> = Event::new("settings.chosen");

const ID: &str = "id";
/// A setting to focus on its group's page (parameter of [`SCREEN`] and
/// [`GROUP_SCREEN`]).
const FOCUS: &str = paperpaw_sdk::builtin::params::FOCUS;

/// The settings plugin.
#[derive(Debug, Default)]
pub struct SettingsPlugin;

/// Whether advanced settings show.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_switch(
    ADVANCED.static_str(),
    "settings.advanced",
    false,
    groups::SYSTEM,
)
.described("settings.advanced.help")];

impl Plugin for SettingsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Settings", Version::new(0, 1, 0))
            .description("The settings pages, built from the settings of every plugin.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "settings.title",
            Box::new(|env, params| {
                let group = params.get(ID).and_then(|id| Id::parse(id).ok());
                // A setting to focus: on its page, unless a group is named.
                let focus = params.get(FOCUS).and_then(|id| env.settings.spec(id));
                let group = group.or_else(|| focus.map(|spec| env.settings.group_of(spec)));
                match group {
                    Some(group) => {
                        let mut page = GroupPage::new(env, group);
                        if let Some(spec) = focus {
                            page.focus_on(env, spec.id.as_str());
                        }
                        Box::new(page) as Box<dyn Screen>
                    }
                    None => Box::new(GroupList::new(env)),
                }
            }),
        ))?;
        r.add_screen(ScreenSpec::new(
            GROUP_SCREEN,
            "settings.title",
            Box::new(|env, params| {
                let group = params.get(ID).and_then(|id| Id::parse(id).ok());
                let mut page = GroupPage::new(env, group.unwrap_or(groups::OTHER));
                if let Some(focus) = params.get(FOCUS) {
                    page.focus_on(env, focus);
                }
                Box::new(page)
            }),
        ))?;
        r.add_screen(ScreenSpec::new(
            themes::SCREEN,
            "settings.theme",
            Box::new(|env, _| Box::new(themes::Themes::new(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            CHOICE,
            "settings.title",
            Box::new(|env, params| Box::new(Choice::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            CHOOSE_SCREEN,
            "settings.title",
            Box::new(|env, params| {
                let id = params.get(ID).unwrap_or_default();
                Box::new(Chooser::new(env, id))
            }),
        ))?;
        r.add_screen(ScreenSpec::new(
            search::SCREEN,
            "settings.search",
            Box::new(|env, params| Box::new(search::SearchScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            secrets::SCREEN,
            "settings.secrets",
            Box::new(|env, _| Box::new(secrets::SecretList::new(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            secrets::SECRET_SCREEN,
            "settings.secrets",
            Box::new(|env, params| Box::new(secrets::SecretPage::new(env, params))),
        ))?;
        r.add_action(ActionSpec::new(secrets::OPEN, "settings.secrets").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(secrets::SCREEN, Params::new()));
            },
        ))?;
        // In the device's part, after the system.
        r.add_menu_entry(MenuEntry::new(menus::SETTINGS, secrets::OPEN).order(95))?;
        r.add_icon(secrets::OPEN, Cow::Borrowed("key"))?;
        r.on(secrets::ENTERED, secrets::entered)?;
        r.bind_slot(slots::SETTINGS, SCREEN, Priority::DEFAULT)?;
        r.bind_slot(slots::CHOOSE_VALUE, CHOOSE_SCREEN, Priority::DEFAULT)?;
        r.add_settings(&SETTINGS)?;
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
/// Asks whether to reset the settings of `group` (all for `None`).
fn ask_reset(env: &Env<'_>, group: Option<(&Id, &str)>) -> Command {
    let question = match group {
        Some((_, title)) => env.i18n.format(
            "settings.reset-page.question",
            &[("page", paperpaw_sdk::Arg::Str(title))],
        ),
        None => env.i18n.tr("settings.reset-all.question").to_string(),
    };
    let context = group.map_or("", |(id, _)| id.as_str());
    choice::open(
        CHOICE,
        &CHOSEN,
        &question,
        context,
        "settings.option.",
        &["reset", "cancel"],
    )
}

/// Resets the settings `reset` chose: of its group, or all but the
/// language.
fn reset(env: &Env<'_>, picked: &choice::Picked, cx: &mut Cx<'_, '_>) {
    if picked.option != "reset" {
        return;
    }
    let group = Id::parse(&picked.path).ok();
    // A placement's: what it set goes, the widget's own values apply.
    if let Some(group) = group.as_ref().filter(|g| g.as_str().contains('@')) {
        for value in env.settings.placement_values(group.as_str()) {
            cx.command(Command::ResetSetting(value));
        }
        cx.command(Command::Notify(
            env.i18n.tr("settings.reset-done").to_string(),
        ));
        return;
    }
    let language = paperpaw_sdk::builtin::settings::LANGUAGE;
    for spec in env.settings.specs() {
        let chosen = match &group {
            Some(group) => env.settings.group_of(spec) == *group && visible(env, spec),
            None => spec.id != *language.id(),
        };
        if chosen {
            cx.command(Command::ResetSetting(spec.id.clone()));
        }
    }
    cx.command(Command::Notify(
        env.i18n.tr("settings.reset-done").to_string(),
    ));
}

fn visible(env: &Env<'_>, spec: &SettingSpec) -> bool {
    // "Only this book" and the like mean nothing outside a scope.
    let scope_only = spec.id == paperpaw_sdk::builtin::settings::SCOPE_ONLY;
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
            .bool(paperpaw_sdk::builtin::settings::DEVELOPER.as_str());
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

/// Shows the values kept again, if values were shown without being kept.
fn end_preview(env: &Env<'_>, cx: &mut Cx<'_, '_>) {
    if env.settings.previewing() {
        cx.command(Command::PreviewSettings(Vec::new()));
    }
}

/// Whether a page is covered by a full screen and settings changed
/// meanwhile: it is made anew once it shows, not for each change.
#[derive(Debug, Default, Clone, Copy)]
struct Covered {
    hidden: bool,
    stale: bool,
}

impl Covered {
    /// A setting changed: `true` if the page is made anew now. Covered,
    /// it is noted and a timer asked for, which comes once the page shows
    /// again (see the pages' `on_timer`).
    fn changed(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.hidden && !self.stale {
            let now = cx.env().now;
            cx.wake_at(now);
        }
        self.stale |= self.hidden;
        !self.hidden
    }

    fn hide(&mut self) {
        self.hidden = true;
    }

    /// The page shows again: `true` if it is to be made anew.
    fn shown(&mut self) -> bool {
        self.hidden = false;
        core::mem::take(&mut self.stale)
    }
}

/// The page shown in a header: its number and the number of pages.
fn paging(rows: &ui::Rows) -> Option<(usize, usize)> {
    Some(rows.paging())
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
