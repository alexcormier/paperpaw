//! Profiles: named sets of setting values, kept in the plugin's folder in
//! `profiles.ini` (a section per profile with its name and the settings'
//! values by their ids, see [`FORMAT`]), with the name of the profile
//! applied last.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{groups, settings};
use paperpaw_sdk::config;
use paperpaw_sdk::data::Format;
use paperpaw_sdk::{Command, Env, Id, SettingKind, Storage};

use crate::PLUGIN;

/// The settings a profile keeps: everything about reading, the display
/// and the status lines.
const GROUPS: [Id; 3] = [groups::READING, groups::DISPLAY, groups::STATUS_LINES];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Profile {
    pub name: String,
    /// Setting ids and values as in `paperpaw.ini`.
    pub values: Vec<(Id, String)>,
}

/// `profiles.ini`. Version 2: a section `[profile.1]` and so on per
/// profile, its `name` and the settings' values by their ids. Version 1
/// had the name as the section (`[Night]`), which an ini file cannot hold
/// with spaces.
pub(crate) const FORMAT: Format = Format::new(2, &[upgrade_names]);

fn path() -> String {
    format!("{}/profiles.ini", Env::plugin_dir(PLUGIN.as_str()))
}

/// Version 1 to 2: the names out of the section headers.
fn upgrade_names(v1: &str) -> String {
    let mut profiles: Vec<Profile> = Vec::new();
    for line in v1.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            profiles.push(Profile {
                name: name.trim().to_string(),
                values: Vec::new(),
            });
        } else if let (Some(profile), Some((id, value))) =
            (profiles.last_mut(), line.split_once('='))
            && let Ok(id) = Id::parse(id.trim())
        {
            profile.values.push((id, value.trim().to_string()));
        }
    }
    text(&profiles)
}

/// The profiles as the file's text (without its version).
fn text(profiles: &[Profile]) -> String {
    let mut text = String::new();
    for (i, profile) in profiles.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(&format!("[profile.{}]\n", i + 1));
        text.push_str(&format!("name = {}\n", config::quote(&profile.name)));
        for (id, value) in &profile.values {
            text.push_str(&format!("{id} = {}\n", config::quote(value)));
        }
    }
    text
}

fn last_path() -> String {
    format!("{}/last.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The profiles kept: none where there is no file yet, `None` where it
/// cannot be read (then nothing may be saved over it).
pub(crate) fn load(storage: &dyn Storage) -> Option<Vec<Profile>> {
    match FORMAT.load(storage, &path(), 64 * 1024) {
        Ok(Some(text)) => Some(parse(&text)),
        Ok(None) => Some(Vec::new()),
        Err(_) => None,
    }
}

/// The profiles of the file's text (in this version).
fn parse(text: &str) -> Vec<Profile> {
    let mut profiles: Vec<(&str, Profile)> = Vec::new();
    for entry in config::parse(text).filter_map(Result::ok) {
        if !entry.section.starts_with("profile.") {
            continue;
        }
        if profiles.last().is_none_or(|(s, _)| *s != entry.section) {
            profiles.push((
                entry.section,
                Profile {
                    name: String::new(),
                    values: Vec::new(),
                },
            ));
        }
        let Some((_, profile)) = profiles.last_mut() else {
            continue;
        };
        let value = entry.value.as_str().into_owned();
        if entry.key == "name" {
            profile.name = value;
        } else if let Ok(id) = Id::parse(entry.key) {
            profile.values.push((id, value));
        }
    }
    profiles.into_iter().map(|(_, p)| p).collect()
}

/// Keeps `profiles`; whether it could.
pub(crate) fn save(storage: &dyn Storage, profiles: &[Profile]) -> bool {
    FORMAT.save(storage, &path(), &text(profiles)).is_ok()
}

/// Whether `a` and `b` name the same profile: as they read, whatever the
/// case.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The name of the profile applied last.
pub(crate) fn last(storage: &dyn Storage) -> String {
    let data = paperpaw_sdk::read_file(storage, &last_path(), 1024).unwrap_or_default();
    String::from_utf8_lossy(&data).trim().to_string()
}

pub(crate) fn set_last(storage: &dyn Storage, name: &str) {
    let _ = storage.write(&last_path(), name.as_bytes());
}

/// The current values of the settings a profile keeps.
pub(crate) fn capture(env: &Env<'_>) -> Vec<(Id, String)> {
    env.settings
        .specs()
        .filter(|spec| GROUPS.contains(&env.settings.group_of(spec)))
        // Folders and languages belong to the device, not to a way of
        // reading.
        .filter(|spec| !matches!(spec.kind, SettingKind::Text | SettingKind::Language))
        // Neither is a way of reading.
        .filter(|spec| spec.id != settings::SCOPE_ONLY && spec.id != settings::NOTICE_SECONDS)
        .filter_map(|spec| {
            let value = env.settings.get(spec.id.as_str())?;
            Some((spec.id.clone(), value.to_config_text()))
        })
        .collect()
}

/// The commands that apply `profile`: only settings that differ, and
/// only values their settings accept.
pub(crate) fn apply(env: &Env<'_>, profile: &Profile) -> Vec<Command> {
    profile
        .values
        .iter()
        .filter_map(|(id, text)| {
            let spec = env.settings.specs().find(|s| s.id == *id)?;
            let value = spec.kind.parse(text).filter(|v| spec.kind.accepts(v))?;
            (env.settings.get(id.as_str()) != Some(&value))
                .then(|| Command::SetSetting(id.clone(), value))
        })
        .collect()
}

/// Whether the settings are as `profile` has them.
pub(crate) fn in_use(env: &Env<'_>, profile: &Profile) -> bool {
    !profile.values.is_empty() && apply(env, profile).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn profiles_are_kept() {
        let storage = MemoryStorage::new();
        assert!(load(&storage).unwrap().is_empty());
        let profiles = [
            Profile {
                name: String::from("Night"),
                values: Vec::from([
                    (Id::new_static("core.night-mode"), String::from("on")),
                    (Id::new_static("core.font-size"), String::from("12")),
                ]),
            },
            Profile {
                name: String::from("Large [print]"),
                values: Vec::from([(Id::new_static("core.font-size"), String::from("18"))]),
            },
        ];
        assert!(save(&storage, &profiles));
        let loaded = load(&storage).unwrap();
        assert_eq!(loaded, profiles, "names with brackets and spaces too");
        set_last(&storage, "Night");
        assert_eq!(last(&storage), "Night");
    }

    #[test]
    fn files_of_version_1_are_read() {
        let storage = MemoryStorage::new().with(
            &path(),
            include_str!("../testdata/profiles.ini.v1")
                .as_bytes()
                .to_vec(),
        );
        let loaded = load(&storage).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Night");
        assert_eq!(
            loaded[0].values,
            [
                (Id::new_static("core.night-mode"), String::from("on")),
                (Id::new_static("reader.font-size"), String::from("12")),
            ]
        );
        assert_eq!(loaded[1].name, "Large print");
        // Saved again, it is version 2.
        assert!(save(&storage, &loaded));
        let text = paperpaw_sdk::read_file(&storage, &path(), 4096).unwrap();
        assert!(text.starts_with(b"# format 2\n[profile.1]\nname = Night\n"));
        assert_eq!(load(&storage).unwrap(), loaded);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_last_profile_of_version_1_is_read() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with(&super::last_path(), include_str!("../testdata/last.txt.v1"));
        assert_eq!(super::last(&storage), "Night reading");
    }
}
