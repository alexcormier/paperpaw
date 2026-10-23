//! The first-run setup with the real kernel and the standard app, on a
//! buttons-only X4 and a touch-only device: it opens on the first start
//! only; every page walked through by buttons and by touch ends at the
//! home screen with the choices kept in `paperpaw.ini` (read by a new
//! kernel, which does not show the guide again); skipped on its first
//! page; back goes a page back; cards that open a list or a screen come
//! back to the guide; and it runs again from the settings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;

const GUIDE: &str = "setup.guide";
const HOME: &str = "home.dashboard";

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// The app on its first start: nothing set, no book read.
fn first_start(device: DeviceInfo) -> App {
    let mut app = App::new(device, Shared::bare("", &[]));
    assert_eq!(app.screen(), GUIDE, "{}", app.described());
    app
}

/// The question on the page shown.
fn question(app: &mut App) -> String {
    app.parts()
        .into_iter()
        .find(|p| p.role == Role::Text)
        .map(|p| p.label)
        .unwrap_or_default()
}

/// The guide's button that says `label` (Next, Back, Skip and the like):
/// tapped, or with the buttons the focus moved onto it and confirmed.
fn press(app: &mut App, label: &str) {
    if touch(app) {
        let button = app
            .parts()
            .into_iter()
            .rev()
            .find(|p| p.role == Role::Button && p.label == label)
            .unwrap_or_else(|| panic!("no {label}:\n{}", app.described()));
        app.tap(center(button.area));
        return;
    }
    for _ in 0..6 {
        if app.focused().as_deref() == Some(label) {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no {label}:\n{}", app.described());
}

/// Walks to the end with *Next* on every page; the questions asked.
fn walk(app: &mut App, next: &str, done: &str) -> Vec<String> {
    let mut asked = Vec::new();
    for _ in 0..20 {
        if app.screen() != GUIDE {
            return asked;
        }
        asked.push(question(app));
        let label = if app.shows(done) { done } else { next };
        press(app, label);
    }
    panic!("the guide does not end:\n{}", app.described());
}

#[test]
fn every_page_walked_through_ends_at_home_and_the_guide_stays_done() {
    for device in DEVICES {
        let mut app = first_start(device);
        let asked = walk(&mut app, "Next", "Start reading");
        assert_eq!(app.screen(), HOME);
        assert_eq!(asked.first().map(String::as_str), Some("Language"));
        assert_eq!(
            asked.last().map(String::as_str),
            Some("Where your books come from")
        );
        for step in ["Font size", "Time zone", "Screen lock"] {
            assert!(asked.iter().any(|a| a == step), "{step}: {asked:?}");
        }
        // Buttons only: how the buttons work is asked too.
        assert_eq!(
            asked.len(),
            if device.input.touch { 7 } else { 8 },
            "{asked:?}"
        );
        assert!(app.storage.exists(SETUP_DONE));
        let app = app.restart();
        assert_eq!(app.screen(), HOME);
    }
}

#[test]
fn the_choices_made_are_kept_and_read_by_a_new_kernel() {
    for device in DEVICES {
        let mut app = first_start(device);
        let size = "core.font-size";
        let before = app.kernel.settings().int(size);
        // The language: German, the second of two.
        if touch(&app) {
            app.tap_on("Deutsch");
        } else {
            // Up from Next to the card; Select picks the other one.
            app.run(actions::UP);
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.kernel.settings().text("core.language"), "de");
        // The guide speaks German at once.
        assert!(app.shows("Weiter"), "{}", app.described());
        press(&mut app, "Weiter");
        // Buttons only: the page on the buttons first.
        if !touch(&app) {
            press(&mut app, "Weiter");
        }
        assert_eq!(question(&mut app), "Schriftgröße");
        if touch(&app) {
            app.tap_on("+");
            app.tap_on("+");
        } else {
            // As on the settings pages: Select, two steps, Select keeps.
            app.run(actions::UP);
            app.run(actions::CONFIRM);
            app.run(actions::RIGHT);
            app.run(actions::RIGHT);
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.kernel.settings().int(size), before + 2);
        walk(&mut app, "Weiter", "Los geht's");
        assert_eq!(app.screen(), HOME);
        let config = app.storage.config();
        assert!(config.contains("language = de"), "{config}");
        assert!(
            config.contains(&format!("font-size = {}", before + 2)),
            "{config}"
        );
        let app = app.restart();
        assert_eq!(app.screen(), HOME);
        assert_eq!(app.kernel.settings().text("core.language"), "de");
        assert_eq!(app.kernel.settings().int(size), before + 2);
    }
}

#[test]
fn skipped_on_its_first_page_it_does_not_come_again() {
    for device in DEVICES {
        let mut app = first_start(device);
        if touch(&app) {
            press(&mut app, "Skip setup");
        } else {
            // Back asks first: going on is the first answer, skipping the
            // second.
            app.run(actions::BACK);
            assert_eq!(app.screen(), "core.choice-dialog", "{}", app.described());
            assert!(!app.storage.exists(SETUP_DONE));
            press(&mut app, "Skip setup");
        }
        assert_eq!(app.screen(), HOME);
        assert!(app.storage.exists(SETUP_DONE));
        // Nothing was changed.
        assert_eq!(app.kernel.settings().text("core.language"), "en");
        assert!(!app.kernel.settings().is_set("core.font-size"));
        let app = app.restart();
        assert_eq!(app.screen(), HOME);
    }
}

#[test]
fn back_goes_a_page_back_and_page_buttons_turn_pages() {
    for device in DEVICES {
        let mut app = first_start(device);
        press(&mut app, "Next");
        let second = question(&mut app);
        press(&mut app, "Next");
        assert_ne!(question(&mut app), second);
        if touch(&app) {
            press(&mut app, "Back");
        } else {
            app.run(actions::BACK);
        }
        assert_eq!(question(&mut app), second);
        app.run(actions::PREV_PAGE);
        assert_eq!(question(&mut app), "Language");
        // Before the first page: nothing.
        app.run(actions::PREV_PAGE);
        assert_eq!(question(&mut app), "Language");
        app.run(actions::NEXT_PAGE);
        assert_eq!(question(&mut app), second);
        assert!(!app.storage.exists(SETUP_DONE));
    }
}

/// Goes to the page asking `wanted` with *Next*.
fn page_of(app: &mut App, wanted: &str) {
    for _ in 0..10 {
        if question(app) == wanted {
            return;
        }
        press(app, "Next");
    }
    panic!("no page {wanted}:\n{}", app.described());
}

/// Chooses the page's card: up to it from *Next* and confirm, or a tap.
fn card(app: &mut App, label: &str) {
    if touch(app) {
        app.tap_on(label);
    } else {
        app.run(actions::UP);
        app.run(actions::CONFIRM);
    }
}

#[test]
fn a_choice_of_many_opens_its_list_and_the_guide_shows_what_was_chosen() {
    for device in DEVICES {
        let mut app = first_start(device);
        page_of(&mut app, "Time zone");
        card(&mut app, "Automatic (system)");
        assert_eq!(app.screen(), "settings.choose", "{}", app.described());
        if touch(&app) {
            app.tap_on("London");
        } else {
            for _ in 0..40 {
                if app.focused().is_some_and(|f| f.starts_with("London")) {
                    break;
                }
                app.run(actions::DOWN);
            }
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), GUIDE);
        assert_eq!(question(&mut app), "Time zone");
        assert!(app.shows("London"), "{}", app.described());
    }
}

#[test]
fn a_page_that_opens_a_screen_comes_back_to_the_guide() {
    for device in DEVICES {
        let mut app = first_start(device);
        page_of(&mut app, PAGE_STEP);
        card(&mut app, "Open");
        assert_ne!(app.screen(), GUIDE);
        app.run(actions::BACK);
        assert_eq!(app.screen(), GUIDE, "{}", app.described());
        assert_eq!(question(&mut app), PAGE_STEP);
    }
}

#[test]
fn it_runs_again_from_the_settings_and_ends_there() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        assert_eq!(app.screen(), HOME);
        app.run(actions::SETTINGS);
        app.run(Id::new_static("setup.open"));
        assert_eq!(app.screen(), GUIDE);
        assert_eq!(question(&mut app), "Language");
        walk(&mut app, "Next", "Start reading");
        assert_eq!(app.screen(), "settings.list");
    }
}

