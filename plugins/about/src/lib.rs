//! *About PaperPaw*, in the device's settings: a page as the other
//! settings pages, its rows the version, the address of the source, and
//! where PaperPaw is the firmware the plugins built into it, which an
//! update brings again; where it loads native code plugins, what they
//! must be built with to fit it (its fingerprint, as the plugin library
//! lists builds).
//!
//! The long texts open in the reader (as Markdown), so they page and read
//! like a book, every word of them: the licences, PaperPaw's and those of
//! what it bundles (fonts, icons, hyphenation patterns), which every copy
//! of it must carry, a chapter each, PaperPaw's logo above them (the
//! picture [`logo::LOGO`] names, if one does); and the list of the
//! plugins built in.
//!
//! Other plugins add lines to the licences through the point [`LINES`]
//! (`about.lines`), each a paragraph below the address: a theme's
//! author, a dictionary's source, a font's licence that is not bundled.

#![no_std]

extern crate alloc;

mod screen;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::logo;
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError,
    Registrar, ScreenSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("about");
/// Shows the page.
pub const OPEN: Id = Id::new_static("about.open");
/// The page.
pub const SCREEN: Id = Id::new_static("about.screen");
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
/// PaperPaw's version.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Where PaperPaw's source is.
pub(crate) const SOURCE: &str = "https://github.com/hthoene/paperpaw";

/// Where the licences are written for the reader: in the plugin's
/// folder, so they are no book.
pub(crate) fn licences_path() -> String {
    format!("{}/licences.md", Env::plugin_dir(PLUGIN.as_str()))
}

/// Where the list of the plugins built in is written for the reader.
pub(crate) fn plugins_path() -> String {
    format!("{}/plugins.md", Env::plugin_dir(PLUGIN.as_str()))
}

/// Writes `text` to `path` for the reader and opens it there; nothing
/// where it cannot be written.
pub(crate) fn read(env: &Env<'_>, path: String, text: &str) -> Option<Command> {
    let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    env.storage
        .write(&path, text.as_bytes())
        .ok()
        .map(|()| paperpaw_sdk::readers::open(path))
}

/// The licences' text, the logo first: the picture set, or PaperPaw's
/// own written beside it; a chapter each.
pub(crate) fn licences(env: &Env<'_>) -> String {
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
        "![PaperPaw]({picture})\n\n# PaperPaw {VERSION}\n\n{intro}\n\n{SOURCE}\n\n{}{LICENCES}",
        lines(env),
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

/// The plugins built into the firmware, where PaperPaw is it, as a text
/// to read.
pub(crate) fn built_in(env: &Env<'_>) -> String {
    let Some(firmware) = env.firmware.filter(|f| !f.plugins.is_empty()) else {
        return String::new();
    };
    let mut text = format!(
        "# {}\n\n{}\n\n",
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
/// them: its fingerprint, and the note that says so.
pub(crate) fn native(env: &Env<'_>) -> Option<(String, String)> {
    let native = env.native?;
    let note = env.i18n.format(
        "about.native.help",
        &[("toolchain", Arg::Str(&native.toolchain))],
    );
    Some((native.fingerprint.to_string(), note))
}

/// The about page.
#[derive(Debug, Default)]
pub struct About;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for About {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "About", Version::new(0, 1, 0))
            .description("PaperPaw's version and licences.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "about.title",
            Box::new(|env, _| Box::new(screen::AboutScreen::new(env))),
        ))?;
        r.add_action(ActionSpec::new(OPEN, "about.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
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
