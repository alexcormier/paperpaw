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
//! A code plugin is `"kind": "code"`, with `latest.builds` (each a
//! `device`, the `abi` it was built for, `url`, `size`, `sha256`) and the
//! `author_key` its author signs with; it is listed where a build fits
//! this device and PaperPaw.
//!
//! The plugins that fit go to an index on the storage: a line each, and
//! where each line begins (4 bytes each). In memory are only the runs of
//! plugins of one group, so the list may grow without limit. A plugin's
//! page asks for it with its files (`plugins/<id>.json`, the same fields
//! and `latest.files`, with the list's own query), read when it comes.
//!
//! A plugin of the library is passed over where the device has a plugin
//! of that id it cannot take the place of ([`may_install`]): one built
//! in, or one of the other kind.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_json::stream::{Event, read_file};
use paperpaw_sdk::install::{self, Digest, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::native::Fingerprint;
use paperpaw_sdk::web::{encode, join_url};
use paperpaw_sdk::{Env, PluginKind, Plugins, Storage, Version};

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

/// What the store installs for: the device as plugin libraries call it,
/// where PaperPaw loads code plugins the build they must be made for and
/// the name their code gets, and the plugins it has.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Device<'a> {
    pub name: &'a str,
    pub fingerprint: Option<Fingerprint>,
    pub code_file: &'a str,
    pub plugins: Option<&'a Plugins>,
}

impl<'a> Device<'a> {
    /// This device, if plugin libraries know it.
    pub(crate) fn of(env: &Env<'a>) -> Option<Device<'a>> {
        Some(Device {
            name: env.catalog_device?,
            fingerprint: env.native.map(|native| native.fingerprint),
            code_file: env.native.map_or("", |native| &native.file),
            plugins: Some(env.plugins),
        })
    }
}

/// Whether a plugin `id` of the library, with code or without, may take
/// the place of what `plugins` has of that id: nothing, or a plugin of
/// the same kind installed before. A built-in plugin comes only with the
/// firmware, and one kind does not replace the other.
pub(crate) fn may_install(plugins: &Plugins, id: &str, code: bool) -> bool {
    plugins.get(id).is_none_or(|info| match info.kind {
        PluginKind::BuiltIn => false,
        PluginKind::Data => !code,
        PluginKind::Code => code,
    })
}

/// A plugin the library offers for this device: a data plugin, or a code
/// plugin with a build for this PaperPaw.
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
    /// The PaperPaw (its SDK) it is made for, e.g. `0.4`.
    pub sdk: String,
    /// It has code, which runs with full access to the device.
    pub code: bool,
    /// Bytes to download, its code with them.
    pub size: u64,
    /// When it was first published and when its newest version, as
    /// `yyyymmdd` (0 where the library does not say).
    pub added: u32,
    pub updated: u32,
}

impl Entry {
    /// The version installed, if it is (a code plugin's update waiting for
    /// the restart counts).
    pub(crate) fn installed(&self, env: &Env<'_>) -> Option<Version> {
        env.plugins
            .get(&self.id)
            .map(|info| info.update.unwrap_or(info.manifest.version))
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
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            self.group,
            self.size,
            u8::from(self.code),
            self.added,
            self.updated,
            clean(&self.id),
            clean(&self.version),
            clean(&self.sdk),
            clean(&self.author),
            clean(&self.licence),
            clean(&self.name),
            clean(&self.summary)
        )
    }

    fn parse(line: &str) -> Option<Entry> {
        let mut f = line.splitn(12, '\t');
        Some(Entry {
            group: f.next()?.parse().ok()?,
            size: f.next()?.parse().ok()?,
            code: f.next()? == "1",
            added: f.next()?.parse().ok()?,
            updated: f.next()?.parse().ok()?,
            id: f.next()?.to_string(),
            version: f.next()?.to_string(),
            sdk: f.next()?.to_string(),
            author: f.next()?.to_string(),
            licence: f.next()?.to_string(),
            name: f.next()?.to_string(),
            summary: f.next()?.to_string(),
        })
    }
}

/// A build of a code plugin, as `latest.builds` lists them.
#[derive(Debug, Clone, Default)]
struct Build {
    device: String,
    abi: String,
    url: String,
    size: u64,
    sha256: String,
    /// The bytes its image takes where the device places it (its
    /// header's), where the library says.
    image_len: Option<u64>,
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
    formats: u32,
    /// The key its author signs its code with, as the library says.
    author_key: String,
    /// The files read so far (a plugin's page only), and the one being
    /// read.
    files: Vec<PackageFile>,
    file: PackageFile,
    /// The build being read, and the one for this device and PaperPaw.
    build: Build,
    code: Option<PackageFile>,
    /// What only a plugin's page says.
    more: More,
    shot: Shot,
}

/// A screenshot of a plugin, as its page lists them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Shot {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

/// Screenshots of a plugin at most, and their longest side.
pub(crate) const MAX_SHOTS: usize = 4;
const MAX_SHOT_SIDE: u32 = 2560;

