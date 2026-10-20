//! Reading aids, as text filters (see `paperpaw_sdk::filters`), set under
//! Settings › Reading › Reading aids:
//!
//! - *Bold word beginnings*: the first half of every word in bold, which
//!   some readers follow more easily ("bionic reading").
//! - *Replace words*: pairs `from = to` separated by `;`, e.g. for old
//!   spellings or names; whole words only, as written.
//!
//! Both are off at first and apply to every book.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::document::{Block, BlockKind, Run, TextStyle};
use paperpaw_sdk::filters::TextFilterSpec;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    Env, Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("aids");
/// Setting: the first half of every word in bold.
pub const BOLD_BEGINNINGS: Setting<bool> = Setting::new("aids.bold-beginnings");
/// Setting: words to replace, `from = to; …`.
pub const REPLACE: Setting<Text> = Setting::new("aids.replace");
const SECTION: &str = "aids.section";

/// The pairs of a replacement list: `from = to` separated by `;`.
fn pairs(list: &str) -> Vec<(String, String)> {
    list.split(';')
        .filter_map(|pair| {
            let (from, to) = pair.split_once('=')?;
            let from = from.trim();
            (!from.is_empty()).then(|| (from.to_string(), to.trim().to_string()))
        })
        .collect()
}

/// Whether the character before `at` and the one from `end` are no part
/// of a word.
fn whole_word(text: &str, at: usize, end: usize) -> bool {
    let before = text[..at].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// Replaces the pairs' words in `block`, last first so earlier places
/// stay where they are.
fn replace(block: &mut Block, pairs: &[(String, String)]) {
    for (from, to) in pairs {
        let mut found: Vec<usize> = block
            .text
            .match_indices(from.as_str())
            .map(|(at, _)| at)
            .filter(|&at| whole_word(&block.text, at, at + from.len()))
            .collect();
        found.reverse();
        for at in found {
            block.replace(at..at + from.len(), to);
        }
    }
}

/// Bold runs for the first half of every word, where no other style is.
fn bold_beginnings(block: &mut Block) {
    if !matches!(block.kind, BlockKind::Paragraph) {
        return;
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut word: Option<(usize, usize)> = None; // start, letters
    let end_word = |start: usize, letters: usize, text: &str, runs: &mut Vec<Run>| {
        if letters < 2 {
            return;
        }
        let bold = letters.div_ceil(2);
        let end = text[start..]
            .char_indices()
            .nth(bold)
            .map_or(text.len(), |(i, _)| start + i);
        runs.push(Run {
            start: start as u32,
            end: end as u32,
            style: TextStyle::BOLD,
        });
    };
    for (i, c) in block.text.char_indices() {
        match (&mut word, c.is_alphabetic()) {
            (Some((_, letters)), true) => *letters += 1,
            (None, true) => word = Some((i, 1)),
            (Some((start, letters)), false) => {
                end_word(*start, *letters, &block.text, &mut runs);
                word = None;
            }
            (None, false) => {}
        }
    }
    if let Some((start, letters)) = word {
        end_word(start, letters, &block.text, &mut runs);
    }
    // Not where the book styles the text itself.
    runs.retain(|run| {
        !block
            .runs
            .iter()
            .any(|r| r.start < run.end && run.start < r.end)
    });
    block.runs.extend(runs);
    block.runs.sort_by_key(|run| run.start);
}

/// The reading aids plugin.
#[derive(Debug, Default)]
pub struct Aids;

/// The aids' part of the reading settings.
static IN_AIDS: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().section(SECTION);

/// Bold word beginnings, and words replaced.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::fixed_switch(
        BOLD_BEGINNINGS.static_str(),
        "aids.setting.bold-beginnings",
        false,
        groups::READING,
    )
    .with_extra(&IN_AIDS)
    .described("aids.setting.bold-beginnings.help"),
    SettingSpec::fixed_text(
        REPLACE.static_str(),
        "aids.setting.replace",
        "",
        groups::READING,
    )
    .advanced()
    .with_extra(&IN_AIDS)
    .described("aids.setting.replace.help"),
];

impl Plugin for Aids {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Reading aids"),
            description: Cow::Borrowed(
                "Bold word beginnings, replaced words: text as you want it.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NOTHING,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_text_filter(TextFilterSpec {
            id: Id::new_static("aids.bold"),
            title: Cow::Borrowed("aids.setting.bold-beginnings"),
            filter: Box::new(|env: &Env<'_>, block: &mut Block| {
                if env.settings.read(&BOLD_BEGINNINGS) {
                    bold_beginnings(block);
                }
            }),
            signature: Box::new(|env: &Env<'_>| {
                if env.settings.read(&BOLD_BEGINNINGS) {
                    String::from("on")
                } else {
                    String::new()
                }
            }),
        })?;
        r.add_text_filter(TextFilterSpec {
            id: Id::new_static("aids.replace"),
            title: Cow::Borrowed("aids.setting.replace"),
            filter: Box::new(|env: &Env<'_>, block: &mut Block| {
                let list = pairs(env.settings.read(&REPLACE));
                if !list.is_empty() {
                    replace(block, &list);
                }
            }),
            signature: Box::new(|env: &Env<'_>| env.settings.read(&REPLACE).trim().to_string()),
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::document::{Align, OffsetMap};

    fn block(text: &str) -> Block {
        Block {
            kind: BlockKind::Paragraph,
            continues: false,
            start: 0,
            end: text.len() as u32,
            text: text.to_string(),
            map: OffsetMap::IDENTITY,
            runs: Vec::new(),
            align: Align::default(),
            indent: None,
            space: None,
            size: None,
            image: None,
        }
    }

    #[test]
    fn words_are_replaced_whole() {
        let mut b = block("Mr. Smith and Mr. Smithers met.");
        replace(&mut b, &pairs("Smith = Jones; Mr. = Mister"));
        assert_eq!(b.text, "Mister Jones and Mister Smithers met.");
        assert_eq!(
            pairs(" a = b ;; =x; c="),
            [
                (String::from("a"), String::from("b")),
                (String::from("c"), String::new())
            ]
        );
    }

    #[test]
    fn beginnings_are_bold() {
        let mut b = block("Read it, Alice!");
        bold_beginnings(&mut b);
        let bold: Vec<&str> = b
            .runs
            .iter()
            .map(|r| &b.text[r.start as usize..r.end as usize])
            .collect();
        assert_eq!(bold, ["Re", "i", "Ali"]);
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
