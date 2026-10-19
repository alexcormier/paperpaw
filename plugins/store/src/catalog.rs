//! The plugin library's list and its plugins (ADR 0023, format 2), read
//! as they stream from the files they were saved to.
//!
//! The list (`plugins.json?device=x4&lang=de`) holds only the plugins for
//! this device, in the order to show them (by group, then by name in the
//! language asked for), without their files:
//!
//! ```json
//! {"format": 2, "plugins": [{
//!   "id": "airy-theme", "name": "Luftig", "summary": "…", "group": "look",
//!   "author": "…", "licence": "Apache-2.0", "kind": "data",
//!   "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 2345}}]}
//! ```
//!
//! The plugins that fit go to an index on the storage: a line each, and
//! where each line begins (4 bytes each). In memory are only the runs of
//! plugins of one group, so the list may grow without limit. A plugin's
//! page asks for it with its files (`plugins/<id>.json`, the same fields
//! and `latest.files`), read when it comes.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_json::stream::{Event, read_file};
use paperpaw_sdk::install::{self, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::web::{encode, join_url};
use paperpaw_sdk::{Env, Storage, Version};

use crate::PLUGIN;

/// The format of the list and of a plugin's page this plugin reads.
const FORMAT: u32 = 2;
/// The longest summary kept, in bytes, and the longest name and other
/// short texts.
const MAX_SUMMARY: usize = 300;
const MAX_NAME: usize = 80;
/// The longest line of the index read.
const MAX_LINE: usize = 1024;
/// The index is written in pieces of about this many bytes.
const WRITE_AT: usize = 2048;
/// Runs of plugins of one group at most: a list in the website's order
/// has one a group; plugins of a list out of order beyond these go with
/// the last run.
const MAX_RUNS: usize = 32;

/// The groups, in the order the website shows them.
pub(crate) const GROUPS: [&str; 7] = [
    "reading", "formats", "library", "sync", "home", "look", "system",
];

/// The group of `name`, the last (`system`) for one this plugin does not
/// know.
pub(crate) fn group(name: &str) -> usize {
    GROUPS
        .iter()
        .position(|g| *g == name)
        .unwrap_or(GROUPS.len() - 1)
}

/// A data plugin the library offers for this device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub id: String,
    /// In the UI's language where the library has it.
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

/// What is known of the plugin being read.
#[derive(Default)]
struct Item {
    entry: Entry,
    kind: String,
    built_in: bool,
    /// `devices`, where the library names them: any, and this one.
    devices: bool,
    device: bool,
    sdk: String,
    formats: u32,
    /// The files read so far (a plugin's page only), and the one being
    /// read.
    files: Vec<PackageFile>,
    file: PackageFile,
}

impl Item {
    fn new() -> Self {
        Self {
            formats: 1,
            ..Self::default()
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
    }

    /// Takes what `key` (the path below the plugin) says.
    fn read(&mut self, key: &[String], value: &str, device: &str) {
        let entry = &mut self.entry;
        match key {
            [key] => match key.as_str() {
                "id" => entry.id = cut(value, MAX_NAME),
                "name" => entry.name = cut(value, MAX_NAME),
                "summary" => entry.summary = cut(value, MAX_SUMMARY),
                "author" => entry.author = cut(value, MAX_NAME),
                "licence" => entry.licence = cut(value, MAX_NAME),
                "group" => entry.group = group(value),
                "kind" => self.kind = cut(value, MAX_NAME),
                "built_in" => self.built_in = value == "true",
                _ => {}
            },
            [devices, at] if devices == "devices" && at == "[]" => {
                self.devices = true;
                self.device |= value == device;
            }
            [latest, key] if latest == "latest" => match key.as_str() {
                "version" => entry.version = cut(value, MAX_NAME),
                "sdk" => self.sdk = cut(value, MAX_NAME),
                "formats" => self.formats = value.parse().unwrap_or(u32::MAX),
                "size" => entry.size = value.parse().unwrap_or(0),
                _ => {}
            },
            [latest, files, at, key] if latest == "latest" && files == "files" && at == "[]" => {
                let file = &mut self.file;
                match key.as_str() {
                    "path" => file.path = value.to_string(),
                    "url" => file.url = value.to_string(),
                    "size" => file.size = value.parse().unwrap_or(0),
                    "sha1" => file.sha1 = value.to_ascii_lowercase(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// A file of `latest.files` was read whole (one more than a package
    /// may have is kept, so that it is refused).
    fn file_done(&mut self) {
        let file = core::mem::take(&mut self.file);
        if self.files.len() <= MAX_FILES {
            self.files.push(file);
        }
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

/// Whether `path` (below a plugin) is that of a file of `latest.files`.
fn is_file(path: &[String]) -> bool {
    matches!(path, [latest, files, at] if latest == "latest" && files == "files" && at == "[]")
}

/// Reads the list at `path` for the device `device`, handing each plugin
/// that fits to `found`. `false` if it is no list this plugin reads.
fn read_list(
    storage: &dyn Storage,
    path: &str,
    device: &str,
    found: &mut dyn FnMut(Entry),
) -> bool {
    let mut format = None;
    let mut item: Option<Item> = None;
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Open([list, at]) if list == "plugins" && at == "[]" => item = Some(Item::new()),
        Event::Close([list, at]) if list == "plugins" && at == "[]" => {
            if let Some(done) = item.take().filter(Item::fits) {
                found(done.entry);
            }
        }
        Event::Value([list, at, rest @ ..], value) if list == "plugins" && at == "[]" => {
            if let Some(item) = item.as_mut() {
                item.read(rest, value, device);
            }
        }
        _ => {}
    });
    format == Some(FORMAT)
}

/// Reads a plugin's page at `path` (`{"format": 2, "plugin": {…}}`): the
/// plugin if it fits this device, with its files, their addresses
/// relative to `base` (the page's).
fn read_plugin(
    storage: &dyn Storage,
    path: &str,
    device: &str,
    base: &str,
) -> Option<(Entry, Package)> {
    let mut format = None;
    let mut item = Item::new();
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_file(rest) => {
            item.file_done();
        }
        Event::Value([plugin, rest @ ..], value) if plugin == "plugin" => {
            item.read(rest, value, device);
        }
        _ => {}
    });
    if format != Some(FORMAT) || !item.fits() {
        return None;
    }
    let files = core::mem::take(&mut item.files)
        .into_iter()
        .map(|mut file| {
            file.url = join_url(base, &file.url);
            file
        })
        .collect();
    let package = Package {
        plugin: item.entry.id.clone(),
        name: item.entry.name.clone(),
        files,
    };
    package.is_valid().then_some((item.entry, package))
}

/// A file of the store: the list fetched, the index and where its lines
/// begin, a plugin's page, the hits of a search.
fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

pub(crate) fn list_part() -> String {
    file("list.part")
}

pub(crate) fn plugin_part() -> String {
    file("plugin.part")
}

fn index_path() -> String {
    file("index.txt")
}

fn head_path() -> String {
    file("index.head")
}

fn offsets_path() -> String {
    file("index.bin")
}

fn hits_path() -> String {
    file("search.bin")
}

/// The first line of an index's head: its format, then the runs of
/// plugins of one group (`group:count`), in order.
const HEADER: &str = "store-index 2";

/// The address of the list at `list` for `device` in `language`.
pub(crate) fn list_url(list: &str, device: &str, language: &str) -> String {
    let join = if list.contains('?') { '&' } else { '?' };
    format!(
        "{list}{join}device={}&lang={}",
        encode(device),
        encode(language)
    )
}

/// The address of plugin `id`'s page, beside the list at `list` (the
/// setting's, without the device and the language), for `device` in
/// `language`.
pub(crate) fn plugin_url(list: &str, device: &str, language: &str, id: &str) -> String {
    let page = join_url(list, &format!("plugins/{}.json", encode(id)));
    list_url(&page, device, language)
}

/// Writes the index of the list at `json` (`index`, `offsets`, `head` on
/// the storage). `false` (and no index) if it is no list this plugin
/// reads.
fn build(
    storage: &dyn Storage,
    json: &str,
    [index, offsets, head]: [&str; 3],
    device: &str,
) -> bool {
    let mut runs: Vec<(usize, u32)> = Vec::new();
    let mut lines = String::new();
    let mut starts: Vec<u8> = Vec::new();
    let mut offset = 0u32;
    let mut written = storage.write(index, b"").is_ok() && storage.write(offsets, b"").is_ok();
    let read = read_list(storage, json, device, &mut |entry| {
        let full = runs.len() >= MAX_RUNS;
        match runs.last_mut() {
            Some((group, count)) if *group == entry.group || full => *count += 1,
            _ => runs.push((entry.group, 1)),
        }
        let line = entry.line();
        starts.extend_from_slice(&offset.to_le_bytes());
        offset = offset.saturating_add(line.len() as u32);
        lines.push_str(&line);
        if lines.len() > WRITE_AT {
            written &= storage.append(index, lines.as_bytes()).is_ok();
            written &= storage.append(offsets, &starts).is_ok();
            lines.clear();
            starts.clear();
        }
    });
    written &= storage.append(index, lines.as_bytes()).is_ok();
    written &= storage.append(offsets, &starts).is_ok();
    let runs: Vec<String> = runs
        .iter()
        .map(|(group, count)| format!("{group}:{count}"))
        .collect();
    let text = format!("{HEADER}\t{}\n", runs.join(","));
    written &= storage.write(head, text.as_bytes()).is_ok();
    if !read || !written {
        for path in [index, offsets, head] {
            let _ = storage.remove(path);
        }
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

/// The `n`th number of 4 bytes in the file at `path`.
fn number_at(storage: &dyn Storage, path: &str, n: usize) -> Option<u32> {
    let mut bytes = [0u8; 4];
    let read = storage.read_at(path, (n as u64) * 4, &mut bytes).ok()?;
    (read == 4).then(|| u32::from_le_bytes(bytes))
}

/// The plugins of the index on the storage: in memory only the runs of
/// plugins of one group, in order.
#[derive(Debug, Default)]
pub(crate) struct Index {
    runs: Vec<(usize, usize)>,
}

impl Index {
    /// The index kept on the storage, if there is one.
    pub(crate) fn kept(storage: &dyn Storage) -> Option<Index> {
        Self::open(storage, &head_path())
    }

    fn open(storage: &dyn Storage, head: &str) -> Option<Index> {
        let text = line_at(storage, head, 0)?;
        let (format, runs) = text.split_once('\t').unwrap_or((&text, ""));
        if format != HEADER {
            return None;
        }
        let runs = runs
            .split(',')
            .filter(|run| !run.is_empty())
            .filter_map(|run| {
                let (group, count) = run.split_once(':')?;
                Some((group.parse().ok()?, count.parse().ok()?))
            })
            .filter(|&(group, _)| group < GROUPS.len())
            .collect();
        Some(Index { runs })
    }

    /// The runs of plugins of one group, in order: (group, how many).
    pub(crate) fn runs(&self) -> &[(usize, usize)] {
        &self.runs
    }

    pub(crate) fn len(&self) -> usize {
        self.runs.iter().map(|(_, count)| count).sum()
    }

    /// Plugin `n`, read from the storage.
    pub(crate) fn entry(&self, storage: &dyn Storage, n: usize) -> Option<Entry> {
        let offset = number_at(storage, &offsets_path(), n)?;
        Entry::parse(&line_at(storage, &index_path(), offset)?)
    }

    /// Searches the names and summaries for `query` (the case of letters
    /// left aside), keeping the hits on the storage; how many there are.
    pub(crate) fn search(&self, storage: &dyn Storage, query: &str) -> usize {
        let query = query.trim().to_lowercase();
        let hits = hits_path();
        let mut written = storage.write(&hits, b"").is_ok();
        let mut found: Vec<u8> = Vec::new();
        let mut count = 0;
        for n in 0..self.len() {
            let Some(entry) = self.entry(storage, n) else {
                continue;
            };
            if entry.name.to_lowercase().contains(&query)
                || entry.summary.to_lowercase().contains(&query)
            {
                found.extend_from_slice(&(n as u32).to_le_bytes());
                count += 1;
                if found.len() >= WRITE_AT {
                    written &= storage.append(&hits, &found).is_ok();
                    found.clear();
                }
            }
        }
        written &= storage.append(&hits, &found).is_ok();
        if written { count } else { 0 }
    }
}

/// The plugin of hit `n` of the last search.
pub(crate) fn hit(storage: &dyn Storage, n: usize) -> Option<usize> {
    number_at(storage, &hits_path(), n).map(|n| n as usize)
}

/// Takes the list fetched to `fetched`: its index, kept for the next
/// visits. `None` (and the file gone) if it is no list this plugin reads.
pub(crate) fn take(storage: &dyn Storage, fetched: &str, device: &str) -> Option<Index> {
    let fresh = [index_path(), offsets_path(), head_path()].map(|path| format!("{path}.part"));
    let built = build(storage, fetched, [&fresh[0], &fresh[1], &fresh[2]], device);
    let _ = storage.remove(fetched);
    if !built {
        return None;
    }
    // The head last: an index is whole where its head is.
    let _ = storage.remove(&head_path());
    for (from, to) in fresh
        .iter()
        .zip([index_path(), offsets_path(), head_path()])
    {
        if paperpaw_sdk::files::move_to(storage, from, &to).is_err() {
            return None;
        }
    }
    // The hits of a search before were of the list before.
    let _ = storage.remove(&hits_path());
    Index::kept(storage)
}

/// The plugin of the page fetched to `fetched` from `address`, and its
/// files to install.
pub(crate) fn plugin(
    storage: &dyn Storage,
    fetched: &str,
    address: &str,
    device: &str,
) -> Option<(Entry, Package)> {
    let read = read_plugin(storage, fetched, device, address);
    let _ = storage.remove(fetched);
    read
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const FILES: [&str; 3] = ["i.txt", "i.bin", "i.head"];

    fn plugin(id: &str, group: &str, extra: &str) -> String {
        format!(
            r#"{{"id": "{id}", "name": "Name {id}", "summary": "Does {id}.", "group": "{group}",
  "kind": "data", "author": "A", "licence": "MIT", {extra}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}}}"#
        )
    }

    fn list(plugins: &[String]) -> MemoryStorage {
        let text = format!(r#"{{"format": 2, "plugins": [{}]}}"#, plugins.join(","));
        MemoryStorage::new().with("p.json", text)
    }

    /// Plugin `n` of the index built in `storage`.
    fn entry(storage: &MemoryStorage, n: usize) -> Entry {
        let offset = number_at(storage, "i.bin", n).unwrap();
        Entry::parse(&line_at(storage, "i.txt", offset).unwrap()).unwrap()
    }

    /// The plugins of the list through the index, in its order, and its
    /// runs of groups.
    fn entries(storage: &MemoryStorage) -> (Vec<Entry>, Vec<(usize, usize)>) {
        assert!(build(storage, "p.json", FILES, "x4"));
        let index = Index::open(storage, "i.head").unwrap();
        let entries = (0..index.len()).map(|n| entry(storage, n)).collect();
        (entries, index.runs)
    }

    #[test]
    fn only_what_fits_is_listed_in_its_order() {
        let storage = list(&[
            plugin("plain", "reading", ""),
            plugin("code", "reading", "").replace(r#""kind": "data""#, r#""kind": "code""#),
            plugin("built", "reading", r#""built_in": true,"#),
            plugin("later", "look", "").replace(r#""formats": 1"#, r#""formats": 2"#),
            plugin("next-sdk", "look", "").replace(r#""sdk": "0.4""#, r#""sdk": "0.9""#),
            plugin("Bad_Id", "look", ""),
            plugin("phone", "look", r#""devices": ["android"],"#),
            plugin("theme", "look", r#""devices": ["x4", "x4-pro"],"#),
            plugin("odd", "elsewhere", ""),
        ]);
        let (entries, runs) = entries(&storage);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["plain", "theme", "odd"]);
        // Groups as they come, an unknown one under `system`.
        assert_eq!(runs, [(0, 1), (5, 1), (6, 1)]);
        assert_eq!(entries[0].summary, "Does plain.");
        assert_eq!(entries[0].size, 30);
    }

    #[test]
    fn many_plugins_take_no_memory_each() {
        let plugins: Vec<String> = (0..500)
            .map(|n| plugin(&format!("p{n}"), "look", ""))
            .collect();
        let storage = list(&plugins);
        let (_, runs) = entries(&storage);
        assert_eq!(runs, [(5, 500)]);
        assert_eq!(entry(&storage, 499).id, "p499");
    }

    #[test]
    fn other_lists_are_not_read() {
        let storage = MemoryStorage::new().with("p.json", r#"{"format": 1, "plugins": []}"#);
        assert!(!build(&storage, "p.json", FILES, "x4"));
        assert!(!build(&storage, "missing.json", FILES, "x4"));
        assert!(!storage.exists("i.txt"));
        assert!(Index::open(&storage, "i.head").is_none());
    }

    fn page(files: &str) -> MemoryStorage {
        let text = format!(
            r#"{{"format": 2, "plugin": {{"id": "airy", "name": "Airy", "summary": "Roomy.",
  "group": "look", "kind": "data", "author": "A", "licence": "MIT",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 20, "files": [{files}]}}}}}}"#
        );
        MemoryStorage::new().with("a.json", text)
    }

    fn file(path: &str) -> String {
        format!(
            r#"{{"path": "{path}", "url": "/api/v1/plugins/airy/1.0.0/{path}", "size": 10, "sha1": "{SHA}"}}"#
        )
    }

    #[test]
    fn a_plugins_files_come_with_its_page() {
        let storage = page(&format!("{},{}", file("i18n/de.ini"), file("plugin.ini")));
        let base = "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en";
        let (entry, package) = read_plugin(&storage, "a.json", "x4", base).unwrap();
        assert_eq!((entry.id.as_str(), entry.name.as_str()), ("airy", "Airy"));
        assert_eq!(package.plugin, "airy");
        let urls: Vec<&str> = package.files.iter().map(|f| f.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://paperpaw.org/api/v1/plugins/airy/1.0.0/i18n/de.ini",
                "https://paperpaw.org/api/v1/plugins/airy/1.0.0/plugin.ini"
            ]
        );
    }

    #[test]
    fn unsafe_files_are_refused() {
        for bad in ["../evil.ini", "/abs.ini", "a/../../b", ".hidden/x"] {
            let storage = page(&format!("{},{}", file("plugin.ini"), file(bad)));
            let read = read_plugin(&storage, "a.json", "x4", "https://x/");
            assert!(read.is_none(), "{bad}");
        }
        let short = file("plugin.ini").replace(SHA, "abc");
        assert!(read_plugin(&page(&short), "a.json", "x4", "https://x/").is_none());
        // No plugin.ini.
        assert!(read_plugin(&page(&file("theme.ini")), "a.json", "x4", "https://x/").is_none());
        // More files than a package may have.
        let many: Vec<String> = (0..=MAX_FILES)
            .map(|n| file(&format!("f{n}.ini")))
            .chain([file("plugin.ini")])
            .collect();
        assert!(read_plugin(&page(&many.join(",")), "a.json", "x4", "https://x/").is_none());
    }

    #[test]
    fn addresses_name_the_device_and_the_language() {
        let list = "https://paperpaw.org/api/v1/plugins.json";
        assert_eq!(
            list_url(list, "x4-pro", "de"),
            "https://paperpaw.org/api/v1/plugins.json?device=x4-pro&lang=de"
        );
        assert_eq!(
            list_url("https://x/p.json?key=1", "x4", "en"),
            "https://x/p.json?key=1&device=x4&lang=en"
        );
        assert_eq!(
            plugin_url(list, "x4", "en", "airy"),
            "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en"
        );
    }

    #[test]
    fn searches_find_names_and_summaries() {
        let storage = list(&[
            plugin("airy", "look", ""),
            plugin("dark", "look", "").replace("Does dark.", "Night colours for AIRY rooms."),
            plugin("keys", "system", ""),
        ]);
        let index = take(&storage, "p.json", "x4").unwrap();
        assert!(!storage.exists("p.json"));
        assert_eq!(index.search(&storage, " Airy "), 2);
        assert_eq!((hit(&storage, 0), hit(&storage, 1)), (Some(0), Some(1)));
        assert_eq!(hit(&storage, 2), None);
        assert_eq!(index.entry(&storage, 2).unwrap().id, "keys");
        assert_eq!(index.search(&storage, "nothing"), 0);
        // A list fetched anew: the hits of the one before go.
        index.search(&storage, "keys");
        let storage = storage.with("p.json", r#"{"format": 2, "plugins": []}"#);
        let index = take(&storage, "p.json", "x4").unwrap();
        assert_eq!(index.len(), 0);
        assert_eq!(hit(&storage, 0), None);
    }
}
