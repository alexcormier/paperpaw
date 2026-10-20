//! Progress sync with the real kernel against answers as a kosync server
//! gives them: books identified by KOReader's partial MD5 (checked against
//! values of the reference algorithm), positions fetched and offered,
//! declined positions remembered, the progress sent at most once a
//! minute and again after a failure, late and hostile answers, the device
//! id and the key kept by earlier versions.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{DEVICE_ID, DIR, KEY, Options, SIGNED_IN, Shared, Sync, body, header};
use paperpaw_core::HttpCall;
use paperpaw_plugin_kosync::SYNC_NOW;
use paperpaw_sdk::Storage;
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::document::Locator;
use proptest::prelude::*;

const BOOK: &str = "books/alice.txt";
const OTHER: &str = "books/other.txt";
const CONFIRM: &str = "kosync.confirm";

/// Bytes `0..n` of a generated file, the same as the reference script
/// that computed the digests below made.
fn generated(n: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 31 + 7) ^ (i >> 8)) as u8).collect()
}

/// A plain text book of a few kilobytes.
fn text() -> Vec<u8> {
    let mut text = String::new();
    for i in 0..400 {
        text.push_str(&format!(
            "Paragraph {i}: Alice was beginning to get very tired.\n\n"
        ));
    }
    text.into_bytes()
}

fn with_books() -> Sync {
    let sync = Sync::with_id();
    sync.storage.write(BOOK, &text()).unwrap();
    sync.storage.write(OTHER, &text()[..5000]).unwrap();
    sync
}

/// The one fetch among `calls`.
fn fetch(calls: &[HttpCall]) -> &HttpCall {
    let fetches: Vec<&HttpCall> = calls.iter().filter(|c| c.method == "GET").collect();
    assert_eq!(fetches.len(), 1, "{calls:?}");
    fetches[0]
}

/// The one sending among `calls`.
fn sending(calls: &[HttpCall]) -> &HttpCall {
    let sent: Vec<&HttpCall> = calls.iter().filter(|c| c.method == "PUT").collect();
    assert_eq!(sent.len(), 1, "{calls:?}");
    sent[0]
}

/// Opens `path` at `percent` and answers its fetch: the server has no
/// progress of the book. Until that answer nothing is sent for it.
fn opened(sync: &mut Sync, path: &str, percent: f32) {
    let calls = sync.open(path, percent);
    assert!(sync.answer(fetch(&calls), 200, "{}").is_empty());
}

fn remote(percentage: &str, device_id: &str) -> String {
    format!(
        r#"{{"document": "d", "percentage": {percentage}, "progress": "",
            "device": "Kobo", "device_id": "{device_id}", "timestamp": 1792483200}}"#
    )
}

#[test]
fn books_are_identified_by_koreaders_partial_md5() {
    // Computed with KOReader's algorithm (1 KiB at 0, then at 1 KiB << 2i
    // for i up to 10, as far as the file reaches) in Python's hashlib.
    let known = [
        (0, "d41d8cd98f00b204e9800998ecf8427e"),
        (1, "89e74e640b8c46257a29de0616794d5d"),
        (1024, "a2392fcde7bda8bff4929201c9e1089e"),
        (1025, "9eda8f91bb6c725990b5c4f6cb865564"),
        (4096, "e700908199db815e1f67e49eb619bf97"),
        (5000, "f90b9f9d8cb127e70c9163b13bfa0609"),
        (70_000, "f31883ee7b2319d7630d2fbcf881661a"),
        (300_000, "fbd6bb705067ef485bde431ee3e6c374"),
    ];
    let mut sync = Sync::with_id();
    for (size, digest) in known {
        let path = format!("books/{size}.pdf");
        sync.storage.write(&path, &generated(size)).unwrap();
        let calls = sync.open(&path, 1.0);
        let call = fetch(&calls);
        assert_eq!(
            call.url,
            format!("https://sync.koreader.rocks/syncs/progress/{digest}"),
            "{size} bytes"
        );
        assert_eq!(
            paperpaw_digest::md5::partial(&sync.storage, &path).as_deref(),
            Some(digest),
            "{size} bytes"
        );
    }
}

#[test]
fn a_book_that_cannot_be_read_is_not_synced() {
    let mut sync = Sync::with_id();
    assert!(sync.open("books/gone.epub", 10.0).is_empty());
    assert!(sync.turn("books/gone.epub", 12.0).is_empty());
    assert!(sync.run(SYNC_NOW).is_empty());
}

