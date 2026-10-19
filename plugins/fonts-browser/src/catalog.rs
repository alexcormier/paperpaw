//! The catalog of fonts: a JSON file from the font library (by default
//! quireink.org's), read as it streams. Each font comes ready to install
//! as a data plugin: its files for devices that draw scalable fonts
//! (`outline`, the font files as they are) and for those that draw font
//! packs (`packs`, made for QuireInk), each with its size and SHA-1.
//!
//! ```json
//! {"format": 1, "fonts": [{
//!   "id": "fredoka", "name": "Fredoka", "category": "sans",
//!   "author": "…", "licence": "OFL-1.1", "version": "2.1.0",
//!   "styles": 2, "sample": "https://…/fredoka/sample.qifp",
//!   "packs": [{"path": "fonts/regular.qifp", "url": "https://…",
//!              "size": 201234, "sha1": "…"}, …],
//!   "outline": […]}]}
//! ```

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_json::stream::{Event, read_file};
use quireink_sdk::web::join_url;
use quireink_sdk::{Env, MemoryClass, Storage, Version};

use crate::PLUGIN;

/// The catalog's formats this plugin reads.
const FORMAT: u32 = 1;
/// Fonts read at most, and files of one font.
const MAX_FONTS: usize = 200;
const MAX_FILES: usize = 16;

/// One file of a font's plugin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FontFile {
    /// Where it goes in the plugin's folder, e.g. `fonts/regular.qifp`.
    pub path: String,
    pub url: String,
    pub size: u64,
    pub sha1: String,
}

/// A font the catalog offers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Font {
    /// E.g. `fredoka`; its plugin is `font-fredoka`.
    pub id: String,
    pub name: String,
    /// `serif`, `sans`, `slab`, `mono`, `display` or `handwriting`.
    pub category: String,
    pub author: String,
    pub licence: String,
    pub version: String,
    /// How many styles it has (regular, bold, italic, bold italic).
    pub styles: u32,
    pub sample: String,
    pub packs: Vec<FontFile>,
    pub outline: Vec<FontFile>,
}

impl Font {
    /// The files for this device: the font files where scalable fonts are
    /// drawn and there is the memory to hold them (a font file is loaded
    /// whole), else the packs, read a glyph at a time.
    pub(crate) fn files(&self, env: &Env<'_>) -> &[FontFile] {
        if outlines(env) && !self.outline.is_empty() {
            &self.outline
        } else {
            &self.packs
        }
    }

    /// Bytes to download.
    pub(crate) fn size(&self, env: &Env<'_>) -> u64 {
        self.files(env).iter().map(|f| f.size).sum()
    }

    /// The id of the plugin it becomes.
    pub(crate) fn plugin(&self) -> String {
        format!("font-{}", self.id)
    }

    /// Whether it is installed, and in which version.
    pub(crate) fn installed(&self, env: &Env<'_>) -> Option<Version> {
        env.plugins
            .get(&self.plugin())
            .map(|info| info.manifest.version)
    }

    /// Whether a newer version than the one installed is offered.
    pub(crate) fn newer(&self, env: &Env<'_>) -> bool {
        match (self.installed(env), Version::parse(&self.version)) {
            (Some(installed), Some(offered)) => offered > installed,
            _ => false,
        }
    }

    /// Whether it is the font chosen for `setting` (its family is named
    /// as the font).
    pub(crate) fn chosen_for(&self, env: &Env<'_>, setting: &str) -> bool {
        self.installed(env).is_some() && env.settings.text(setting) == self.name
    }

    /// Fit for this device: something to download, nothing that leaves
    /// its folder.
    fn usable(&self, outlines: bool) -> bool {
        let files = if outlines && !self.outline.is_empty() {
            &self.outline
        } else {
            &self.packs
        };
        is_name(&self.id) && !self.name.is_empty() && !files.is_empty()
    }
}

/// Whether this device takes font files rather than packs.
fn outlines(env: &Env<'_>) -> bool {
    quireink_sdk::text::OUTLINES && env.device.memory == MemoryClass::Large
}

