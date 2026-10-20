//! The passwords and keys file (`.paperpaw/secrets.txt`) and the Wi-Fi
//! networks kept in it: veiled values come back exactly, labels and ids
//! stay on their line, and hostile files are read without panics.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::wifi;
use proptest::prelude::*;

fn kept(changes: &[(String, String, String)]) -> Secrets {
    let mut secrets = Secrets::default();
    for (id, label, value) in changes {
        secrets.apply(SecretChange::keep(id, label, value));
    }
    secrets
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Any value (any Unicode, tabs, line breaks) comes back exactly, and
    /// none of it is readable in the file.
    #[test]
    fn values_come_back_exactly(
        id in "[a-z]{1,8}\\.[a-z-]{1,8}",
        label in "[A-Z ]{0,20}",
        value in "\\PC{0,40}|[\t\n\r ]{0,4}",
    ) {
        let secrets = kept(&[(id.clone(), label, value.clone())]);
        let text = secrets.encode();
        let read = Secrets::decode(&text);
        prop_assert_eq!(read.get(&id), Some(value.as_str()));
        prop_assert_eq!(&read, &secrets);
        prop_assert_eq!(text.lines().count(), 1);
        // Not readable at a glance in its field (where it could not be
        // hexadecimal).
        let field = text.trim_end().rsplit('\t').next().unwrap_or_default();
        if value.chars().count() >= 4 && value.chars().any(|c| !c.is_ascii_alphanumeric()) {
            prop_assert!(!field.contains(&value));
        }
    }

    /// Labels and ids with tabs, line breaks and backslashes stay on
    /// their line and come back as they are: every secret is read back.
    #[test]
    fn labels_with_tabs_stay_on_their_line_and_come_back(
        entries in proptest::collection::vec(("[a-z]{1,4}[\t\n\r\\\\ a-z]{0,6}", ".{0,12}", "\\PC{0,12}"), 1..6)
    ) {
        let mut secrets = Secrets::default();
        for (id, label, value) in &entries {
            secrets.apply(SecretChange::keep(id, label, value));
        }
        let text = secrets.encode();
        prop_assert_eq!(text.lines().count(), secrets.iter().count());
        let read = Secrets::decode(&text);
        prop_assert_eq!(read.iter().count(), secrets.iter().count());
        for (kept, back) in secrets.iter().zip(read.iter()) {
            prop_assert_eq!(&back.id, &kept.id);
            prop_assert_eq!(&back.label, &kept.label);
            prop_assert_eq!(&back.value, &kept.value);
            prop_assert_eq!(back.changeable, kept.changeable);
        }
    }

    /// Whatever the file holds, decoding it does not panic and gives no
    /// secret without an id.
    #[test]
    fn any_text_decodes_without_panic(text in "[a-z0-9\t\nc.]{0,200}|\\PC{0,100}") {
        let secrets = Secrets::decode(&text);
        prop_assert!(secrets.iter().all(|s| !s.id.is_empty()));
    }

    /// A network's password is found again by its name after the file is
    /// written and read, for names with spaces and any printable text.
    #[test]
    fn wifi_passwords_are_found_by_network(ssid in "[^\t\n\r]{1,32}", password in "\\PC{0,63}") {
        let mut secrets = Secrets::default();
        secrets.apply(wifi::keep(&ssid, &password));
        let read = Secrets::decode(&secrets.encode());
        prop_assert_eq!(wifi::known(&read), vec![(ssid.clone(), password)]);
        let mut read = read;
        read.apply(wifi::forget(&ssid));
        prop_assert!(wifi::known(&read).is_empty());
    }
}

#[test]
fn broken_lines_are_skipped_and_the_rest_read() {
    let good = kept(&[("a.key".into(), "A".into(), "v".into())]).encode();
    let text = [
        // Too few fields.
        "a.one",
        "a.two\tlabel",
        "a.three\tlabel\tc",
        // No id.
        "\tlabel\tc\t00",
        // Not hexadecimal, an odd length, not UTF-8 once unveiled.
        "a.hex\tlabel\tc\tzz",
        "a.odd\tlabel\tc\t123",
        "a.utf\tlabel\tc\tae",
        // A fifth field makes the value no hexadecimal.
        "a.five\tlabel\tc\t00\textra",
        good.trim_end(),
    ]
    .join("\n");
    let read = Secrets::decode(&text);
    assert_eq!(
        read.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["a.key"]
    );
    assert_eq!(read.get("a.key"), Some("v"));
}