/// What a plugin's page tells beyond the list: where its source is, since
/// when its author publishes, whether its build was checked against its
/// source, screenshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct More {
    pub source: String,
    /// `yyyymmdd`, 0 where not said.
    pub author_since: u32,
    pub author_plugins: u32,
    /// `verified`, `unverified`, `failed`, or empty.
    pub verified: String,
    /// Bytes of memory it takes while running at most, where its author
    /// measured it (`latest.heap`).
    pub heap: Option<u64>,
    /// The bytes the code of the build for this device takes where it is
    /// placed (`latest.builds[].image_len`), where the library says.
    pub image_len: Option<u64>,
    pub shots: Vec<Shot>,
}

/// `yyyymmdd` of a date written `YYYY-MM-DD` (or a year alone), 0 for
/// anything else.
pub(crate) fn date(text: &str) -> u32 {
    let digits: String = text.chars().filter(char::is_ascii_digit).take(8).collect();
    match digits.len() {
        8 => digits.parse().unwrap_or(0),
        4 => digits.parse::<u32>().map_or(0, |y| y * 10000 + 101),
        _ => 0,
    }
}

impl Item {
    fn new() -> Self {
        Self {
            formats: 1,
            ..Self::default()
        }
    }

    /// Whether this device can install it: a data plugin, or one with code
    /// built for it, that takes the place of none of its own.
    fn fits(&self, device: &Device<'_>) -> bool {
        let entry = &self.entry;
        let kind = match self.kind.as_str() {
            "data" => true,
            "code" => self.code.is_some(),
            _ => false,
        };
        let code = self.code.is_some();
        kind && !self.built_in
            && device
                .plugins
                .is_none_or(|plugins| may_install(plugins, &entry.id, code))
            && (!self.devices || self.device)
            && install::is_plugin_name(&entry.id)
            && !entry.name.is_empty()
            && Version::parse(&entry.version).is_some()
            && install::fits(&entry.sdk, self.formats)
    }

