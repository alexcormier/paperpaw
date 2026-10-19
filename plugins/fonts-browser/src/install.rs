//! Installing a font: the SDK's [`Installer`] downloads its files into a
//! folder of their own, checks each and loads the font's plugin once all
//! are there; a download goes on when the screens that started it are
//! closed. Here: which files, and what is said and chosen after.

use alloc::vec::Vec;

use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Installer, Package, PackageFile, Progress, Received};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Env, Id, SettingValue, Storage};

use crate::PLUGIN;
use crate::catalog::Font;

/// The answer to a file's download.
pub(crate) const DOWNLOADED: Event<HttpReply> = Event::new("fonts-browser.downloaded");

/// Downloads fonts; its job is kept in this plugin's folder.
pub(crate) const INSTALLER: Installer = Installer::new(PLUGIN, DOWNLOADED);

/// The installation going on, if any.
pub(crate) fn current(storage: &dyn Storage) -> Option<Progress> {
    INSTALLER.progress(storage)
}

/// Stops the installation going on and forgets what it downloaded.
pub(crate) fn cancel(storage: &dyn Storage) {
    INSTALLER.cancel(storage);
}

/// Starts installing `font` (or a newer version of it).
pub(crate) fn start(env: &Env<'_>, font: &Font, out: &mut Vec<Command>) {
    let package = Package {
        plugin: font.plugin(),
        name: font.name.clone(),
        files: font
            .files(env)
            .iter()
            .map(|f| PackageFile {
                path: f.path.clone(),
                url: f.url.clone(),
                size: f.size,
                sha1: f.sha1.clone(),
            })
            .collect(),
    };
    if !INSTALLER.start(env, &package, out) {
        out.push(failed(env, &font.name));
    }
}

fn failed(env: &Env<'_>, name: &str) -> Command {
    Command::Notify(
        env.i18n
            .format("fonts-browser.failed", &[("name", Arg::Str(name))]),
    )
}

/// A file arrived (or did not): once the font is installed, it is chosen
/// again where it was.
pub(crate) fn received(env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
    match INSTALLER.received(env, reply, out) {
        Received::Installed(done) => {
            // The version before gives way, and the chosen font with it.
            let chosen: Vec<Id> = [settings::READING_FONT, settings::STATUS_FONT]
                .into_iter()
                .filter(|s| env.settings.text(s.as_str()) == done.name)
                .map(|s| s.id().clone())
                .collect();
            for setting in chosen {
                out.push(Command::SetSetting(
                    setting,
                    SettingValue::Text(done.name.clone().into()),
                ));
            }
            out.push(Command::Notify(env.i18n.format(
                "fonts-browser.ready",
                &[("name", Arg::Str(&done.name))],
            )));
        }
        Received::Failed(job) => out.push(failed(env, &job.name)),
        Received::Next(_) | Received::Other => {}
    }
}
