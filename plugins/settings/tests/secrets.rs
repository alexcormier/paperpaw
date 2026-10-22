//! *Passwords and keys* with the real kernel and the standard app, on a
//! buttons-only X4 and a touch-only device: listed only once there are
//! some, never shown in clear (not in the list, not on a secret's page,
//! not handed to the keyboard, not in `paperpaw.ini`, not in clear in
//! their own file), typed anew (as a password, trimmed, an empty one
//! ignored) and removed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::secrets::{FILE, SecretChange};

const VALUE: &str = "s3cr3t-Value-42";
const NEW: &str = "n3w-Secret-99";
const ENTRY: &str = "Passwords and keys";

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

fn titles(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

/// Chooses the row called `label`: down to it and confirm, or a tap (on
/// a later page if needed).
fn choose(app: &mut App, label: &str) {
    if touch(app) {
        for _ in 0..10 {
            if titles(app).iter().any(|t| t == label) {
                app.tap_on(label);
                return;
            }
            app.tap_next_page();
        }
    } else {
        for _ in 0..200 {
            if app
                .focused()
                .is_some_and(|f| f.split(", ").next() == Some(label))
            {
                app.run(actions::CONFIRM);
                return;
            }
            app.run(actions::DOWN);
        }
    }
    panic!("no row {label}:\n{}", app.described());
}

/// Every row of the settings' main page, page by page.
fn main_page(app: &mut App) -> Vec<String> {
    app.home();
    app.run(actions::SETTINGS);
    let mut seen = Vec::new();
    for _ in 0..6 {
        for t in titles(app) {
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        if touch(app) {
            app.tap_next_page();
        } else {
            app.run(actions::NEXT_PAGE);
        }
    }
    app.run(actions::BACK);
    app.run(actions::SETTINGS);
    seen
}

/// The app with one secret kept, on the list of secrets.
fn with_secret(device: DeviceInfo) -> App {
    let mut app = App::new(device, Shared::memory("", &[]));
    app.kernel
        .set_secret(SecretChange::keep("test.key", "Test service key", VALUE));
    app.run(actions::SETTINGS);
    choose(&mut app, ENTRY);
    assert_eq!(app.screen(), "settings.secrets");
    app
}

/// Nothing anywhere shows `value`: not the screens, not the
/// configuration, not the secrets' file in clear.
fn hidden_everywhere(app: &mut App, value: &str) {
    let shown = app.described();
    assert!(!shown.contains(value), "{shown}");
    assert!(!app.storage.config().contains(value));
    let kept = app.storage.text(FILE).unwrap_or_default();
    assert!(!kept.contains(value), "{kept}");
}

#[test]
fn the_entry_shows_only_once_there_are_secrets() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        assert!(!main_page(&mut app).iter().any(|t| t == ENTRY));
        app.kernel
            .set_secret(SecretChange::keep("test.key", "Test service key", VALUE));
        assert!(main_page(&mut app).iter().any(|t| t == ENTRY));
    }
}

#[test]
fn a_secret_is_listed_by_its_label_never_its_value() {
    for device in DEVICES {
        let mut app = with_secret(device);
        let rows: Vec<String> = app
            .parts()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .map(|p| p.label)
            .collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].starts_with("Test service key"), "{rows:?}");
        hidden_everywhere(&mut app, VALUE);
        // Its page: what can be done, not what it is.
        choose(&mut app, "Test service key");
        assert_eq!(app.screen(), "settings.secret");
        assert_eq!(titles(&mut app), ["Type anew", "Remove"]);
        hidden_everywhere(&mut app, VALUE);
    }
}

#[test]
fn typed_anew_as_a_password_without_the_old_value() {
    for device in DEVICES {
        let mut app = with_secret(device);
        choose(&mut app, "Test service key");
        app.will_type(&format!("  {NEW}  "));
        choose(&mut app, "Type anew");
        // Asked as a password, not starting from the value kept.
        let question = app.asked().pop().unwrap();
        assert_eq!(question.get(params::MODE), Some("password"));
        assert_ne!(question.get(params::TEXT), Some(VALUE));
        assert!(!question.iter().any(|(_, v)| v.contains(VALUE)));
        // Kept, trimmed; back on the list.
        assert_eq!(app.kernel.secrets().get("test.key"), Some(NEW));
        assert_eq!(app.screen(), "settings.secrets", "{}", app.described());
        hidden_everywhere(&mut app, NEW);
        hidden_everywhere(&mut app, VALUE);
        // A new kernel reads it from its file.
        let app = app.restart();
        assert_eq!(app.kernel.secrets().get("test.key"), Some(NEW));
    }
}

#[test]
fn nothing_typed_keeps_the_secret_as_it_was() {
    for device in DEVICES {
        let mut app = with_secret(device);
        choose(&mut app, "Test service key");
        app.will_type("   ");
        choose(&mut app, "Type anew");
        assert_eq!(app.kernel.secrets().get("test.key"), Some(VALUE));
    }
}

#[test]
fn a_secret_removed_is_gone_with_its_file_and_entry() {
    for device in DEVICES {
        let mut app = with_secret(device);
        assert!(app.storage.exists(FILE));
        choose(&mut app, "Test service key");
        choose(&mut app, "Remove");
        assert_eq!(app.screen(), "settings.secrets");
        assert_eq!(app.kernel.secrets().get("test.key"), None);
        assert!(
            app.shows("show here once they are entered"),
            "{}",
            app.described()
        );
        // The last one gone: its file too, and the entry in the settings.
        assert!(!app.storage.exists(FILE));
        assert!(!main_page(&mut app).iter().any(|t| t == ENTRY));
    }
}

#[test]
fn a_secret_that_cannot_be_typed_anew_offers_only_removing() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        let mut change = SecretChange::keep("test.hash", "Lock code", "c0ffee");
        change.changeable = false;
        app.kernel.set_secret(change);
        app.run(actions::SETTINGS);
        choose(&mut app, ENTRY);
        choose(&mut app, "Lock code");
        assert_eq!(titles(&mut app), ["Remove"]);
        hidden_everywhere(&mut app, "c0ffee");
    }
}

#[test]
fn a_secret_page_for_a_secret_not_kept_only_goes_back() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        app.open_screen(
            Id::new_static("settings.secret"),
            Params::new().with("id", "no.such-secret"),
        );
        assert_eq!(app.screen(), "settings.secret");
        assert_eq!(titles(&mut app), ["Remove"]);
        choose(&mut app, "Remove");
        assert!(app.kernel.secrets().is_empty());
        assert!(!app.storage.exists(FILE));
    }
}

#[test]
fn many_secrets_are_all_reached_and_none_shown() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory("", &[]));
        for n in 0..25 {
            app.kernel.set_secret(SecretChange::keep(
                &format!("test.key-{n:02}"),
                &format!("Key {n:02}"),
                &format!("{VALUE}-{n:02}"),
            ));
        }
        app.run(actions::SETTINGS);
        choose(&mut app, ENTRY);
        choose(&mut app, "Key 24");
        assert_eq!(app.screen(), "settings.secret");
        assert!(app.shows("Key 24"));
        hidden_everywhere(&mut app, VALUE);
    }
}
