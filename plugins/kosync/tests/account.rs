//! The account screen with the real kernel, by buttons and by touch:
//! signing in keeps only the MD5 of the password, as a secret; creating
//! an account sends valid JSON for any name; the server's refusals are
//! said; another server signs out; nothing goes out without a network or
//! while an answer is awaited.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{KEY, Options, SIGNED_IN, Shared, Sync, body, header};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Id, Params, Storage};

const ACCOUNT: &str = "kosync.account";
const PASSWORD: &str = "Tr0ub4dor&3 pässwort";

fn open(options: Options) -> Sync {
    let mut sync = Sync::on(Shared::default(), options);
    let now = sync.later(100);
    sync.kernel
        .open_screen(now, Id::new_static("kosync.account"), Params::new());
    assert_eq!(sync.screen(), Some(ACCOUNT));
    sync
}

fn signed_out() -> Sync {
    open(Options {
        config: "",
        key: None,
        ..Options::default()
    })
}

/// Types `text` as the answer to the question for `field` (`server`,
/// `user`, `password`).
fn typed(sync: &mut Sync, field: &str, text: &str) {
    let calls = sync.send(
        &format!("kosync.{field}-entered"),
        Params::new().with("text", text),
    );
    assert!(calls.is_empty());
}

/// Every file in storage, to look for what must not be there.
fn everything(storage: &Shared) -> String {
    let mut all = String::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        let mut entries = Vec::new();
        let _ = storage.list(&dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
        for (name, is_dir) in entries {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                dirs.push(path);
            } else {
                all.push_str(&String::from_utf8_lossy(&storage.0.get(&path).unwrap()));
            }
        }
    }
    all
}

#[test]
fn signing_in_keeps_only_the_md5_of_the_password() {
    let key = paperpaw_sdk::digest::md5::of(PASSWORD);
    let mut sync = signed_out();
    typed(&mut sync, "user", "  alice ");
    typed(&mut sync, "password", PASSWORD);
    // The focus moved on to "Sign in".
    let calls = sync.run(actions::CONFIRM);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[0].url, "https://sync.koreader.rocks/users/auth");
    assert_eq!(header(&calls[0], "x-auth-user"), Some("alice"));
    assert_eq!(header(&calls[0], "x-auth-key"), Some(key.as_str()));
    assert!(sync.shown().contains("Connecting"));
    sync.answer(&calls[0], 200, r#"{"authorized": "OK"}"#);
    assert_eq!(sync.secret(), Some(key.clone()));
    assert_eq!(sync.user(), "alice");
    let shown = sync.shown();
    assert!(
        shown.contains("Signed in as alice on sync.koreader.rocks."),
        "{shown}"
    );
    assert!(shown.contains("Sign out"), "{shown}");
    let kept = everything(&sync.storage);
    assert!(!kept.contains(PASSWORD), "{kept}");
    // Kept as a secret, not in the settings.
    assert!(!kept.contains(&key), "{kept}");
}

#[test]
fn refusals_of_the_server_are_said_and_nothing_is_kept() {
    let cases = [
        (401, "User name or password is wrong"),
        (0, "The server cannot be reached."),
        (502, "The server reported an error (502)."),
    ];
    for (status, said) in cases {
        let mut sync = signed_out();
        typed(&mut sync, "user", "alice");
        typed(&mut sync, "password", "wrong");
        let calls = sync.run(actions::CONFIRM);
        sync.answer(&calls[0], status, "");
        assert_eq!(sync.secret(), None, "{status}");
        assert_eq!(sync.user(), "", "{status}");
        let shown = sync.shown();
        assert!(shown.contains(said), "{status}: {shown}");
        assert!(!shown.contains("Sign out"), "{status}: {shown}");
    }
}

