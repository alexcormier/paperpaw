//! The widget pack: many more widgets for the home screen (and any screen
//! that shows widgets), each with settings of its own and the shared look
//! of `paperpaw_sdk::widgets` (its frame, size and alignment):
//!
//! - time: clocks of several kinds, a calendar, the year so far (`time`);
//! - words and pictures: quotes, notes, to-dos, pictures (`words`);
//! - books: covers, progress, what to read next, highlights (`books`).
//!
//! Each part brings its own translations (`i18n/<part>.<language>.ini`).

#![no_std]

extern crate alloc;

mod books;
mod showings;
mod time;
mod words;

#[cfg(test)]
mod draw_tests;
#[cfg(test)]
mod harness;

use alloc::borrow::Cow;

use paperpaw_sdk::widgets;
use paperpaw_sdk::{Env, Id, Manifest, Plugin, Rect, RegisterError, Registrar, SDK_VERSION};

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
            // Released with PaperPaw, also as a file from the store.
            version: paperpaw_sdk::package_version!(),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        time::register(r)?;
        words::register(r)?;
        books::register(r)?;
        Ok(())
    }
}

/// The height widget `id`'s frame takes above and below its content.
pub(crate) fn frame_height(env: &Env<'_>, id: &str) -> u16 {
    let probe = Rect::new(0, 0, 100, 100);
    100 - widgets::look::look(env, id, probe).area.height
}

/// Today as days since 1970 in the local time zone; 0 where the device
/// does not know the date.
pub(crate) fn today(env: &Env<'_>) -> i64 {
    use paperpaw_sdk::time::{local_seconds, unix_seconds};
    unix_seconds(env).map_or(0, |unix| local_seconds(env, unix).div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

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
