//! The settings screens with the real kernel and the standard app, on a
//! buttons-only X4 and a touch-only device: searching by words of the
//! language shown (English and German, accents or not), a hit opening its
//! page at the setting; a choice's list, the value chosen kept in
//! `paperpaw.ini` and read by a new kernel; text typed for a setting;
//! advanced settings; resetting a page and everything.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;

const LIST: &str = "settings.list";
const GROUP: &str = "settings.group";
const RESULTS: &str = "settings.search";
const CHOOSER: &str = "settings.choose";

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// The rows shown, whole labels.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect()
}

/// The first part of each row's label.
fn titles(app: &mut App) -> Vec<String> {
    rows(app)
        .into_iter()
        .map(|r| r.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

/// Whether a row shown as `shown` is the one called `wanted`: the same,
/// or shortened to fit (ending in "…"), or its first part.
fn same(shown: &str, wanted: &str) -> bool {
    let first = shown.split(", ").next().unwrap_or_default();
    shown == wanted
        || first == wanted
        || shown.starts_with(&format!("{wanted}, "))
        || first
            .strip_suffix('…')
            .is_some_and(|short| wanted.starts_with(short))
}

/// Chooses the row called `label`: down to it (the first press may only
/// show the focus) and confirm, or a tap (on a later page if needed).
fn choose(app: &mut App, label: &str) {
    if touch(app) {
        for _ in 0..10 {
            let part = app
                .parts()
                .into_iter()
                .find(|p| p.role == Role::Row && same(&p.label, label));
            if let Some(part) = part {
                app.tap(center(part.area));
                return;
            }
            app.tap_next_page();
        }
    } else {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| same(&f, label)) {
                app.run(actions::CONFIRM);
                return;
            }
            app.run(actions::DOWN);
        }
    }
    panic!("no row {label}:\n{}", app.described());
}

fn settings(device: DeviceInfo, config: &str) -> App {
    let mut app = App::new(device, Shared::memory(config, &[]));
    app.run(actions::SETTINGS);
    assert_eq!(app.screen(), LIST);
    app
}

/// Searches the settings for `words`, as typed on the keyboard: the
/// first row of the main page, in either language.
fn search(app: &mut App, words: &str) {
    app.will_type(words);
    if !titles(app)
        .iter()
        .any(|t| t == "Search the settings" || t == "Einstellungen suchen")
    {
        // On another page: the main page anew, from its top.
        app.run(actions::BACK);
        app.run(actions::SETTINGS);
    }
    let title = titles(app)
        .into_iter()
        .find(|t| t == "Search the settings" || t == "Einstellungen suchen")
        .unwrap_or_else(|| panic!("no search:\n{}", app.described()));
    choose(app, &title);
}

#[test]
fn search_finds_settings_by_words_in_english() {
    for device in DEVICES {
        let mut app = settings(device, "");
        search(&mut app, "  SIZE   font ");
        assert_eq!(app.screen(), RESULTS, "{}", app.described());
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Font size"), "{hits:?}");
        // Every word counts: "size" alone finds more.
        app.run(actions::BACK);
        search(&mut app, "size");
        let more = titles(&mut app);
        assert!(more.len() > hits.len(), "{more:?}");
    }
}

#[test]
fn search_finds_settings_by_words_in_german_with_or_without_accents() {
    for device in DEVICES {
        let mut app = settings(device, "[core]\nlanguage = de\n");
        search(&mut app, "schriftgröße");
        assert_eq!(app.screen(), RESULTS, "{}", app.described());
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Schriftgröße"), "{hits:?}");
        app.run(actions::BACK);
        search(&mut app, "SCHRIFTGROSSE");
        let folded = titles(&mut app);
        assert!(folded.iter().any(|h| h == "Schriftgröße"), "{folded:?}");
    }
}

#[test]
fn a_hit_opens_its_page_with_the_setting_focused() {
    let mut app = settings(X4, "");
    search(&mut app, "font size");
    choose(&mut app, "Font size");
    assert_eq!(app.screen(), GROUP);
    assert!(
        app.focused().unwrap().starts_with("Font size"),
        "{}",
        app.described()
    );
    // Back to the results, then the main page.
    app.run(actions::BACK);
    assert_eq!(app.screen(), RESULTS);
    app.run(actions::BACK);
    assert_eq!(app.screen(), LIST);

    let mut app = settings(TOUCH, "");
    search(&mut app, "font size");
    choose(&mut app, "Font size");
    assert_eq!(app.screen(), GROUP);
    assert!(app.shows("Font size"), "{}", app.described());
}

#[test]
fn search_finds_the_pages_of_other_plugins_and_opens_them() {
    for device in DEVICES {
        let mut app = settings(device, "");
        search(&mut app, "customise menus");
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Customise menus"), "{hits:?}");
        choose(&mut app, "Customise menus");
        assert_eq!(app.screen(), "menu-editor.menus");
    }
}

#[test]
fn a_search_without_hits_says_so_and_an_empty_one_opens_nothing() {
    for device in DEVICES {
        let mut app = settings(device, "");
        search(&mut app, "zzqx flibbertigibbet");
        assert_eq!(app.screen(), RESULTS);
        assert!(
            app.shows("No setting has these words."),
            "{}",
            app.described()
        );
        assert!(rows(&mut app).is_empty());
        app.run(actions::CONFIRM);
        assert_eq!(app.screen(), RESULTS);
        app.run(actions::BACK);
        search(&mut app, "   ");
        assert_eq!(app.screen(), LIST);
    }
}

