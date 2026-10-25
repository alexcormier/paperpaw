//! Where a plugin's own settings groups are listed on the settings' main
//! page: by their order, and one that names none at the end of the
//! section for customising PaperPaw, by name, never at the top of the
//! section for books.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::{
    Manifest, Plugin, RegisterError, Registrar, SettingGroupSpec, SettingSpec, Version,
};

/// A community plugin with two groups of its own that name no order, and
/// one that does.
struct Duck;

static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::switch("duck.big", "Big duck", false, Id::new_static("duck.group")),
    SettingSpec::switch("duck.ants", "Ants", false, Id::new_static("duck.ants")),
    SettingSpec::switch("duck.pond", "Pond", false, Id::new_static("duck.pond")),
];

impl Plugin for Duck {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("duck"), "Duck", Version::new(0, 1, 0))
            .description("A duck on the home screen.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting_group(SettingGroupSpec::new(Id::new_static("duck.group"), "Duck"))?;
        r.add_setting_group(SettingGroupSpec::new(
            Id::new_static("duck.ants"),
            "Ant hill",
        ))?;
        // One that names its place: among the books' groups.
        r.add_setting_group(SettingGroupSpec::new(Id::new_static("duck.pond"), "Pond").order(30))?;
        r.add_settings(&SETTINGS)
    }
}

fn titles(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
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
        if app.kernel.device().input.touch {
            app.tap_next_page();
        } else {
            app.run(actions::NEXT_PAGE);
        }
    }
    seen
}

#[test]
fn groups_without_an_order_come_last_among_those_for_customising() {
    for device in DEVICES {
        let mut app = App::with_plugin(device, Shared::memory("", &[]), Duck);
        let rows = main_page(&mut app);
        let at = |title: &str| {
            rows.iter()
                .position(|r| r == title)
                .unwrap_or_else(|| panic!("no {title}: {rows:?}"))
        };
        // Not at the top of the books' section.
        assert!(at("Duck") > at("Reading"), "{rows:?}");
        assert!(at("Duck") > at("Appearance"), "{rows:?}");
        assert!(at("Duck") < at("Advanced settings"), "{rows:?}");
        // By name among themselves.
        assert_eq!(at("Ant hill") + 1, at("Duck"), "{rows:?}");
        // One that names an order is where it says.
        assert!(at("Pond") < at("Appearance"), "{rows:?}");
        assert!(at("Pond") > at("Reading"), "{rows:?}");
    }
}
