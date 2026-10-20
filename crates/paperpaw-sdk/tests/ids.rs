//! Ids: which texts are ids (lowercase segments of `[a-z0-9-]` separated
//! by `.`, placements as `@` and a number), their namespace, ownership,
//! and placements.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

use paperpaw_sdk::{Id, InvalidId};
use proptest::prelude::*;

/// What an id is, written out plainly: segments of `[a-z0-9-]`, each
/// with an optional `@` and digits, separated by dots.
fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|segment| {
            let (name, placement) = match segment.split_once('@') {
                Some((name, n)) => (name, Some(n)),
                None => (segment, None),
            };
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && placement.is_none_or(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any text is an id exactly when it has the form of one.
    #[test]
    fn ids_are_what_the_rules_say(text in "[a-z0-9.@_A-Z -]{0,12}|\\PC{0,6}") {
        prop_assert_eq!(Id::parse(&text).is_ok(), is_id(&text), "{:?}", text);
    }

    /// An id is its text: written and parsed again it is the same, its
    /// namespace is its first segment, and it belongs to that plugin.
    #[test]
    fn ids_keep_their_text(segments in proptest::collection::vec("[a-z0-9-]{1,5}", 1..5)) {
        let text = segments.join(".");
        let id = Id::parse(&text).unwrap();
        prop_assert_eq!(id.as_str(), text.as_str());
        prop_assert_eq!(&Id::parse(&id.to_string()).unwrap(), &id);
        prop_assert_eq!(id.namespace(), segments[0].as_str());
        prop_assert!(id.is_owned_by(&Id::parse(&segments[0]).unwrap()));
        prop_assert_eq!(id.placed(), None);
        prop_assert_eq!(id.clone().static_str(), "");
    }

    /// A placement of an id is an id, and without its placement it is
    /// the id again; settings below a placement are found the same way.
    #[test]
    fn placements_are_ids_and_come_off_again(
        segments in proptest::collection::vec("[a-z0-9-]{1,5}", 1..4),
        n in any::<u32>(),
        setting in "[a-z]{1,5}",
    ) {
        let id = Id::parse(&segments.join(".")).unwrap();
        let placed = id.placement(n);
        prop_assert!(Id::parse(placed.as_str()).is_ok());
        prop_assert_eq!(placed.placed(), Some(id.to_string()));
        let below = Id::parse(&format!("{placed}.{setting}")).unwrap();
        prop_assert_eq!(below.placed(), Some(format!("{id}.{setting}")));
        // The placement of a plugin's own id is still the plugin's.
        if segments.len() > 1 {
            prop_assert_eq!(placed.namespace(), segments[0].as_str());
        }
    }
}

#[test]
fn texts_that_are_no_ids() {
    for invalid in [
        "", ".", "..", "a.", ".a", "a..b", "A", "a.B", "a b", "a_b", "a/b", "../a", "a\\b", "a:b",
        "Ã¤", "a\u{0}", "a\n", " a", "a@", "a@x", "a@-1", "a@1@2", "@1", "a.@1", "a@1b.c",
    ] {
        assert_eq!(Id::parse(invalid), Err(InvalidId), "{invalid:?}");
    }
    for valid in [
        "a",
        "-",
        "0",
        "a-",
        "-a",
        "9.9",
        "a.b-c.d0",
        "a@0",
        "a@007.b",
        "a.b@1.c@2",
    ] {
        assert!(Id::parse(valid).is_ok(), "{valid:?}");
    }
    assert!(!InvalidId.to_string().is_empty());
}

#[test]
fn namespaces_are_whole_segments() {
    let id = Id::parse("reader.font-size").unwrap();
    assert!(id.is_owned_by(&Id::new_static("reader")));
    assert!(!id.is_owned_by(&Id::new_static("read")));
    assert!(!id.is_owned_by(&Id::new_static("reader.font-size")));
    // An id of one segment is its own namespace.
    let plugin = Id::new_static("reader");
    assert!(plugin.is_owned_by(&plugin));
    // A placed first segment names another namespace.
    let placed = Id::parse("widgets@2.face").unwrap();
    assert_eq!(placed.namespace(), "widgets@2");
    assert!(!placed.is_owned_by(&Id::new_static("widgets")));
}

#[test]
fn ids_order_and_compare_by_text() {
    let mut ids = [
        Id::new_static("b.a"),
        Id::parse("a.b").unwrap(),
        Id::new_static("a"),
        Id::new_static("a-b"),
    ];
    ids.sort();
    let texts: Vec<&str> = ids.iter().map(Id::as_str).collect();
    assert_eq!(texts, ["a", "a-b", "a.b", "b.a"]);
    assert_eq!(format!("{:?}", Id::new_static("a.b")), "Id(a.b)");
    const FIXED: Id = Id::new_static("core.home");
    assert_eq!(FIXED.static_str(), "core.home");
}

#[test]
fn placing_twice_is_no_id() {
    // A placement of a placement is not an id: `placement` takes ids
    // that are not placed yet.
    let placed = Id::new_static("widgets.clock").placement(2).placement(3);
    assert_eq!(placed.as_str(), "widgets.clock@2@3");
    assert!(Id::parse(placed.as_str()).is_err());
}
