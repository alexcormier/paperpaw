//! A data plugin folder looked at as files: what the installer, the
//! kernel's loader and the plugin library take, without running anything.
//!
//! The rules come from three places, kept in step with them:
//!
//! - the SDK's `install` module, which the store and the fonts browser
//!   download with: [`is_plugin_name`], [`is_safe_path`], [`MAX_FILES`]
//!   and [`Package::is_valid`];
//! - the kernel's loader (`crates/paperpaw-core/src/data_plugin.rs`):
//!   which files it reads, how large they may be, `[extends]` and
//!   `[family]` in `plugin.ini`;
//! - the plugin library at paperpaw.org (its `packages.rs` and
//!   `catalogue.rs`): the kinds of files a package may hold, its size,
//!   and an id and a version named in `plugin.ini` that its catalogue
//!   entry repeats.

use std::path::{Path, PathBuf};

use paperpaw_sdk::install::{self, MAX_FILES, Package, PackageFile};
use paperpaw_sdk::{DATA_FORMATS, Id, Version};

use crate::report::{Finding, Report};
use crate::target::DataPlugin;

/// Largest data file the kernel reads (a catalog, a key map, the
/// settings): larger ones it leaves out.
pub const MAX_DATA_FILE: u64 = 64 * 1024;
/// Largest `plugin.ini` the kernel reads.
pub const MAX_MANIFEST: u64 = 4 * 1024;
/// The kinds of files the plugin library takes in a package.
pub const WEBSITE_KINDS: [&str; 8] = ["ini", "css", "ttf", "otf", "ttc", "txt", "md", "png"];
/// The most a package may hold unpacked, at the plugin library.
pub const WEBSITE_MAX_CONTENT: u64 = 40 * 1024 * 1024;
/// The longest id the plugin library takes.
const WEBSITE_MAX_ID: usize = 48;

/// The keys of `[plugin]` the kernel reads.
const PLUGIN_KEYS: [&str; 5] = ["id", "name", "description", "version", "formats"];
/// The faces of a family, as `[family]` names them.
const FACES: [&str; 4] = ["regular", "bold", "italic", "bold-italic"];
/// Files at the top of the folder that the kernel reads.
const TOP_FILES: [&str; 5] = [
    "plugin.ini",
    "settings.ini",
    "keymap.ini",
    "styles.css",
    "presets.ini",
];

/// One file of the folder.
#[derive(Debug, Clone)]
pub struct File {
    /// Its path in the folder, with `/`.
    pub path: String,
    pub size: u64,
    pub full: PathBuf,
}

/// A line of an ini file: section, key, value and line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub section: String,
    pub key: String,
    pub value: String,
    pub line: u32,
}

/// What `plugin.ini` says.
#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    /// `formats`, where given and a number.
    pub formats: Option<u32>,
    /// `[extends]`: a pack in `fonts/` and the font it extends.
    pub extends: Vec<Line>,
    /// `[family]`: its name and faces.
    pub family: Vec<Line>,
}

/// A data plugin folder as read.
#[derive(Debug, Clone, Default)]
pub struct Folder {
    pub files: Vec<File>,
    pub manifest: Manifest,
    /// `settings.ini`, line by line.
    pub settings: Vec<Line>,
    /// `presets.ini`, line by line.
    pub presets: Vec<Line>,
    /// `keymap.ini`, line by line.
    pub keymap: Vec<Line>,
    pub has_presets: bool,
}

impl Folder {
    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|f| f.path == path)
    }

    /// The id it is installed as: `plugin.ini`'s, else the folder's name.
    pub fn id<'a>(&'a self, plugin: &'a DataPlugin) -> &'a str {
        self.manifest.id.as_deref().unwrap_or(&plugin.id)
    }
}

