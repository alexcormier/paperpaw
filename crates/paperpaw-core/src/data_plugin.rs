//! Data plugins: folders on the storage with a `plugin.ini` and data
//! files, no code. They are installed by copying a folder into a plugin
//! folder (e.g. `plugins/` on an SD card) and work on every device.
//!
//! ```text
//! plugins/large-print/
//!   plugin.ini     [plugin] id, name, description, version, formats
//!   i18n/*.ini     translation catalogs (also whole new languages)
//!   keymap.ini     key bindings
//!   settings.ini   new defaults for any plugin's settings ([reader] font-size = 14)
//!   fonts/*.ttf    fonts to read with (also .otf, .ttc), by family
//!   fonts/*.ppfp   packs with characters other fonts lack, named in
//!                  plugin.ini: [extends] cyrillic.ppfp = fonts-default.literata
//!   fonts/…        a family's faces, named in plugin.ini: [family] name =
//!                  Fredoka, regular = fredoka.ppfp, bold, italic,
//!                  bold-italic (packs, or .ttf/.otf where those are drawn)
//!   styles.css     a stylesheet for books, after a book's own
//!   presets.ini    ready-made sets of settings, e.g. home screen layouts,
//!                  a section each: [nook] kind = home.layout, name,
//!                  description, then the values (home.page-1 = …)
//! ```
//!
//! The kernel lists them like every other plugin, so they can be switched
//! off in the plugin manager. Files are read once at start-up; only what a
//! plugin contributes stays in memory.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::Storage;
use paperpaw_sdk::FontFace;
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, Version};

/// Largest data file read (a translation catalog, a key map).
const MAX_FILE: usize = 64 * 1024;
/// Largest `plugin.ini`.
const MAX_MANIFEST: usize = 4 * 1024;
/// The version of the formats of data plugins this PaperPaw reads.
pub(crate) const FORMATS: u32 = paperpaw_sdk::DATA_FORMATS;

/// The styles of a family's faces in `[family]`: (key, bold, italic).
const FACES: [(&str, bool, bool); 4] = [
    ("regular", false, false),
    ("bold", true, false),
    ("italic", false, true),
    ("bold-italic", true, true),
];

/// A plugin made of data files.
pub(crate) struct DataPlugin {
    manifest: Manifest,
    /// Its folder, e.g. `plugins/large-print`.
    folder: String,
    /// Taken by `register`, which runs once.
    translations: RefCell<Vec<String>>,
    keymap: RefCell<Option<String>>,
    styles: RefCell<Option<String>>,
    settings: RefCell<Option<String>>,
    presets: RefCell<Option<String>>,
    /// Paths of font files.
    fonts: RefCell<Vec<String>>,
    /// Packs that extend other fonts: their paths and those fonts' ids.
    extensions: RefCell<Vec<(String, String)>>,
    /// A family's faces: their key in `[family]` and file, the family.
    faces: RefCell<Vec<(&'static str, String)>>,
    family: Option<String>,
}

impl DataPlugin {
    /// Its folder on the storage.
    pub(crate) fn folder(&self) -> &str {
        &self.folder
    }
}

impl Plugin for DataPlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for source in self.translations.take() {
            r.add_translations(Cow::Owned(source))?;
        }
        if let Some(css) = self.styles.take() {
            r.add_stylesheet(Cow::Owned(css))?;
        }
        if let Some(source) = self.keymap.take() {
            r.add_keymap(Cow::Owned(source))?;
        }
        let mut used: Vec<String> = Vec::new();
        for path in self.fonts.take() {
            let mut name = slug(path.rsplit('/').next().unwrap_or(&path));
            // The same file name in two folders.
            while used.contains(&name) {
                name.push('-');
                name.push('2');
            }
            used.push(name.clone());
            let id = Id::parse(&format!("{}.{name}", self.manifest.id)).map_err(|_| {
                RegisterError::Invalid {
                    message: Cow::Owned(format!("{path}: unusable file name")),
                }
            })?;
            r.add_font_file(id, path)?;
        }
        for (path, extends) in self.extensions.take() {
            let name = slug(path.rsplit('/').next().unwrap_or(&path));
            let id = Id::parse(&format!("{}.{name}", self.manifest.id));
            let extends = Id::parse(&extends);
            let (Ok(id), Ok(extends)) = (id, extends) else {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(format!("{path}: unusable name or font")),
                });
            };
            r.add_font_extension(id, path, extends)?;
        }
        if let Some(family) = &self.family {
            for (key, path) in self.faces.take() {
                let (bold, italic) = FACES
                    .iter()
                    .find(|(k, ..)| *k == key)
                    .map_or((false, false), |&(_, b, i)| (b, i));
                let id = Id::parse(&format!("{}.{key}", self.manifest.id)).map_err(|_| {
                    RegisterError::Invalid {
                        message: Cow::Owned(format!("{path}: unusable name")),
                    }
                })?;
                r.add_font_face(FontFace::new(id, path, family.clone(), bold, italic))?;
            }
        }
        if let Some(source) = self.settings.take() {
            for entry in paperpaw_config::parse(&source) {
                let entry = entry.map_err(|error| RegisterError::Parse {
                    line: error.line,
                    message: Cow::Owned(error.to_string()),
                })?;
                let id = Id::parse(&format!("{}.{}", entry.section, entry.key)).map_err(|_| {
                    RegisterError::Parse {
                        line: entry.line,
                        message: Cow::Owned(format!(
                            "invalid setting `{}.{}`",
                            entry.section, entry.key
                        )),
                    }
                })?;
                r.set_setting_default(id, Cow::Owned(entry.value.as_str().into_owned()))?;
            }
        }
        if let Some(source) = self.presets.take() {
            for preset in presets(&self.manifest.id, &source)? {
                r.add_preset(preset)?;
            }
        }
        Ok(())
    }
}

