//! Finding one's way in a large library on the X4: a group's plugins in
//! the order asked for, a page of a long list gone to, the newest, the
//! plugins installed with their updates first and all updated at once,
//! the room left said where the device can tell, and a plugin's page with
//! its source and screenshots.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Shared, Shopping};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{Id, Instant, Params, Storage, Version};

const BASE: &str = "https://paperpaw.org/api/v1/plugins";

/// A plugin of the list: id, name, group, version, first published,
/// newest version published, size.
fn entry(id: &str, group: &str, version: &str, added: &str, date: &str, size: u64) -> String {
    let name = name_of(id);
    format!(
        r#"{{"id": "{id}", "name": "{name}", "summary": "Does {id}.", "group": "{group}",
  "author": "Someone", "licence": "MIT", "kind": "data", "added": "{added}",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {size}, "date": "{date}"}}}}"#
    )
}

/// `apple` is called Apple.
fn name_of(id: &str) -> String {
    let mut name = id.to_string();
    name[..1].make_ascii_uppercase();
    name
}

fn list(entries: &[String]) -> String {
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(", "))
}

fn manifest(id: &str, version: &str) -> String {
    format!(
        "[plugin]\nid = {id}\nname = {}\nversion = {version}\nformats = 1\n",
        name_of(id)
    )
}

/// A plugin's page at the library: its plugin.ini, and `more` fields.
fn page(id: &str, version: &str, more: &str) -> String {
    let ini = manifest(id, version);
    format!(
        r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{}", "summary": "Does {id}.",
  "group": "reading", "author": "Someone", "licence": "MIT", "kind": "data", {more}
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {},
    "files": [{{"path": "plugin.ini", "url": "{id}/{version}/plugin.ini", "size": {},
                "sha1": "{}"}}]}}}}}}"#,
        name_of(id),
        ini.len(),
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes())
    )
}

fn page_url(id: &str) -> String {
    format!("{BASE}/{id}.json?device=x4&lang=en")
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

/// Three plugins of reading, each newest and latest updated in another
/// order than by name.
fn fruit() -> String {
    list(&[
        entry("apple", "reading", "1.0.0", "2025-01-01", "2026-01-01", 300),
        entry("berry", "reading", "1.0.0", "2026-05-01", "2026-02-01", 300),
        entry(
            "cherry",
            "reading",
            "1.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ])
}

#[test]
fn a_group_is_sorted_as_asked() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(before(&shown, "Apple", "Berry") && before(&shown, "Berry", "Cherry"));
    // Newest first.
    shopping.choose("Sort by");
    shopping.choose("Newest");
    let shown = shopping.shown();
    assert!(shown.contains("Sort by, Newest"), "{shown}");
    assert!(before(&shown, "Berry", "Cherry") && before(&shown, "Cherry", "Apple"));
    // Most recently updated first.
    shopping.choose("Sort by");
    shopping.choose("Recently updated");
    let shown = shopping.shown();
    assert!(before(&shown, "Cherry", "Berry") && before(&shown, "Berry", "Apple"));
}

#[test]
fn the_newest_come_first() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("New");
    let shown = shopping.shown();
    assert!(shown.starts_with("New"), "{shown}");
    assert!(before(&shown, "Berry", "Cherry") && before(&shown, "Cherry", "Apple"));
}

#[test]
fn a_long_list_goes_to_a_page() {
    let many: Vec<String> = (0..40)
        .map(|n| {
            entry(
                &format!("p{n:02}"),
                "reading",
                "1.0.0",
                "2026-01-01",
                "2026-01-01",
                10,
            )
        })
        .collect();
    let mut shopping = Shopping::new(&list(&many));
    shopping.choose("Reading");
    assert!(shopping.shown().contains("P00"));
    // Which page: as the keyboard a long press of Select opens answers.
    shopping.now += 100;
    let typed = Params::new().with(params::TEXT, "3");
    shopping
        .kernel
        .send_event(Instant(shopping.now), Id::new_static("store.jumped"), typed);
    let shown = shopping.shown();
    assert!(!shown.contains("P00"), "{shown}");
    assert!(!shown.contains("Sort by"), "{shown}");
    assert!(shown.contains("P1") || shown.contains("P2"), "{shown}");
}

/// A storage with `apple` and `cherry` installed in 1.0.0.
fn with_two() -> Shared {
    let storage = Shared::default();
    for id in ["apple", "cherry"] {
        let path = format!("plugins/{id}/plugin.ini");
        storage
            .write(&path, manifest(id, "1.0.0").as_bytes())
            .unwrap();
    }
    storage
}

fn version(shopping: &Shopping, id: &str) -> Option<Version> {
    shopping
        .kernel
        .plugins()
        .get(id)
        .map(|p| p.manifest.version)
}

#[test]
fn updates_come_first_and_all_at_once() {
    let updates = list(&[
        entry("apple", "reading", "1.1.0", "2025-01-01", "2026-01-01", 300),
        entry("berry", "reading", "1.0.0", "2026-05-01", "2026-02-01", 300),
        entry(
            "cherry",
            "reading",
            "2.0.0",
            "2025-06-01",
            "2026-09-01",
            300,
        ),
    ]);
    let mut shopping = Shopping::with(Some("x4"), None, &updates, with_two());
    let shown = shopping.shown();
    assert!(shown.contains("Installed, 2 updates"), "{shown}");
    shopping.choose("Installed");
    shopping.check("installed");
    // *Update all*: one after another, each page fetched in its turn.
    shopping.choose("Update all");
    shopping.serve(&page_url("apple"), page("apple", "1.1.0", "").as_bytes());
    let asked = shopping.requests();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].url, format!("{BASE}/apple/1.1.0/plugin.ini"));
    let ini = manifest("apple", "1.1.0");
    shopping.answer(&asked[0], 200, ini.as_bytes());
    assert_eq!(version(&shopping, "apple"), Some(Version::new(1, 1, 0)));
    shopping.serve(&page_url("cherry"), page("cherry", "2.0.0", "").as_bytes());
    let ini = manifest("cherry", "2.0.0");
    shopping.serve(&format!("{BASE}/cherry/2.0.0/plugin.ini"), ini.as_bytes());
    assert_eq!(version(&shopping, "cherry"), Some(Version::new(2, 0, 0)));
    assert!(shopping.requests().is_empty());
    // Nothing left to update.
    let shown = shopping.shown();
    assert!(!shown.contains("Update all"), "{shown}");
}