/// Checks `plugin`'s folder into `report`; what was read, unless the
/// folder could not be read at all.
pub fn check(plugin: &DataPlugin, report: &mut Report) -> Option<Folder> {
    let id = plugin.id.clone();
    let mut add = |finding: Finding| report.add(finding.plugin(id.clone()));
    let mut files = Vec::new();
    if let Err(error) = walk(&plugin.folder, &plugin.folder, &mut files) {
        add(Finding::error("package.files", "", error));
        return None;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut folder = Folder {
        files,
        ..Folder::default()
    };
    manifest(plugin, &mut folder, &mut add);
    folder.settings = lines(&folder, "settings.ini", &mut add);
    folder.presets = lines(&folder, "presets.ini", &mut add);
    folder.keymap = lines(&folder, "keymap.ini", &mut add);
    folder.has_presets = folder.file("presets.ini").is_some();
    files_and_kinds(plugin, &folder, &mut add);
    catalogs(&folder, &mut add);
    fonts(&folder, &mut add);
    pictures(plugin, &folder, &mut add);
    package(plugin, &folder, &mut add);
    Some(folder)
}

/// Every file under `dir`, hidden ones too (the installer and the website
/// see them).
fn walk(root: &Path, dir: &Path, out: &mut Vec<File>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let full = entry.path();
        let meta = entry
            .metadata()
            .map_err(|e| format!("{}: {e}", full.display()))?;
        if meta.is_dir() {
            walk(root, &full, out)?;
        } else if meta.is_file() {
            let relative = full.strip_prefix(root).unwrap_or(&full);
            out.push(File {
                path: relative.to_string_lossy().replace('\\', "/"),
                size: meta.len(),
                full,
            });
        }
    }
    Ok(())
}

/// The extension of `path`, lower-case.
fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, x)| x.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A text file the kernel reads: present, small enough, UTF-8.
fn text(folder: &Folder, path: &str, max: u64, add: &mut dyn FnMut(Finding)) -> Option<String> {
    let file = folder.file(path)?;
    if file.size > max {
        add(Finding::error(
            "package.files",
            path,
            format!(
                "{} KB, but PaperPaw reads such files only up to {} KB and leaves this one out; make it smaller or split it.",
                file.size.div_ceil(1024),
                max / 1024
            ),
        ));
        return None;
    }
    let bytes = match std::fs::read(&file.full) {
        Ok(bytes) => bytes,
        Err(error) => {
            add(Finding::error(
                "package.files",
                path,
                format!("unreadable: {error}"),
            ));
            return None;
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_) => {
            add(Finding::error(
                "package.files",
                path,
                "Not UTF-8 text, so PaperPaw leaves it out; save it as UTF-8.",
            ));
            None
        }
    }
}

/// An ini file's lines; syntax errors reported.
fn lines(folder: &Folder, path: &str, add: &mut dyn FnMut(Finding)) -> Vec<Line> {
    let Some(source) = text(folder, path, MAX_DATA_FILE, add) else {
        return Vec::new();
    };
    parse(&source, path, add)
}

fn parse(source: &str, path: &str, add: &mut dyn FnMut(Finding)) -> Vec<Line> {
    let mut out = Vec::new();
    for entry in paperpaw_config::parse(source) {
        match entry {
            Ok(entry) => out.push(Line {
                section: entry.section.to_string(),
                key: entry.key.to_string(),
                value: entry.value.as_str().into_owned(),
                line: entry.line,
            }),
            Err(error) => add(Finding::error(
                "package.syntax",
                format!("{path}:{}", error.line),
                format!(
                    "{error}; PaperPaw refuses the file (and with plugin.ini or settings.ini the whole plugin)."
                ),
            )),
        }
    }
    out
}

