//! Installing a font: the SDK's [`Installer`] downloads its files into a
//! folder of their own, checks each and has the kernel install the font's
//! plugin once all are there (a font chosen from the version before stays
//! chosen); a download goes on when the screens that started it are
//! closed. Here: which files, and what is said after.

use alloc::vec::Vec;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Digest, Installer, Package, PackageFile, Received};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Env};

use crate::PLUGIN;
use crate::catalog::Font;

/// The answer to a file's download.
pub(crate) const DOWNLOADED: Event<HttpReply> = Event::new("fonts-browser.downloaded");

/// Downloads fonts; its job is kept in this plugin's folder.
pub(crate) const INSTALLER: Installer = Installer::new(PLUGIN, DOWNLOADED);

/// Starts installing `font` (or a newer version of it).
pub(crate) fn start(env: &Env<'_>, font: &Font, out: &mut Vec<Command>) {
    let package = Package::new(font.plugin(), font.name.clone()).files(
        font.files
            .iter()
            .map(|f| {
                PackageFile::new(
                    f.path.clone(),
                    f.url.clone(),
                    f.size,
                    Digest::Sha1(f.sha1.clone()),
                )
            })
            .collect(),
    );
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

/// A file arrived (or did not): said once the font is ready, or could
/// not be downloaded.
pub(crate) fn received(env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
    match INSTALLER.received(env, reply, out) {
        Received::Complete(done) => {
            INSTALLER.install(env, out);
            out.push(Command::Notify(env.i18n.format(
                "fonts-browser.ready",
                &[("name", Arg::Str(&done.name))],
            )));
        }
        Received::Failed(job) => out.push(failed(env, &job.name)),
        Received::Next(_) | Received::Other => {}
        _ => {}
    }
}