/// Whether `id` is usable in a plugin's id and a file's name.
fn is_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/`
/// between them.
fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 100
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
}

/// Where the catalog is kept between visits.
pub(crate) fn path() -> String {
    format!("{}/catalog.json", Env::plugin_dir(PLUGIN.as_str()))
}

/// The catalog kept from the last visit.
pub(crate) fn kept(env: &Env<'_>) -> Vec<Font> {
    let base = env.settings.read(&crate::CATALOG);
    read(env.storage, &path(), base.trim(), outlines(env)).unwrap_or_default()
}

/// The catalog fetched just now, kept at `path`.
pub(crate) fn fetched(env: &Env<'_>, path: &str, base: &str) -> Option<Vec<Font>> {
    read(env.storage, path, base, outlines(env))
}

/// The fonts of the catalog file at `path` fit for this device (font
/// files where `outlines`, else packs), by name, their addresses relative
/// to `base` (the catalog's); `None` if it is no catalog this plugin
/// reads.
fn read(storage: &dyn Storage, path: &str, base: &str, outlines: bool) -> Option<Vec<Font>> {
    let mut fonts: Vec<Font> = Vec::new();
    let mut format = None;
    read_file(storage, path, &mut |event| match event {
        Event::Open(at) if at == ["fonts", "[]"] && fonts.len() < MAX_FONTS => {
            fonts.push(Font::default());
        }
        Event::Open([fonts_key, item, list, file])
            if fonts_key == "fonts" && item == "[]" && file == "[]" =>
        {
            if let Some(font) = fonts.last_mut() {
                let files = match list.as_str() {
                    "packs" => &mut font.packs,
                    "outline" => &mut font.outline,
                    _ => return,
                };
                if files.len() < MAX_FILES {
                    files.push(FontFile::default());
                }
            }
        }
        Event::Value([key], value) if key == "format" => format = value.parse::<u32>().ok(),
        Event::Value([fonts_key, item, key], value) if fonts_key == "fonts" && item == "[]" => {
            let Some(font) = fonts.last_mut() else {
                return;
            };
            let value = value.to_string();
            match key.as_str() {
                "id" => font.id = value,
                "name" => font.name = value,
                "category" => font.category = value,
                "author" => font.author = value,
                "licence" => font.licence = value,
                "version" => font.version = value,
                "styles" => font.styles = value.parse().unwrap_or(0),
                "sample" => font.sample = value,
                _ => {}
            }
        }
        Event::Value([fonts_key, item, list, file, key], value)
            if fonts_key == "fonts" && item == "[]" && file == "[]" =>
        {
            let Some(font) = fonts.last_mut() else {
                return;
            };
            let files = match list.as_str() {
                "packs" => &mut font.packs,
                "outline" => &mut font.outline,
                _ => return,
            };
            let Some(entry) = files.last_mut() else {
                return;
            };
            match key.as_str() {
                "path" => entry.path = value.to_string(),
                "url" => entry.url = value.to_string(),
                "size" => entry.size = value.parse().unwrap_or(0),
                "sha1" => entry.sha1 = value.to_ascii_lowercase(),
                _ => {}
            }
        }
        _ => {}
    });
    if format != Some(FORMAT) {
        return None;
    }
    for font in &mut fonts {
        if !font.sample.is_empty() {
            font.sample = join_url(base, &font.sample);
        }
        for files in [&mut font.packs, &mut font.outline] {
            for file in files.iter_mut() {
                file.url = join_url(base, &file.url);
            }
            // Every file inside the folder, with what checks it; a
            // `plugin.ini` among them.
            let whole = files
                .iter()
                .all(|f| is_inside(&f.path) && !f.url.is_empty() && f.sha1.len() == 40)
                && files.iter().any(|f| f.path == "plugin.ini");
            if !whole {
                files.clear();
            }
        }
    }
    fonts.retain(|f| f.usable(outlines));
    fonts.sort_by_key(|f| f.name.to_lowercase());
    Some(fonts)
}
