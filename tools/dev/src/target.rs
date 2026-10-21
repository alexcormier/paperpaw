//! What is checked: plugins of the app by id (PaperPaw's own, or one a
//! plugin's repository added, see [`crate::main`]), data plugin folders
//! and `.ppn` files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::AddPlugins;

/// A data plugin, as the device would find it in `plugins`: its files,
/// read from a folder or handed in (by the browser's emulator).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlugin {
    /// Its id, from `plugin.ini` (else the folder's name).
    pub id: String,
    /// The folder it was read from; `None` for one handed in.
    pub folder: Option<PathBuf>,
    /// Its files by their path in it (with `/`), hidden ones too.
    pub files: BTreeMap<String, Vec<u8>>,
}

impl DataPlugin {
    /// The plugin in `folder`, every file read.
    pub fn read(folder: &Path) -> Result<DataPlugin, String> {
        let mut files = BTreeMap::new();
        read_dir(folder, folder, &mut files)?;
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(DataPlugin {
            id: data_plugin_id(&files).unwrap_or(name),
            folder: Some(folder.to_path_buf()),
            files,
        })
    }

    /// A plugin of `files` (path in it, bytes), its id `plugin.ini`'s or
    /// else `id`.
    pub fn new(id: &str, files: BTreeMap<String, Vec<u8>>) -> DataPlugin {
        DataPlugin {
            id: data_plugin_id(&files).unwrap_or_else(|| id.to_string()),
            folder: None,
            files,
        }
    }

    /// The file at `path` in it.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Where it is, for messages: its folder, else its id.
    pub fn place(&self) -> String {
        match &self.folder {
            Some(folder) => folder.display().to_string(),
            None => self.id.clone(),
        }
    }
}

/// Every file under `dir` into `out`, by its path from `root`.
fn read_dir(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let full = entry.path();
        let meta = entry
            .metadata()
            .map_err(|e| format!("{}: {e}", full.display()))?;
        if meta.is_dir() {
            read_dir(root, &full, out)?;
        } else if meta.is_file() {
            let relative = full.strip_prefix(root).unwrap_or(&full);
            let bytes = std::fs::read(&full).map_err(|e| format!("{}: {e}", full.display()))?;
            out.insert(relative.to_string_lossy().replace('\\', "/"), bytes);
        }
    }
    Ok(())
}

/// What a run looks at.
#[derive(Debug, Clone, Default)]
pub struct Target {
    /// The plugins whose screens, widgets, settings and texts are looked
    /// at, by id. Empty: every plugin of the app (PaperPaw's own
    /// development).
    pub plugins: Vec<String>,
    /// Data plugins put into the emulated device's `plugins` folder.
    pub data: Vec<DataPlugin>,
    /// Code plugins built for a device, looked at as files.
    pub ppn: Vec<PathBuf>,
    /// Adds a plugin with code to the app.
    pub add: Option<AddPlugins>,
}

impl Target {
    /// Takes a path from the command line: a data plugin folder (it has
    /// a `plugin.ini`) or a `.ppn` file.
    pub fn take_path(&mut self, path: &Path) -> Result<(), String> {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ppn"))
        {
            if !path.is_file() {
                return Err(format!("{}: no such file", path.display()));
            }
            self.ppn.push(path.to_path_buf());
            return Ok(());
        }
        if path.join("plugin.ini").is_file() {
            self.take_data(DataPlugin::read(path)?);
            return Ok(());
        }
        if path.join("Cargo.toml").is_file() {
            return Err(format!(
                "{} is a crate: a plugin with code is checked from its own repository, \
                 with `paperpaw_dev::main` in an example (see docs/plugins.md, \
                 \"Checking your plugin\")",
                path.display()
            ));
        }
        Err(format!(
            "{}: neither a data plugin folder (with plugin.ini) nor a .ppn file",
            path.display()
        ))
    }

    /// Looks at a data plugin.
    pub fn take_data(&mut self, plugin: DataPlugin) {
        if !self.plugins.contains(&plugin.id) {
            self.plugins.push(plugin.id.clone());
        }
        self.data.push(plugin);
    }

    /// Whether `id` (a screen, widget, setting, translation key or
    /// action) belongs to a plugin looked at: its namespace is one of
    /// them, or none was named.
    pub fn ours(&self, id: &str) -> bool {
        self.plugins.is_empty() || self.owner(id).is_some()
    }

    /// The plugin looked at whose namespace `id` is in.
    pub fn owner(&self, id: &str) -> Option<&str> {
        let namespace = id.split(['.', '@', ' ', '/']).next().unwrap_or(id);
        self.plugins
            .iter()
            .find(|p| p.as_str() == namespace)
            .map(String::as_str)
    }

    /// Whether the app is looked at in the emulator at all (not only
    /// `.ppn` files).
    pub fn runs(&self) -> bool {
        !self.plugins.is_empty() || !self.data.is_empty() || self.ppn.is_empty()
    }
}

/// The id a data plugin's `plugin.ini` gives, if any.
pub fn data_plugin_id(files: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    let text = std::str::from_utf8(files.get("plugin.ini")?).ok()?;
    paperpaw_config::parse(text)
        .filter_map(Result::ok)
        .find(|entry| entry.section == "plugin" && entry.key == "id")
        .map(|entry| entry.value.as_str().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_belong_by_namespace() {
        let target = Target {
            plugins: vec!["hello".into()],
            ..Target::default()
        };
        assert!(target.ours("hello.screen"));
        assert!(target.ours("hello.widget@2 0/0 2x1"));
        assert!(!target.ours("helloworld.screen"));
        assert!(!target.ours("core.home"));
        assert!(Target::default().ours("core.home"));
    }

    #[test]
    fn a_sample_data_plugin_is_taken() {
        let folder =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/plugins/large-print");
        let mut target = Target::default();
        target.take_path(&folder).unwrap();
        assert_eq!(target.data.len(), 1);
        assert_eq!(target.plugins, [target.data[0].id.clone()]);
        assert!(target.data[0].file("plugin.ini").is_some());
    }

    #[test]
    fn a_data_plugin_handed_in_is_named_by_its_manifest() {
        let files = BTreeMap::from([(
            "plugin.ini".to_string(),
            b"[plugin]\nid = dusk\nname = Dusk\n".to_vec(),
        )]);
        let plugin = DataPlugin::new("given", files);
        assert_eq!(plugin.id, "dusk");
        assert_eq!(plugin.place(), "dusk");
        assert_eq!(DataPlugin::new("given", BTreeMap::new()).id, "given");
    }
}