/// `plugin.ini`: present, readable, its `[plugin]` section as the kernel
/// and the plugin library want it.
fn manifest(plugin: &DataPlugin, folder: &mut Folder, add: &mut dyn FnMut(Finding)) {
    let place = "plugin.ini";
    if folder.file(place).is_none() {
        add(Finding::error(
            "package.manifest",
            place,
            "No plugin.ini: neither the device nor the plugin library takes the folder as a plugin.",
        ));
        return;
    }
    let Some(source) = text(folder, place, MAX_MANIFEST, add) else {
        return;
    };
    let lines = parse(&source, place, add);
    let manifest = &mut folder.manifest;
    for line in &lines {
        let at = format!("{place}:{}", line.line);
        match line.section.as_str() {
            "plugin" => {
                let value = Some(line.value.trim().to_string());
                match line.key.as_str() {
                    "id" => manifest.id = value,
                    "name" => manifest.name = value,
                    "description" => manifest.description = value,
                    "version" => manifest.version = value,
                    "formats" => match line.value.trim().parse::<u32>() {
                        Ok(n) => manifest.formats = Some(n),
                        Err(_) => add(Finding::error(
                            "package.formats",
                            at,
                            format!(
                                "`formats = {}` is no number, so PaperPaw refuses the plugin; write `formats = {DATA_FORMATS}`.",
                                line.value
                            ),
                        )),
                    },
                    other => add(Finding::warning(
                        "package.manifest",
                        at,
                        format!(
                            "`{other}` is not read in [plugin] (only {}); a typo?",
                            PLUGIN_KEYS.join(", ")
                        ),
                    )),
                }
            }
            "extends" => manifest.extends.push(line.clone()),
            "family" => manifest.family.push(line.clone()),
            "" => add(Finding::warning(
                "package.manifest",
                at,
                format!(
                    "`{}` is outside any section and not read; put it under [plugin].",
                    line.key
                ),
            )),
            other => add(Finding::warning(
                "package.manifest",
                at,
                format!(
                    "[{other}] is not read in plugin.ini (only [plugin], [extends] and [family])."
                ),
            )),
        }
    }
    let manifest = &folder.manifest;
    match &manifest.id {
        None => add(Finding::error(
            "package.id",
            place,
            format!(
                "plugin.ini names no id: the device takes the folder's name, but the plugin library needs `id = {}` in [plugin].",
                plugin.id
            ),
        )),
        Some(id) => {
            if !install::is_plugin_name(id) {
                add(Finding::error(
                    "package.id",
                    place,
                    format!(
                        "`{id}` is no plugin id: lower-case letters, digits and `-`, at most 40, not starting with `-`."
                    ),
                ));
            } else if id.ends_with('-') || id.len() > WEBSITE_MAX_ID {
                add(Finding::error(
                    "package.id",
                    place,
                    format!(
                        "The plugin library takes no id like `{id}`: at most {WEBSITE_MAX_ID} characters, not ending with `-`."
                    ),
                ));
            }
            let folder_name = plugin
                .folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !folder_name.is_empty() && &folder_name != id {
                add(Finding::info(
                    "package.id",
                    place,
                    format!(
                        "The folder is named `{folder_name}`, the id is `{id}`: the installer puts it in a folder `{id}`."
                    ),
                ));
            }
        }
    }
    match &manifest.version {
        None => add(Finding::error(
            "package.version",
            place,
            "plugin.ini names no version: the device takes 0.1.0, but the plugin library needs `version = 1.0.0` (or so) in [plugin], the same as its catalogue entry.",
        )),
        Some(text) if Version::parse(text).is_none() => add(Finding::error(
            "package.version",
            place,
            format!(
                "`version = {text}` is no version, so PaperPaw refuses the plugin; write it like 1.0.0."
            ),
        )),
        Some(text) if text.split('.').count() != 3 => add(Finding::error(
            "package.version",
            place,
            format!("The plugin library takes versions like 1.2.0 only, not `{text}`."),
        )),
        Some(_) => {}
    }
    if manifest.name.as_deref().is_none_or(str::is_empty) {
        add(Finding::warning(
            "package.manifest",
            place,
            "No name in [plugin]: the plugin manager shows its id instead.",
        ));
    }
    if manifest.description.as_deref().is_none_or(str::is_empty) {
        add(Finding::warning(
            "package.manifest",
            place,
            "No description in [plugin]: the plugin manager shows nothing under its name.",
        ));
    }
    match manifest.formats {
        Some(n) if n > DATA_FORMATS => add(Finding::error(
            "package.formats",
            place,
            format!(
                "Made for data formats {n}, but this PaperPaw reads {DATA_FORMATS} and leaves the plugin out."
            ),
        )),
        Some(0) => add(Finding::warning(
            "package.formats",
            place,
            format!(
                "`formats = 0` is no data format; the first is 1, write `formats = {DATA_FORMATS}`."
            ),
        )),
        Some(_) => {}
        None if lines
            .iter()
            .any(|l| l.section == "plugin" && l.key == "formats") => {}
        None => add(Finding::info(
            "package.formats",
            place,
            format!("No `formats`: read as 1; `formats = {DATA_FORMATS}` says it outright."),
        )),
    }
    if !folder.manifest.family.is_empty() {
        family(folder, add);
    }
    for line in &folder.manifest.extends {
        let at = format!("{place}:{}", line.line);
        let path = format!("fonts/{}", line.key);
        if !install::is_safe_path(&path) || line.key.contains('/') {
            add(Finding::error(
                "package.fonts",
                at.clone(),
                format!(
                    "`{}` is no file name in fonts/: name a pack there, e.g. `cyrillic.ppfp = fonts-default.literata`.",
                    line.key
                ),
            ));
        } else if folder.file(&path).is_none() {
            add(Finding::error(
                "package.fonts",
                at.clone(),
                format!("[extends] names {path}, which is not in the folder."),
            ));
        }
        if Id::parse(line.value.trim()).is_err() {
            add(Finding::error(
                "package.fonts",
                at,
                format!(
                    "`{}` is no font id, so PaperPaw refuses the plugin; name one such as fonts-default.literata.",
                    line.value
                ),
            ));
        }
    }
}

