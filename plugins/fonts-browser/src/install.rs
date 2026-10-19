//! Installing a font: its files downloaded one after another into a
//! folder of their own (`plugins/.font-….part`, which PaperPaw does not
//! load), each checked by its size and SHA-1; once all are there, the
//! folder becomes the font's plugin and is loaded at once. What is left
//! to do is kept in a file, so a download goes on when the screens that
//! started it are closed.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{PLUGIN_DIR, settings};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{Arg, Command, Env, HttpRequest, Id, Params, SettingValue, Storage};

use crate::PLUGIN;
use crate::catalog::Font;

/// The answer to a file's download.
pub(crate) const DOWNLOADED: Event<HttpReply> = Event::new("fonts-browser.downloaded");

/// Parameter of a download: the file's path in the font's folder.
const FILE: &str = "file";

/// What is left to do (see [`Job`]).
fn job_path() -> String {
    format!("{}/install.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// An installation going on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Job {
    /// The font's plugin, e.g. `font-fredoka`, and its name (that of its
    /// family).
    pub plugin: String,
    pub name: String,
    /// Where its files go meanwhile, and its folder after.
    pub staging: String,
    pub target: String,
    /// Files in all.
    pub total: usize,
    /// The files still to download: path, size, SHA-1, address.
    pub left: Vec<(String, u64, String, String)>,
}

impl Job {
    fn encode(&self) -> String {
        let mut out = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            self.plugin, self.name, self.staging, self.target, self.total
        );
        for (path, size, sha1, url) in &self.left {
            out.push_str(&format!("{path}\t{size}\t{sha1}\t{url}\n"));
        }
        out
    }

    fn decode(text: &str) -> Option<Job> {
        let mut lines = text.lines();
        let mut job = Job {
            plugin: lines.next()?.to_string(),
            name: lines.next()?.to_string(),
            staging: lines.next()?.to_string(),
            target: lines.next()?.to_string(),
            total: lines.next()?.parse().ok()?,
            left: Vec::new(),
        };
        for line in lines.filter(|l| !l.is_empty()) {
            let mut parts = line.splitn(4, '\t');
            let path = parts.next()?.to_string();
            let size = parts.next()?.parse().ok()?;
            let sha1 = parts.next()?.to_string();
            let url = parts.next()?.to_string();
            job.left.push((path, size, sha1, url));
        }
        Some(job)
    }

    /// Files downloaded so far.
    pub(crate) fn done(&self) -> usize {
        self.total.saturating_sub(self.left.len())
    }
}

/// The installation going on, if any.
pub(crate) fn current(storage: &dyn Storage) -> Option<Job> {
    let text = paperpaw_sdk::read_file(storage, &job_path(), 16 * 1024).ok()?;
    Job::decode(core::str::from_utf8(&text).ok()?)
}

fn save(storage: &dyn Storage, job: &Job) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&job_path(), job.encode().as_bytes());
}

/// Stops the installation going on and forgets what it downloaded.
pub(crate) fn cancel(storage: &dyn Storage) {
    if let Some(job) = current(storage) {
        let _ = storage.remove_dir_all(&job.staging);
    }
    let _ = storage.remove(&job_path());
}

/// A folder in the plugins' folder not taken yet, from `name`.
fn free_folder(storage: &dyn Storage, name: &str) -> String {
    let taken = |dir: &str| {
        let mut any = false;
        let _ = storage.list(dir, &mut |_| any = true);
        any
    };
    let first = format!("{PLUGIN_DIR}/{name}");
    if !taken(&first) {
        return first;
    }
    (2..100)
        .map(|n| format!("{PLUGIN_DIR}/{name}-{n}"))
        .find(|dir| !taken(dir))
        .unwrap_or(first)
}

/// Starts installing `font` (or a newer version of it).
pub(crate) fn start(env: &Env<'_>, font: &Font, out: &mut Vec<Command>) {
    let storage = env.storage;
    cancel(storage);
    let plugin = font.plugin();
    let staging = format!("{PLUGIN_DIR}/.{plugin}.part");
    let job = Job {
        name: font.name.clone(),
        target: free_folder(storage, &plugin),
        plugin,
        staging,
        total: font.files(env).len(),
        left: font
            .files(env)
            .iter()
            .map(|f| (f.path.clone(), f.size, f.sha1.clone(), f.url.clone()))
            .collect(),
    };
    let _ = storage.create_dir(&job.staging);
    save(storage, &job);
    next(&job, out);
}

