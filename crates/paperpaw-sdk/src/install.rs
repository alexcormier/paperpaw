//! Installing, updating and removing data plugins from the internet, for
//! stores and browsers of fonts and plugins (ADR 0023).
//!
//! A [`Package`] names a data plugin's files, each with its address, size
//! and SHA-1, as a plugin library lists them. An [`Installer`] downloads
//! them one after another into a folder PaperPaw does not load
//! (`plugins/.<id>.part`), checks each by its size and SHA-1, and once all
//! are there has the kernel install it ([`Command::InstallPlugin`]): in
//! place of every version of it before, which stays until the new one is
//! all there, and with what was chosen from it still chosen. Nothing is
//! unpacked and nothing is held whole in memory: each file goes from the
//! network straight to the storage.
//!
//! What is left to do is kept in the plugin's own folder, so a download
//! goes on while none of its screens is open: the plugin hears the
//! answers with [`Registrar::on`](crate::Registrar::on) and hands them to
//! [`Installer::received`].
//!
//! ```
//! use paperpaw_sdk::event::Event;
//! use paperpaw_sdk::install::{Installer, Received};
//! use paperpaw_sdk::{Command, Id};
//!
//! const INSTALLER: Installer =
//!     Installer::new(Id::new_static("my-store"), Event::new("my-store.downloaded"));
//!
//! // In `register`:
//! // r.on(INSTALLER.reply(), |env, reply, out| {
//! //     if let Received::Installed(done) = INSTALLER.received(env, reply, out) {
//! //         out.push(Command::Notify(format!("{} is ready", done.name)));
//! //     }
//! // })?;
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_platform::Storage;

use crate::builtin::PLUGIN_DIR;
use crate::event::Event;
use crate::replies::HttpReply;
use crate::{Command, DATA_FORMATS, Env, HttpRequest, Id, Params, SDK_VERSION, Version};

/// Files of one package at most.
pub const MAX_FILES: usize = 64;
/// The longest path of a file in a plugin's folder.
const MAX_PATH: usize = 100;
/// The longest line of a job kept on the storage.
const MAX_LINE: usize = 2048;

/// Parameters of a download: the job and the file's path in the plugin's
/// folder, to tell answers to downloads of jobs stopped before.
const JOB: &str = "install-job";
const FILE: &str = "install-file";

/// Whether a data plugin made for the SDK `sdk` (e.g. `0.4`) and the data
/// formats `formats` is read by this PaperPaw: its SDK not later than this
/// one, its formats not later than [`DATA_FORMATS`]. A missing SDK (an
/// empty text) passes.
pub fn fits(sdk: &str, formats: u32) -> bool {
    let sdk_fits = sdk.trim().is_empty()
        || Version::parse(sdk)
            .is_some_and(|v| (v.major, v.minor) <= (SDK_VERSION.major, SDK_VERSION.minor));
    sdk_fits && formats <= DATA_FORMATS
}