/// The presets of a `presets.ini`, a section each: its `kind`, `name`
/// and `description`, then its values (keys with a dot: settings).
fn presets(plugin: &Id, source: &str) -> Result<Vec<Preset>, RegisterError> {
    let mut out: Vec<Preset> = Vec::new();
    let mut section = "";
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|error| RegisterError::Parse {
            line: error.line,
            message: Cow::Owned(error.to_string()),
        })?;
        if entry.section.is_empty() {
            continue;
        }
        if entry.section != section {
            section = entry.section;
            let id =
                Id::parse(&format!("{plugin}.{section}")).map_err(|_| RegisterError::Parse {
                    line: entry.line,
                    message: Cow::Owned(format!("invalid preset `{section}`")),
                })?;
            out.push(Preset::new(id, plugin.clone(), "", "", String::new()));
        }
        let Some(preset) = out.last_mut() else {
            continue;
        };
        let value = entry.value.as_str();
        match entry.key {
            "kind" => {
                preset.kind = Id::parse(value.trim()).map_err(|_| RegisterError::Parse {
                    line: entry.line,
                    message: Cow::Owned(format!("invalid kind `{value}`")),
                })?;
            }
            "name" => preset.title = Cow::Owned(value.into_owned()),
            "description" => preset.description = Cow::Owned(value.into_owned()),
            key if key.contains('.') => {
                let values = preset.values.to_mut();
                values.push_str(key);
                values.push_str(" = ");
                values.push_str(&value);
                values.push('\n');
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Every data plugin in the folders of `dir`, in name order. Folders
/// without a usable `plugin.ini` are reported through `warn` and skipped.
pub(crate) fn discover(
    storage: &dyn Storage,
    dir: &str,
    warn: &mut dyn FnMut(String),
) -> Vec<DataPlugin> {
    let mut folders: Vec<String> = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        if entry.is_dir && !entry.name.starts_with('.') {
            folders.push(entry.name.to_string());
        }
    });
    folders.sort_unstable();
    folders
        .iter()
        .filter_map(|name| {
            let path = format!("{dir}/{name}");
            load(storage, &path, name).map_err(&mut *warn).ok()
        })
        .collect()
}

/// The plugin in folder `path`, e.g. `plugins/font-fredoka`.
pub(crate) fn load_folder(storage: &dyn Storage, path: &str) -> Result<DataPlugin, String> {
    let path = path.trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    load(storage, path, name)
}

/// The plugin in folder `path`, its id `name` unless its `plugin.ini`
/// names one (for a folder named otherwise, e.g. one downloaded into).
pub(crate) fn load_folder_as(
    storage: &dyn Storage,
    path: &str,
    name: &str,
) -> Result<DataPlugin, String> {
    load(storage, path.trim_end_matches('/'), name)
}

/// The id of the plugin in folder `path` as its `plugin.ini` says it
/// (`name` where it says none), whether or not it could be loaded.
pub(crate) fn folder_plugin_id(storage: &dyn Storage, path: &str, name: &str) -> Option<String> {
    let source = read(storage, &format!("{path}/plugin.ini"), MAX_MANIFEST)?;
    let named = paperpaw_config::parse(&source)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "id")
        .map(|entry| entry.value.as_str().trim().to_string());
    Some(named.unwrap_or_else(|| name.to_string()))
}

