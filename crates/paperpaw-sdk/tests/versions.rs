//! Versions: the SDK's `Version` (parsing, SemVer compatibility) and
//! `firmware::compare_versions` for update files and offers (pre-releases,
//! build metadata, garbage).

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

use std::cmp::Ordering;

use paperpaw_sdk::firmware::compare_versions;
use paperpaw_sdk::{SDK_VERSION, Version};
use proptest::prelude::*;

fn version() -> impl Strategy<Value = Version> {
    (any::<u16>(), any::<u16>(), any::<u16>()).prop_map(|(a, b, c)| Version::new(a, b, c))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A version written out is read back as itself.
    #[test]
    fn written_versions_are_read_back(v in version()) {
        prop_assert_eq!(Version::parse(&v.to_string()), Some(v));
        prop_assert_eq!(Version::parse(&format!("  {v}\n")), Some(v));
    }

    /// Parsing any text does not panic, and what it reads is written the
    /// same way again.
    #[test]
    fn any_text_parses_without_panic(text in "\\PC{0,20}|[0-9.+-]{0,12}") {
        if let Some(v) = Version::parse(&text) {
            prop_assert_eq!(Version::parse(&v.to_string()), Some(v));
        }
    }

    /// Every version works with what it requires itself, and with what
    /// an older one of its line requires; never with a newer one.
    #[test]
    fn compatibility_follows_semver(v in version(), w in version()) {
        prop_assert!(v.satisfies(&v));
        if v.satisfies(&w) {
            prop_assert!(v >= w);
            if w.major == 0 {
                prop_assert_eq!((v.major, v.minor), (0, w.minor));
            } else {
                prop_assert_eq!(v.major, w.major);
            }
        }
        if w > v {
            prop_assert!(!v.satisfies(&w));
        }
    }

    /// Comparing versions is an order: each way round gives the reverse,
    /// and a version equals itself.
    #[test]
    fn comparison_is_an_order(
        a in "[0-9]{1,3}(\\.[0-9]{1,3}){0,3}(-[a-z0-9.]{0,6})?(\\+[a-z0-9]{0,4})?|\\PC{0,10}",
        b in "[0-9]{1,3}(\\.[0-9]{1,3}){0,3}(-[a-z0-9.]{0,6})?(\\+[a-z0-9]{0,4})?|\\PC{0,10}",
    ) {
        prop_assert_eq!(compare_versions(&a, &b), compare_versions(&b, &a).reverse());
        prop_assert_eq!(compare_versions(&a, &a), Ordering::Equal);
    }

    /// Numbers count as numbers, not text: `0.10.0` is after `0.9.9`.
    #[test]
    fn numbers_compare_as_numbers(a in version(), b in version()) {
        prop_assert_eq!(compare_versions(&a.to_string(), &b.to_string()), a.cmp(&b));
    }

    /// A pre-release comes before its release and after the release
    /// before; build metadata does not count.
    #[test]
    fn prereleases_and_builds(v in version(), pre in "[a-z0-9.]{1,8}", build in "[a-z0-9.-]{1,8}") {
        let release = v.to_string();
        let pre_release = format!("{v}-{pre}");
        prop_assert_eq!(compare_versions(&pre_release, &release), Ordering::Less);
        prop_assert_eq!(compare_versions(&format!("{release}+{build}"), &release), Ordering::Equal);
        prop_assert_eq!(
            compare_versions(&format!("{pre_release}+{build}"), &pre_release),
            Ordering::Equal
        );
        if v.patch > 0 {
            let before = Version::new(v.major, v.minor, v.patch - 1).to_string();
            prop_assert_eq!(compare_versions(&pre_release, &before), Ordering::Greater);
        }
    }
}

#[test]
fn short_and_odd_forms_parse_as_documented() {
    assert_eq!(Version::parse("1"), Some(Version::new(1, 0, 0)));
    assert_eq!(Version::parse("1.2"), Some(Version::new(1, 2, 0)));
    for invalid in [
        "",
        " ",
        ".",
        "1.",
        "1..2",
        ".1",
        "1.2.3.",
        "1.2.3-dev",
        "1.2.3+x4",
        "v1.2.3",
        "-1.0.0",
        "65536.0.0",
        "1.2.99999",
        "1,2,3",
        "١.٢.٣",
    ] {
        assert_eq!(Version::parse(invalid), None, "{invalid:?}");
    }
    assert_eq!(
        Version::parse("65535.65535.65535"),
        Some(Version::new(65535, 65535, 65535))
    );
}

#[test]
fn sdk_compatibility_of_this_line() {
    let older_patch = Version::new(SDK_VERSION.major, SDK_VERSION.minor, 0);
    assert!(SDK_VERSION.satisfies(&older_patch));
    let next_minor = Version::new(SDK_VERSION.major, SDK_VERSION.minor + 1, 0);
    assert!(!SDK_VERSION.satisfies(&next_minor));
    if SDK_VERSION.major == 0 && SDK_VERSION.minor > 0 {
        // Before 1.0, another minor version is another line.
        let previous = Version::new(0, SDK_VERSION.minor - 1, 0);
        assert!(!SDK_VERSION.satisfies(&previous));
    }
}

#[test]
fn update_versions_as_firmware_names_them() {
    let order = [
        "0.3.9",
        "0.4.0-dev",
        "0.4.0",
        "0.4.1-dev",
        "0.4.1",
        "0.9.0",
        "0.10.0",
        "1.0.0-alpha",
        "1.0.0-beta",
        "1.0.0",
    ];
    for pair in order.windows(2) {
        assert_eq!(
            compare_versions(pair[0], pair[1]),
            Ordering::Less,
            "{pair:?}"
        );
    }
    assert_eq!(
        compare_versions("0.4.0+x4", "0.4.0+x4-pro"),
        Ordering::Equal
    );
    assert_eq!(compare_versions("0.04.0", "0.4.0"), Ordering::Equal);
    // Text that is no version comes before every version.
    assert_eq!(compare_versions("", "0.0.1"), Ordering::Less);
    assert_eq!(compare_versions("garbage", "0.0.0"), Ordering::Less);
    assert_eq!(
        compare_versions("99999999999999999999999.0.0", "1.0.0"),
        Ordering::Less
    );
}

/// SemVer orders numbered pre-releases by number (`rc.2` before
/// `rc.10`); compared as text, `rc.10` comes first, so an update to
/// `1.0.0-rc.10` would not be offered over `1.0.0-rc.9`.
#[test]
#[ignore = "bug: compare_versions compares numbered pre-releases as text (rc.10 before rc.9)"]
fn numbered_prereleases_compare_by_number() {
    assert_eq!(
        compare_versions("1.0.0-rc.9", "1.0.0-rc.10"),
        Ordering::Less
    );
    assert_eq!(compare_versions("1.0.0-2", "1.0.0-10"), Ordering::Less);
}