/// `[family]`: a name and a regular face, faces that are files.
fn family(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let place = "plugin.ini";
    let lines = &folder.manifest.family;
    let name = lines
        .iter()
        .find(|l| l.key == "name")
        .is_some_and(|l| !l.value.trim().is_empty());
    let regular = lines
        .iter()
        .any(|l| l.key == "regular" && !l.value.trim().is_empty());
    if !name || !regular {
        add(Finding::error(
            "package.fonts",
            place,
            "[family] needs a name and a regular face; without them PaperPaw leaves the family out.",
        ));
    }
    for line in lines {
        let at = format!("{place}:{}", line.line);
        if line.key == "name" {
            continue;
        }
        if !FACES.contains(&line.key.as_str()) {
            add(Finding::warning(
                "package.fonts",
                at,
                format!(
                    "`{}` is no face of a family (only {}); it is not read.",
                    line.key,
                    FACES.join(", ")
                ),
            ));
            continue;
        }
        let value = line.value.trim();
        let path = format!("fonts/{value}");
        if value.contains("..") || !install::is_safe_path(&path) {
            add(Finding::error(
                "package.fonts",
                at,
                format!("`{value}` is no file in fonts/; PaperPaw does not read it."),
            ));
        } else if folder.file(&path).is_none() {
            add(Finding::error(
                "package.fonts",
                at,
                format!(
                    "The {} face is {path}, which is not in the folder.",
                    line.key
                ),
            ));
        }
    }
}

/// Whether the kernel reads `path` (and as what), given `plugin.ini`.
fn read_as(folder: &Folder, path: &str, id: &str) -> Option<&'static str> {
    if TOP_FILES.contains(&path) {
        return Some("data");
    }
    let parts: Vec<&str> = path.split('/').collect();
    let ext = extension(path);
    if parts.len() == 2 && parts[0] == "i18n" && path.ends_with(".ini") {
        return Some("catalog");
    }
    if parts[0] == "fonts" {
        let named = folder
            .manifest
            .extends
            .iter()
            .any(|l| format!("fonts/{}", l.key) == path)
            || folder
                .manifest
                .family
                .iter()
                .any(|l| l.key != "name" && format!("fonts/{}", l.value.trim()) == path);
        if named {
            return Some("font");
        }
        let scalable = ["ttf", "otf", "ttc"].contains(&ext.as_str());
        let shallow = parts.len() == 2 || (parts.len() == 3 && !parts[1].starts_with('.'));
        if scalable && shallow {
            return Some("font");
        }
    }
    let named_by_settings = folder
        .settings
        .iter()
        .any(|l| l.value.trim() == format!("plugins/{id}/{path}"));
    if named_by_settings {
        return Some("picture");
    }
    None
}