    /// Takes what `key` (the path below the plugin) says.
    fn read(&mut self, key: &[String], value: &str, device: &Device<'_>) {
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
                "author_key" => self.author_key = cut(value, MAX_NAME),
                "added" => entry.added = date(value),
                // Its home; `latest.source.url` (that version's source)
                // goes before it.
                "source" if value.starts_with("https://") && self.more.source.is_empty() => {
                    self.more.source = cut(value, 200);
                }
                "author_since" => self.more.author_since = date(value),
                "author_plugins" => self.more.author_plugins = value.parse().unwrap_or(0),
                _ => {}
            },
            [latest, source, url] if latest == "latest" && source == "source" && url == "url" => {
                if value.starts_with("https://") {
                    self.more.source = cut(value, 200);
                }
            }
            [shots, at, key] if shots == "screenshots" && at == "[]" => match key.as_str() {
                "url" => self.shot.url = cut(value, 300),
                "width" => self.shot.width = value.parse().unwrap_or(0),
                "height" => self.shot.height = value.parse().unwrap_or(0),
                "size" => self.shot.size = value.parse().unwrap_or(0),
                _ => {}
            },
            [devices, at] if devices == "devices" && at == "[]" => {
                self.devices = true;
                self.device |= value == device.name;
            }
            [latest, key] if latest == "latest" => match key.as_str() {
                "version" => entry.version = cut(value, MAX_NAME),
                "sdk" => entry.sdk = cut(value, MAX_NAME),
                "formats" => self.formats = value.parse().unwrap_or(u32::MAX),
                "size" => entry.size = value.parse().unwrap_or(0),
                "date" => entry.updated = date(value),
                "heap" => self.more.heap = value.parse().ok().filter(|&h| h > 0),
                // Code plugins only: the device's build was rebuilt from
                // its source and matches.
                "verified" if self.more.verified.is_empty() => {
                    self.more.verified = if value == "true" {
                        "verified"
                    } else {
                        "unverified"
                    }
                    .into();
                }
                _ => {}
            },
            [latest, files, at, key] if latest == "latest" && files == "files" && at == "[]" => {
                let file = &mut self.file;
                match key.as_str() {
                    "path" => file.path = value.to_string(),
                    "url" => file.url = value.to_string(),
                    "size" => file.size = value.parse().unwrap_or(0),
                    "sha1" => file.digest = Digest::Sha1(value.to_ascii_lowercase()),
                    _ => {}
                }
            }
            [latest, builds, at, key] if latest == "latest" && builds == "builds" && at == "[]" => {
                let build = &mut self.build;
                match key.as_str() {
                    "device" => build.device = cut(value, MAX_NAME),
                    "abi" => build.abi = cut(value, MAX_NAME),
                    "url" => build.url = value.to_string(),
                    "size" => build.size = value.parse().unwrap_or(0),
                    "sha256" => build.sha256 = value.to_ascii_lowercase(),
                    "image_len" => build.image_len = value.parse().ok(),
                    "rebuilt" if value == "differs" => self.more.verified = "failed".into(),
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

    /// A build of `latest.builds` was read whole: kept as the plugin's
    /// code if it is for this device and PaperPaw.
    fn build_done(&mut self, device: &Device<'_>) {
        let build = core::mem::take(&mut self.build);
        let fits = build.device == device.name
            && !device.code_file.is_empty()
            && device
                .fingerprint
                .is_some_and(|f| Fingerprint::parse(&build.abi) == Some(f));
        if fits && self.code.is_none() {
            self.more.image_len = build.image_len;
            self.code = Some(PackageFile {
                path: device.code_file.to_string(),
                url: build.url,
                size: build.size,
                digest: Digest::Sha256(build.sha256),
            });
        }
    }

    /// A screenshot of `screenshots` was read whole.
    fn shot_done(&mut self) {
        let shot = core::mem::take(&mut self.shot);
        // The library keeps them as their authors made them: only those
        // PaperPaw decodes, and not too large to decode here.
        let path = shot
            .url
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let readable = [".png", ".jpg", ".jpeg"]
            .iter()
            .any(|end| path.ends_with(end));
        let small = shot.width <= MAX_SHOT_SIDE && shot.height <= MAX_SHOT_SIDE;
        if self.more.shots.len() < MAX_SHOTS && readable && small {
            self.more.shots.push(shot);
        }
    }

    /// The plugin as listed: its code's size counted in.
    fn finish(mut self) -> Entry {
        if let Some(code) = &self.code {
            self.entry.code = true;
            self.entry.size = self.entry.size.saturating_add(code.size);
        }
        self.entry
    }
}

/// An Ed25519 public key written as 64 hex digits.
fn key(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.trim().as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let digit = |b: u8| char::from(b).to_digit(16);
    let mut key = [0u8; 32];
    for (byte, pair) in key.iter_mut().zip(hex.chunks(2)) {
        *byte = u8::try_from(digit(pair[0])? * 16 + digit(pair[1])?).ok()?;
    }
    Some(key)
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

/// Whether `path` (below a plugin) is that of a build of `latest.builds`.
fn is_build(path: &[String]) -> bool {
    matches!(path, [latest, builds, at] if latest == "latest" && builds == "builds" && at == "[]")
}

/// Reads the list at `path` for `device`, handing each plugin that fits to
/// `found`. `false` if it is no list this plugin reads.
fn read_list(
    storage: &dyn Storage,
    path: &str,
    device: &Device<'_>,
    found: &mut dyn FnMut(Entry),
) -> bool {
    let mut format = None;
    let mut item: Option<Item> = None;
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Open([list, at]) if list == "plugins" && at == "[]" => item = Some(Item::new()),
        Event::Close([list, at]) if list == "plugins" && at == "[]" => {
            if let Some(done) = item.take().filter(|item| item.fits(device)) {
                found(done.finish());
            }
        }
        Event::Close([list, at, rest @ ..])
            if list == "plugins" && at == "[]" && is_build(rest) =>
        {
            if let Some(item) = item.as_mut() {
                item.build_done(device);
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

/// A plugin as its page at the library gives it: what it is, its files
/// to install (a code plugin's code among them, as [`Device::code_file`]),
/// and the key its author signs its code with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plugin {
    pub entry: Entry,
    pub package: Package,
    pub author_key: Option<[u8; 32]>,
    pub more: More,
}

/// Reads a plugin's page at `path` (`{"format": 2, "plugin": {…}}`): the
/// plugin if it fits `device`, with its files, their addresses relative to
/// `base` (the page's).
fn read_plugin(
    storage: &dyn Storage,
    path: &str,
    device: &Device<'_>,
    base: &str,
) -> Option<Plugin> {
    let mut format = None;
    let mut item = Item::new();
    read_file(storage, path, &mut |event| match event {
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_file(rest) => {
            item.file_done();
        }
        Event::Close([plugin, rest @ ..]) if plugin == "plugin" && is_build(rest) => {
            item.build_done(device);
        }
        Event::Close([plugin, shots, at])
            if plugin == "plugin" && shots == "screenshots" && at == "[]" =>
        {
            item.shot_done();
        }
        Event::Value([plugin, rest @ ..], value) if plugin == "plugin" => {
            item.read(rest, value, device);
        }
        _ => {}
    });
    if format != Some(FORMAT) || !item.fits(device) {
        return None;
    }
    let mut files = core::mem::take(&mut item.files);
    files.extend(item.code.clone());
    for file in &mut files {
        file.url = join_url(base, &file.url);
    }
    let author_key = key(&item.author_key);
    let code = item.code.is_some();
    let mut more = core::mem::take(&mut item.more);
    for shot in &mut more.shots {
        shot.url = join_url(base, &shot.url);
    }
    let entry = item.finish();
    let package = Package {
        plugin: entry.id.clone(),
        name: entry.name.clone(),
        files,
    };
    // A code plugin names the key its author signs with.
    let whole = package.is_valid() && (!code || author_key.is_some());
    whole.then_some(Plugin {
        entry,
        package,
        author_key,
        more,
    })
}

/// A file of the store: the list fetched, the index and where its lines
/// begin, a plugin's page, the hits of a search.
fn file(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

pub(crate) fn list_part() -> String {
    file("list.part")
}

/// Where plugin `id`'s page is fetched to: a file of its own, so that
/// pages fetched at once do not write into each other.
pub(crate) fn plugin_part(id: &str) -> String {
    file(&format!("plugin-{id}.part"))
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
const HEADER: &str = "store-index 4";

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
/// setting's, without the device and the language) and with its query
/// (a library may want a key there), for `device` in `language`.
pub(crate) fn plugin_url(list: &str, device: &str, language: &str, id: &str) -> String {
    let list = list.split('#').next().unwrap_or(list);
    let (address, query) = list.split_once('?').unwrap_or((list, ""));
    let mut page = join_url(address, &format!("plugins/{}.json", encode(id)));
    if !query.is_empty() {
        page.push('?');
        page.push_str(query);
    }
    list_url(&page, device, language)
}

/// Writes the index of the list at `json` (`index`, `offsets`, `head` on
/// the storage). `false` (and no index) if it is no list this plugin
/// reads.
fn build(
    storage: &dyn Storage,
    json: &str,
    [index, offsets, head]: [&str; 3],
    device: &Device<'_>,
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

    #[cfg(test)]
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
        self.pick(
            storage,
            |_, entry| {
                (entry.name.to_lowercase().contains(&query)
                    || entry.summary.to_lowercase().contains(&query))
                .then_some(0)
            },
            Order::Name,
            usize::MAX,
        )
    }

    /// Each plugin with its place, read through the index once, in order.
    pub(crate) fn scan(&self, storage: &dyn Storage, found: &mut dyn FnMut(usize, Entry)) {
        let path = index_path();
        let mut buf = [0u8; 512];
        let mut line: Vec<u8> = Vec::new();
        let (mut at, mut n) = (0u64, 0usize);
        while let Ok(read) = storage.read_at(&path, at, &mut buf) {
            if read == 0 {
                break;
            }
            at += read as u64;
            for &byte in &buf[..read] {
                if byte != b'\n' {
                    if line.len() <= MAX_LINE {
                        line.push(byte);
                    }
                    continue;
                }
                if let Some(entry) = core::str::from_utf8(&line).ok().and_then(Entry::parse) {
                    found(n, entry);
                }
                line.clear();
                n += 1;
            }
        }
    }

    /// Keeps the plugins `keep` takes (with a rank, for
    /// [`Order::Rank`]), in `order`, `most` of them at most, on the storage
    /// (read back with [`hit`]); how many there are.
    ///
    /// However many plugins there are, little is held at once: by name
    /// they are written as they are read; by a key, the best `most` are
    /// kept while reading where that is few, and otherwise runs of
    /// [`RUN`] are sorted, kept on the storage and merged.
    pub(crate) fn pick(
        &self,
        storage: &dyn Storage,
        mut keep: impl FnMut(usize, &Entry) -> Option<u32>,
        order: Order,
        most: usize,
    ) -> usize {
        let hits = hits_path();
        let mut out = Output::new(storage, &hits);
        if order == Order::Name {
            self.scan(storage, &mut |n, entry| {
                if out.count < most && keep(n, &entry).is_some() {
                    out.push(n as u32);
                }
            });
            return out.finish();
        }
        let key = |entry: &Entry, rank: u32| match order {
            Order::Added => entry.added,
            Order::Updated => entry.updated,
            Order::Name | Order::Rank => rank,
        };
        if most <= RUN {
            // The best few, kept in order while reading.
            let mut best: Vec<Keyed> = Vec::new();
            self.scan(storage, &mut |n, entry| {
                if let Some(rank) = keep(n, &entry) {
                    let item = (key(&entry, rank), n as u32);
                    let at = best.partition_point(|b| before(*b, item));
                    if at < most {
                        if best.len() == most {
                            best.pop();
                        }
                        best.insert(at, item);
                    }
                }
            });
            for (_, n) in best {
                out.push(n);
            }
            return out.finish();
        }
        // Runs sorted on the storage, then merged.
        let runs_path = file("sort.bin");
        let mut runs: Vec<u32> = Vec::new();
        let mut run: Vec<Keyed> = Vec::new();
        let mut kept = storage.write(&runs_path, b"").is_ok();
        let mut flush = |run: &mut Vec<Keyed>, runs: &mut Vec<u32>| {
            run.sort_by(|a, b| order_of(*a, *b));
            let mut bytes = Vec::with_capacity(run.len() * 8);
            for (key, n) in run.iter() {
                bytes.extend_from_slice(&key.to_le_bytes());
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            kept &= storage.append(&runs_path, &bytes).is_ok();
            runs.push(run.len() as u32);
            run.clear();
        };
        self.scan(storage, &mut |n, entry| {
            if let Some(rank) = keep(n, &entry) {
                run.push((key(&entry, rank), n as u32));
                if run.len() == RUN {
                    flush(&mut run, &mut runs);
                }
            }
        });
        if !run.is_empty() {
            flush(&mut run, &mut runs);
        }
        drop(run);
        if !kept {
            let _ = storage.remove(&runs_path);
            return 0;
        }
        let mut cursors: Vec<Cursor> = Vec::with_capacity(runs.len());
        let mut at = 0u64;
        for len in runs {
            cursors.push(Cursor::new(at, len));
            at += u64::from(len) * 8;
        }
        while out.count < most {
            let mut first: Option<(usize, Keyed)> = None;
            for (i, cursor) in cursors.iter_mut().enumerate() {
                if let Some(item) = cursor.head(storage, &runs_path)
                    && first.is_none_or(|(_, best)| before(item, best))
                {
                    first = Some((i, item));
                }
            }
            let Some((i, (_, n))) = first else {
                break;
            };
            cursors[i].next();
            out.push(n);
        }
        let _ = storage.remove(&runs_path);
        out.finish()
    }
}

/// A plugin with the key it is ordered by: (key, its place in the index).
type Keyed = (u32, u32);

/// Plugins ordered by a key in memory at once (8 bytes each): longer lists
/// are sorted in runs of this many on the storage.
pub(crate) const RUN: usize = 512;

/// Entries of a run read at once while runs are merged.
const MERGE_READ: usize = 16;

/// The order of plugins by a key: the highest (newest) first; of one key,
/// in the list's order.
fn order_of(a: Keyed, b: Keyed) -> core::cmp::Ordering {
    b.0.cmp(&a.0).then(a.1.cmp(&b.1))
}

fn before(a: Keyed, b: Keyed) -> bool {
    order_of(a, b) == core::cmp::Ordering::Less
}

/// Where a sorted run on the storage is read while merging.
struct Cursor {
    /// The byte of the next entry not read yet, and how many are left.
    at: u64,
    left: u32,
    read: [Keyed; MERGE_READ],
    /// The next of those read, and how many were.
    next: usize,
    len: usize,
}

impl Cursor {
    fn new(at: u64, left: u32) -> Self {
        Self {
            at,
            left,
            read: [(0, 0); MERGE_READ],
            next: 0,
            len: 0,
        }
    }

    /// The run's first entry not taken yet.
    fn head(&mut self, storage: &dyn Storage, path: &str) -> Option<Keyed> {
        if self.next == self.len {
            let want = (self.left as usize).min(MERGE_READ);
            if want == 0 {
                return None;
            }
            let mut bytes = [0u8; MERGE_READ * 8];
            let mut got = 0;
            while got < want * 8 {
                match storage.read_at(path, self.at + got as u64, &mut bytes[got..want * 8]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got += n,
                }
            }
            let entries = got / 8;
            for (slot, chunk) in self.read.iter_mut().zip(bytes[..entries * 8].chunks(8)) {
                let word = |at: usize| {
                    u32::from_le_bytes([chunk[at], chunk[at + 1], chunk[at + 2], chunk[at + 3]])
                };
                *slot = (word(0), word(4));
            }
            self.at += (entries * 8) as u64;
            self.left = if entries == want {
                self.left - want as u32
            } else {
                0
            };
            self.next = 0;
            self.len = entries;
            if entries == 0 {
                return None;
            }
        }
        Some(self.read[self.next])
    }

    fn next(&mut self) {
        self.next += 1;
    }
}

/// The plugins picked, written to the storage a piece at a time.
struct Output<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    bytes: Vec<u8>,
    count: usize,
    written: bool,
}

impl<'a> Output<'a> {
    fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
        Self {
            written: storage.write(path, b"").is_ok(),
            storage,
            path,
            bytes: Vec::new(),
            count: 0,
        }
    }

    fn push(&mut self, n: u32) {
        self.bytes.extend_from_slice(&n.to_le_bytes());
        self.count += 1;
        if self.bytes.len() >= WRITE_AT {
            self.written &= self.storage.append(self.path, &self.bytes).is_ok();
            self.bytes.clear();
        }
    }

    /// How many were picked; none where they could not be kept.
    fn finish(mut self) -> usize {
        self.written &= self.storage.append(self.path, &self.bytes).is_ok();
        if self.written { self.count } else { 0 }
    }
}

/// The order a view of the plugins shows them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// The list's: by name.
    Name,
    /// Newest first.
    Added,
    /// Most recently updated first.
    Updated,
    /// Highest rank first (as `pick` was given), then by name.
    Rank,
}

impl Order {
    pub(crate) const ALL: [Order; 3] = [Order::Name, Order::Added, Order::Updated];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Order::Name => "name",
            Order::Added => "added",
            Order::Updated => "updated",
            Order::Rank => "rank",
        }
    }

    pub(crate) fn of(key: &str) -> Order {
        Order::ALL
            .into_iter()
            .find(|o| o.key() == key)
            .unwrap_or(Order::Name)
    }
}

/// The plugin of hit `n` of the last search.
pub(crate) fn hit(storage: &dyn Storage, n: usize) -> Option<usize> {
    number_at(storage, &hits_path(), n).map(|n| n as usize)
}

/// Takes the list fetched to `fetched`: its index, kept for the next
/// visits. `None` (and the file gone) if it is no list this plugin reads.
pub(crate) fn take(storage: &dyn Storage, fetched: &str, device: &Device<'_>) -> Option<Index> {
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

/// The plugin of the page fetched to `fetched` from `address`, with its
/// files to install.
pub(crate) fn plugin(
    storage: &dyn Storage,
    fetched: &str,
    address: &str,
    device: &Device<'_>,
) -> Option<Plugin> {
    let read = read_plugin(storage, fetched, device, address);
    let _ = storage.remove(fetched);
    read
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const FILES: [&str; 3] = ["i.txt", "i.bin", "i.head"];
    /// The X4 running a PaperPaw that loads code plugins of this build.
    const X4: Device<'static> = Device {
        name: "x4",
        fingerprint: Some(Fingerprint(0x0123_4567_89ab_cdef)),
        code_file: "code.ppn",
        plugins: None,
    };

    fn plugin(id: &str, group: &str, extra: &str) -> String {
        format!(
            r#"{{"id": "{id}", "name": "Name {id}", "summary": "Does {id}.", "group": "{group}",
  "kind": "data", "author": "A", "licence": "MIT", {extra}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}}}"#
        )
    }

    /// A build for `device` and `abi`.
    fn build_of(device: &str, abi: &str) -> String {
        format!(
            r#"{{"device": "{device}", "abi": "{abi}", "url": "/b/{device}.ppn", "size": 70000, "sha256": "{SHA256}"}}"#
        )
    }

    /// A code plugin with `builds`.
    fn code(id: &str, builds: &[String]) -> String {
        plugin(id, "reading", &format!(r#""author_key": "{KEY}","#))
            .replace(r#""kind": "data""#, r#""kind": "code""#)
            .replace(
                r#""size": 30}"#,
                &format!(r#""size": 30, "builds": [{}]}}"#, builds.join(",")),
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

    /// The plugins of the list for `device` through the index, in its
    /// order, and its runs of groups.
    fn entries_for(
        storage: &MemoryStorage,
        device: &Device<'_>,
    ) -> (Vec<Entry>, Vec<(usize, usize)>) {
        assert!(build(storage, "p.json", FILES, device));
        let index = Index::open(storage, "i.head").unwrap();
        let entries = (0..index.len()).map(|n| entry(storage, n)).collect();
        (entries, index.runs)
    }

    fn entries(storage: &MemoryStorage) -> (Vec<Entry>, Vec<(usize, usize)>) {
        entries_for(storage, &X4)
    }

    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn only_what_fits_is_listed_in_its_order() {
        let storage = list(&[
            plugin("plain", "reading", ""),
            plugin("built", "reading", r#""built_in": true,"#),
            plugin("later", "look", "").replace(r#""formats": 1"#, r#""formats": 2"#),
            plugin("next-sdk", "look", "").replace(r#""sdk": "0.4""#, r#""sdk": "0.9""#),
            plugin("Bad_Id", "look", ""),
            plugin("phone", "look", r#""devices": ["android"],"#),
            plugin("theme", "look", r#""devices": ["x4", "x4-pro"],"#),
            plugin("odd", "elsewhere", ""),
        ]);
        let (entries, runs) = entries(&storage);
        assert_eq!(ids(&entries), ["plain", "theme", "odd"]);
        // Groups as they come, an unknown one under `system`.
        assert_eq!(runs, [(0, 1), (5, 1), (6, 1)]);
        assert_eq!(entries[0].summary, "Does plain.");
        assert_eq!((entries[0].size, entries[0].code), (30, false));
    }

    #[test]
    fn code_plugins_are_listed_where_a_build_fits() {
        let fits = build_of("x4", "0123456789abcdef");
        let storage = list(&[
            code(
                "clock",
                &[build_of("x4-pro", "0123456789abcdef"), fits.clone()],
            ),
            code("other-build", &[build_of("x4", "fedcba9876543210")]),
            code("pro-only", &[build_of("x4-pro", "0123456789abcdef")]),
            code("no-builds", &[]),
        ]);
        let (entries, _) = entries(&storage);
        assert_eq!(ids(&entries), ["clock"]);
        // Its code counted in, and what it is made for.
        let clock = &entries[0];
        assert_eq!(
            (clock.code, clock.size, clock.sdk.as_str()),
            (true, 70_030, "0.4")
        );
        // A PaperPaw that loads no code plugins lists none.
        let without = Device {
            fingerprint: None,
            code_file: "",
            ..X4
        };
        let storage = list(&[code("clock", &[fits])]);
        assert!(entries_for(&storage, &without).0.is_empty());
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
        assert!(!build(&storage, "p.json", FILES, &X4));
        assert!(!build(&storage, "missing.json", FILES, &X4));
        assert!(!storage.exists("i.txt"));
        assert!(Index::open(&storage, "i.head").is_none());
    }

    fn page(files: &str) -> MemoryStorage {
        page_of(files, "data", "")
    }

    fn page_of(files: &str, kind: &str, more: &str) -> MemoryStorage {
        let text = format!(
            r#"{{"format": 2, "plugin": {{"id": "airy", "name": "Airy", "summary": "Roomy.",
  "group": "look", "kind": "{kind}", "author": "A", "licence": "MIT", {more}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 20, "files": [{files}]{}}}}}}}"#,
            if kind == "code" {
                format!(r#", "builds": [{}]"#, build_of("x4", "0123456789abcdef"))
            } else {
                String::new()
            }
        );
        MemoryStorage::new().with("a.json", text)
    }

    fn file(path: &str) -> String {
        format!(
            r#"{{"path": "{path}", "url": "/api/v1/plugins/airy/1.0.0/{path}", "size": 10, "sha1": "{SHA1}"}}"#
        )
    }

    const BASE: &str = "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en";

    #[test]
    fn a_plugins_files_come_with_its_page() {
        let storage = page(&format!("{},{}", file("i18n/de.ini"), file("plugin.ini")));
        let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
        let (entry, package) = (&plugin.entry, &plugin.package);
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
        assert_eq!(plugin.author_key, None);
    }

    #[test]
    fn a_code_plugins_page_brings_its_code_and_its_authors_key() {
        let author = format!(r#""author_key": "{KEY}","#);
        let storage = page_of(&file("plugin.ini"), "code", &author);
        let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
        let code = plugin.package.files.last().unwrap();
        assert_eq!(code.path, "code.ppn");
        assert_eq!(code.url, "https://paperpaw.org/b/x4.ppn");
        assert_eq!(code.digest, Digest::Sha256(SHA256.into()));
        assert_eq!(plugin.author_key, Some([0x11; 32]));
        assert!(plugin.entry.code);
        // Without the author's key, a code plugin is not offered.
        let storage = page_of(&file("plugin.ini"), "code", "");
        assert!(read_plugin(&storage, "a.json", &X4, BASE).is_none());
    }

    #[test]
    fn unsafe_files_are_refused() {
        for bad in ["../evil.ini", "/abs.ini", "a/../../b", ".hidden/x"] {
            let storage = page(&format!("{},{}", file("plugin.ini"), file(bad)));
            let read = read_plugin(&storage, "a.json", &X4, "https://x/");
            assert!(read.is_none(), "{bad}");
        }
        let short = file("plugin.ini").replace(SHA1, "abc");
        assert!(read_plugin(&page(&short), "a.json", &X4, "https://x/").is_none());
        // No plugin.ini.
        assert!(read_plugin(&page(&file("theme.ini")), "a.json", &X4, "https://x/").is_none());
        // More files than a package may have.
        let many: Vec<String> = (0..=MAX_FILES)
            .map(|n| file(&format!("f{n}.ini")))
            .chain([file("plugin.ini")])
            .collect();
        assert!(read_plugin(&page(&many.join(",")), "a.json", &X4, "https://x/").is_none());
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
        // A library's key goes with its pages.
        assert_eq!(
            plugin_url("https://x/lib/p.json?key=1#top", "x4", "en", "airy"),
            "https://x/lib/plugins/airy.json?key=1&device=x4&lang=en"
        );
    }

    #[test]
    fn searches_find_names_and_summaries() {
        let storage = list(&[
            plugin("airy", "look", ""),
            plugin("dark", "look", "").replace("Does dark.", "Night colours for AIRY rooms."),
            plugin("keys", "system", ""),
        ]);
        let index = take(&storage, "p.json", &X4).unwrap();
        assert!(!storage.exists("p.json"));
        assert_eq!(index.search(&storage, " Airy "), 2);
        assert_eq!((hit(&storage, 0), hit(&storage, 1)), (Some(0), Some(1)));
        assert_eq!(hit(&storage, 2), None);
        assert_eq!(index.entry(&storage, 2).unwrap().id, "keys");
        assert_eq!(index.search(&storage, "nothing"), 0);
        // A list fetched anew: the hits of the one before go.
        index.search(&storage, "keys");
        let storage = storage.with("p.json", r#"{"format": 2, "plugins": []}"#);
        let index = take(&storage, "p.json", &X4).unwrap();
        assert_eq!(index.len(), 0);
        assert_eq!(hit(&storage, 0), None);
    }

    #[test]
    fn dates_are_read_as_numbers() {
        assert_eq!(date("2026-10-07"), 20261007);
        assert_eq!(date("2026-10-07T12:00:00Z"), 20261007);
        assert_eq!(date("2024"), 20240101);
        assert_eq!(date("soon"), 0);
        assert_eq!(date(""), 0);
    }

    #[test]
    fn a_page_tells_its_source_author_and_screenshots() {
        let more = r#""source": "https://example.org/airy", "author_since": "2024-03-02",
  "author_plugins": 3, "screenshots": [
    {"url": "s/1.png", "width": 360, "height": 600, "size": 2000},
    {"url": "s/2.webp", "width": 360, "height": 600, "size": 2000},
    {"url": "s/3.JPG?v=2", "width": 360, "height": 600, "size": 2000},
    {"url": "s/4.png", "width": 4000, "height": 600, "size": 2000},
    {"url": "http://example.org/5.jpeg", "width": 10, "height": 10, "size": 20},
    {"url": "s/6.png", "width": 10, "height": 10, "size": 20}],"#;
        let storage = page_of(&file("plugin.ini"), "data", more);
        let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
        let more = &plugin.more;
        assert_eq!(more.source, "https://example.org/airy");
        assert_eq!((more.author_since, more.author_plugins), (20240302, 3));
        // Data plugins carry no build to check.
        assert_eq!(more.verified, "");
        // Only what PaperPaw decodes, not too large, four at most; their
        // addresses relative to the page's.
        let shots: Vec<&str> = more.shots.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(
            shots,
            [
                "https://paperpaw.org/api/v1/plugins/s/1.png",
                "https://paperpaw.org/api/v1/plugins/s/3.JPG?v=2",
                "http://example.org/5.jpeg",
                "https://paperpaw.org/api/v1/plugins/s/6.png"
            ]
        );
        assert_eq!((more.heap, more.image_len), (None, None));
    }

    #[test]
    fn a_code_plugins_page_tells_whether_its_build_was_checked() {
        let author = format!(r#""author_key": "{KEY}", "source": "https://example.org/clock","#);
        let with = |latest: &str, build: &str| {
            let storage = page_of(&file("plugin.ini"), "code", &author);
            let text = paperpaw_sdk::read_file(&storage, "a.json", 1 << 16).unwrap();
            let text = String::from_utf8(text)
                .unwrap()
                .replace(r#""latest": {"#, &format!(r#""latest": {{{latest}"#))
                .replace(r#""device": "x4","#, &format!(r#""device": "x4", {build}"#));
            let storage = MemoryStorage::new().with("a.json", text);
            read_plugin(&storage, "a.json", &X4, BASE).unwrap().more
        };
        let more = with(
            r#""verified": true, "heap": 40000,
  "source": {"url": "https://example.org/clock/src/tag/v1.0.0"},"#,
            r#""image_len": 9000, "rebuilt": "matches","#,
        );
        assert_eq!(more.verified, "verified");
        assert_eq!(more.source, "https://example.org/clock/src/tag/v1.0.0");
        assert_eq!((more.heap, more.image_len), (Some(40000), Some(9000)));
        let more = with(r#""verified": false,"#, r#""rebuilt": "differs","#);
        assert_eq!(more.verified, "failed");
        assert_eq!(more.source, "https://example.org/clock");
        let more = with(r#""verified": false,"#, r#""rebuilt": "none","#);
        assert_eq!(more.verified, "unverified");
    }

    #[test]
    fn plugins_are_picked_in_the_order_asked() {
        let dated = |id: &str, added: &str, date: &str| {
            plugin(id, "look", "")
                .replace(r#""kind""#, &format!(r#""added": "{added}", "kind""#))
                .replace(r#""size": 30"#, &format!(r#""size": 30, "date": "{date}""#))
        };
        let storage = list(&[
            dated("a", "2025-01-01", "2026-03-01"),
            dated("b", "2026-01-01", "2026-01-01"),
            dated("c", "2025-06-01", "2026-09-01"),
            plugin("d", "system", ""),
        ]);
        let index = take(&storage, "p.json", &X4).unwrap();
        let picked = |order: Order, most: usize| {
            let count = index.pick(
                &storage,
                |_, e| (e.group == group("look")).then_some(u32::from(e.id == "c")),
                order,
                most,
            );
            (0..count)
                .map(|n| index.entry(&storage, hit(&storage, n).unwrap()).unwrap().id)
                .collect::<Vec<_>>()
        };
        assert_eq!(picked(Order::Name, usize::MAX), ["a", "b", "c"]);
        assert_eq!(picked(Order::Name, 2), ["a", "b"]);
        assert_eq!(picked(Order::Added, usize::MAX), ["b", "c", "a"]);
        assert_eq!(picked(Order::Added, 1), ["b"]);
        assert_eq!(picked(Order::Updated, usize::MAX), ["c", "a", "b"]);
        assert_eq!(picked(Order::Rank, usize::MAX), ["c", "a", "b"]);
        // Read through the index in one pass, every plugin.
        let mut ids = Vec::new();
        index.scan(&storage, &mut |n, e| ids.push((n, e.id)));
        assert_eq!(ids.len(), 4);
        assert_eq!(ids[3], (3, String::from("d")));
    }

    #[test]
    fn long_lists_are_sorted_in_runs_on_the_storage() {
        // More plugins than a run holds, their dates in no order, many of
        // one day.
        let count = 3 * RUN + 77;
        let day = |i: usize| 20250101 + ((i * 7919) % 400) as u32;
        let plugins: Vec<String> = (0..count)
            .map(|i| {
                let date = day(i).to_string();
                let date = format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]);
                plugin(&format!("p{i:05}"), "look", "")
                    .replace(r#""kind""#, &format!(r#""added": "{date}", "kind""#))
            })
            .collect();
        let storage = list(&plugins);
        let index = take(&storage, "p.json", &X4).unwrap();
        let mut expected: Vec<(u32, u32)> = (0..count).map(|i| (day(i), i as u32)).collect();
        expected.sort_by(|a, b| order_of(*a, *b));
        for most in [usize::MAX, RUN + 1, 30] {
            let picked = index.pick(&storage, |_, _| Some(0), Order::Added, most);
            let want: Vec<u32> = expected.iter().take(most).map(|(_, n)| *n).collect();
            assert_eq!(picked, want.len());
            let got: Vec<u32> = (0..picked)
                .map(|h| hit(&storage, h).unwrap() as u32)
                .collect();
            assert_eq!(got, want, "{most}");
        }
        // The runs are gone again.
        assert!(!storage.exists(&file("sort.bin")));
    }

    #[test]
    fn keys_are_read_from_hex() {
        assert_eq!(key(KEY), Some([0x11; 32]));
        assert_eq!(key("11"), None);
        assert_eq!(key(&"zz".repeat(32)), None);
    }
}
