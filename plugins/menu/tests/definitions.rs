//! Menus as anyone may define them in `[menus]` of `paperpaw.ini`:
//! unknown ids, duplicates, sections, settings of every kind, a menu that
//! names itself. Each opens on both
//! kinds of device, its entries are walked round with the buttons, and
//! choosing any entry neither panics nor leaves a menu open that back
//! cannot close.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use proptest::prelude::*;

fn entry() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "core.home",
        "core.library",
        "core.settings",
        "core.night-mode",
        "core.ui-size",
        "core.week-start",
        "menu.style",
        "core.main",
        "core.tools",
        "my.menu",
        "no.such-thing",
        "files.sort",
        "files.root",
        "home.edit-pages",
        "settings.advanced",
    ])
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(48)
    })]

    #[test]
    fn any_menu_opens_is_walked_round_and_any_entry_chosen(
        main in prop::collection::vec(entry(), 0..12),
        mine in prop::collection::vec(entry(), 0..6),
        pick in 0usize..12,
        touch in any::<bool>(),
    ) {
        let config = format!(
            "[menus]\ncore.main = {}\nmy.menu = {}\n",
            main.join(", "),
            mine.join(", ")
        );
        let device = if touch { TOUCH } else { X4 };
        let mut app = App::new(device, Shared::memory(&config, &[]));
        app.run(actions::MENU);
        prop_assert_eq!(app.screen(), "menu.list");
        let rows: Vec<Part> = app
            .parts()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .collect();
        if !touch {
            // Round with the buttons, back to where it began.
            let start = app.focused();
            for _ in 0..rows.len().max(1) * 3 {
                app.run(actions::DOWN);
                if app.focused() == start {
                    break;
                }
            }
            prop_assert_eq!(app.focused(), start);
        }
        if let Some(row) = rows.get(pick % rows.len().max(1)) {
            if touch {
                app.tap(center(row.area));
            } else {
                // From the first row (where the walk ended), down to it.
                let index = rows.iter().position(|r| r == row).unwrap_or(0);
                app.repeat(actions::DOWN, index);
                app.run(actions::CONFIRM);
            }
        }
        // However deep it went, back leads home.
        app.home();
        prop_assert_eq!(app.screen(), "home.dashboard");
    }
}

fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect()
}

#[test]
fn a_menu_that_names_itself_leaves_itself_out() {
    for device in DEVICES {
        let config = "[menus]\ncore.main = core.main, core.home\n";
        let mut app = App::new(device, Shared::memory(config, &[]));
        app.run(actions::MENU);
        assert_eq!(rows(&mut app), ["Home"]);
    }
}