/// Every file: a path the installer takes, a kind the website takes, one
/// the kernel reads; their count and size.
fn files_and_kinds(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin).to_string();
    for file in &folder.files {
        let path = file.path.as_str();
        if !install::is_safe_path(path) {
            let hidden = path.split('/').any(|p| p.starts_with('.'));
            add(Finding::error(
                "package.files",
                path,
                if hidden {
                    "A hidden file: the installer takes no names starting with `.`; remove it from the folder.".to_string()
                } else {
                    "The installer takes only paths of letters, digits, `-`, `_` and `.` (at most 100 characters); rename it.".to_string()
                },
            ));
        }
        let ext = extension(path);
        if !WEBSITE_KINDS.contains(&ext.as_str()) {
            let message = if ext == "ppfp" {
                format!(
                    "The plugin library at paperpaw.org takes no font packs (.ppfp) in a plugin, only {} files; a font's packs come from its font catalogue.",
                    WEBSITE_KINDS.join(", ")
                )
            } else {
                format!(
                    "The plugin library at paperpaw.org takes only {} files in a data plugin.",
                    WEBSITE_KINDS.join(", ")
                )
            };
            add(Finding::error("package.website", path, message));
        }
        if read_as(folder, path, &id).is_none() && !["txt", "md"].contains(&ext.as_str()) {
            let hint = match ext.as_str() {
                "ini" => {
                    " (it reads plugin.ini, settings.ini, keymap.ini, presets.ini and i18n/*.ini)"
                }
                "css" => " (it reads styles.css)",
                "ppfp" => " (packs are read where [extends] or [family] in plugin.ini names them)",
                "ttf" | "otf" | "ttc" => " (fonts are read in fonts/ and one folder below it)",
                "png" => {
                    " (a picture is read where settings.ini names it, e.g. `logo = plugins/<id>/logo.png`)"
                }
                _ => "",
            };
            add(Finding::warning(
                "package.files",
                path,
                format!("PaperPaw does not read this file{hint}; a typo in its name or place?"),
            ));
        }
    }
    if folder.files.len() > MAX_FILES {
        add(Finding::error(
            "package.files",
            "",
            format!(
                "{} files, but the installer takes at most {MAX_FILES}; fewer files (e.g. fewer catalogs or fonts) fit.",
                folder.files.len()
            ),
        ));
    }
    let total: u64 = folder.files.iter().map(|f| f.size).sum();
    if total > WEBSITE_MAX_CONTENT {
        add(Finding::error(
            "package.website",
            "",
            format!(
                "{} MB unpacked, but the plugin library takes at most {} MB.",
                total / (1024 * 1024),
                WEBSITE_MAX_CONTENT / (1024 * 1024)
            ),
        ));
    }
    let _ = text(folder, "styles.css", MAX_DATA_FILE, add);
}

/// Translation catalogs: read by the kernel and the website alike.
fn catalogs(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    for file in &folder.files {
        let path = file.path.as_str();
        if !(path.starts_with("i18n/") && path.ends_with(".ini") && path.matches('/').count() == 1)
        {
            continue;
        }
        let Some(source) = text(folder, path, MAX_DATA_FILE, add) else {
            continue;
        };
        if let Err(error) = paperpaw_i18n::Catalog::parse(source) {
            add(Finding::error(
                "package.i18n",
                path,
                format!(
                    "{error}: PaperPaw does not load the plugin, and the plugin library refuses it."
                ),
            ));
        }
    }
}