/// The folders in `dir` holding the plugin `id` (by their `plugin.ini`, or
/// by their name where it names no id), loadable or not.
pub(crate) fn folders_of(storage: &dyn Storage, dir: &str, id: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        if entry.is_dir && !entry.name.starts_with('.') {
            names.push(entry.name.to_string());
        }
    });
    names
        .into_iter()
        .filter(|name| {
            let path = format!("{dir}/{name}");
            folder_plugin_id(storage, &path, name).as_deref() == Some(id)
        })
        .map(|name| format!("{dir}/{name}"))
        .collect()
}

/// The plugin in folder `path`.
fn load(storage: &dyn Storage, path: &str, folder: &str) -> Result<DataPlugin, String> {
    let manifest = read(storage, &format!("{path}/plugin.ini"), MAX_MANIFEST)
        .ok_or_else(|| format!("{path}: no plugin.ini"))?;
    let source = manifest;
    let manifest =
        parse_manifest(&source, folder).map_err(|e| format!("{path}/plugin.ini: {e}"))?;
    let mut translations = Vec::new();
    let catalogs = format!("{path}/i18n");
    let mut files: Vec<String> = Vec::new();
    let _ = storage.list(&catalogs, &mut |entry| {
        if !entry.is_dir && entry.name.ends_with(".ini") {
            files.push(entry.name.to_string());
        }
    });
    files.sort_unstable();
    for file in files {
        let path = format!("{catalogs}/{file}");
        translations
            .push(read(storage, &path, MAX_FILE).ok_or_else(|| format!("{path}: unreadable"))?);
    }
    let (family, faces) = family(&source, path);
    let mut fonts = font_files(storage, &format!("{path}/fonts"));
    // A family's faces are not fonts of their own as well.
    fonts.retain(|file| !faces.iter().any(|(_, face)| face == file));
    Ok(DataPlugin {
        manifest,
        folder: path.to_string(),
        translations: RefCell::new(translations),
        keymap: RefCell::new(read(storage, &format!("{path}/keymap.ini"), MAX_FILE)),
        styles: RefCell::new(read(storage, &format!("{path}/styles.css"), MAX_FILE)),
        settings: RefCell::new(read(storage, &format!("{path}/settings.ini"), MAX_FILE)),
        presets: RefCell::new(read(storage, &format!("{path}/presets.ini"), MAX_FILE)),
        fonts: RefCell::new(fonts),
        extensions: RefCell::new(extensions(&source, path)),
        faces: RefCell::new(faces),
        family,
    })
}

/// The font files in `dir` and its folders (one level, e.g. a folder per
/// family), by name.
fn font_files(storage: &dyn Storage, dir: &str) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    list_fonts(storage, dir, &mut files, Some(&mut folders));
    for folder in &folders {
        list_fonts(storage, folder, &mut files, None);
    }
    files.sort_unstable();
    files
}

/// Adds the font files in `dir` to `files` and its folders to `folders`.
fn list_fonts(
    storage: &dyn Storage,
    dir: &str,
    files: &mut Vec<String>,
    mut folders: Option<&mut Vec<String>>,
) {
    let _ = storage.list(dir, &mut |entry| {
        let name = entry.name.to_ascii_lowercase();
        let path = format!("{dir}/{}", entry.name);
        if entry.is_dir {
            if let Some(folders) = folders.as_deref_mut()
                && !name.starts_with('.')
            {
                folders.push(path);
            }
        } else if [".ttf", ".otf", ".ttc"]
            .iter()
            .any(|ext| name.ends_with(ext))
        {
            files.push(path);
        }
    });
}

/// The fonts of `dirs` as one plugin, `font-folder`; `None` without any.
pub(crate) fn font_folder(storage: &dyn Storage, dirs: &[String]) -> Option<DataPlugin> {
    let fonts: Vec<String> = dirs.iter().flat_map(|d| font_files(storage, d)).collect();
    if fonts.is_empty() {
        return None;
    }
    Some(DataPlugin {
        manifest: Manifest {
            id: Id::new_static("font-folder"),
            name: Cow::Borrowed("Font folder"),
            description: Cow::Borrowed("The fonts in the fonts folder, to read with."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        },
        // Folders the shell names; not removed by PaperPaw.
        folder: String::new(),
        translations: RefCell::new(Vec::new()),
        keymap: RefCell::new(None),
        styles: RefCell::new(None),
        settings: RefCell::new(None),
        presets: RefCell::new(None),
        fonts: RefCell::new(fonts),
        extensions: RefCell::new(Vec::new()),
        faces: RefCell::new(Vec::new()),
        family: None,
    })
}

/// The packs `[extends]` in a `plugin.ini` names: their paths under the
/// plugin's `fonts` folder and the fonts they extend.
fn extensions(source: &str, path: &str) -> Vec<(String, String)> {
    paperpaw_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "extends")
        .map(|entry| {
            (
                format!("{path}/fonts/{}", entry.key),
                entry.value.as_str().trim().to_string(),
            )
        })
        .collect()
}

