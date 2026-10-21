//! `profiles.ini` and `last.txt` on the storage: every version's file in
//! `testdata` read, the current writer read back (also for names and
//! values with brackets, `=`, quotes, line breaks and spaces, property
//! based), damaged and hostile files read without panics, and a file of a
//! newer version left as it is.

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;

fn profile(name: &str, values: &[(&'static str, &str)]) -> Profile {
    Profile {
        name: name.to_string(),
        values: values
            .iter()
            .map(|(id, value)| (Id::new_static(id), value.to_string()))
            .collect(),
    }
}

/// The profiles `testdata/profiles.ini.v2` holds.
fn version_2_profiles() -> Vec<Profile> {
    Vec::from([
        profile(
            "Night reading",
            &[("core.night-mode", "on"), ("core.font-size", "12")],
        ),
        profile("  Large [print] = big ", &[("core.font-size", "18")]),
        profile("Quote \" and \\ back", &[("core.line-spacing", "")]),
    ])
}

#[test]
fn files_of_version_2_are_read() {
    let file = include_str!("../../testdata/profiles.ini.v2");
    let storage = MemoryStorage::new().with(&path(), file);
    assert_eq!(load(&storage).unwrap(), version_2_profiles());
}

/// The writer of today still writes what `testdata` holds for version 2,
/// so the file there is what version 2 is.
#[test]
fn the_writer_writes_version_2() {
    let storage = MemoryStorage::new();
    save(&storage, &version_2_profiles());
    let written = String::from_utf8(storage.get(&path()).unwrap()).unwrap();
    let kept = include_str!("../../testdata/profiles.ini.v2").replace("\r\n", "\n");
    assert_eq!(written, kept);
}

#[test]
fn version_1_upgrades_keep_only_valid_ids_and_skip_lines_before_a_profile() {
    let v1 = "core.night-mode = on\n\
              [ Spaced ]\n\
              core.font-size = 14\n\
              not an id! = 3\n\
              no equals sign\n\
              [Empty]\n\
              []\n\
              reader.margin = 2 = 3\n";
    let storage = MemoryStorage::new().with(&path(), v1);
    let loaded = load(&storage).unwrap();
    assert_eq!(
        loaded,
        [
            profile("Spaced", &[("core.font-size", "14")]),
            profile("Empty", &[]),
            profile("", &[("reader.margin", "2 = 3")]),
        ]
    );
}

#[test]
fn damaged_files_give_what_can_be_read() {
    for text in [
        "",
        "# format 2\n",
        "# format 2",
        "# format 2\n[profile.1]\nname = \"unclosed\n",
        "# format 2\n[profile.1\nname = x\n",
        "# format 2\nname = outside\n[other]\nname = not a profile\n",
        "# format\n[x]\n",
        "# format 999999999999999999999\n[profile.1]\nname = x\n",
        "\u{feff}[Night]\ncore.night-mode = on\n",
        "[[[\n]]]\n= =\n",
    ] {
        let storage = MemoryStorage::new().with(&path(), text);
        let _ = load(&storage);
    }
    // A section that is not a profile is skipped, one that is, is read.
    let storage = MemoryStorage::new().with(
        &path(),
        "# format 2\n[other]\nname = no\n[profile.9]\nname = yes\n",
    );
    assert_eq!(load(&storage).unwrap(), [profile("yes", &[])]);
    // Not UTF-8: cannot be read, so nothing may be saved over it; no panic.
    let storage = MemoryStorage::new().with(
        &path(),
        b"# format 2\n[profile.1]\nname = \xff\xfe\n".to_vec(),
    );
    assert_eq!(load(&storage), None);
}

#[test]
fn a_profile_without_a_name_line_keeps_its_values() {
    let storage = MemoryStorage::new().with(
        &path(),
        "# format 2\n[profile.1]\ncore.night-mode = on\nname = Later\n",
    );
    assert_eq!(
        load(&storage).unwrap(),
        [profile("Later", &[("core.night-mode", "on")])]
    );
}

#[test]
fn a_file_of_a_newer_version_is_left_as_it_is() {
    let newer = "# format 3\n[set.1]\ntitle = From the future\n";
    let storage = MemoryStorage::new().with(&path(), newer);
    // Not read, not written over.
    assert_eq!(load(&storage), None);
    save(&storage, &[profile("Now", &[])]);
    assert_eq!(storage.get(&path()).unwrap(), newer.as_bytes());
}

#[test]
fn files_larger_than_the_limit_are_not_read() {
    let mut text = String::from("# format 2\n[profile.1]\nname = Big\n");
    while text.len() <= 64 * 1024 {
        text.push_str("core.font-size = 12\n");
    }
    let storage = MemoryStorage::new().with(&path(), text);
    assert_eq!(load(&storage), None);
}

#[test]
fn the_last_profile_is_one_line_trimmed() {
    let storage = MemoryStorage::new();
    assert_eq!(last(&storage), "");
    set_last(&storage, "Night");
    assert_eq!(last(&storage), "Night");
    storage.write(&last_path(), b"  Spaced \r\n").unwrap();
    assert_eq!(last(&storage), "Spaced");
    // Not UTF-8: read as far as it goes.
    storage.write(&last_path(), b"N\xffght").unwrap();
    assert_eq!(last(&storage), "N\u{fffd}ght");
    // Longer than read: cut, no panic.
    storage
        .write(&last_path(), "é".repeat(2000).as_bytes())
        .unwrap();
    assert!(last(&storage).chars().all(|c| c == 'é' || c == '\u{fffd}'));
}

/// Text as a name or value may be: any characters but `\r` (which the
/// quoting drops on purpose) and other control characters but line breaks
/// and tabs.
fn text_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => any::<char>().prop_filter("no control", |c| !c.is_control()),
            1 => prop::sample::select(Vec::from([
                ']', '[', '=', '"', '\\', '#', ';', ' ', '\n', '\t'
            ])),
        ],
        0..24,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn id_strategy() -> impl Strategy<Value = Id> {
    "[a-z][a-z0-9-]{0,8}\\.[a-z][a-z0-9-]{0,8}".prop_map(|s| Id::parse(&s).unwrap())
}

fn profiles_strategy() -> impl Strategy<Value = Vec<Profile>> {
    proptest::collection::vec(
        (
            text_strategy(),
            proptest::collection::vec((id_strategy(), text_strategy()), 0..5),
        )
            .prop_map(|(name, values)| Profile { name, values }),
        0..5,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Saved and loaded, profiles are as they were.
    #[test]
    fn profiles_round_trip(profiles in profiles_strategy()) {
        let storage = MemoryStorage::new();
        save(&storage, &profiles);
        prop_assert_eq!(load(&storage), Some(profiles));
    }

    /// Whatever the file holds, reading it does not panic.
    #[test]
    fn any_file_is_read_without_panic(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let storage = MemoryStorage::new().with(&path(), data);
        let _ = load(&storage);
    }

    /// Any text as version 1 is upgraded without panic, and what comes
    /// out is read as version 2 again the same.
    #[test]
    fn any_version_1_text_upgrades(text in "(\\[[^\\]\n]{0,8}\\]\n|[a-z.=# \\[\\]]{0,16}\n){0,8}") {
        let storage = MemoryStorage::new().with(&path(), text.as_str());
        let loaded = load(&storage).unwrap_or_default();
        save(&storage, &loaded);
        prop_assert_eq!(load(&storage), Some(loaded));
    }
}
