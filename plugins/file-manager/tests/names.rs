//! Renaming with names made up at random (dots, slashes, backslashes,
//! colons, control characters, other scripts): whatever is typed, the
//! file stays in its folder under exactly one name, nothing else on the
//! storage changes, and the file manager stays usable.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use proptest::prelude::*;

/// Every file of the storage with its content, but PaperPaw's own data.
fn files(storage: &Shared, dir: &str, out: &mut Vec<(String, String)>) {
    for name in storage.names(dir) {
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        if let Some(folder) = path.strip_suffix('/') {
            if !folder.starts_with(".paperpaw") {
                files(storage, folder, out);
            }
        } else if path != paperpaw_sdk::builtin::CONFIG_FILE {
            let text = storage.text(&path).unwrap_or_default();
            out.push((path, text));
        }
    }
}

fn typed() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            ".", "..", "/", "\\", ":", "\0", "\n", "\t", " ", "a", "Z", "ü", "日", "-", "_", "(2)",
            ".txt", "beta.txt", "~", "*", "?",
        ]),
        0..8,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64))
    })]

    #[test]
    fn a_renamed_file_stays_in_its_folder_under_one_name(text in typed()) {
        let storage = Shared::memory("", &[
            ("books/alpha.txt", b"Alpha"),
            ("books/beta.txt", b"Beta"),
            ("notes.txt", b"Notes"),
        ]);
        let mut before = Vec::new();
        files(&storage, "", &mut before);
        let mut app = App::new(X4, storage.clone());
        app.run(Id::new_static("file-manager.open"));
        app.run(actions::CONFIRM);
        prop_assert!(app.shows("alpha.txt"));
        // alpha.txt is after the folder-less list's top: down to it.
        while !app.focused().unwrap_or_default().starts_with("alpha.txt") {
            app.run(actions::DOWN);
        }
        app.run(actions::MENU);
        app.will_type(&text);
        // Rename, after Open.
        app.run(actions::DOWN);
        app.run(actions::CONFIRM);
        prop_assert_eq!(app.screen(), "file-manager.browser");
        let mut after = Vec::new();
        files(&storage, "", &mut after);
        prop_assert_eq!(after.len(), before.len());
        let alpha: Vec<&String> = after
            .iter()
            .filter(|(_, text)| text == "Alpha")
            .map(|(path, _)| path)
            .collect();
        prop_assert_eq!(alpha.len(), 1);
        prop_assert!(alpha[0].starts_with("books/"), "{}", alpha[0]);
        prop_assert!(!alpha[0]["books/".len()..].contains('/'), "{}", alpha[0]);
        // The others are where they were.
        for (path, text) in &before {
            if text != "Alpha" {
                prop_assert!(after.contains(&(path.clone(), text.clone())), "{path}");
            }
        }
        // Still usable: back to the top.
        app.run(actions::BACK);
        prop_assert!(app.shows("books"));
    }
}