#[test]
fn fetches_carry_the_account_and_go_to_the_server_set() {
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    let call = fetch(&calls);
    assert_eq!(header(call, "x-auth-user"), Some("alice"));
    assert_eq!(header(call, "x-auth-key"), Some(KEY));
    assert_eq!(
        header(call, "Accept"),
        Some("application/vnd.koreader.v1+json")
    );

    // An own server below a path, over plain http, entered with a `/`.
    let storage = Shared::default();
    storage.write(BOOK, &text()).unwrap();
    let mut sync = Sync::on(
        storage,
        Options {
            config: "[kosync]\nuser = alice\nserver = http://nas:8081/kosync/\n",
            ..Options::default()
        },
    );
    let calls = sync.open(BOOK, 10.0);
    assert!(
        fetch(&calls)
            .url
            .starts_with("http://nas:8081/kosync/syncs/progress/"),
        "{calls:?}"
    );
}

#[test]
fn nothing_is_sent_signed_out_or_switched_off() {
    let cases = [
        ("", Some(KEY)),
        (SIGNED_IN, None),
        (SIGNED_IN, Some("")),
        ("[kosync]\nuser = alice\nauto = off\n", Some(KEY)),
    ];
    for (config, key) in cases {
        let storage = Shared::default();
        storage.write(BOOK, &text()).unwrap();
        let mut sync = Sync::on(
            storage,
            Options {
                config,
                key,
                ..Options::default()
            },
        );
        assert!(sync.open(BOOK, 10.0).is_empty(), "{config} {key:?}");
        sync.later(120_000);
        assert!(sync.turn(BOOK, 30.0).is_empty(), "{config} {key:?}");
        assert!(
            sync.reading(events::BOOK_CLOSED, BOOK, 30.0).is_empty(),
            "{config} {key:?}"
        );
        let now = sync.later(100);
        sync.kernel.suspend(now);
        assert!(sync.calls().is_empty(), "{config} {key:?}");
    }
}

#[test]
fn signed_out_the_sync_action_opens_the_account() {
    let storage = Shared::default();
    let mut sync = Sync::on(
        storage,
        Options {
            config: "",
            ..Options::default()
        },
    );
    sync.open(BOOK, 10.0);
    assert!(sync.run(SYNC_NOW).is_empty());
    assert_eq!(sync.screen(), Some("kosync.account"));
}

#[test]
fn a_position_further_elsewhere_is_offered_and_taken() {
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    sync.answer(fetch(&calls), 200, &remote("0.5", "KOBO"));
    assert_eq!(sync.screen(), Some(CONFIRM));
    let shown = sync.shown();
    assert!(
        shown.contains("Kobo is further: 50 %. Go there?"),
        "{shown}"
    );
    sync.run(actions::CONFIRM);
    assert_ne!(sync.screen(), Some(CONFIRM));
    let gone = sync.gone.borrow().clone();
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].path, BOOK);
    // Half way through the book, as the format counts it.
    let document = sync.kernel.formats().open(&sync.storage, BOOK).unwrap();
    let at = Locator::new(gone[0].section, gone[0].offset);
    let progress = document.progress(at);
    assert!((progress - 0.5).abs() < 0.01, "{progress}");
}

/// A progress out of 0 to 1 is no position at all: nothing is offered,
/// and nothing sent over it. The end of the book still goes inside it.
#[test]
fn progress_out_of_range_is_not_offered_and_the_end_goes_inside_the_book() {
    for (percentage, offered) in [("7", false), ("1e300", false), ("-3", false), ("1", true)] {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        let after = sync.answer(fetch(&calls), 200, &remote(percentage, "KOBO"));
        assert_eq!(sync.screen() == Some(CONFIRM), offered, "{percentage}");
        if !offered {
            assert!(after.is_empty(), "{percentage}: {after:?}");
            continue;
        }
        sync.run(actions::CONFIRM);
        let gone = sync.gone.borrow().clone();
        assert_eq!(gone.len(), 1, "{percentage}");
        let document = sync.kernel.formats().open(&sync.storage, BOOK).unwrap();
        assert!(gone[0].section < document.sections(), "{percentage}");
        assert!(
            gone[0].offset <= document.section_len(gone[0].section),
            "{percentage}"
        );
    }
}

