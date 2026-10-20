//! Random sequences of edits of the main menu with the buttons (an
//! entry's options: up, down, remove, cancel): the editor and the menu
//! always agree, the way to the settings stays, and a new kernel reads
//! back the same menu from `paperpaw.ini`.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use proptest::prelude::*;

/// The labels of the rows shown.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

/// The main menu as it shows from the home screen.
fn shown_main_menu(app: &mut App) -> Vec<String> {
    app.home();
    app.run(actions::MENU);
    let shown = rows(app);
    app.run(actions::BACK);
    shown
}

/// The entries of the main menu in its editor (without *Add* and
/// *Reset*).
fn edited(app: &mut App) -> Vec<String> {
    rows(app)
        .into_iter()
        .take_while(|r| r != "Add an entry")
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(32)
    })]

    #[test]
    fn any_edits_keep_the_settings_and_read_back_the_same(
        edits in prop::collection::vec((0usize..8, 0usize..4), 1..10),
    ) {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(X4, storage.clone());
        app.run(Id::new_static("menu-editor.open"));
        for _ in 0..40 {
            if app.focused().as_deref() == Some("Main menu") {
                break;
            }
            app.run(actions::DOWN);
        }
        app.run(actions::CONFIRM);
        for (entry, option) in edits {
            let entries = edited(&mut app);
            let entry = entry % entries.len();
            // From wherever the focus is, up to the first row, then down.
            for _ in 0..12 {
                app.run(actions::UP);
            }
            for _ in 0..20 {
                if app.focused().as_deref() == entries.first().map(String::as_str) {
                    break;
                }
                app.run(actions::DOWN);
            }
            app.repeat(actions::DOWN, entry);
            app.run(actions::CONFIRM);
            prop_assert_eq!(app.screen(), "menu-editor.entry");
            app.repeat(actions::DOWN, option);
            app.run(actions::CONFIRM);
            if app.screen() == "menu-editor.entry" {
                // Refused: any press closes it.
                app.run(actions::CONFIRM);
            }
            prop_assert_eq!(app.screen(), "menu-editor.entries");
        }
        let entries = edited(&mut app);
        prop_assert!(entries.iter().any(|e| e == "Settings"), "{:?}", entries);
        let shown = shown_main_menu(&mut app);
        prop_assert_eq!(&shown, &entries);
        let mut app = app.restart();
        prop_assert_eq!(shown_main_menu(&mut app), shown);
    }
}