/// Asks for the job's next file.
fn next(job: &Job, out: &mut Vec<Command>) {
    let Some((path, _, _, url)) = job.left.first() else {
        return;
    };
    let mut request = HttpRequest::new("GET", url, DOWNLOADED);
    request.save_to = Some(format!("{}/{path}", job.staging));
    request.params = Params::new().with(FILE, path.clone());
    out.push(Command::Http(request));
}

/// The SHA-1 of a file as hex, read in pieces.
fn sha1_of(storage: &dyn Storage, path: &str) -> Option<String> {
    let mut sha = paperpaw_digest::sha1::Sha1::new();
    let mut buf = alloc::vec![0u8; 1024];
    let mut at = 0u64;
    loop {
        let n = storage.read_at(path, at, &mut buf).ok()?;
        if n == 0 {
            return Some(sha.hex());
        }
        sha.update(&buf[..n]);
        at += n as u64;
    }
}

/// A file arrived (or did not): checked, then the next one, or the font
/// installed once all are there.
pub(crate) fn received(env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
    let storage = env.storage;
    let Some(mut job) = current(storage) else {
        return;
    };
    let file = reply.context.get(FILE).unwrap_or_default();
    let Some((path, size, sha1, _)) = job.left.first().cloned() else {
        return;
    };
    // An answer to a download stopped before.
    if path != file {
        return;
    }
    let saved = format!("{}/{path}", job.staging);
    let whole = (200..300).contains(&reply.status)
        && storage.size(&saved).ok() == Some(size)
        && sha1_of(storage, &saved).as_deref() == Some(sha1.as_str());
    if !whole {
        cancel(storage);
        out.push(Command::Notify(env.i18n.format(
            "fonts-browser.failed",
            &[("name", Arg::Str(&job.name))],
        )));
        return;
    }
    job.left.remove(0);
    if !job.left.is_empty() {
        save(storage, &job);
        return next(&job, out);
    }
    finish(env, &job, out);
}

/// All files are there: the folder becomes the font's plugin, in place of
/// the version before, and the font is chosen again where it was.
fn finish(env: &Env<'_>, job: &Job, out: &mut Vec<Command>) {
    let storage = env.storage;
    let _ = storage.remove(&job_path());
    if storage.rename(&job.staging, &job.target).is_err() {
        let _ = storage.remove_dir_all(&job.staging);
        out.push(Command::Notify(env.i18n.format(
            "fonts-browser.failed",
            &[("name", Arg::Str(&job.name))],
        )));
        return;
    }
    let chosen: Vec<Id> = [settings::READING_FONT, settings::STATUS_FONT]
        .into_iter()
        .filter(|s| env.settings.text(s.as_str()) == job.name)
        .map(|s| s.id().clone())
        .collect();
    if let Some(info) = env.plugins.get(&job.plugin) {
        out.push(Command::RemovePlugin(info.manifest.id.clone()));
    }
    out.push(Command::LoadPlugin(job.target.clone()));
    for setting in chosen {
        out.push(Command::SetSetting(
            setting,
            SettingValue::Text(job.name.clone().into()),
        ));
    }
    out.push(Command::Notify(
        env.i18n
            .format("fonts-browser.ready", &[("name", Arg::Str(&job.name))]),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_are_kept_as_text() {
        let job = Job {
            plugin: "font-round".into(),
            name: "Round Sans".into(),
            staging: "plugins/.font-round.part".into(),
            target: "plugins/font-round".into(),
            total: 3,
            left: alloc::vec![
                (
                    "plugin.ini".into(),
                    120,
                    "ab".into(),
                    "https://x/plugin.ini".into()
                ),
                (
                    "fonts/regular.ppfp".into(),
                    9000,
                    "cd".into(),
                    "https://x/r?a=b".into()
                ),
            ],
        };
        let decoded = Job::decode(&job.encode()).unwrap();
        assert_eq!(decoded, job);
        assert_eq!(decoded.done(), 1);
        assert!(Job::decode("font-round\nRound\n").is_none());
    }
}
