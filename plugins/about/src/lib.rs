//! *About PaperPaw*, in the device's settings: its version, its licence
//! and the licences of what it bundles (fonts, icons, hyphenation
//! patterns), which every copy of it must carry. Shown by the reader (as
//! Markdown), so it pages and reads like a book; PaperPaw's logo above it
//! (the picture [`logo::LOGO`] names, if one does). Where PaperPaw is the
//! firmware, also the plugins built into it, which an update brings
//! again; where it loads native code plugins, what they must be built
//! with to fit it (its fingerprint, as the plugin library lists builds).
//!
//! Other plugins add lines to the page through the point [`LINES`]
//! (`about.lines`), each a paragraph below the address: a theme's
//! author, a dictionary's source, a font's licence that is not bundled.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::logo;
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Env, Id, Manifest, MenuEntry, Plugin, RegisterError, Registrar,
    Version,
};

pub const PLUGIN: Id = Id::new_static("about");
/// Shows the page.
pub const OPEN: Id = Id::new_static("about.open");
/// The point other plugins add lines to the page through, a paragraph
/// each, after the plugin's name; as plain text (what Markdown makes of
/// signs is not made), at most [`MOST_LINES`] bytes a plugin.
pub const LINES: &str = "about.lines";
/// Bytes of lines a plugin adds, at most.
pub const MOST_LINES: usize = 512;
/// The licences, from `tools/licences/build.py`.
const LICENCES: &str = include_str!("../licences.md");
/// PaperPaw's logo, as the page shows it.
const LOGO: &[u8] = include_bytes!("../logo.png");
/// Where the page is written for the reader: in the plugin's folder, so
/// it is no book.
fn page_path() -> String {
    format!("{}/about.md", Env::plugin_dir(PLUGIN.as_str()))
}

/// The page's text, the logo first: the picture set, or PaperPaw's own
/// written beside the page.
fn page(env: &Env<'_>) -> String {
    let intro = env.i18n.tr("about.intro");
    let set = env.settings.read(&logo::LOGO).trim().trim_matches('/');
    let picture = if !set.is_empty() && env.storage.exists(set) {
        format!("/{set}")
    } else {
        let path = format!("{}/logo.png", Env::plugin_dir(PLUGIN.as_str()));
        if env.storage.size(&path).ok() != Some(LOGO.len() as u64) {
            let _ = env.storage.write(&path, LOGO);
        }
        String::from("logo.png")
    };
    format!(
        "![PaperPaw]({picture})\n\n# PaperPaw {}\n\n{intro}\n\nhttps://github.com/hthoene/paperpaw\n\n{}{}{}{LICENCES}",
        env!("CARGO_PKG_VERSION"),
        lines(env),
        built_in(env),
        native(env),
    )
}

/// The lines other plugins add ([`LINES`]), a paragraph each, named by
/// their plugin; as plain text, so none makes a heading, a picture or a
/// list of its own; at most [`MOST_LINES`] bytes a plugin.
fn lines(env: &Env<'_>) -> String {
    let mut text = String::new();
    let mut used: Vec<(&Id, usize)> = Vec::new();
    for (plugin, contribution) in env.services.contributions(LINES) {
        for line in contribution
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            let at = match used.iter().position(|(p, _)| *p == plugin) {
                Some(at) => at,
                None => {
                    used.push((plugin, 0));
                    used.len() - 1
                }
            };
            let left = MOST_LINES.saturating_sub(used[at].1);
            if left == 0 {
                break;
            }
            let mut end = line.len().min(left);
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            used[at].1 += end;
            let name = env
                .plugins
                .get(plugin.as_str())
                .map_or(plugin.as_str(), |info| info.name(env.i18n));
            text.push('*');
            plain(name, &mut text);
            text.push_str(":* ");
            plain(&line[..end], &mut text);
            text.push_str("\n\n");
        }
    }
    text
}

/// `text` as plain text in Markdown: its signs escaped.
fn plain(text: &str, out: &mut String) {
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
}

/// The plugins built into the firmware, where PaperPaw is it.
fn built_in(env: &Env<'_>) -> String {
    let Some(firmware) = env.firmware.filter(|f| !f.plugins.is_empty()) else {
        return String::new();
    };
    let mut text = format!(
        "## {}\n\n{}\n\n",
        env.i18n.tr("about.plugins.title"),
        env.i18n.tr("about.plugins.help")
    );
    for plugin in firmware.plugins.iter() {
        text.push_str(&format!("- {} {}\n", plugin.name, plugin.version));
    }
    text.push('\n');
    text
}

/// What native code plugins must be built with, where PaperPaw loads
/// them.
fn native(env: &Env<'_>) -> String {
    let Some(native) = env.native else {
        return String::new();
    };
    let fingerprint = native.fingerprint.to_string();
    let help = env.i18n.format(
        "about.native.help",
        &[
            ("fingerprint", Arg::Str(&fingerprint)),
            ("toolchain", Arg::Str(&native.toolchain)),
        ],
    );
    format!("## {}\n\n{help}\n\n", env.i18n.tr("about.native.title"))
}

/// The about page.
#[derive(Debug, Default)]
pub struct About;

impl Plugin for About {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "About", Version::new(0, 1, 0))
            .description("PaperPaw's version and licences.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(OPEN, "about.title").handler(
            |env: &Env<'_>, out: &mut Vec<Command>| {
                let path = page_path();
                let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
                if env.storage.write(&path, page(env).as_bytes()).is_ok() {
                    out.push(paperpaw_sdk::readers::open(path));
                }
            },
        ))?;
        // In the device's part of the settings, last.
        r.add_menu_entry(MenuEntry::new(menus::SETTINGS, OPEN).order(99))?;
        r.add_icon(OPEN, Cow::Borrowed("info-circle"))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use paperpaw_i18n::{Catalog, check};

    use super::LICENCES;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Words only, so lines joined into paragraphs compare equal.
    fn words(text: &str) -> std::string::String {
        text.split_whitespace()
            .collect::<std::vec::Vec<_>>()
            .join(" ")
    }

    #[test]
    fn every_licence_is_there() {
        // As they are in the repository: run tools/licences/build.py
        // after one changed.
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");
        for file in [
            "NOTICE",
            "LICENSE",
            "plugins/fonts-default/fonts/Literata-OFL.txt",
            "plugins/fonts-default/fonts/SourceSans3-OFL.txt",
            "plugins/fonts-default/fonts/TablerIcons-MIT.txt",
        ] {
            let Ok(text) = std::fs::read_to_string(std::format!("{root}{file}")) else {
                continue;
            };
            assert!(
                words(LICENCES).contains(&words(&text)),
                "{file} changed: run tools/licences/build.py"
            );
        }
        assert!(LICENCES.contains("Gerard D.C. Kuiken"));
        assert!(LICENCES.contains("Stephan Hennig"));
    }
}
