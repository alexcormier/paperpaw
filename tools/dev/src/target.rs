//! What is checked: plugins of the app by id (PaperPaw's own, or one a
//! plugin's repository added, see [`crate::main`]), data plugin folders
//! and `.ppn` files.

use std::path::{Path, PathBuf};

use crate::AddPlugins;

/// A data plugin folder, as the device would find it in `plugins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPlugin {
    /// Its id, from `plugin.ini` (else the folder's name).
    pub id: String,
    pub folder: PathBuf,
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
            let id = data_plugin_id(path);
            if !self.plugins.contains(&id) {
                self.plugins.push(id.clone());
            }
            self.data.push(DataPlugin {
                id,
                folder: path.to_path_buf(),
            });
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

/// The id a data plugin folder's `plugin.ini` gives, else its name.
pub fn data_plugin_id(folder: &Path) -> String {
    let from_ini = std::fs::read_to_string(folder.join("plugin.ini"))
        .ok()
        .and_then(|text| {
            paperpaw_config::parse(&text)
                .filter_map(Result::ok)
                .find(|entry| entry.section == "plugin" && entry.key == "id")
                .map(|entry| entry.value.as_str().into_owned())
        });
    from_ini.unwrap_or_else(|| {
        folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
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
    }
}