/// Font files and packs are what they say they are.
fn fonts(folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let mut scalable = false;
    let mut packs = false;
    for file in &folder.files {
        let ext = extension(&file.path);
        let expected: &[&[u8]] = match ext.as_str() {
            "ttf" => &[b"\x00\x01\x00\x00", b"true", b"OTTO"],
            "otf" => &[b"OTTO", b"\x00\x01\x00\x00"],
            "ttc" => &[b"ttcf"],
            "ppfp" => &[b"PPFP", b"QIFP"],
            _ => continue,
        };
        if ext == "ppfp" {
            packs = true;
        } else {
            scalable = true;
        }
        let head = head(&file.full, 4);
        if !expected.iter().any(|magic| head.starts_with(magic)) {
            add(Finding::error(
                "package.fonts",
                file.path.as_str(),
                format!("Not a .{ext} file inside, so PaperPaw cannot draw with it."),
            ));
        }
    }
    if scalable && !packs {
        add(Finding::info(
            "package.fonts",
            "fonts/",
            "Font files (.ttf, .otf, .ttc) are drawn where scalable fonts are (Android, the emulator), not on the X4, which draws packs (.ppfp).",
        ));
    }
}

/// Pictures are PNG, and those settings name are there.
fn pictures(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin);
    for file in &folder.files {
        if extension(&file.path) == "png" && !head(&file.full, 8).starts_with(b"\x89PNG\r\n\x1a\n")
        {
            add(Finding::error(
                "package.pictures",
                file.path.as_str(),
                "Not a PNG picture inside; save it as PNG.",
            ));
        }
    }
    let own = format!("plugins/{id}/");
    for line in &folder.settings {
        let value = line.value.trim();
        let Some(path) = value.strip_prefix("plugins/") else {
            continue;
        };
        let at = format!("settings.ini:{}", line.line);
        match value.strip_prefix(&own) {
            Some(path) if folder.file(path).is_none() => add(Finding::error(
                "package.pictures",
                at,
                format!("`{}` names {path}, which is not in the folder.", line.key),
            )),
            Some(_) => {}
            None => add(Finding::warning(
                "package.pictures",
                at,
                format!(
                    "`{}` names plugins/{path}, outside this plugin's own folder ({own}); installed, it is `{own}…`.",
                    line.key
                ),
            )),
        }
    }
}

/// The installer's own check of the package the store would download,
/// and its size.
fn package(plugin: &DataPlugin, folder: &Folder, add: &mut dyn FnMut(Finding)) {
    let id = folder.id(plugin).to_string();
    let files: Vec<PackageFile> = folder
        .files
        .iter()
        .map(|file| PackageFile {
            path: file.path.clone(),
            url: format!(
                "https://paperpaw.org/api/v1/plugins/{id}/1.0.0/{}",
                file.path
            ),
            size: file.size,
            sha1: std::fs::read(&file.full)
                .map(|bytes| paperpaw_digest::sha1::hex(&bytes))
                .unwrap_or_default(),
        })
        .collect();
    let package = Package {
        plugin: id.clone(),
        name: folder.manifest.name.clone().unwrap_or_else(|| id.clone()),
        files,
    };
    if !package.is_valid() {
        add(Finding::error(
            "package.files",
            "",
            "The SDK's installer refuses this package (see the findings above: id, paths, file count, plugin.ini).",
        ));
    }
    add(Finding::info(
        "package.size",
        "",
        format!(
            "{} files, {} to download.",
            package.files.len(),
            size_text(package.size())
        ),
    ));
}

/// The first `n` bytes of a file (fewer if it is shorter).
fn head(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0; n];
    let read = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    buf.truncate(read);
    buf
}