#[test]
fn a_device_used_before_does_not_show_it() {
    for device in DEVICES {
        let storage = Shared::bare(
            "",
            &[
                ("books/a.txt", b"Some text."),
                (".paperpaw/library/opened.txt", b"books/a.txt"),
            ],
        );
        let app = App::new(device, storage);
        assert_eq!(app.screen(), HOME);
        assert!(app.storage.exists(SETUP_DONE));
    }
}

/// Whether the page's number is being changed with the buttons: its −
/// and + filled, as on the settings pages.
fn changing(app: &mut App) -> bool {
    app.parts()
        .iter()
        .any(|p| p.role == Role::Button && p.label == "−" && p.on == Some(true))
}

#[test]
fn a_number_is_changed_with_the_buttons_as_on_the_settings_pages() {
    let mut app = first_start(X4);
    let size = "core.font-size";
    let before = app.kernel.settings().int(size);
    page_of(&mut app, "Font size");
    // On the card, right moves on: the page is never a trap.
    app.run(actions::UP);
    app.run(actions::RIGHT);
    assert_eq!(app.kernel.settings().int(size), before);
    assert_eq!(app.focused().as_deref(), Some("Next"));
    // Select changes it; left and right move it, shown at once.
    app.run(actions::LEFT);
    assert!(!changing(&mut app));
    app.run(actions::CONFIRM);
    assert!(changing(&mut app), "{}", app.described());
    app.run(actions::RIGHT);
    app.run(actions::RIGHT);
    assert_eq!(app.kernel.settings().int(size), before + 2);
    // Back: the value before again, the page stays.
    app.run(actions::BACK);
    assert_eq!(app.kernel.settings().int(size), before);
    assert_eq!(question(&mut app), "Font size");
    assert!(!changing(&mut app));
    // Select keeps it.
    app.run(actions::CONFIRM);
    app.run(actions::RIGHT);
    app.run(actions::CONFIRM);
    assert_eq!(app.kernel.settings().int(size), before + 1);
    assert!(
        app.storage
            .config()
            .contains(&format!("font-size = {}", before + 1))
    );
    // A page button keeps it and turns the page, as ever.
    app.run(actions::CONFIRM);
    app.run(actions::RIGHT);
    app.run(actions::NEXT_PAGE);
    assert_eq!(app.kernel.settings().int(size), before + 2);
    assert_ne!(question(&mut app), "Font size");
}

