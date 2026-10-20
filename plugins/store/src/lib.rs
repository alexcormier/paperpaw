//! The plugin store: the plugins of a plugin library (paperpaw.org's by
//! default) for this device, by group as the website shows them, with a
//! search, each with a page to install, update or remove it (ADR 0023).
//! Fonts are one more entry, which opens the fonts browser where it is
//! installed.
//!
//! The list is read as it streams into an index kept on the storage, so
//! it may be as long as it likes; a plugin's files go straight to the
//! storage, one after another, each checked ([`paperpaw_sdk::install`]).
//! Plugins with code are listed where PaperPaw loads them and a build
//! fits; their code is checked whole and signed by its author (`trust`,
//! ADR 0024). Built-in plugins are not listed, nor a plugin that would
//! take the place of one: those come with the firmware.

#![no_std]

extern crate alloc;

mod catalog;
mod list;
mod page;
mod queue;
mod room;
mod rows;
mod setup;
mod shots;
mod source;
mod trust;
mod view;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{events, groups, menus};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Installer, Progress, Received};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::choice::Choice;
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, SDK_VERSION, ScreenSpec, SettingSpec, Version,
};
use trust::Verdict;

pub const PLUGIN: Id = Id::new_static("store");
/// Opens the store.
pub const OPEN: Id = Id::new_static("store.open");
const LIST_SCREEN: Id = Id::new_static("store.list");
const PAGE_SCREEN: Id = Id::new_static("store.plugin");
const VIEW_SCREEN: Id = Id::new_static("store.view");
const SOURCE_SCREEN: Id = Id::new_static("store.source");
const SHOTS_SCREEN: Id = Id::new_static("store.shots");
const CHOICE: Id = Id::new_static("store.choice");
/// The fonts browser's list, offered as the entry *Fonts*.
pub(crate) const FONTS: Id = Id::new_static("fonts-browser.open");
/// The plugin manager's list of other places to get plugins.
const MANAGER_MORE: Id = Id::new_static("plugin-manager.more");

/// Where the list of plugins comes from.
pub const CATALOG: Setting<paperpaw_sdk::setting::Text> = Setting::new("store.catalog");

/// The plugin library's address, for developers and libraries of one's
/// own.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_text(
    CATALOG.static_str(),
    "store.setting.catalog",
    "https://paperpaw.org/api/v1/plugins.json",
    groups::DEVELOPER,
)
.described("store.setting.catalog.help")];

/// The answer to a file's download.
const DOWNLOADED: Event<HttpReply> = Event::new("store.downloaded");

/// Installs plugins; its job is kept in this plugin's folder.
pub(crate) const INSTALLER: Installer = Installer::new(PLUGIN, DOWNLOADED);

/// The plugin store.
#[derive(Debug, Default)]
pub struct Store;

impl Plugin for Store {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("More plugins"),
            description: Cow::Borrowed(
                "Finds plugins for this device on the internet and installs, updates and removes them.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec {
            id: LIST_SCREEN,
            title: Cow::Borrowed("store.title"),
            factory: Box::new(|env, _| Box::new(list::ListScreen::new(env))),
        })?;
        r.add_screen(ScreenSpec {
            id: PAGE_SCREEN,
            title: Cow::Borrowed("store.title"),
            factory: Box::new(|env, params| Box::new(page::PageScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: VIEW_SCREEN,
            title: Cow::Borrowed("store.title"),
            factory: Box::new(|env, params| Box::new(view::ViewScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: SOURCE_SCREEN,
            title: Cow::Borrowed("store.source"),
            factory: Box::new(|env, params| Box::new(source::SourceScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: SHOTS_SCREEN,
            title: Cow::Borrowed("store.shots"),
            factory: Box::new(|env, params| Box::new(shots::ShotsScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOICE,
            title: Cow::Borrowed("store.title"),
            factory: Box::new(|env, params| Box::new(Choice::new(env, params))),
        })?;
        r.add_action(ActionSpec::new(OPEN, "store.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(LIST_SCREEN, Params::new()));
            },
        ))?;
        // With the tools, and in the plugin manager.
        for (menu, order) in [(menus::TOOLS, 90), (MANAGER_MORE, 10)] {
            r.add_menu_entry(MenuEntry {
                menu,
                item: OPEN,
                order,
            })?;
        }
        r.add_icon(OPEN, Cow::Borrowed("cloud-download"))?;
        // Downloads go on while no screen of the store is open; one cut
        // off by a restart is begun anew from the plugin's page.
        r.on(DOWNLOADED, received)?;
        r.on(queue::FETCHED, queue::fetched)?;
        // The first-run setup's offer of the plugins most readers add.
        setup::register(r)?;
        r.on(events::STARTED, |env, _, _| {
            INSTALLER.cancel(env.storage);
            queue::clear(env);
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// A file arrived (or did not): once all are here, the plugin installed
/// if it may be (see [`trust`]), and said; then the next update waiting
/// in the queue ([`queue`]) has its turn.
fn received(env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
    let mut said = Vec::new();
    let installed = match INSTALLER.received(env, reply, out) {
        Received::Complete(done) => (finish(env, &done, &mut said), done.name),
        Received::Failed(job) => {
            said.push(failed(env, &job.name));
            (false, job.name)
        }
        Received::Next(_) | Received::Other => return,
    };
    // A quiet queue (the first-run setup's) says it all once, at its end:
    // its notices wait, what installs goes on.
    if queue::quiet(env) {
        queue::note(env, &installed.1, installed.0);
        out.extend(
            said.into_iter()
                .filter(|c| !matches!(c, Command::Notify(_))),
        );
    } else {
        out.extend(said);
    }
    queue::next(env, out);
}

/// Installs the plugin all of whose files are here, if it may be, and
/// says so; whether it was installed.
fn finish(env: &Env<'_>, done: &Progress, out: &mut Vec<Command>) -> bool {
    let name = [("name", Arg::Str(&done.name))];
    let folder = INSTALLER.folder(env.storage).unwrap_or_default();
    let (key, installed) = match trust::verdict(env, &done.plugin, &folder) {
        Verdict::Install { code } => {
            let before = env.plugins.get(&done.plugin).is_some();
            if trust::install(env, code, out).is_none() {
                INSTALLER.cancel(env.storage);
                out.push(failed(env, &done.name));
                return false;
            }
            let key = match (code.is_some(), before) {
                (true, true) => "store.updated-code",
                (true, false) => "store.installed-code",
                (false, true) => "store.updated",
                (false, false) => "store.installed",
            };
            (key, true)
        }
        // It waits on its page for the reader's word.
        Verdict::OtherAuthor { .. } => ("store.other-author.notice", false),
        Verdict::Refused(refusal) => {
            INSTALLER.cancel(env.storage);
            (refusal.key(), false)
        }
    };
    out.push(Command::Notify(env.i18n.format(key, &name)));
    installed
}

fn failed(env: &Env<'_>, name: &str) -> Command {
    Command::Notify(env.i18n.format("store.failed", &[("name", Arg::Str(name))]))
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
