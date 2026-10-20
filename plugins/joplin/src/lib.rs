//! Joplin: a book's highlights and notes kept as a note in Joplin, one
//! note per book, updated when its notes are exported again (*Export as
//! Markdown* at the end of a book's bookmarks and highlights), in a
//! notebook named in the settings.
//!
//! The notes go to where Joplin synchronises to, so they reach every
//! device the user runs Joplin on at its next sync, with no computer
//! running: Joplin Cloud, Joplin Server or a WebDAV folder such as
//! Nextcloud (see [`Service`]). *Joplin* in the tools sets up the
//! account; the password is kept with the other passwords and keys (see
//! `paperpaw_sdk::secrets`). What cannot be sent (no connection, Joplin
//! down) waits and is sent later: with the next export, by the action
//! *Send to Joplin* (also for rules), and when Wi-Fi comes back where
//! PaperPaw manages it. Sync targets with Joplin's end-to-end encryption
//! get nothing, rather than notes in plain text.
//!
//! The notes are read and written in pieces and sent from a file, and
//! Joplin's answers are saved to files of which only the end is read, so
//! this works on small devices too. Shipped switched off: switch it on
//! in the plugins.
//!
//! It reads the Markdown the reader writes (as the `readwise` plugin
//! does), so it works with any reader that writes the same and emits
//! `core.notes-exported`.

#![no_std]

extern crate alloc;

mod item;
mod kept;
mod locks;
mod markdown;
mod send;
mod service;
mod setup;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    ActionSpec, Command, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError, Registrar,
    SDK_VERSION, ScreenSpec, SettingExtra, SettingSpec, Version,
};

pub use send::DEFAULT_NOTEBOOK;
pub use service::Service;

use send::{Run, Start};

pub const PLUGIN: Id = Id::new_static("joplin");
/// Opens the set-up.
pub const OPEN: Id = Id::new_static("joplin.open");
/// Sends the notes that wait.
pub const SEND: Id = Id::new_static("joplin.send");
/// Where the notes go ([`Service::key`]), the address (Joplin Server's,
/// or the WebDAV folder), the e-mail or user name, the notebook.
pub const SERVICE: Setting<Text> = Setting::new("joplin.service");
pub const ADDRESS: Setting<Text> = Setting::new("joplin.address");
pub const USER: Setting<Text> = Setting::new("joplin.user");
pub const NOTEBOOK: Setting<Text> = Setting::new("joplin.notebook");
/// The password, among the passwords and keys.
pub const PASSWORD: &str = "joplin.password";
pub(crate) const SCREEN: Id = Id::new_static("joplin.setup");
/// Emitted with a message (`text`) when a sending ends.
pub(crate) const DONE: Event<Finished> = Event::new("joplin.done");

paperpaw_sdk::event_data! {
    /// How a sending ended ([`DONE`]), in words.
    pub(crate) struct Finished {
        pub(crate) text: String,
    }
}

/// The account's settings, all set on the set-up.
static ON_SETUP: SettingExtra = SettingExtra::new().editor(SCREEN);
static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_choice(
        SERVICE.static_str(),
        "joplin.service",
        &Service::KEYS,
        Service::KEYS[0],
        groups::LIBRARY,
    )
    .with_extra(&ON_SETUP)
    .described("joplin.service.help"),
    SettingSpec::fixed_text(ADDRESS.static_str(), "joplin.address", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .advanced()
        .described("joplin.address.help"),
    SettingSpec::fixed_text(USER.static_str(), "joplin.user", "", groups::LIBRARY)
        .with_extra(&ON_SETUP)
        .advanced()
        .described("joplin.user.help"),
    SettingSpec::fixed_text(
        NOTEBOOK.static_str(),
        "joplin.notebook",
        DEFAULT_NOTEBOOK,
        groups::LIBRARY,
    )
    .with_extra(&ON_SETUP)
    .advanced()
    .described("joplin.notebook.help"),
];

/// The Joplin plugin.
#[derive(Debug, Default)]
pub struct Joplin;

impl Plugin for Joplin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Joplin"),
            description: Cow::Borrowed("Keeps the highlights and notes of each book in Joplin."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NETWORK,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let run: Rc<RefCell<Run>> = Rc::default();
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("joplin.title"),
            factory: Box::new(|env, _| Box::new(setup::SetupScreen::new(env))),
        })?;
        r.add_action(ActionSpec::new(OPEN, "joplin.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            },
        ))?;
        let state = run.clone();
        r.add_action(ActionSpec::new(SEND, "joplin.send").handler(
            move |env: &Env<'_>, out: &mut Vec<Command>| {
                state.borrow_mut().start(env, Start::Action, out);
            },
        ))?;
        r.add_menu_entry(MenuEntry {
            menu: menus::TOOLS,
            item: OPEN,
            order: 49,
        })?;
        r.add_icon(OPEN, Cow::Borrowed("notebook"))?;
        r.add_icon(SEND, Cow::Borrowed("cloud-upload"))?;

        // Any reader's notes, once exported as Markdown.
        let state = run.clone();
        r.on(events::NOTES_EXPORTED, move |env, exported, out| {
            if kept::add_waiting(env.storage, &exported.path).is_ok() {
                state.borrow_mut().start(env, Start::Export, out);
            }
        })?;
        let state = run.clone();
        r.on(events::WIFI_CHANGED, move |env, _, out| {
            if env.wifi.is_some_and(|w| w.state.is_online()) {
                state.borrow_mut().start(env, Start::Retry, out);
            }
        })?;
        let state = run.clone();
        r.on(events::SETTING_CHANGED, move |_, changed, _| {
            if [SERVICE.as_str(), ADDRESS.as_str(), USER.as_str()].contains(&changed.id.as_str()) {
                state.borrow_mut().account_changed();
            }
        })?;
        let state = run.clone();
        r.on(setup::ENTERED, move |_, typed, out| {
            if setup::entered(typed, out) {
                state.borrow_mut().account_changed();
            }
        })?;
        type Step = fn(&mut Run, &Env<'_>, &HttpReply, &mut Vec<Command>);
        let steps: [(Event<HttpReply>, Step); 7] = [
            (send::SIGNED_IN, Run::signed_in),
            (send::CHECKED, Run::checked),
            (send::LOCKS, Run::locks_listed),
            (send::NOTEBOOK_FOUND, Run::notebook_found),
            (send::NOTEBOOK_MADE, Run::notebook_made),
            (send::NOTE_FOUND, Run::note_found),
            (send::NOTE_SENT, Run::note_sent),
        ];
        for (event, step) in steps {
            let state = run.clone();
            r.on(event, move |env, reply, out| {
                step(&mut state.borrow_mut(), env, reply, out);
            })?;
        }
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
}
