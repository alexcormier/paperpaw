//! The plugin library's list (`plugins.json`, format 1, ADR 0023), read
//! as it streams from the file it was saved to: only the data plugins fit
//! for this device are kept, without their files; a plugin's files are
//! read again when it is installed.
//!
//! ```json
//! {"format": 1, "plugins": [{
//!   "id": "airy-theme", "name": "Airy", "summary": "…",
//!   "i18n": {"de": {"name": "Luftig", "summary": "…"}},
//!   "kind": "data", "group": "look", "author": "…", "licence": "…",
//!   "devices": ["android", "x4", "x4-pro"], "built_in": false,
//!   "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1,
//!     "files": [{"path": "plugin.ini", "url": "…", "size": 412, "sha1": "…"}]}}]}
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_json::stream::{Event, read_file};
use paperpaw_sdk::install::{self, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::web::join_url;
use paperpaw_sdk::{Env, Storage, Version};

use crate::{CATALOG, PLUGIN};

/// The list's format this plugin reads.
const FORMAT: u32 = 1;
/// Plugins kept at most.
const MAX_PLUGINS: usize = 200;
/// The longest summary kept, in bytes, and the longest name and other
/// short texts.
const MAX_SUMMARY: usize = 300;
const MAX_NAME: usize = 80;
/// The longest line of the index read.
const MAX_LINE: usize = 1024;
/// The index is written in pieces of about this many bytes.
const WRITE_AT: usize = 2048;
/// Bytes of a name its place in the list is sorted by.
const SORT_BYTES: usize = 12;

/// The groups, in the order the website shows them.
pub(crate) const GROUPS: [&str; 7] = [
    "reading", "formats", "library", "sync", "home", "look", "system",
];

/// A data plugin the library offers for this device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: String,
    /// In the UI's language where the list has it.
    pub name: String,
    pub summary: String,
    /// Index into [`GROUPS`].
    pub group: usize,
    pub author: String,
    pub licence: String,
    pub version: String,
    /// Bytes to download.
    pub size: u64,
}

impl Entry {
    /// The version installed, if it is.
    pub(crate) fn installed(&self, env: &Env<'_>) -> Option<Version> {
        env.plugins.get(&self.id).map(|info| info.manifest.version)
    }

    /// Whether a newer version than the one installed is offered.
    pub(crate) fn newer(&self, env: &Env<'_>) -> bool {
        match (self.installed(env), Version::parse(&self.version)) {
            (Some(installed), Some(offered)) => offered > installed,
            _ => false,
        }
    }
}

/// What is known of the plugin being read.
#[derive(Default)]
struct Item {
    entry: Entry,
    /// Name and summary in the UI's language.
    name: String,
    summary: String,
    group: String,
    kind: String,
    built_in: bool,
    /// `devices` named any device, and this one.
    devices: bool,
    device: bool,
    sdk: String,
    formats: u32,
    files: usize,
    /// Every file so far fits; a `plugin.ini` among them.
    files_fit: bool,
    manifest: bool,
    /// The file being read.
    file: PackageFile,
    /// The files, kept only when asked for.
    package: Vec<PackageFile>,
}

impl Item {
    fn new() -> Self {
        Self {
            files_fit: true,
            formats: 1,
            ..Self::default()
        }
    }

    /// The file just read is done.
    fn file_done(&mut self, keep: bool) {
        let file = core::mem::take(&mut self.file);
        self.files += 1;
        self.manifest |= file.path == "plugin.ini";
        if !file.is_valid() || self.files > MAX_FILES {
            self.files_fit = false;
        } else if keep {
            self.package.push(file);
        }
    }

    /// Whether this device can install it.
    fn fits(&self) -> bool {
        let entry = &self.entry;
        self.kind == "data"
            && !self.built_in
            && (!self.devices || self.device)
            && install::is_plugin_name(&entry.id)
            && !entry.name.is_empty()
            && Version::parse(&entry.version).is_some()
            && install::fits(&self.sdk, self.formats)
            && self.files > 0
            && self.files_fit
            && self.manifest
    }

    fn finish(mut self) -> Entry {
        if !self.name.is_empty() {
            self.entry.name = self.name;
        }
        if !self.summary.is_empty() {
            self.entry.summary = self.summary;
        }
        self.entry.group = GROUPS
            .iter()
            .position(|g| *g == self.group)
            .unwrap_or(GROUPS.len() - 1);
        self.entry
    }
}