/// The family `[family]` in a `plugin.ini` names, with its faces' keys and
/// paths under the plugin's `fonts` folder; no faces without a name or a
/// regular face.
fn family(source: &str, path: &str) -> (Option<String>, Vec<(&'static str, String)>) {
    let mut name = None;
    let mut faces = Vec::new();
    for entry in paperpaw_config::parse(source).filter_map(Result::ok) {
        if entry.section != "family" {
            continue;
        }
        let value = entry.value.as_str().trim().to_string();
        if entry.key == "name" {
            name = Some(value);
        } else if let Some((key, ..)) = FACES.iter().find(|(k, ..)| *k == entry.key)
            && !value.is_empty()
            && !value.contains("..")
        {
            faces.push((*key, format!("{path}/fonts/{value}")));
        }
    }
    match name.filter(|n| !n.is_empty()) {
        Some(name) if faces.iter().any(|(key, _)| *key == "regular") => (Some(name), faces),
        _ => (None, Vec::new()),
    }
}

/// An id segment from a file name: `Book Antiqua-Bold.ttf` gives
/// `book-antiqua-bold`.
fn slug(file: &str) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = String::from(out.trim_end_matches('-'));
    if out.is_empty() {
        String::from("font")
    } else {
        out
    }
}

fn read(storage: &dyn Storage, path: &str, max: usize) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, path, max).ok()?;
    String::from_utf8(data).ok()
}