#[test]
fn staying_is_remembered_for_that_position_only() {
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    sync.answer(fetch(&calls), 200, &remote("0.5", "KOBO"));
    // "Stay here" is the second button.
    sync.run(actions::RIGHT);
    sync.run(actions::CONFIRM);
    assert_ne!(sync.screen(), Some(CONFIRM));
    assert!(sync.gone.borrow().is_empty());

    // Back from sleep: fetched again, the same position not offered.
    let now = sync.later(100);
    sync.kernel.resume(now);
    let calls = sync.calls();
    sync.answer(fetch(&calls), 200, &remote("0.5", "KOBO"));
    assert_ne!(sync.screen(), Some(CONFIRM));

    // Asked for, it is.
    let calls = sync.run(SYNC_NOW);
    sync.answer(fetch(&calls), 200, &remote("0.5", "KOBO"));
    assert_eq!(sync.screen(), Some(CONFIRM));
    sync.run(actions::BACK);

    // Read on there meanwhile: offered by itself again.
    let now = sync.later(100);
    sync.kernel.resume(now);
    let calls = sync.calls();
    sync.answer(fetch(&calls), 200, &remote("0.6", "KOBO"));
    assert_eq!(sync.screen(), Some(CONFIRM));
}

#[test]
fn positions_of_this_device_or_not_further_are_not_offered() {
    let cases = [
        // Sent from here (another book file with the same text, say).
        remote("0.9", DEVICE_ID),
        // Behind, or within the margin.
        remote("0.05", "KOBO"),
        remote("0.1015", "KOBO"),
        // No progress for the book yet.
        String::from("{}"),
    ];
    for answer in cases {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        assert!(sync.answer(fetch(&calls), 200, &answer).is_empty());
        assert_ne!(sync.screen(), Some(CONFIRM), "{answer}");
        assert!(sync.gone.borrow().is_empty());
    }
}

#[test]
fn hostile_answers_open_nothing_and_break_nothing() {
    let deep = "[".repeat(100_000);
    let long_name = format!(
        r#"{{"percentage": 0.9, "device": "{}", "device_id": "X"}}"#,
        "Ω".repeat(20_000)
    );
    let bodies: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"not json".to_vec(),
        b"{".to_vec(),
        b"[0.9]".to_vec(),
        br#"{"percentage": "0.9 %", "device_id": "X"}"#.to_vec(),
        br#"{"percentage": null}"#.to_vec(),
        br#"{"percentage": true}"#.to_vec(),
        br#"{"percentage": {"value": 0.9}}"#.to_vec(),
        br#"{"percentage": 0.9"#.to_vec(),
        deep.into_bytes(),
        vec![0xff, 0xfe, b'{', 0x80],
    ];
    for answer in &bodies {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        sync.answer_bytes(fetch(&calls), 200, answer);
        assert_ne!(
            sync.screen(),
            Some(CONFIRM),
            "{}",
            String::from_utf8_lossy(&answer[..answer.len().min(40)])
        );
    }
    // A progress given as text is read as the number it is (some servers
    // answer so).
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    sync.answer(
        fetch(&calls),
        200,
        r#"{"percentage": "0.9", "device_id": "X"}"#,
    );
    assert_eq!(sync.screen(), Some(CONFIRM));
    // A long name in another script is shown, not cut into a panic.
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    sync.answer(fetch(&calls), 200, &long_name);
    assert_eq!(sync.screen(), Some(CONFIRM));
    assert!(sync.shown().contains("ΩΩΩ"));
}

#[test]
fn an_answer_for_a_book_left_meanwhile_is_ignored() {
    let mut sync = with_books();
    let first = sync.open(BOOK, 10.0);
    let first = fetch(&first).clone();
    // Another book before the first one's fetch is answered: its fetch.
    let calls = sync.open(OTHER, 50.0);
    let second = fetch(&calls).clone();
    sync.answer(&first, 200, &remote("0.9", "KOBO"));
    assert_ne!(sync.screen(), Some(CONFIRM));
    // The same answer twice: offered once.
    sync.answer(&second, 200, &remote("0.9", "KOBO"));
    sync.answer(&second, 200, &remote("0.9", "KOBO"));
    let offers = sync
        .kernel
        .screens()
        .filter(|s| s.as_str() == CONFIRM)
        .count();
    assert_eq!(offers, 1);
}