/// Cuts `text` to at most `max` bytes, at a character's end.
fn cut(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Reads the list at `path` for the device `device` (as libraries name
/// it) in the language `language`, handing each plugin that fits to
/// `found`; with `wanted`, also that plugin's files, as the list gives
/// their addresses. `None` if it is no list this plugin reads.
fn read(
    storage: &dyn Storage,
    path: &str,
    device: &str,
    language: &str,
    wanted: Option<&str>,
    found: &mut dyn FnMut(Entry),
) -> Option<Option<Package>> {
    let mut package = None;
    let mut format = None;
    let mut item: Option<Item> = None;
    let mut count = 0;
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Open([list, at]) if list == "plugins" && at == "[]" => item = Some(Item::new()),
        Event::Close([list, at]) if list == "plugins" && at == "[]" => {
            let Some(mut done) = item.take() else {
                return;
            };
            if !done.fits() {
                return;
            }
            if wanted == Some(done.entry.id.as_str()) {
                package = Some(Package {
                    plugin: done.entry.id.clone(),
                    name: String::new(),
                    files: core::mem::take(&mut done.package),
                });
            }
            if count < MAX_PLUGINS {
                count += 1;
                found(done.finish());
            }
        }
        Event::Close([list, at, latest, files, file])
            if list == "plugins"
                && at == "[]"
                && latest == "latest"
                && files == "files"
                && file == "[]" =>
        {
            if let Some(item) = item.as_mut() {
                // Files are kept only for the plugin asked for; its id
                // may come after them.
                item.file_done(wanted.is_some());
            }
        }
        Event::Value([list, at, rest @ ..], value) if list == "plugins" && at == "[]" => {
            let Some(item) = item.as_mut() else {
                return;
            };
            let entry = &mut item.entry;
            match rest {
                [key] => match key.as_str() {
                    "id" => entry.id = cut(value, MAX_NAME),
                    "name" => entry.name = cut(value, MAX_NAME),
                    "summary" => entry.summary = cut(value, MAX_SUMMARY),
                    "author" => entry.author = cut(value, MAX_NAME),
                    "licence" => entry.licence = cut(value, MAX_NAME),
                    "group" => item.group = cut(value, MAX_NAME),
                    "kind" => item.kind = cut(value, MAX_NAME),
                    "built_in" => item.built_in = value == "true",
                    _ => {}
                },
                [devices, at] if devices == "devices" && at == "[]" => {
                    item.devices = true;
                    item.device |= value == device;
                }
                [i18n, lang, key] if i18n == "i18n" && lang == language => match key.as_str() {
                    "name" => item.name = cut(value, MAX_NAME),
                    "summary" => item.summary = cut(value, MAX_SUMMARY),
                    _ => {}
                },
                [latest, key] if latest == "latest" => match key.as_str() {
                    "version" => entry.version = cut(value, MAX_NAME),
                    "sdk" => item.sdk = cut(value, MAX_NAME),
                    "formats" => item.formats = value.parse().unwrap_or(u32::MAX),
                    _ => {}
                },
                [latest, files, at, key]
                    if latest == "latest" && files == "files" && at == "[]" =>
                {
                    let file = &mut item.file;
                    match key.as_str() {
                        "path" => file.path = value.to_string(),
                        "url" => file.url = value.to_string(),
                        "size" => {
                            file.size = value.parse().unwrap_or(0);
                            entry.size += file.size;
                        }
                        "sha1" => file.sha1 = value.to_ascii_lowercase(),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        _ => {}
    });
    (format == Some(FORMAT)).then_some(package)
}

impl Entry {
    /// Its line in the index, tabs and line ends of the list left out.
    fn line(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.group,
            self.size,
            clean(&self.id),
            clean(&self.version),
            clean(&self.author),
            clean(&self.licence),
            clean(&self.name),
            clean(&self.summary)
        )
    }

    fn parse(line: &str) -> Option<Entry> {
        let mut f = line.splitn(8, '\t');
        Some(Entry {
            group: f.next()?.parse().ok()?,
            size: f.next()?.parse().ok()?,
            id: f.next()?.to_string(),
            version: f.next()?.to_string(),
            author: f.next()?.to_string(),
            licence: f.next()?.to_string(),
            name: f.next()?.to_string(),
            summary: f.next()?.to_string(),
        })
    }
}

/// The first line of an index: for which device and language it is.
fn header(device: &str, language: &str) -> String {
    format!("store-index 1\t{device}\t{language}\n")
}

/// Writes the index of the list at `json` to `index`: a line for each
/// plugin that fits, so that nothing of the list is held in memory.
/// `false` (and no index) if it is no list this plugin reads.
fn build(storage: &dyn Storage, json: &str, index: &str, device: &str, language: &str) -> bool {
    let mut text = header(device, language);
    let mut written = storage.write(index, text.as_bytes()).is_ok();
    text.clear();
    let read = read(storage, json, device, language, None, &mut |entry| {
        text.push_str(&entry.line());
        if text.len() > WRITE_AT {
            written &= storage.append(index, text.as_bytes()).is_ok();
            text.clear();
        }
    });
    if !text.is_empty() {
        written &= storage.append(index, text.as_bytes()).is_ok();
    }
    if read.is_none() || !written {
        let _ = storage.remove(index);
        return false;
    }
    true
}

/// The line at `offset` of the file at `path`.
fn line_at(storage: &dyn Storage, path: &str, offset: u32) -> Option<String> {
    let mut buf = [0u8; 256];
    let mut at = u64::from(offset);
    let mut line = Vec::new();
    loop {
        let n = storage.read_at(path, at, &mut buf).ok()?;
        if n == 0 {
            break;
        }
        if let Some(end) = buf[..n].iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&buf[..end]);
            break;
        }
        line.extend_from_slice(&buf[..n]);
        if line.len() > MAX_LINE {
            return None;
        }
        at += n as u64;
    }
    String::from_utf8(line).ok()
}

/// The plugins of an index, by group and name: each one's group and
/// where its line is (8 bytes each; the lines stay on the storage).
#[derive(Debug, Default)]
pub(crate) struct Index {
    path: String,
    rows: Vec<(u8, u32)>,
}

impl Index {
    /// The index at `path` if it is for `device` and `language`.
    fn open(storage: &dyn Storage, path: &str, device: &str, language: &str) -> Option<Index> {
        let wanted = header(device, language);
        let mut first = [0u8; 128];
        let n = storage.read_at(path, 0, &mut first).ok()?;
        if !first[..n].starts_with(wanted.as_bytes()) {
            return None;
        }
        // By group, then by the first letters of the name.
        let mut keys: Vec<(u8, [u8; SORT_BYTES], u32)> = Vec::new();
        let mut buf = [0u8; 512];
        let mut at = 0u64;
        let mut start = 0u64;
        let mut line: Vec<u8> = Vec::new();
        loop {
            let n = storage.read_at(path, at, &mut buf).ok()?;
            if n == 0 {
                break;
            }
            for (i, &byte) in buf[..n].iter().enumerate() {
                if byte != b'\n' {
                    if line.len() < MAX_LINE {
                        line.push(byte);
                    }
                    continue;
                }
                // The header (at 0) is no plugin.
                if start > 0
                    && let Some((group, name)) = sort_key(&line)
                {
                    keys.push((group, name, start as u32));
                }
                line.clear();
                start = at + i as u64 + 1;
            }
            at += n as u64;
        }
        keys.sort_unstable();
        // A list of its own: the keys' would stay, two and a half times
        // as large.
        let mut rows = Vec::with_capacity(keys.len());
        rows.extend(keys.iter().map(|&(group, _, at)| (group, at)));
        Some(Index {
            path: path.to_string(),
            rows,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The group of plugin `n`.
    pub(crate) fn group(&self, n: usize) -> usize {
        self.rows.get(n).map_or(0, |(group, _)| usize::from(*group))
    }

    /// Plugin `n`, read from the storage.
    pub(crate) fn entry(&self, storage: &dyn Storage, n: usize) -> Option<Entry> {
        let (_, at) = self.rows.get(n)?;
        Entry::parse(&line_at(storage, &self.path, *at)?)
    }
}

/// A line's group and the first bytes of its name, lower case.
fn sort_key(line: &[u8]) -> Option<(u8, [u8; SORT_BYTES])> {
    let mut fields = line.split(|&b| b == b'\t');
    let group = core::str::from_utf8(fields.next()?).ok()?.parse().ok()?;
    let name = fields.nth(5)?;
    let mut key = [0u8; SORT_BYTES];
    for (k, b) in key.iter_mut().zip(name) {
        *k = b.to_ascii_lowercase();
    }
    Some((group, key))
}

/// Where the list is kept between visits, its index, and the address it
/// came from.
pub(crate) fn path() -> String {
    format!("{}/plugins.json", Env::plugin_dir(PLUGIN.as_str()))
}

fn index_path() -> String {
    format!("{}/index.txt", Env::plugin_dir(PLUGIN.as_str()))
}

fn base_path() -> String {
    format!("{}/address.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The address of the list kept: where it came from (after redirects),
/// else the setting's.
fn base(env: &Env<'_>) -> String {
    paperpaw_sdk::read_file(env.storage, &base_path(), 1024)
        .ok()
        .and_then(|text| String::from_utf8(text).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| env.settings.read(&CATALOG).trim().to_string())
}

fn device(env: &Env<'_>) -> &'static str {
    install::device_name(env.device)
}

/// Takes the list fetched to `fetched` (from `address`): its index, and
/// the list kept for the next visits. `None` (and the file gone) if it
/// is no list this plugin reads.
pub(crate) fn take(env: &Env<'_>, fetched: &str, address: &str) -> Option<Index> {
    let storage = env.storage;
    let part = format!("{}.part", index_path());
    let language = env.i18n.language();
    if !build(storage, fetched, &part, device(env), language) {
        let _ = storage.remove(fetched);
        return None;
    }
    let _ = paperpaw_sdk::files::move_to(storage, fetched, &path());
    let _ = paperpaw_sdk::files::move_to(storage, &part, &index_path());
    let _ = storage.write(&base_path(), address.as_bytes());
    Index::open(storage, &index_path(), device(env), language)
}

/// The index of the list kept from the last visit, made anew where it was
/// for another language.
pub(crate) fn kept(env: &Env<'_>) -> Option<Index> {
    let storage = env.storage;
    let (device, language) = (device(env), env.i18n.language());
    Index::open(storage, &index_path(), device, language).or_else(|| {
        build(storage, &path(), &index_path(), device, language)
            .then(|| Index::open(storage, &index_path(), device, language))
            .flatten()
    })
}

/// The plugin `id` of the list kept, and its files to install.
pub(crate) fn find(env: &Env<'_>, id: &str) -> Option<(Entry, Package)> {
    let mut entry = None;
    let package = read(
        env.storage,
        &path(),
        device(env),
        env.i18n.language(),
        Some(id),
        &mut |found| {
            if found.id == id {
                entry = Some(found);
            }
        },
    )??;
    let entry = entry?;
    let mut package = package;
    let base = base(env);
    for file in &mut package.files {
        file.url = join_url(&base, &file.url);
    }
    package.name = entry.name.clone();
    Some((entry, package))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn plugin(id: &str, extra: &str, files: &str) -> String {
        format!(
            r#"{{"id": "{id}", "name": "Name {id}", "summary": "Does {id}.",
  "kind": "data", "group": "look", "author": "A", "licence": "MIT",
  {extra}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "files": [{files}]}}}}"#
        )
    }

    fn file(path: &str) -> String {
        format!(r#"{{"path": "{path}", "url": "/f/{path}", "size": 10, "sha1": "{SHA}"}}"#)
    }

    fn list(plugins: &[String]) -> MemoryStorage {
        let text = format!(r#"{{"format": 1, "plugins": [{}]}}"#, plugins.join(","));
        MemoryStorage::new().with("p.json", text)
    }

    /// The plugins of the list for `device` in `language`, through the
    /// index, in its order.
    fn entries(storage: &MemoryStorage, device: &str, language: &str) -> Vec<Entry> {
        assert!(build(storage, "p.json", "i.txt", device, language));
        let index = Index::open(storage, "i.txt", device, language).unwrap();
        (0..index.len())
            .map(|n| index.entry(storage, n).unwrap())
            .collect()
    }

    fn ids(storage: &MemoryStorage, device: &str) -> Vec<String> {
        entries(storage, device, "de")
            .into_iter()
            .map(|e| e.id)
            .collect()
    }

    #[test]
    fn only_what_fits_is_listed() {
        let ini = file("plugin.ini");
        let storage = list(&[
            plugin("plain", "", &ini),
            plugin("only-x4", r#""devices": ["x4"],"#, &ini),
            plugin("only-pro", r#""devices": ["x4-pro", "android"],"#, &ini),
            plugin("code", r#""kind": "code","#, &ini),
            plugin("built", r#""built_in": true,"#, &ini),
            plugin("later", "", &ini).replace(r#""formats": 1"#, r#""formats": 2"#),
            plugin("next-sdk", "", &ini).replace(r#""sdk": "0.4""#, r#""sdk": "0.9""#),
            plugin("no-ini", "", &file("theme.ini")),
            plugin("no-files", "", ""),
            plugin("Bad_Id", "", &ini),
        ]);
        assert_eq!(ids(&storage, "x4"), ["only-x4", "plain"]);
        assert_eq!(ids(&storage, "x4-pro"), ["only-pro", "plain"]);
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        for bad in ["../evil.ini", "/abs.ini", "a/../../b", ".hidden/x"] {
            let files = format!("{},{}", file("plugin.ini"), file(bad));
            let storage = list(&[plugin("p", "", &files)]);
            assert!(ids(&storage, "x4").is_empty(), "{bad}");
        }
        let short_sha = file("plugin.ini").replace(SHA, "abc");
        assert!(ids(&list(&[plugin("p", "", &short_sha)]), "x4").is_empty());
    }

    #[test]
    fn names_come_in_the_language_shown() {
        let i18n = r#""i18n": {"de": {"name": "Luftig", "summary": "Viel Raum."}, "fr": {"name": "Aéré"}},"#;
        let storage = list(&[plugin("airy", i18n, &file("plugin.ini"))]);
        let de = entries(&storage, "x4", "de");
        assert_eq!(
            (de[0].name.as_str(), de[0].summary.as_str()),
            ("Luftig", "Viel Raum.")
        );
        let en = entries(&storage, "x4", "en");
        assert_eq!(
            (en[0].name.as_str(), en[0].summary.as_str()),
            ("Name airy", "Does airy.")
        );
        let fr = entries(&storage, "x4", "fr");
        assert_eq!(
            (fr[0].name.as_str(), fr[0].summary.as_str()),
            ("Aéré", "Does airy.")
        );
    }

    #[test]
    fn groups_go_in_the_websites_order() {
        let ini = file("plugin.ini");
        let storage = list(&[
            plugin("b", "", &ini),
            plugin("a", "", &ini).replace(r#""group": "look""#, r#""group": "system""#),
            plugin("c", "", &ini).replace(r#""group": "look""#, r#""group": "reading""#),
            plugin("d", "", &ini).replace(r#""group": "look""#, r#""group": "odd""#),
        ]);
        assert_eq!(ids(&storage, "x4"), ["c", "b", "a", "d"]);
    }

    #[test]
    fn a_plugins_files_are_read_when_asked() {
        let files = format!("{},{}", file("plugin.ini"), file("i18n/de.ini"));
        // The id after the files.
        let late = r#"{"name": "Late", "kind": "data", "latest": {"version": "2.0", "files": [FILES]}, "id": "late"}"#
            .replace("FILES", &files);
        let storage = list(&[plugin("p", "", &files), late]);
        let mut entries = Vec::new();
        let package = read(&storage, "p.json", "x4", "en", Some("late"), &mut |e| {
            entries.push(e)
        });
        let package = package.unwrap();
        assert_eq!(entries.len(), 2);
        let package = package.unwrap();
        assert_eq!(package.plugin, "late");
        let paths: Vec<&str> = package.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["plugin.ini", "i18n/de.ini"]);
        assert_eq!(entries[0].size, 20);
    }

    #[test]
    fn other_lists_are_not_read() {
        let storage = MemoryStorage::new().with("p.json", r#"{"format": 2, "plugins": []}"#);
        assert!(!build(&storage, "p.json", "i.txt", "x4", "en"));
        assert!(!build(&storage, "missing.json", "i.txt", "x4", "en"));
        assert!(!storage.exists("i.txt"));
        // An index for another device or language is made anew.
        let storage = list(&[plugin("p", "", &file("plugin.ini"))]);
        assert!(build(&storage, "p.json", "i.txt", "x4", "en"));
        assert!(Index::open(&storage, "i.txt", "x4", "de").is_none());
        assert!(Index::open(&storage, "i.txt", "x4-pro", "en").is_none());
    }
}