#[test]
fn windows_line_ends_and_blank_lines_are_read() {
    let good = kept(&[
        ("a.key".into(), "A".into(), "one".into()),
        ("b.key".into(), "B".into(), "two".into()),
    ])
    .encode();
    let text = format!("\r\n{}\r\n\r\n", good.replace('\n', "\r\n"));
    let read = Secrets::decode(&text);
    assert_eq!(read.get("a.key"), Some("one"));
    assert_eq!(read.get("b.key"), Some("two"));
}

#[test]
fn hexadecimal_in_capitals_is_read() {
    let text = kept(&[("a.key".into(), "A".into(), "Secret!".into())]).encode();
    let upper: String = text
        .split('\t')
        .enumerate()
        .map(|(i, f)| {
            if i == 3 {
                f.to_uppercase()
            } else {
                f.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\t");
    assert_eq!(Secrets::decode(&upper).get("a.key"), Some("Secret!"));
}

#[test]
fn duplicate_ids_give_the_first_and_change_it() {
    let one = kept(&[("a.key".into(), "A".into(), "first".into())]).encode();
    let two = kept(&[("a.key".into(), "A".into(), "second".into())]).encode();
    let mut read = Secrets::decode(&format!("{one}{two}"));
    assert_eq!(read.get("a.key"), Some("first"));
    read.apply(SecretChange::keep("a.key", "", "third"));
    assert_eq!(read.get("a.key"), Some("third"));
    assert_eq!(read.iter().count(), 2);
}

#[test]
fn changes_keep_labels_and_flags_as_asked() {
    let mut secrets = Secrets::default();
    // Removing what is not there changes nothing.
    secrets.apply(SecretChange::remove("a.none"));
    assert!(secrets.is_empty());
    let mut fixed = SecretChange::keep("a.hash", "Hash", "abc");
    fixed.changeable = false;
    secrets.apply(fixed);
    let read = Secrets::decode(&secrets.encode());
    assert!(!read.iter().next().unwrap().changeable);
    // A new label replaces the old one; the flag follows the change.
    secrets.apply(SecretChange::keep("a.hash", "New label", "def"));
    let s = secrets.iter().next().unwrap();
    assert_eq!(
        (s.label.as_str(), s.value.as_str(), s.changeable),
        ("New label", "def", true)
    );
    assert_eq!(FILE, ".paperpaw/secrets.txt");
}

#[test]
fn an_empty_value_is_kept_as_empty() {
    let secrets = kept(&[("a.key".into(), "A".into(), String::new())]);
    assert_eq!(Secrets::decode(&secrets.encode()).get("a.key"), Some(""));
}

#[test]
fn only_wifi_secrets_are_networks() {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("wifi", "no dot", "x"));
    secrets.apply(SecretChange::keep("wifix.net", "other prefix", "x"));
    secrets.apply(wifi::keep("", "empty name"));
    assert_eq!(
        wifi::known(&secrets),
        [(String::new(), String::from("empty name"))]
    );
    assert_eq!(wifi::secret_id("Home"), "wifi.Home");
}

#[test]
fn signal_bars_cover_every_strength() {
    let bars = |signal| {
        wifi::WifiNetwork {
            ssid: String::new(),
            signal,
            secured: true,
        }
        .bars()
    };
    assert_eq!(bars(i8::MAX), 4);
    assert_eq!(bars(-55), 4);
    assert_eq!(bars(-56), 3);
    assert_eq!(bars(-67), 3);
    assert_eq!(bars(-68), 2);
    assert_eq!(bars(-78), 2);
    assert_eq!(bars(-79), 1);
    assert_eq!(bars(i8::MIN), 1);
}

#[test]
fn unknown_join_failures_are_other() {
    assert_eq!(wifi::JoinFailure::parse(""), wifi::JoinFailure::Other);
    assert_eq!(
        wifi::JoinFailure::parse("PASSWORD"),
        wifi::JoinFailure::Other
    );
    let joining = wifi::WifiState::Joining { ssid: "n".into() };
    assert!(joining.is_on() && !joining.is_online());
    assert_eq!(joining.ssid(), Some("n"));
    assert_eq!(wifi::WifiState::On.ssid(), None);
}

/// A network whose name holds a tab (names are any 32 bytes) keeps its
/// id once the file is read again: its password is found by its name.
#[test]
fn a_network_name_with_a_tab_keeps_its_password() {
    let mut secrets = Secrets::default();
    secrets.apply(wifi::keep("Cafe\tGuest", "pw"));
    let read = Secrets::decode(&secrets.encode());
    assert_eq!(
        wifi::known(&read),
        [(String::from("Cafe\tGuest"), String::from("pw"))]
    );
}