/// The `[plugin]` section of a `plugin.ini`. The id defaults to the
/// folder's name; the version to 0.1.0.
fn parse_manifest(source: &str, folder: &str) -> Result<Manifest, String> {
    let (mut id, mut name, mut description, mut version) = (None, None, None, None);
    let mut formats = None;
    for entry in paperpaw_config::parse(source) {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.section != "plugin" {
            continue;
        }
        let value = entry.value.as_str().into_owned();
        match entry.key {
            "id" => id = Some(value),
            "name" => name = Some(value),
            "description" => description = Some(value),
            "version" => version = Some(value),
            "formats" => formats = Some(value),
            _ => {}
        }
    }
    if let Some(text) = formats {
        let made_for: u32 = text
            .trim()
            .parse()
            .map_err(|_| format!("invalid formats `{text}`"))?;
        if made_for > FORMATS {
            return Err(format!(
                "made for a later PaperPaw (formats {made_for}; this one reads {FORMATS})"
            ));
        }
    }
    let id = id.unwrap_or_else(|| folder.to_string());
    let id = Id::parse(&id)
        .ok()
        .filter(|id| id.as_str() == id.namespace())
        .ok_or_else(|| format!("invalid id `{id}`"))?;
    let version = match version {
        Some(text) => Version::parse(&text).ok_or_else(|| format!("invalid version `{text}`"))?,
        None => Version::new(0, 1, 0),
    };
    Ok(Manifest {
        name: Cow::Owned(name.unwrap_or_else(|| id.as_str().to_string())),
        id,
        description: Cow::Owned(description.unwrap_or_default()),
        version,
        sdk: SDK_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn finds_folders_with_a_manifest() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/large/plugin.ini",
                "[plugin]\nname = Large print\ndescription = Bigger text.\nversion = 1.2.0\n",
            )
            .with("plugins/large/settings.ini", "[core]\nui-size = 14\n")
            .with("plugins/large/styles.css", "p { line-height: 1.6 }")
            .with("plugins/large/i18n/de.ini", "[meta]\nlanguage = de\n")
            .with("plugins/broken/readme.txt", "")
            .with("plugins/bad-id/plugin.ini", "[plugin]\nid = a.b\n");
        let mut warnings = Vec::new();
        let plugins = discover(&storage, "plugins", &mut |w| warnings.push(w));
        assert_eq!(plugins.len(), 1);
        let large = &plugins[0];
        assert_eq!(large.manifest.id.as_str(), "large");
        assert_eq!(large.manifest.name, "Large print");
        assert_eq!(large.manifest.version, Version::new(1, 2, 0));
        assert_eq!(large.translations.borrow().len(), 1);
        assert!(large.settings.borrow().is_some());
        assert!(large.keymap.borrow().is_none());
        assert_eq!(
            large.styles.borrow().as_deref(),
            Some("p { line-height: 1.6 }")
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    #[test]
    fn finds_fonts_in_plugins_and_font_folders() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/serif/plugin.ini",
                "[plugin]
",
            )
            .with("plugins/serif/fonts/Book Antiqua-Bold.TTF", "x")
            .with("plugins/serif/fonts/readme.txt", "")
            .with("fonts/a.otf", "x")
            .with("more-fonts/b.ttc", "x")
            .with("more-fonts/family/c.ttf", "x");
        let plugins = discover(&storage, "plugins", &mut |_| {});
        assert_eq!(
            *plugins[0].fonts.borrow(),
            ["plugins/serif/fonts/Book Antiqua-Bold.TTF"]
        );
        let dirs = [String::from("fonts"), String::from("more-fonts")];
        let folder = font_folder(&storage, &dirs).unwrap();
        assert_eq!(
            *folder.fonts.borrow(),
            ["fonts/a.otf", "more-fonts/b.ttc", "more-fonts/family/c.ttf"]
        );
        assert!(font_folder(&storage, &[String::from("nothing")]).is_none());
        assert_eq!(slug("Book Antiqua-Bold.TTF"), "book-antiqua-bold");
        // Packs that extend other fonts, named in plugin.ini.
        let source = "[plugin]
id = cyrillic
[extends]
literata.ppfp = fonts-default.literata
";
        assert_eq!(
            extensions(source, "plugins/cyrillic"),
            [(
                String::from("plugins/cyrillic/fonts/literata.ppfp"),
                String::from("fonts-default.literata")
            )]
        );
        assert_eq!(slug("….ttf"), "font");
    }

    #[test]
    fn families_name_their_faces() {
        let storage = MemoryStorage::new()
            .with(
                "plugins/font-round/plugin.ini",
                "[plugin]
name = Round
[family]
name = Round
regular = Round.ttf
bold = round-bold.ppfp
oblique = nothing.ppfp
italic = ../escape.ppfp
",
            )
            .with("plugins/font-round/fonts/Round.ttf", "x")
            .with("plugins/font-round/fonts/Extra.ttf", "x");
        let plugin = load_folder(&storage, "plugins/font-round/").unwrap();
        assert_eq!(plugin.folder(), "plugins/font-round");
        assert_eq!(plugin.family.as_deref(), Some("Round"));
        assert_eq!(
            *plugin.faces.borrow(),
            [
                (
                    "regular",
                    String::from("plugins/font-round/fonts/Round.ttf")
                ),
                (
                    "bold",
                    String::from("plugins/font-round/fonts/round-bold.ppfp")
                ),
            ]
        );
        // A face is no font of its own as well.
        assert_eq!(
            *plugin.fonts.borrow(),
            ["plugins/font-round/fonts/Extra.ttf"]
        );
        // No family without a name and a regular face.
        assert_eq!(
            family(
                "[family]
bold = a.ppfp
name = A
",
                "p"
            ),
            (None, Vec::new())
        );
        assert_eq!(
            family(
                "[family]
regular = a.ppfp
",
                "p"
            ),
            (None, Vec::new())
        );
    }

    #[test]
    fn presets_are_read_a_section_each() {
        let source = "# Layouts
[nook]
kind = home.layout
name = Reading nook
                      description = The book, large.
home.page-1 = home.continue 0/0 4x3
                      home.rows = 4
[plain]
home.status-bar = off
";
        let presets = presets(&Id::new_static("cosy"), source).unwrap();
        assert_eq!(presets.len(), 2);
        let nook = &presets[0];
        assert_eq!(nook.id.as_str(), "cosy.nook");
        assert_eq!(nook.kind.as_str(), "home.layout");
        assert_eq!(nook.title, "Reading nook");
        assert_eq!(nook.value("home.page-1"), Some("home.continue 0/0 4x3"));
        assert_eq!(nook.value("home.rows"), Some("4"));
        // Without a kind, the plugin's own.
        assert_eq!(presets[1].kind.as_str(), "cosy");
        assert!(
            super::presets(
                &Id::new_static("cosy"),
                "[a]
kind = Not An Id
"
            )
            .is_err()
        );
    }

    #[test]
    fn plugins_made_for_later_formats_are_left_out() {
        let ini = |formats: &str| format!("[plugin]\nname = Large print\n{formats}");
        assert!(parse_manifest(&ini(""), "large").is_ok());
        assert!(parse_manifest(&ini("formats = 1\n"), "large").is_ok());
        let later = parse_manifest(&ini("formats = 2\n"), "large").unwrap_err();
        assert!(later.contains("later PaperPaw"), "{later}");
        assert!(parse_manifest(&ini("formats = new\n"), "large").is_err());
    }
}