/// Pages turned while a book's fetch is out are held back until the
/// answer; another book opened meanwhile starts a fetch of its own, which
/// drops that hold, and the first book's answer is then ignored: its
/// position is never sent (until it is read again here).
#[test]
#[ignore = "bug: a position held back for a fetch is lost when another book is opened before the answer"]
fn a_position_held_for_a_fetch_still_goes_after_another_book_is_opened() {
    let mut sync = with_books();
    let digest = paperpaw_digest::md5::partial(&sync.storage, BOOK).unwrap();
    let calls = sync.open(BOOK, 10.0);
    let first = fetch(&calls).clone();
    assert!(sync.turn(BOOK, 30.0).is_empty(), "held until the answer");
    let mut calls = sync.open(OTHER, 50.0);
    let second = fetch(&calls).clone();
    // Neither server is further.
    calls.extend(sync.answer(&first, 200, "{}"));
    calls.extend(sync.answer(&second, 200, "{}"));
    let now = sync.later(100);
    sync.kernel.suspend(now);
    calls.extend(sync.calls());
    let sent: Vec<String> = calls
        .iter()
        .filter(|c| c.method == "PUT")
        .filter_map(|c| {
            let sent = paperpaw_json::object(&body(c));
            paperpaw_json::text(&sent, "document").map(str::to_string)
        })
        .collect();
    assert!(sent.contains(&digest), "{calls:?}");
}

#[test]
fn the_same_book_is_fetched_again_only_after_a_minute() {
    let mut sync = with_books();
    let calls = sync.open(BOOK, 10.0);
    fetch(&calls);
    // The reader shown again after a menu.
    assert!(sync.open(BOOK, 10.0).is_empty());
    sync.later(60_000);
    let calls = sync.open(BOOK, 10.0);
    fetch(&calls);
}

#[test]
fn failures_of_a_sync_asked_for_are_said() {
    let cases = [
        (0, "The server cannot be reached."),
        (401, "User name or password is wrong"),
        (403, "User name or password is wrong"),
        (500, "The server reported an error (500)."),
        (200, "The progress is in sync."),
    ];
    for (status, said) in cases {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        // Failures of fetches by itself say nothing.
        sync.answer(fetch(&calls), status, "{}");
        assert_ne!(sync.screen(), Some(CONFIRM), "{status}");
        // Asked for, the server's progress is fetched first, and this
        // device's sent only once the server answered it is not further.
        let calls = sync.run(SYNC_NOW);
        assert_eq!(calls.len(), 1, "{calls:?}");
        let after = sync.answer(fetch(&calls), status, "{}");
        assert_eq!(sync.screen(), Some(CONFIRM), "{status}");
        let shown = sync.shown();
        assert!(shown.contains(said), "{status}: {shown}");
        if status == 200 {
            sending(&after);
        } else {
            assert!(after.is_empty(), "{status}: {after:?}");
        }
    }
}

#[test]
fn the_progress_goes_at_most_once_a_minute_and_again_after_a_failure() {
    let mut sync = Sync::with_id();
    sync.storage.write("books/b.pdf", &generated(3000)).unwrap();
    let digest = paperpaw_digest::md5::partial(&sync.storage, "books/b.pdf").unwrap();
    opened(&mut sync, "books/b.pdf", 10.0);
    let calls = sync.turn("books/b.pdf", 25.0);
    let call = sending(&calls).clone();
    assert_eq!(call.url, "https://sync.koreader.rocks/syncs/progress");
    assert_eq!(header(&call, "x-auth-key"), Some(KEY));
    let sent = paperpaw_json::object(&body(&call));
    assert_eq!(
        paperpaw_json::text(&sent, "document"),
        Some(digest.as_str())
    );
    assert_eq!(paperpaw_json::number(&sent, "percentage"), Some(0.25));
    assert_eq!(paperpaw_json::text(&sent, "progress"), Some(""));
    assert_eq!(
        paperpaw_json::text(&sent, "device"),
        Some("PaperPaw (Test device)")
    );
    assert_eq!(paperpaw_json::text(&sent, "device_id"), Some(DEVICE_ID));
    sync.answer(&call, 200, "{}");

    // Within the minute: kept back.
    assert!(sync.turn("books/b.pdf", 26.0).is_empty());
    sync.later(60_000);
    let calls = sync.turn("books/b.pdf", 27.0);
    let call = sending(&calls).clone();
    // Not taken: sent with the next page, not a minute later.
    sync.answer(&call, 503, "busy");
    let calls = sync.turn("books/b.pdf", 28.0);
    let call = sending(&calls).clone();
    sync.answer(&call, 200, "{}");
    assert!(sync.turn("books/b.pdf", 29.0).is_empty());
}

#[test]
fn closing_and_sleeping_send_what_was_not_sent() {
    let mut sync = Sync::with_id();
    sync.storage.write("books/b.pdf", &generated(3000)).unwrap();
    opened(&mut sync, "books/b.pdf", 10.0);
    let calls = sync.turn("books/b.pdf", 20.0);
    sync.answer(sending(&calls), 200, "{}");
    // A page within the minute is kept back, then sent on sleep.
    assert!(sync.turn("books/b.pdf", 21.0).is_empty());
    let now = sync.later(100);
    sync.kernel.suspend(now);
    let calls = sync.calls();
    let sent = paperpaw_json::object(&body(sending(&calls)));
    assert_eq!(paperpaw_json::number(&sent, "percentage"), Some(0.21));
    // Nothing new: closing sends nothing more.
    assert!(
        sync.reading(events::BOOK_CLOSED, "books/b.pdf", 21.0)
            .is_empty()
    );
}

