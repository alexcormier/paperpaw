//! A plugin's files: dashes the project does not use, catalog keys, and
//! what a data plugin's `settings.ini` and `presets.ini` set.

use std::path::{Path, PathBuf};

use paperpaw_i18n::Catalog;

use super::catalogs;
use crate::report::{Finding, Level};

/// The dashes PaperPaw's texts do not use, with their names.
pub const DASHES: [(char, &str); 2] = [
    ('\u{2014}', "an em dash (U+2014)"),
    ('\u{2013}', "an en dash (U+2013)"),
];

/// The dashes in `text` (a file at `place`, or a message), a finding a
/// line: errors in PaperPaw's own plugins (`own`), warnings in others.
pub fn dashes(plugin: &str, place: &str, text: &str, own: bool) -> Vec<Finding> {
    let level = if own { Level::Error } else { Level::Warning };
    let mut findings = Vec::new();
    let lines = text.lines().count();
    for (n, line) in text.lines().enumerate() {
        let found: Vec<&str> = DASHES
            .iter()
            .filter(|(c, _)| line.contains(*c))
            .map(|(_, name)| *name)
            .collect();
        if found.is_empty() {
            continue;
        }
        let at = if lines > 1 {
            format!("{place}:{}", n + 1)
        } else {
            place.to_string()
        };
        findings.push(
            Finding::new(
                "lint.dash",
                level,
                at,
                format!(
                    "Has {}: PaperPaw's texts use none; write a comma, a colon or \
                     parentheses instead (a hyphen for ranges): `{}`.",
                    found.join(" and "),
                    line.trim()
                ),
            )
            .plugin(plugin.to_string()),
        );
    }
    findings
}

/// The `.ini` files of a plugin's folder and its `i18n` folder, by name.
pub fn ini_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in [folder.to_path_buf(), folder.join("i18n")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ini")))
            .collect();
        found.sort();
        files.extend(found);
    }
    files
}

/// `path` relative to `base`, with slashes, for places.
pub fn relative(path: &Path, base: &Path) -> String {
    crate::report::slashed(path.strip_prefix(base).unwrap_or(path))
}

/// The catalog files of a plugin in `folder`'s `i18n`: dashes, keys well
/// formed and in its namespace. `shown` is how paths are written.
pub fn catalog_files(
    plugin: &str,
    folder: &Path,
    shown: &Path,
    own: bool,
    translates: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in ini_files(folder) {
        let in_i18n = file.parent().is_some_and(|p| p.ends_with("i18n"));
        let place = relative(&file, shown);
        let Ok(text) = std::fs::read_to_string(&file) else {
            findings.push(
                Finding::error("lint.catalog", place, "Cannot be read as UTF-8 text.")
                    .plugin(plugin.to_string()),
            );
            continue;
        };
        findings.extend(dashes(plugin, &place, &text, own));
        if !in_i18n {
            continue;
        }
        match Catalog::parse(text) {
            Ok(catalog) => {
                findings.extend(catalogs::file_keys(plugin, &place, &catalog, translates))
            }
            Err(error) => findings.push(
                Finding::error(
                    "lint.catalog",
                    place,
                    format!("Is not a catalog PaperPaw reads: {error}."),
                )
                .plugin(plugin.to_string()),
            ),
        }
    }
    findings
}

/// What a data plugin's `settings.ini` and `presets.ini` set that no
/// plugin has (`known` says which settings exist): a misspelt id is
/// ignored on the device.
pub fn settings_set(
    plugin: &str,
    folder: &Path,
    shown: &Path,
    known: &dyn Fn(&str) -> bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (name, whole) in [("settings.ini", false), ("presets.ini", true)] {
        let file = folder.join(name);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let place = relative(&file, shown);
        for entry in paperpaw_config::parse(&text).filter_map(Result::ok) {
            let id = if whole {
                // Preset values have the whole id; the rest describe it.
                if !entry.key.contains('.') {
                    continue;
                }
                entry.key.to_string()
            } else {
                format!("{}.{}", entry.section, entry.key)
            };
            // Settings of a placed widget exist once it is placed.
            if id.contains('@') || known(&id) {
                continue;
            }
            findings.push(
                Finding::warning(
                    "lint.unknown-setting",
                    format!("{place}:{}", entry.line),
                    format!(
                        "Sets `{id}`, which no plugin of the app has: a misspelt id \
                         is ignored."
                    ),
                )
                .plugin(plugin.to_string()),
            );
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture data plugin with dashes put in (the repository has
    /// none, the fixture neither).
    fn fixture() -> PathBuf {
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lint/data-plugin");
        let to =
            std::env::temp_dir().join(format!("paperpaw-dev-lint-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&to);
        crate::session::copy_dir(&from, &to).unwrap();
        let dash = |file: &str, from: &str, to_: &str| {
            let path = to.join(file);
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(&path, text.replace(from, to_)).unwrap();
        };
        dash("plugin.ini", "wanting;", "wanting \u{2014}");
        dash("i18n/en.ini", "1 to 2", "1\u{2013}2");
        to
    }

    #[test]
    fn dashes_are_found_line_by_line() {
        let text = "a = fine - here\nb = not \u{2014} here\nc = 1\u{2013}2\n";
        let found = dashes("demo", "x.ini", text, false);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].place, "x.ini:2");
        assert_eq!(found[0].level, Level::Warning);
        assert!(found[1].message.contains("en dash"));
        assert_eq!(dashes("demo", "x.ini", text, true)[0].level, Level::Error);
        assert!(dashes("demo", "message", "plain - text", true).is_empty());
    }

    #[test]
    fn a_data_plugins_files() {
        let folder = fixture();
        let found = catalog_files("wanting", &folder, &folder, false, &|_| false);
        let places: Vec<&str> = found.iter().map(|f| f.place.as_str()).collect();
        assert!(places.contains(&"plugin.ini:3"), "{found:#?}");
        assert!(places.contains(&"i18n/en.ini:7"), "{found:#?}");
        assert!(
            found
                .iter()
                .any(|f| f.check == "lint.id" && f.message.contains("`elsewhere.title`")),
            "{found:#?}"
        );
        let known = |id: &str| id == "core.ui-size";
        let set = settings_set("wanting", &folder, &folder, &known);
        assert_eq!(set.len(), 2, "{set:#?}");
        assert!(set[0].message.contains("`core.ui-sise`"));
        assert_eq!(set[0].place, "settings.ini:3");
        assert!(set[1].message.contains("`home.colums`"));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