/// How high the ink of the first line of the sample of the text is, as
/// drawn at `size` points.
fn sample_ink(app: &mut App, size: i64) -> u16 {
    app.set("core.font-size", &size.to_string());
    app.render();
    // Told inside the card's button, which names the value: as drawn.
    let drawn = app.kernel.described();
    let sample = drawn
        .iter()
        .find(|d| d.label.starts_with("The quick brown fox"))
        .unwrap_or_else(|| {
            panic!(
                "no sample at {size}:
{}",
                app.described()
            )
        });
    let a = sample.area;
    let (mut top, mut bottom) = (None, 0);
    for y in a.y..a.y + a.height {
        let inked = (a.x..a.x + a.width).any(|x| app.inked(i32::from(x), i32::from(y)));
        if inked {
            top.get_or_insert(y);
            bottom = y;
        } else if top.is_some() {
            break;
        }
    }
    top.map_or(0, |top| bottom - top + 1)
}

/// The sample of the text shows the size chosen, in the reading font: it
/// grows with each size, also past 12, and is never smaller at a larger
/// size (the tests' bitmap fonts stop at 48 pixels, about 16 points here).
#[test]
fn the_sample_shows_the_size_chosen() {
    for device in DEVICES {
        let mut app = first_start(device);
        page_of(&mut app, "Font size");
        let inks: Vec<u16> = [10, 12, 16, 20, 24]
            .iter()
            .map(|s| sample_ink(&mut app, *s))
            .collect();
        assert!(inks[..3].windows(2).all(|w| w[0] < w[1]), "{inks:?}");
        assert!(inks.windows(2).all(|w| w[0] <= w[1]), "{inks:?}");
    }
}