/// Whether `id` can be a data plugin's id and its folder's name: lower-case
/// letters, digits and `-`.
pub fn is_plugin_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/`
/// between them, none starting with `.`.
pub fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
}

/// One file of a data plugin to download.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageFile {
    /// Where it goes in the plugin's folder, e.g. `i18n/de.ini`.
    pub path: String,
    /// Where it comes from.
    pub url: String,
    pub size: u64,
    /// Its SHA-1, as 40 hex digits.
    pub sha1: String,
}

impl PackageFile {
    /// Whether it stays in the plugin's folder and can be checked.
    pub fn is_valid(&self) -> bool {
        is_safe_path(&self.path)
            && !self.url.is_empty()
            && !self.url.contains(['\t', '\n', '\r'])
            && self.sha1.len() == 40
            && self.sha1.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// A data plugin to install: its id (the folder it gets), a name for
/// messages, its files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Package {
    pub plugin: String,
    pub name: String,
    pub files: Vec<PackageFile>,
}

impl Package {
    /// Whether it can be installed: a fit id, at most [`MAX_FILES`] valid
    /// files, each path once, a `plugin.ini` among them.
    pub fn is_valid(&self) -> bool {
        is_plugin_name(&self.plugin)
            && !self.files.is_empty()
            && self.files.len() <= MAX_FILES
            && self.files.iter().all(PackageFile::is_valid)
            && self.files.iter().any(|f| f.path == "plugin.ini")
            && self
                .files
                .iter()
                .enumerate()
                .all(|(i, f)| self.files[..i].iter().all(|g| g.path != f.path))
    }

    /// Bytes to download.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// An installation going on: which plugin, and how far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub plugin: String,
    pub name: String,
    /// Files downloaded and checked so far, and in all.
    pub done: usize,
    pub total: usize,
}

/// What became of an answer handed to [`Installer::received`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// None of the downloads of this installer's job, e.g. one of a job
    /// stopped before.
    Other,
    /// A file is here; the next one is asked for.
    Next(Progress),
    /// All files are here: the kernel installs the plugin
    /// ([`Command::InstallPlugin`], in `out` already).
    Installed(Progress),
    /// A file did not come or came damaged: what was downloaded is gone,
    /// and the version installed before stays.
    Failed(Progress),
}

/// Downloads and installs data plugins for the plugin `owner`, one at a
/// time; see the [module documentation](self).
#[derive(Debug, Clone)]
pub struct Installer {
    owner: Id,
    reply: Event<HttpReply>,
}

/// The job kept on the storage: what its first line says.
struct Job {
    /// Told apart from the jobs before: one more than the last (see
    /// [`Installer::next_job`]).
    id: u64,
    plugin: String,
    name: String,
    total: usize,
}

impl Job {
    /// Where its files go meanwhile.
    fn staging(&self) -> String {
        staging(&self.plugin)
    }
}

/// The folder a plugin is downloaded into, which PaperPaw does not load.
fn staging(plugin: &str) -> String {
    format!("{PLUGIN_DIR}/.{plugin}.part")
}

impl Installer {
    /// An installer for `owner` (whose folder keeps the job), its answers
    /// coming as `reply`.
    pub const fn new(owner: Id, reply: Event<HttpReply>) -> Self {
        Self { owner, reply }
    }

    /// The event the answers come as: the owner subscribes to it and
    /// hands them to [`received`](Self::received).
    pub fn reply(&self) -> Event<HttpReply> {
        self.reply.clone()
    }

    fn job_path(&self) -> String {
        format!("{}/install.txt", Env::plugin_dir(self.owner.as_str()))
    }

    fn done_path(&self) -> String {
        format!("{}/install.done", Env::plugin_dir(self.owner.as_str()))
    }

    fn count_path(&self) -> String {
        format!("{}/install.count", Env::plugin_dir(self.owner.as_str()))
    }

    /// A number no job of this installer had: the jobs are counted on the
    /// storage, across restarts.
    fn next_job(&self, storage: &dyn Storage) -> Option<u64> {
        let last: u64 = crate::read_file(storage, &self.count_path(), 24)
            .ok()
            .and_then(|text| core::str::from_utf8(&text).ok()?.trim().parse().ok())
            .unwrap_or(0);
        let next = last.wrapping_add(1);
        storage
            .write(&self.count_path(), next.to_string().as_bytes())
            .ok()
            .map(|()| next)
    }

    fn job(&self, storage: &dyn Storage) -> Option<Job> {
        let header = nth_line(storage, &self.job_path(), 0)?;
        let mut parts = header.split('\t');
        Some(Job {
            id: parts.next()?.parse().ok()?,
            plugin: parts.next()?.to_string(),
            name: parts.next()?.to_string(),
            total: parts.next()?.parse().ok()?,
        })
    }

    fn done(&self, storage: &dyn Storage) -> usize {
        crate::read_file(storage, &self.done_path(), 16)
            .ok()
            .and_then(|text| core::str::from_utf8(&text).ok()?.trim().parse().ok())
            .unwrap_or(0)
    }

    /// The `n`th file of the job (from 0).
    fn file(&self, storage: &dyn Storage, n: usize) -> Option<PackageFile> {
        let line = nth_line(storage, &self.job_path(), n + 1)?;
        let mut parts = line.splitn(4, '\t');
        Some(PackageFile {
            path: parts.next()?.to_string(),
            size: parts.next()?.parse().ok()?,
            sha1: parts.next()?.to_string(),
            url: parts.next()?.to_string(),
        })
    }

    /// The installation going on, if any (read from the storage: screens
    /// keep it, and ask again when an answer came).
    pub fn progress(&self, storage: &dyn Storage) -> Option<Progress> {
        let job = self.job(storage)?;
        Some(Progress {
            done: self.done(storage).min(job.total),
            plugin: job.plugin,
            name: job.name,
            total: job.total,
        })
    }

    /// Starts installing `package`, a new plugin or a newer version of
    /// one installed (which stays until the new one is all here); an
    /// installation going on is stopped, and answers to its downloads are
    /// passed over (each job has a number of its own, counted in the
    /// owner's folder). `false` (and nothing done) if the package is not
    /// valid ([`Package::is_valid`]) or the job could not be kept.
    pub fn start(&self, env: &Env<'_>, package: &Package, out: &mut Vec<Command>) -> bool {
        if !package.is_valid() {
            return false;
        }
        let storage = env.storage;
        self.cancel(storage);
        let _ = storage.create_dir(crate::data::DIR);
        let _ = storage.create_dir(&format!("{}/plugins", crate::data::DIR));
        let _ = storage.create_dir(&Env::plugin_dir(self.owner.as_str()));
        let Some(id) = self.next_job(storage) else {
            return false;
        };
        let job = Job {
            id,
            plugin: package.plugin.clone(),
            name: plain(&package.name),
            total: package.files.len(),
        };
        let mut text = format!("{}\t{}\t{}\t{}\n", job.id, job.plugin, job.name, job.total);
        for file in &package.files {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                file.path,
                file.size,
                file.sha1.to_ascii_lowercase(),
                file.url
            ));
        }
        let _ = storage.create_dir(PLUGIN_DIR);
        let staging = job.staging();
        let _ = storage.remove_dir_all(&staging);
        let _ = storage.create_dir(&staging);
        if storage.write(&self.job_path(), text.as_bytes()).is_err() {
            let _ = storage.remove_dir_all(&staging);
            return false;
        }
        self.ask(storage, &job, 0, out);
        true
    }

    /// Asks for the job's file `n`, its folders made first.
    fn ask(&self, storage: &dyn Storage, job: &Job, n: usize, out: &mut Vec<Command>) {
        let Some(file) = self.file(storage, n) else {
            return;
        };
        let staging = job.staging();
        let mut dir = staging.clone();
        let parts: Vec<&str> = file.path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            dir.push('/');
            dir.push_str(part);
            let _ = storage.create_dir(&dir);
        }
        let mut request = HttpRequest::new("GET", &file.url, self.reply.clone());
        request.save_to = Some(format!("{staging}/{}", file.path));
        request.params = Params::new()
            .with(JOB, job.id.to_string())
            .with(FILE, file.path);
        out.push(Command::Http(request));
    }

    /// Stops the installation going on and forgets what it downloaded;
    /// answers to its downloads are passed over.
    pub fn cancel(&self, storage: &dyn Storage) {
        if let Some(job) = self.job(storage) {
            let _ = storage.remove_dir_all(&job.staging());
        }
        let _ = storage.remove(&self.job_path());
        let _ = storage.remove(&self.done_path());
    }

    /// Removes the data plugin `plugin` ([`Command::RemovePlugin`]),
    /// stopping its installation if one is going on.
    pub fn remove(&self, env: &Env<'_>, plugin: &Id, out: &mut Vec<Command>) {
        if self
            .job(env.storage)
            .is_some_and(|job| job.plugin == plugin.as_str())
        {
            self.cancel(env.storage);
        }
        out.push(Command::RemovePlugin(plugin.clone()));
    }

    /// An answer to one of the downloads: the file checked, then the next
    /// one asked for, or the plugin installed once all are there.
    pub fn received(&self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) -> Received {
        let storage = env.storage;
        let Some(job) = self.job(storage) else {
            return Received::Other;
        };
        let done = self.done(storage);
        let Some(file) = self.file(storage, done) else {
            return Received::Other;
        };
        // An answer to a download of a job stopped before, or to one asked
        // for twice.
        let job_id = job.id.to_string();
        if reply.context.get(JOB) != Some(job_id.as_str())
            || reply.context.get(FILE) != Some(file.path.as_str())
        {
            return Received::Other;
        }
        let mut progress = Progress {
            plugin: job.plugin.clone(),
            name: job.name.clone(),
            done,
            total: job.total,
        };
        let saved = format!("{}/{}", job.staging(), file.path);
        let whole = (200..300).contains(&reply.status)
            && storage.size(&saved).ok() == Some(file.size)
            && sha1_of(storage, &saved).as_deref() == Some(file.sha1.as_str());
        if !whole {
            self.cancel(storage);
            return Received::Failed(progress);
        }
        progress.done = done + 1;
        if progress.done < job.total {
            let _ = storage.write(&self.done_path(), progress.done.to_string().as_bytes());
            self.ask(storage, &job, progress.done, out);
            return Received::Next(progress);
        }
        // All here: the kernel puts it in place of the version before.
        let _ = storage.remove(&self.job_path());
        let _ = storage.remove(&self.done_path());
        match Id::parse(&job.plugin) {
            Ok(id) => {
                out.push(Command::InstallPlugin(id, job.staging()));
                Received::Installed(progress)
            }
            Err(_) => {
                let _ = storage.remove_dir_all(&job.staging());
                Received::Failed(progress)
            }
        }
    }
}

/// `text` on one line without tabs, for the job's first line.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Line `n` (from 0) of the text file at `path`, read in pieces.
fn nth_line(storage: &dyn Storage, path: &str, n: usize) -> Option<String> {
    let mut buf = [0u8; 256];
    let mut at = 0u64;
    let mut line = 0;
    let mut text = Vec::new();
    loop {
        let read = storage.read_at(path, at, &mut buf).ok()?;
        if read == 0 {
            break;
        }
        at += read as u64;
        for &byte in &buf[..read] {
            if byte == b'\n' {
                if line == n {
                    return String::from_utf8(text).ok();
                }
                line += 1;
            } else if line == n {
                if text.len() >= MAX_LINE {
                    return None;
                }
                text.push(byte);
            }
        }
    }
    (line == n && !text.is_empty())
        .then(|| String::from_utf8(text).ok())
        .flatten()
}

/// The SHA-1 of a file as hex, read in pieces.
fn sha1_of(storage: &dyn Storage, path: &str) -> Option<String> {
    let mut sha = paperpaw_digest::sha1::Sha1::new();
    let mut buf = [0u8; 512];
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn file(path: &str) -> PackageFile {
        PackageFile {
            path: path.into(),
            url: format!("https://x/{path}"),
            size: 3,
            sha1: "a".repeat(40),
        }
    }

    #[test]
    fn paths_stay_in_the_folder() {
        assert!(is_safe_path("plugin.ini"));
        assert!(is_safe_path("i18n/de.ini"));
        for bad in [
            "", "../x", "a/../b", "/abs", "a//b", ".hidden", "a/.b", "a\\b", "a b",
        ] {
            assert!(!is_safe_path(bad), "{bad}");
        }
        assert!(is_plugin_name("airy-theme"));
        for bad in ["", "Airy", "a_b", "../x", "-x", "a.b"] {
            assert!(!is_plugin_name(bad), "{bad}");
        }
    }

    #[test]
    fn packages_are_checked() {
        let mut package = Package {
            plugin: "airy".into(),
            name: "Airy".into(),
            files: alloc::vec![file("plugin.ini"), file("i18n/de.ini")],
        };
        assert!(package.is_valid());
        assert_eq!(package.size(), 6);
        package.files.push(file("i18n/de.ini"));
        assert!(!package.is_valid(), "a path twice");
        package.files.pop();
        package.files[1].sha1 = "xyz".into();
        assert!(!package.is_valid(), "no SHA-1");
        package.files.remove(1);
        package.files[0].path = "theme.ini".into();
        assert!(!package.is_valid(), "no plugin.ini");
    }

    #[test]
    fn what_fits() {
        assert!(fits("", 1));
        assert!(fits("0.3", 1));
        assert!(fits(
            &format!("{}.{}", SDK_VERSION.major, SDK_VERSION.minor),
            DATA_FORMATS
        ));
        assert!(!fits(
            &format!("{}.{}", SDK_VERSION.major, SDK_VERSION.minor + 1),
            1
        ));
        assert!(!fits("1.0", 1));
        assert!(!fits("0.3", DATA_FORMATS + 1));
        assert!(!fits("new", 1));
    }

    #[test]
    fn lines_are_read_in_pieces() {
        let long = "x".repeat(700);
        let text = format!("first\n{long}\nthird\nlast");
        let storage = MemoryStorage::new().with("f.txt", text);
        assert_eq!(nth_line(&storage, "f.txt", 0).as_deref(), Some("first"));
        assert_eq!(nth_line(&storage, "f.txt", 1), Some(long));
        assert_eq!(nth_line(&storage, "f.txt", 2).as_deref(), Some("third"));
        assert_eq!(nth_line(&storage, "f.txt", 3).as_deref(), Some("last"));
        assert_eq!(nth_line(&storage, "f.txt", 4), None);
        assert_eq!(nth_line(&storage, "missing", 0), None);
    }

    #[test]
    fn jobs_are_counted_across_restarts() {
        let storage = MemoryStorage::new();
        let installer = Installer::new(Id::new_static("store"), Event::new("store.downloaded"));
        assert_eq!(installer.next_job(&storage), Some(1));
        assert_eq!(installer.next_job(&storage), Some(2));
        // A new start: the count is on the storage, not in memory.
        let installer = Installer::new(Id::new_static("store"), Event::new("store.downloaded"));
        assert_eq!(installer.next_job(&storage), Some(3));
        assert!(storage.exists(".paperpaw/plugins/store/install.count"));
    }
}