/// `2.3 KB`, `512 bytes`, `4.1 MB`.
pub fn size_text(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::Level;

    fn folder(name: &str, files: &[(&str, &[u8])]) -> DataPlugin {
        let root = std::env::temp_dir()
            .join(format!("paperpaw-dev-folder-{}", std::process::id()))
            .join(name);
        let _ = std::fs::remove_dir_all(&root);
        for (path, bytes) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        DataPlugin {
            id: crate::target::data_plugin_id(&root),
            folder: root,
        }
    }

    fn findings(plugin: &DataPlugin) -> Vec<(Level, &'static str, String, String)> {
        let mut report = Report::default();
        check(plugin, &mut report);
        let _ = std::fs::remove_dir_all(&plugin.folder);
        report
            .findings
            .into_iter()
            .map(|f| (f.level, f.check, f.place, f.message))
            .collect()
    }

    #[test]
    fn a_sound_folder_passes() {
        let plugin = folder(
            "sound",
            &[
                (
                    "plugin.ini",
                    b"[plugin]\nid = sound\nname = Sound\ndescription = A test.\nversion = 1.0.0\nformats = 1\n",
                ),
                ("settings.ini", b"[core]\nui-size = 14\n"),
                (
                    "i18n/de.ini",
                    b"[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nsound.plugin.name = Klang\n",
                ),
                ("README.md", b"# Sound\n"),
            ],
        );
        let found = findings(&plugin);
        assert!(found.iter().all(|f| f.0 == Level::Info), "{found:#?}");
    }

    #[test]
    fn what_the_installer_and_the_website_refuse() {
        let plugin = folder(
            "bad",
            &[
                (
                    "plugin.ini",
                    b"[plugin]\nname = Bad\nversion = 1.0\nformats = 9\nsdk = 0.4\n",
                ),
                (".DS_Store", b""),
                ("run.exe", b"MZ"),
                ("fonts/a.ppfp", b"PPFP"),
                ("fonts/b.ttf", b"nope"),
                ("style.css", b"p {}"),
                ("i18n/de.ini", b"[messages]\nx = y\n"),
                ("settings.ini", b"[core]\nlogo = plugins/bad/logo.png\n"),
            ],
        );
        let found = findings(&plugin);
        let has = |check: &str, place: &str, level: Level| {
            found
                .iter()
                .any(|f| f.1 == check && f.2 == place && f.0 == level)
        };
        assert!(has("package.id", "plugin.ini", Level::Error), "{found:#?}");
        assert!(has("package.version", "plugin.ini", Level::Error));
        assert!(has("package.formats", "plugin.ini", Level::Error));
        assert!(has("package.manifest", "plugin.ini:5", Level::Warning));
        assert!(has("package.files", ".DS_Store", Level::Error));
        assert!(has("package.website", "run.exe", Level::Error));
        assert!(has("package.website", "fonts/a.ppfp", Level::Error));
        assert!(has("package.files", "fonts/a.ppfp", Level::Warning));
        assert!(has("package.fonts", "fonts/b.ttf", Level::Error));
        assert!(has("package.files", "style.css", Level::Warning));
        assert!(has("package.i18n", "i18n/de.ini", Level::Error));
        assert!(has("package.pictures", "settings.ini:2", Level::Error));
        assert!(
            has("package.files", "", Level::Error),
            "the installer refuses it"
        );
    }

    #[test]
    fn families_and_packs_are_named() {
        let plugin = folder(
            "font-round",
            &[
                (
                    "plugin.ini",
                    b"[plugin]\nid = font-round\nname = Round\ndescription = A font.\nversion = 1.0.0\n[family]\nname = Round\nbold = bold.ppfp\noblique = x.ppfp\n[extends]\ncyrillic.ppfp = Not An Id\n",
                ),
                ("fonts/bold.ppfp", b"PPFP...."),
            ],
        );
        let found = findings(&plugin);
        let at = |place: &str| found.iter().filter(|f| f.2 == place).count();
        // No regular face; no such file to extend with, nor such a font.
        assert!(at("plugin.ini") >= 1, "{found:#?}");
        assert!(at("plugin.ini:9") == 1, "{found:#?}");
        assert!(at("plugin.ini:11") == 2, "{found:#?}");
        assert!(
            !found
                .iter()
                .any(|f| f.2 == "fonts/bold.ppfp" && f.1 == "package.files"),
            "a face is read: {found:#?}"
        );
    }
}