#[test]
fn the_device_id_is_made_once_and_kept() {
    let storage = Shared::default();
    storage.write("books/b.pdf", &generated(3000)).unwrap();
    let mut sync = Sync::on(storage.clone(), Options::default());
    opened(&mut sync, "books/b.pdf", 10.0);
    let calls = sync.turn("books/b.pdf", 20.0);
    let sent = paperpaw_json::object(&body(sending(&calls)));
    let id = paperpaw_json::text(&sent, "device_id").unwrap().to_string();
    assert_eq!(id.len(), 32);
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
    );
    assert_eq!(
        storage.text(&format!("{DIR}/device.ini")).unwrap(),
        format!("[device]\nid = {id}\n")
    );

    // Started again: the same id.
    let mut sync = Sync::on(storage, Options::default());
    opened(&mut sync, "books/b.pdf", 10.0);
    let calls = sync.turn("books/b.pdf", 30.0);
    let sent = paperpaw_json::object(&body(sending(&calls)));
    assert_eq!(paperpaw_json::text(&sent, "device_id"), Some(id.as_str()));
}

#[test]
fn a_key_kept_by_earlier_versions_moves_to_the_secrets() {
    let old = include_str!("../testdata/account.ini.v1");
    let storage = Shared::default();
    storage
        .write(&format!("{DIR}/account.ini"), old.as_bytes())
        .unwrap();
    let sync = Sync::on(
        storage.clone(),
        Options {
            key: None,
            ..Options::default()
        },
    );
    assert_eq!(
        sync.secret().as_deref(),
        Some("5f4dcc3b5aa765d61d8327deb882cf99")
    );
    let kept = sync.kernel.secrets().iter().find(|s| s.id == "kosync.key");
    // A hash: not to be typed anew in the list.
    assert!(!kept.unwrap().changeable);
    assert!(!storage.exists(&format!("{DIR}/account.ini")));

    // A key kept already wins over the old file, which goes all the same.
    let storage = Shared::default();
    storage
        .write(&format!("{DIR}/account.ini"), old.as_bytes())
        .unwrap();
    let sync = Sync::on(storage.clone(), Options::default());
    assert_eq!(sync.secret().as_deref(), Some(KEY));
    assert!(!storage.exists(&format!("{DIR}/account.ini")));
}

#[test]
fn an_unreadable_old_account_file_gives_no_key() {
    let storage = Shared::default();
    let file = format!("{DIR}/account.ini");
    storage.write(&file, &[0xff, 0x00, 0x80]).unwrap();
    let sync = Sync::on(
        storage.clone(),
        Options {
            key: None,
            ..Options::default()
        },
    );
    assert_eq!(sync.secret(), None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Whatever the server answers, nothing panics, and a position is
    /// offered exactly when another device is further than the margin
    /// (a progress out of 0 to 1 is none).
    #[test]
    fn answers_offer_exactly_positions_further_elsewhere(
        percentage in prop_oneof![-2.0f64..3.0, any::<f64>()],
        here in any::<bool>(),
        device in r"\PC{0,40}",
    ) {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        let id = if here { DEVICE_ID } else { "KOBO" };
        let answer = format!(
            r#"{{"percentage": {percentage:?}, "device": {}, "device_id": "{id}"}}"#,
            paperpaw_json::string(&device)
        );
        sync.answer(fetch(&calls), 200, &answer);
        let local = f64::from(10.0f32) / 100.0;
        let further = percentage <= 1.0 && percentage > local + 0.002;
        prop_assert_eq!(sync.screen() == Some(CONFIRM), further && !here, "{}", answer);
    }

    /// Arbitrary bytes as an answer never open anything.
    #[test]
    fn arbitrary_bytes_never_offer_anything(answer in proptest::collection::vec(any::<u8>(), 0..300)) {
        let mut sync = with_books();
        let calls = sync.open(BOOK, 10.0);
        sync.answer_bytes(fetch(&calls), 200, &answer);
        let text = String::from_utf8_lossy(&answer);
        if !text.contains("percentage") {
            prop_assert_ne!(sync.screen(), Some(CONFIRM));
        }
    }
}
