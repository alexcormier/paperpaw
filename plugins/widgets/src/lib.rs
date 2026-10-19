//! The widget pack: many more widgets for the home screen (and any screen
//! that shows widgets), each with settings of its own and the shared look
//! of `quireink_sdk::widgets` (its frame, size and alignment):
//!
//! - time: clocks of several kinds, a calendar, the year so far (`time`);
//! - words and pictures: quotes, notes, to-dos, pictures (`words`);
//! - books: covers, progress, what to read next, highlights (`books`).
//!
//! Each part brings its own translations (`i18n/<part>.<language>.ini`).

#![no_std]

extern crate alloc;

mod books;
mod time;
mod words;

use alloc::borrow::Cow;

use quireink_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, Version};

pub const PLUGIN: Id = Id::new_static("widgets");

/// The widget pack.
#[derive(Debug, Default, Clone, Copy)]
pub struct Widgets;

impl Plugin for Widgets {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Widget pack"),
            description: Cow::Borrowed(
                "Many more widgets for the home screen: clocks, a calendar, quotes, books, notes and more.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        time::register(r)?;
        words::register(r)?;
        books::register(r)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let parts = [
            (
                include_str!("../i18n/time.en.ini"),
                include_str!("../i18n/time.de.ini"),
            ),
            (
                include_str!("../i18n/words.en.ini"),
                include_str!("../i18n/words.de.ini"),
            ),
            (
                include_str!("../i18n/books.en.ini"),
                include_str!("../i18n/books.de.ini"),
            ),
        ];
        for (en, de) in parts {
            let en = Catalog::parse(en).unwrap();
            let de = Catalog::parse(de).unwrap();
            let issues = check::compare(&en, &de);
            assert!(issues.is_empty(), "{issues:?}");
        }
        // The parts share one namespace: a key in two of them says the
        // same in both (the later would hide the earlier).
        let mut seen: alloc::vec::Vec<(&str, &str)> = alloc::vec::Vec::new();
        for (en, _) in parts {
            for line in en.lines() {
                let Some((key, text)) = line.split_once(" = ") else {
                    continue;
                };
                match seen.iter().find(|(k, _)| *k == key) {
                    Some((_, before)) => assert_eq!(*before, text, "{key}"),
                    None => seen.push((key, text)),
                }
            }
        }
    }
}