#[test]
fn advanced_settings_are_found_only_once_shown() {
    for device in DEVICES {
        let mut app = settings(device, "");
        // The progress sync's server is an advanced setting.
        search(&mut app, "server");
        let before = titles(&mut app);
        app.run(actions::BACK);
        choose(&mut app, "Show advanced settings");
        assert!(app.kernel.settings().bool("settings.advanced"));
        search(&mut app, "server");
        let after = titles(&mut app);
        assert!(after.len() > before.len(), "{before:?} {after:?}");
    }
}

#[test]
fn a_choice_of_more_than_two_opens_its_list_and_the_value_is_kept() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "first day week");
        let hit = titles(&mut app).remove(0);
        choose(&mut app, &hit);
        assert_eq!(app.screen(), GROUP);
        if device.input.touch {
            choose(&mut app, &hit);
        } else {
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), CHOOSER, "{}", app.described());
        let options = titles(&mut app);
        assert_eq!(options, ["As the language has it", "Monday", "Sunday"]);
        // With buttons, the focus on the value set.
        if !device.input.touch {
            assert_eq!(app.focused().as_deref(), Some("As the language has it"));
        }
        choose(&mut app, "Sunday");
        assert_eq!(app.screen(), GROUP);
        assert_eq!(
            app.kernel.settings().text("core.week-start"),
            "core.week.sunday"
        );
        assert!(
            storage.config().contains("week-start = core.week.sunday"),
            "{}",
            storage.config()
        );
        let app = app.restart();
        assert_eq!(
            app.kernel.settings().text("core.week-start"),
            "core.week.sunday"
        );
    }
}

#[test]
fn the_language_chosen_shows_at_once_and_after_a_restart() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(device, storage.clone());
        app.open_screen(
            Id::new_static("settings.choose"),
            Params::new().with("id", "core.language"),
        );
        assert_eq!(app.screen(), CHOOSER);
        choose(&mut app, "Deutsch");
        assert_eq!(app.kernel.settings().text("core.language"), "de");
        app.run(actions::SETTINGS);
        assert!(app.shows("Einstellungen"), "{}", app.described());
        let mut app = app.restart();
        app.run(actions::SETTINGS);
        assert!(app.shows("Einstellungen"), "{}", app.described());
    }
}

#[test]
fn a_choice_opened_for_no_setting_shows_nothing_to_choose() {
    for id in [
        "",
        "no.such-setting",
        "core.font-size",
        "..",
        "core.language.x",
    ] {
        for device in DEVICES {
            let storage = Shared::memory("", &[]);
            let mut app = App::new(device, storage.clone());
            let before = storage.config();
            app.open_screen(
                Id::new_static("settings.choose"),
                Params::new().with("id", id),
            );
            assert_eq!(app.screen(), CHOOSER, "{id:?}");
            assert!(rows(&mut app).is_empty(), "{id:?}");
            app.run(actions::CONFIRM);
            app.tap(Point::new(240, 300));
            assert_eq!(storage.config(), before, "{id:?}");
        }
    }
}

/// Typed on the keyboard the settings page opens for a text setting
/// (Calibre's address: the progress sync's server, checked here before, is
/// set on its account screen now, as another server signs out).
#[test]
fn text_typed_for_a_setting_is_kept_trimmed() {
    for device in DEVICES {
        let storage = Shared::memory(
            "[settings]
advanced = on
[calibre]
address = 192.168.1.5
",
            &[],
        );
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "calibre address");
        let hit = rows(&mut app)
            .into_iter()
            .find(|r| r.starts_with("Calibre's address"))
            .unwrap_or_else(|| panic!("{}", app.described()));
        let hit = hit.split(", ").next().unwrap().to_string();
        choose(&mut app, &hit);
        assert_eq!(app.screen(), GROUP);
        app.will_type("  192.168.1.20:9090  ");
        if device.input.touch {
            choose(&mut app, &hit);
        } else {
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), GROUP, "{}", app.described());
        // The keyboard started from the value it had.
        let question = app.asked().pop().unwrap();
        assert!(
            question
                .get(params::TEXT)
                .is_some_and(|t| t == "192.168.1.5")
        );
        assert_eq!(
            app.kernel.settings().text("calibre.address"),
            "192.168.1.20:9090"
        );
        assert!(storage.config().contains("address = 192.168.1.20:9090"));
    }
}

#[test]
fn a_page_reset_asks_first_and_sets_its_settings_back() {
    for device in DEVICES {
        let storage = Shared::memory("[core]\nfont-size = 20\n", &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "font size");
        choose(&mut app, "Font size");
        choose(&mut app, "Reset to defaults");
        assert!(app.shows("Reset the settings of"), "{}", app.described());
        choose(&mut app, "Cancel");
        assert!(app.kernel.settings().is_set("core.font-size"));
        choose(&mut app, "Reset to defaults");
        choose(&mut app, "Yes, reset");
        assert!(!app.kernel.settings().is_set("core.font-size"));
        assert!(
            !storage.config().contains("font-size = 20"),
            "{}",
            storage.config()
        );
    }
}

#[test]
fn everything_reset_keeps_the_language() {
    for device in DEVICES {
        let config = "[core]\nlanguage = de\nfont-size = 20\n[files]\nsort = files.sort.name\n";
        let storage = Shared::memory(config, &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        choose(&mut app, "Alle Einstellungen zurücksetzen");
        assert!(app.shows("Sprache bleibt"), "{}", app.described());
        choose(&mut app, "Ja, zurücksetzen");
        let settings = app.kernel.settings();
        assert!(!settings.is_set("core.font-size"));
        assert!(!settings.is_set("files.sort"));
        assert_eq!(settings.text("core.language"), "de");
    }
}