#[test]
fn the_room_left_is_said_where_known() {
    let entries = list(&[
        entry("apple", "reading", "1.0.0", "2025-01-01", "2026-01-01", 300),
        entry("big", "reading", "1.0.0", "2025-01-01", "2026-01-01", 9000),
    ]);
    // Not known: not said.
    let mut shopping = Shopping::new(&entries);
    assert!(!shopping.shown().contains("free"));
    shopping.choose("Reading");
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", "").as_bytes());
    assert!(!shopping.shown().contains("free"));

    // 5 KB free: beside the title, on a plugin's page, and a plugin too
    // large offers no *Install*.
    let storage = Shared::default();
    storage.1.set(Some(5000));
    let mut shopping = Shopping::with(Some("x4"), None, &entries, storage);
    let shown = shopping.shown();
    assert!(shown.contains("4.9 KB free"), "{shown}");
    shopping.choose("Reading");
    shopping.choose("Apple");
    shopping.serve(&page_url("apple"), page("apple", "1.0.0", "").as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Needs 61 B, 4.9 KB free"), "{shown}");
    assert!(shown.contains("Install, 61 B"), "{shown}");
    shopping.run(actions::BACK);
    shopping.choose("Big");
    // Its files take 9000 bytes, as the list said.
    let big = page("big", "1.0.0", "").replacen(r#""size": 57,"#, r#""size": 9000,"#, 1);
    shopping.serve(&page_url("big"), big.as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
}

#[test]
fn a_plugins_page_says_where_its_source_is() {
    let mut shopping = Shopping::new(&fruit());
    shopping.choose("Reading");
    shopping.choose("Apple");
    let more = r#""source": "https://codeberg.org/someone/apple",
  "author_since": "2024-03-02", "author_plugins": 3,
  "screenshots": [
    {"url": "shots/apple-1.png", "width": 360, "height": 600, "size": 20000, "alt": "The list"},
    {"url": "shots/apple-2.webp", "width": 360, "height": 600, "size": 20000},
    {"url": "shots/apple-3.png", "width": 3000, "height": 600, "size": 20000}],"#;
    let body = page("apple", "1.0.0", more).replace(
        r#""latest": {"#,
        r#""latest": {"source": {"url": "https://codeberg.org/someone/apple/src/tag/v1.0.0"}, "#,
    );
    shopping.serve(&page_url("apple"), body.as_bytes());
    // Data only: no build to check; the author since when; the
    // screenshots this device can decode.
    shopping.check("page-full");
    // The source of this version, as a code to scan.
    shopping.choose("Source code");
    let shown = shopping.shown();
    assert!(
        shown.contains("codeberg.org/someone/apple/src/tag/v1.0.0"),
        "{shown}"
    );
    shopping.run(actions::BACK);
    // The screenshot fetched when shown.
    shopping.choose("Screenshots");
    shopping.request(&format!("{BASE}/shots/apple-1.png"));
}
