//! Libraries of books with names made up at random (letters of several
//! scripts, spaces, dots, upper and lower case, with and without an
//! extension), in either order and view: the library lists exactly the
//! books a format opens, each once, and buttons reach every one. Also
//! book information for paths that lead nowhere.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use proptest::prelude::*;

/// A file name: a stem and an extension the formats know, or not.
fn name() -> impl Strategy<Value = String> {
    (
        "[a-zA-Z0-9äöüÉß日本語 ._-]{1,30}",
        prop::sample::select(vec![".txt", ".TXT", ".epub", ".md", "", ".exe", ".txt.bak"]),
    )
        .prop_map(|(stem, ext)| format!("{stem}{ext}"))
        .prop_filter("a name the storage takes", |n| {
            paperpaw_platform::storage::is_valid_path(&format!("books/{n}"))
        })
}

/// Whether the library lists `name` as a book.
fn is_book(name: &str) -> bool {
    let lower = name.to_lowercase();
    !name.starts_with('.')
        && [".txt", ".epub", ".md"]
            .iter()
            .any(|ext| lower.ends_with(ext) && lower.len() > ext.len())
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48))
    })]

    #[test]
    fn every_book_is_listed_once_and_reached_by_buttons(
        names in prop::collection::btree_set(name(), 1..14),
        by_name in any::<bool>(),
        covers in any::<bool>(),
    ) {
        let files: Vec<(String, Vec<u8>)> = names
            .iter()
            .map(|n| (format!("books/{n}"), b"Some text.".to_vec()))
            .collect();
        let files: Vec<(&str, &[u8])> =
            files.iter().map(|(p, d)| (p.as_str(), d.as_slice())).collect();
        let sort = if by_name { "files.sort.name" } else { "files.sort.recent" };
        let view = if covers { "files.view.covers" } else { "files.view.list" };
        let config = format!("[files]\nsort = {sort}\nview = {view}\n");
        let mut app = App::new(X4, Shared::memory(&config, &files));
        app.run(actions::LIBRARY);
        prop_assert_eq!(app.screen(), "files.browser");
        let books = names.iter().filter(|n| is_book(n)).count();
        let shown = app.parts().into_iter().filter(|p| p.role == Role::Row).count();
        prop_assert!(shown <= books);
        if books == 0 {
            prop_assert!(app.shows("No books here yet"));
            return Ok(());
        }
        // From the first book, one step at a time to the order in the
        // header: one step a book.
        let header = app
            .parts()
            .into_iter()
            .find(|p| p.role == Role::Button)
            .map(|p| p.label)
            .unwrap();
        prop_assert!(shown >= 1);
        let mut steps = 0;
        while app.focused().as_deref() != Some(header.as_str()) && steps < 100 {
            app.run(actions::RIGHT);
            steps += 1;
        }
        prop_assert_eq!(steps, books);
    }
}

#[test]
fn book_information_for_paths_that_lead_nowhere_shows_and_closes() {
    let long = "x/".repeat(300);
    for device in DEVICES {
        let storage = Shared::memory("", &[("books/a.txt", b"Some text.")]);
        let mut app = App::new(device, storage);
        app.run(actions::LIBRARY);
        for path in [
            "books/missing.txt",
            "",
            "../../outside.txt",
            "/absolute.txt",
            "books\\a.txt",
            "books/",
            long.as_str(),
        ] {
            app.open_screen(
                Id::new_static("files.info"),
                Params::new().with(params::PATH, path),
            );
            assert_eq!(app.screen(), "files.info", "{path:?}");
            let _ = app.described();
            app.run(actions::BACK);
            assert_eq!(app.screen(), "files.browser", "{path:?}");
        }
    }
}
