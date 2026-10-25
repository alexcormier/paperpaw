//! Finding plugins in a library of thousands: what the library picks for
//! the start, a group in its fair order of the day or from A to Z, and a
//! letter gone to; and a library that picks nothing, as before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Shopping;
use paperpaw_sdk::builtin::actions;

/// A plugin of the list, called `name`, in group `group`.
fn entry(id: &str, name: &str, group: &str) -> String {
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Does {id}.", "group": "{group}",
  "author": "Someone", "licence": "MIT", "kind": "data", "added": "2026-01-01",
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 10,
    "date": "2026-01-01"}}}}"#
    )
}

/// A list of `entries` in the order given, with `sections` where a
/// library picks.
fn list(entries: &[String], sections: Option<&str>) -> String {
    let sections = sections.map_or(String::new(), |s| format!(r#", "sections": {s}"#));
    format!(
        r#"{{"format": 2, "plugins": [{}]{sections}}}"#,
        entries.join(", ")
    )
}

/// Three plugins of reading in the library's fair order: not by name.
fn fair(sections: &str) -> String {
    list(
        &[
            entry("cherry", "Cherry", "reading"),
            entry("apple", "Apple", "reading"),
            entry("berry", "Berry", "reading"),
        ],
        Some(sections),
    )
}

/// Where `first` shows before `second`.
fn before(shown: &str, first: &str, second: &str) -> bool {
    let at = |text: &str| {
        shown
            .find(text)
            .unwrap_or_else(|| panic!("{text}: {shown}"))
    };
    at(first) < at(second)
}

#[test]
fn the_librarys_picks_make_the_start() {
    let sections = r#"{"recommended": ["berry", "apple"], "new": ["cherry"],
        "trending": ["gone"], "updated": [], "discover": ["apple", "gone"]}"#;
    let mut shopping = Shopping::new(&fair(sections));
    let shown = shopping.shown();
    for row in ["Search", "Recommended", "New", "Discover", "Reading"] {
        assert!(shown.contains(row), "{row}: {shown}");
    }
    // Nothing picked, no row.
    assert!(!shown.contains("Popular now"), "{shown}");
    assert!(before(&shown, "Search", "Recommended"));
    assert!(before(&shown, "Discover", "Reading"));
    // A pick in the library's order.
    shopping.choose("Recommended");
    let shown = shopping.shown();
    assert!(shown.starts_with("Recommended"), "{shown}");
    assert!(before(&shown, "Berry", "Apple"), "{shown}");
    assert!(!shown.contains("Cherry"), "{shown}");
    // A plugin the device cannot take is left out.
    shopping.run(actions::BACK);
    shopping.choose("Discover");
    let shown = shopping.shown();
    assert!(shown.contains("Apple"), "{shown}");
}

#[test]
fn a_group_shows_the_fair_mix_or_a_to_z() {
    let sections = r#"{"recommended": [], "new": [], "trending": [], "updated": [],
        "discover": []}"#;
    let mut shopping = Shopping::new(&fair(sections));
    // No picks: the newest, as before.
    assert!(shopping.shown().contains("New"));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(shown.contains("Sort by, Fair mix"), "{shown}");
    assert!(before(&shown, "Cherry", "Apple") && before(&shown, "Apple", "Berry"));
    shopping.choose("Sort by");
    shopping.choose("A to Z");
    let shown = shopping.shown();
    assert!(shown.contains("Sort by, A to Z"), "{shown}");
    assert!(before(&shown, "Apple", "Berry") && before(&shown, "Berry", "Cherry"));
}

#[test]
fn a_library_that_picks_nothing_shows_the_start_as_before() {
    let mut shopping = Shopping::new(&list(
        &[
            entry("apple", "Apple", "reading"),
            entry("berry", "Berry", "reading"),
        ],
        None,
    ));
    let shown = shopping.shown();
    assert!(shown.contains("New"), "{shown}");
    assert!(!shown.contains("Recommended"), "{shown}");
    shopping.choose("Reading");
    let shown = shopping.shown();
    // The list's own order is A to Z; no fair mix to choose.
    assert!(shown.contains("Sort by, A to Z"), "{shown}");
    assert!(!shown.contains("Go to letter"), "{shown}");
    shopping.choose("Sort by");
    assert!(!shopping.shown().contains("Fair mix"));
}

#[test]
fn a_letter_goes_to_the_plugins_from_it_on() {
    // Fifty-two plugins, two a letter, in no order of names.
    let entries: Vec<String> = (0..52)
        .map(|n| {
            let letter = char::from(b'A' + ((n * 7) % 26) as u8);
            let id = format!("p{n:02}");
            entry(&id, &format!("{letter}{} plugin", n / 26), "reading")
        })
        .collect();
    let sections = r#"{"recommended": [], "new": [], "trending": [], "updated": [],
        "discover": []}"#;
    let mut shopping = Shopping::new(&list(&entries, Some(sections)));
    shopping.choose("Reading");
    assert!(shopping.shown().contains("Go to letter"));
    shopping.choose("Go to letter");
    // The letters in a grid: on to M with the buttons.
    for _ in 0..30 {
        let on_m = shopping
            .kernel
            .describe()
            .into_iter()
            .any(|p| p.focused && p.label == "M");
        if on_m {
            break;
        }
        shopping.run(actions::RIGHT);
    }
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    // From A to Z now, the focus on the first of M.
    let focused: Vec<String> = shopping
        .kernel
        .describe()
        .into_iter()
        .filter(|p| p.focused)
        .map(|p| p.label)
        .collect();
    assert!(
        focused.iter().any(|label| label.starts_with("M0 plugin")),
        "{focused:?}: {shown}"
    );
}
