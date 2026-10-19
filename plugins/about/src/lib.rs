//! *About PaperPaw*, in the device's settings: its version, its licence
//! and the licences of what it bundles (fonts, icons, hyphenation
//! patterns), which every copy of it must carry. Shown by the reader (as
//! Markdown), so it pages and reads like a book; PaperPaw's logo above it
//! (the picture [`logo::LOGO`] names, if one does). Where PaperPaw is the
//! firmware, also the plugins built into it, which an update brings
//! again.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::format;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{menus, params, slots};
use paperpaw_sdk::logo;
use paperpaw_sdk::{
    ActionSpec, Command, Env, Id, Manifest, MenuEntry, Params, Plugin, RegisterError, Registrar,
    SDK_VERSION, Version,
};

pub const PLUGIN: Id = Id::new_static("about");
/// Shows the page.
pub const OPEN: Id = Id::new_static("about.open");
/// The licences, from `tools/licences/build.py`.
const LICENCES: &str = include_str!("../licences.md");
/// PaperPaw's logo, as the page shows it.
const LOGO: &[u8] = include_bytes!("../logo.png");
/// Where the page is written for the reader: in the plugin's folder, so
/// it is no book.
fn page_path() -> alloc::string::String {
    format!("{}/about.md", Env::plugin_dir(PLUGIN.as_str()))
}

/// The page's text, the logo first: the picture set, or PaperPaw's own
/// written beside the page.
fn page(env: &Env<'_>) -> alloc::string::String {
    let intro = env.i18n.tr("about.intro");
    let set = env.settings.read(&logo::LOGO).trim().trim_matches('/');
    let picture = if !set.is_empty() && env.storage.exists(set) {
        format!("/{set}")
    } else {
        let path = format!("{}/logo.png", Env::plugin_dir(PLUGIN.as_str()));
        if env.storage.size(&path).ok() != Some(LOGO.len() as u64) {
            let _ = env.storage.write(&path, LOGO);
        }
        alloc::string::String::from("logo.png")
    };
    format!(
        "![PaperPaw]({picture})\n\n# PaperPaw {}\n\n{intro}\n\nhttps://github.com/hthoene/paperpaw\n\n{LICENCES}",
        env!("CARGO_PKG_VERSION")
    )
}

/// The about page.
#[derive(Debug, Default)]
pub struct About;

impl Plugin for About {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("About"),
            description: Cow::Borrowed("PaperPaw's version and licences."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(OPEN, "about.title").handler(
            |env: &Env<'_>, out: &mut Vec<Command>| {
                let path = page_path();
                let _ = env.storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
                if env.storage.write(&path, page(env).as_bytes()).is_ok() {
                    out.push(Command::OpenSlot(
                        slots::READER,
                        Params::new().with(params::PATH, path),
                    ));
                }
            },
        ))?;
        // In the device's part of the settings, last.
        r.add_menu_entry(MenuEntry {
            menu: menus::SETTINGS,
            item: OPEN,
            order: 99,
        })?;
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