#[test]
fn creating_an_account_sends_valid_json_for_any_name() {
    let name = "Zoë \"the\" \\ reader\t✓";
    let mut sync = signed_out();
    typed(&mut sync, "user", name);
    typed(&mut sync, "password", "secret");
    // "Create account" is below "Sign in".
    sync.run(actions::DOWN);
    let calls = sync.run(actions::CONFIRM);
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://sync.koreader.rocks/users/create");
    assert_eq!(header(call, "x-auth-key"), None);
    let sent = paperpaw_sdk::json::object(&body(call));
    assert_eq!(paperpaw_sdk::json::text(&sent, "username"), Some(name));
    assert_eq!(paperpaw_sdk::json::text(&sent, "password"), Some(KEY));

    // The name is taken: said, and tried again with another.
    sync.answer(
        call,
        402,
        r#"{"code": 2002, "message": "Username is already registered."}"#,
    );
    assert!(sync.shown().contains("This user name is taken."));
    assert_eq!(sync.secret(), None);
    typed(&mut sync, "user", "zoe");
    typed(&mut sync, "password", "secret");
    sync.run(actions::DOWN);
    let calls = sync.run(actions::CONFIRM);
    sync.answer(&calls[0], 201, r#"{"username": "zoe"}"#);
    assert_eq!(sync.secret().as_deref(), Some(KEY));
    assert_eq!(sync.user(), "zoe");
}

#[test]
fn a_status_meant_for_the_other_request_is_no_success() {
    // 201 answers creating, 200 signing in; a 402 on signing in is no
    // "taken".
    let mut sync = signed_out();
    typed(&mut sync, "user", "alice");
    typed(&mut sync, "password", "secret");
    let calls = sync.run(actions::CONFIRM);
    sync.answer(&calls[0], 201, "");
    assert_eq!(sync.secret(), None);
    typed(&mut sync, "password", "secret");
    let calls = sync.run(actions::CONFIRM);
    sync.answer(&calls[0], 402, "");
    assert!(!sync.shown().contains("taken"));
    assert_eq!(sync.secret(), None);
}

#[test]
fn nothing_is_asked_without_user_and_password() {
    let mut sync = signed_out();
    // Down to "Sign in".
    for _ in 0..3 {
        sync.run(actions::DOWN);
    }
    assert!(sync.run(actions::CONFIRM).is_empty());
    assert!(
        sync.shown()
            .contains("Enter a user name and a password first.")
    );
    // A name of spaces is none; an empty password is none.
    typed(&mut sync, "user", "   ");
    typed(&mut sync, "password", "secret");
    assert!(sync.run(actions::CONFIRM).is_empty());
    typed(&mut sync, "user", "alice");
    typed(&mut sync, "password", "");
    sync.run(actions::DOWN);
    assert!(sync.run(actions::CONFIRM).is_empty());
    assert!(
        sync.shown()
            .contains("Enter a user name and a password first.")
    );
}

#[test]
fn without_a_network_nothing_goes_out() {
    let mut sync = open(Options {
        config: "",
        key: None,
        network: false,
        ..Options::default()
    });
    assert!(
        sync.shown()
            .contains("This device has no network connection.")
    );
    typed(&mut sync, "user", "alice");
    typed(&mut sync, "password", "secret");
    assert!(sync.run(actions::CONFIRM).is_empty());
    assert!(sync.shown().contains("no network"));
}

#[test]
fn another_server_signs_out_the_same_one_does_not() {
    let mut sync = open(Options::default());
    assert!(sync.shown().contains("Signed in as alice"));
    typed(&mut sync, "server", " https://sync.koreader.rocks/ ");
    assert_eq!(sync.secret().as_deref(), Some(KEY));
    assert_eq!(sync.user(), "alice");

    typed(&mut sync, "server", "nas.local:8081/kosync/");
    assert_eq!(sync.secret(), None);
    assert_eq!(sync.user(), "");
    assert_eq!(
        sync.kernel.settings().text("kosync.server"),
        "https://nas.local:8081/kosync"
    );
    let shown = sync.shown();
    assert!(
        shown.contains("Server changed to nas.local:8081/kosync. Sign in to your account there."),
        "{shown}"
    );
    // Signing in goes there.
    typed(&mut sync, "user", "alice");
    typed(&mut sync, "password", "secret");
    let calls = sync.run(actions::CONFIRM);
    assert_eq!(calls[0].url, "https://nas.local:8081/kosync/users/auth");
}

#[test]
fn by_touch_one_signs_out_and_in_once_while_waiting() {
    let mut sync = open(Options {
        touch: true,
        ..Options::default()
    });
    assert!(sync.tap_on("Sign out").is_empty());
    assert_eq!(sync.secret(), None);
    assert_eq!(sync.user(), "");
    let shown = sync.shown();
    assert!(!shown.contains("Sign out"), "{shown}");
    assert!(
        shown.contains("Sign in with your account"),
        "{shown}"
    );

    typed(&mut sync, "user", "alice");
    typed(&mut sync, "password", "secret");
    let calls = sync.tap_on("Sign in");
    assert_eq!(calls.len(), 1);
    // Tapped again while the server has not answered: nothing more.
    assert!(sync.tap_on("Sign in").is_empty());
    assert!(sync.tap_on("Create account").is_empty());
    sync.answer(&calls[0], 200, "");
    assert_eq!(sync.secret().as_deref(), Some(KEY));
    assert!(sync.shown().contains("Sign out"));
}

#[test]
fn buttons_reach_sign_out_at_the_end_of_the_rows() {
    let mut sync = open(Options {
        config: SIGNED_IN,
        ..Options::default()
    });
    for _ in 0..5 {
        sync.run(actions::DOWN);
    }
    sync.kernel.render(&mut |_, _| {});
    let parts = sync.kernel.describe();
    let focused = parts.iter().find(|p| p.focused).unwrap();
    assert!(focused.label.starts_with("Sign out"), "{parts:?}");
    assert!(sync.run(actions::CONFIRM).is_empty());
    assert_eq!(sync.secret(), None);
    assert_eq!(sync.user(), "");
    // The row is gone; the focus stays on a row there is.
    sync.kernel.render(&mut |_, _| {});
    let parts = sync.kernel.describe();
    assert!(parts.iter().any(|p| p.focused), "{parts:?}");
}

/// The explanation below the rows shows whole where the screen holds it:
/// never cut to "…".
#[test]
fn the_explanation_shows_whole() {
    let mut sync = signed_out();
    let shown = sync.shown();
    assert!(
        shown.contains("Only a hash of the password is kept."),
        "{shown}"
    );
    assert!(!shown.contains('…'), "{shown}");
}

/// Where letters are so large that the explanation has no room, a row
/// shows it whole, to read.
#[test]
fn an_explanation_without_room_is_read_by_its_row() {
    let mut sync = open(Options {
        config: "[core]\nui-size = 24\n",
        key: None,
        ..Options::default()
    });
    // The rows page there: the last one is it.
    for _ in 0..5 {
        sync.run(actions::DOWN);
    }
    let shown = sync.shown();
    assert!(shown.contains("More about this"), "{shown}");
    assert!(!shown.contains('…'), "{shown}");
    sync.run(actions::CONFIRM);
    assert_eq!(sync.screen(), Some("core.confirm-dialog"));
    // All of it, the page buttons scrolling to its end.
    let mut read = sync.shown();
    for _ in 0..10 {
        sync.run(actions::NEXT_PAGE);
        read.push_str(&sync.shown());
    }
    assert!(read.contains("hash"), "{read}");
    assert!(read.contains("kept."), "{read}");
}
