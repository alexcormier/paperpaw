//! The web page's requests with hostile paths and names: uploads,
//! folders, deletions and downloads that try to get out of the storage's
//! top or into PaperPaw's own data (`../`, absolute paths, backslashes,
//! drive letters, encoded dots, NUL), names taken already, odd keys and
//! methods; and, property based, that no target whatever makes a panic
//! or touches a hidden path.

extern crate std;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage, read_file};
use proptest::prelude::*;

use super::*;

const KEY: &str = "k3y";
const BODY: &str = ".paperpaw/tmp/request-1";

fn ask(
    storage: &MemoryStorage,
    method: &str,
    target: &str,
    body: Option<&str>,
) -> (u16, Option<Done>) {
    let request = Request {
        number: 1,
        method,
        target,
        body_file: body,
        at: Instant(0),
    };
    let mut guard = Guard::new(KEY.to_string());
    let (response, done) = answer(storage, &mut guard, "<page>", request);
    (response.status, done)
}

/// Uploads `data` to `path` (as the page sends it, encoded or not).
fn upload(storage: &MemoryStorage, path: &str, data: &[u8]) -> (u16, Option<Done>) {
    storage.write(BODY, data).unwrap();
    ask(
        storage,
        "PUT",
        &format!("/api/file?k={KEY}&path={path}"),
        Some(BODY),
    )
}

/// Every file on `storage` with its contents, sorted.
fn files(storage: &MemoryStorage) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut dirs = alloc::vec![String::new()];
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
                found.push((path.clone(), storage.get(&path).unwrap_or_default()));
            }
        }
    }
    found.sort();
    found
}

fn library() -> MemoryStorage {
    MemoryStorage::new()
        .with("Books/a.epub", b"old".to_vec())
        .with(".paperpaw/secrets.txt", b"secret".to_vec())
        .with("paperpaw.ini", b"[core]\n".to_vec())
}

/// Paths that lead out of the top, into hidden folders or are no paths a
/// file system takes.
const HOSTILE: [&str; 22] = [
    "../x.epub",
    "Books/../../x.epub",
    "Books/..",
    "..",
    "%2E%2E/x.epub",
    "%2e%2e%2fx.epub",
    "Books%2F..%2F..%2Fx.epub",
    "..%5Cx.epub",
    "Books\\..\\x.epub",
    "C:\\x.epub",
    "C:/x.epub",
    "Books/x%00.epub",
    "Books/x\u{0}.epub",
    ".paperpaw/secrets.txt",
    "%2Epaperpaw/secrets.txt",
    "Books/.hidden.epub",
    "Books/./a.epub",
    "Books//a.epub",
    "Books/ a.epub",
    "Books/a.epub ",
    "Books/a%0Ab.epub",
    "Books/a\tb.epub",
];

#[test]
fn uploads_out_of_reach_are_refused_and_change_nothing() {
    for path in HOSTILE {
        let storage = library();
        let before = files(&storage);
        let (status, done) = upload(&storage, path, b"evil");
        assert_eq!(status, 400, "{path}");
        // Told on the device as not received.
        assert!(matches!(done, Some(Done::NotReceived(_))), "{path}");
        // Only the body itself is there in addition (the kernel removes
        // it after the answer).
        let mut after = files(&storage);
        after.retain(|(p, _)| p != BODY);
        assert_eq!(after, before, "{path}");
    }
}

#[test]
fn folders_and_deletions_out_of_reach_are_refused() {
    for path in HOSTILE {
        let storage = library();
        let before = files(&storage);
        let mkdir = format!("/api/mkdir?k={KEY}&path={path}");
        assert_eq!(ask(&storage, "POST", &mkdir, None).0, 400, "{path}");
        let delete = format!("/api/delete?k={KEY}&path={path}");
        assert_eq!(ask(&storage, "POST", &delete, None).0, 400, "{path}");
        let get = format!("/api/file?k={KEY}&path={path}");
        assert_eq!(ask(&storage, "GET", &get, None).0, 400, "{path}");
        let list = format!("/api/list?k={KEY}&path={path}");
        assert_eq!(ask(&storage, "GET", &list, None).0, 400, "{path}");
        assert_eq!(files(&storage), before, "{path}");
    }
}

#[test]
fn absolute_paths_are_taken_from_the_top() {
    let storage = library();
    let (status, done) = upload(&storage, "/Books/b.epub", b"new");
    assert_eq!(status, 200);
    assert_eq!(done, Some(Done::Received("b.epub".into())));
    assert_eq!(storage.get("Books/b.epub").unwrap(), b"new");
    // A folder that is not there, as on another system.
    assert_eq!(upload(&storage, "/etc/passwd", b"x").0, 400);
    assert!(!storage.exists("etc/passwd"));
}

#[test]
fn a_name_taken_gets_a_number_and_nothing_is_written_over() {
    let storage = library().with("Books/README", b"r".to_vec());
    for (n, expected) in [(2, "a (2).epub"), (3, "a (3).epub"), (4, "a (4).epub")] {
        let data = format!("upload {n}");
        let (status, done) = upload(&storage, "Books/a.epub", data.as_bytes());
        assert_eq!(status, 200);
        assert_eq!(done, Some(Done::Received(expected.into())));
        let path = format!("Books/{expected}");
        assert_eq!(storage.get(&path).unwrap(), data.as_bytes());
    }
    assert_eq!(storage.get("Books/a.epub").unwrap(), b"old");
    // Without an extension, and named like a folder.
    let (_, done) = upload(&storage, "Books/README", b"2");
    assert_eq!(done, Some(Done::Received("README (2)".into())));
    let (_, done) = upload(&storage, "Books", b"file");
    assert_eq!(done, Some(Done::Received("Books (2)".into())));
    assert!(files::is_dir(&storage, "Books"));
    // A name beginning with its dot stays whole.
    storage.write("Books/x.tar.gz", b"1").unwrap();
    let (_, done) = upload(&storage, "Books/x.tar.gz", b"2");
    assert_eq!(done, Some(Done::Received("x.tar (2).gz".into())));
}

#[test]
fn unicode_and_encoded_names_are_received() {
    let storage = library().with("Bücher/keep", b"".to_vec());
    let (status, done) = upload(&storage, "B%C3%BCcher/%E6%9C%AC+1.epub", b"book");
    assert_eq!(status, 200);
    assert_eq!(done, Some(Done::Received("本 1.epub".into())));
    assert_eq!(storage.get("Bücher/本 1.epub").unwrap(), b"book");
    // Encoded once only: what looks encoded after decoding is a name.
    let (status, _) = upload(&storage, "Books/%252E%252E", b"x");
    assert_eq!(status, 200);
    assert!(storage.exists("Books/%2E%2E"));
}

#[test]
fn a_large_upload_is_moved_whole() {
    let storage = library();
    let data: Vec<u8> = (0..4 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let (status, _) = upload(&storage, "Books/big.pdf", &data);
    assert_eq!(status, 200);
    assert_eq!(read_file(&storage, "Books/big.pdf", 8 << 20).unwrap(), data);
    assert!(!storage.exists(BODY));
    // An empty one, and one without a body at all.
    assert_eq!(upload(&storage, "Books/empty.txt", b"").0, 200);
    assert_eq!(storage.get("Books/empty.txt").unwrap(), b"");
    let target = format!("/api/file?k={KEY}&path=Books/nobody.txt");
    assert_eq!(ask(&storage, "PUT", &target, None).0, 200);
    assert_eq!(storage.get("Books/nobody.txt").unwrap(), b"");
}

#[test]
fn an_upload_without_a_name_or_into_a_file_is_refused() {
    let storage = library();
    for path in ["", "/", "Books/a.epub/inner.epub", "Nowhere/x.epub"] {
        let (status, done) = upload(&storage, path, b"x");
        assert_eq!(status, 400, "{path:?}");
        assert!(matches!(done, Some(Done::NotReceived(_))), "{path:?}");
    }
    assert_eq!(storage.get("Books/a.epub").unwrap(), b"old");
}

#[test]
fn folders_get_numbers_and_deletions_take_folders_whole() {
    let storage = library();
    let mkdir = format!("/api/mkdir?k={KEY}&path=Books");
    let (status, done) = ask(&storage, "POST", &mkdir, None);
    assert_eq!(
        (status, done),
        (200, Some(Done::Folder("Books (2)".into())))
    );
    let delete = format!("/api/delete?k={KEY}&path=Books");
    let (status, done) = ask(&storage, "POST", &delete, None);
    assert_eq!((status, done), (200, Some(Done::Deleted("Books".into()))));
    assert!(!storage.exists("Books/a.epub"));
    // Not the top itself.
    let top = format!("/api/delete?k={KEY}&path=/");
    assert_eq!(ask(&storage, "POST", &top, None).0, 404);
    assert!(storage.exists("paperpaw.ini"));
    assert!(storage.exists(".paperpaw/secrets.txt"));
}

#[test]
fn the_key_must_be_the_first_and_exact() {
    let storage = library();
    for target in [
        "/api/list",
        "/api/list?k=",
        "/api/list?K=k3y",
        "/api/list?k=k3y2",
        "/api/list?k=K3Y",
        "/api/list?k=wrong&k=k3y",
        "/api/list?k=%6B3y%00",
    ] {
        assert_eq!(ask(&storage, "GET", target, None).0, 403, "{target}");
    }
    // Encoded, it is the same key.
    assert_eq!(ask(&storage, "GET", "/api/list?k=%6B3y", None).0, 200);
    // Without the key nothing is written.
    storage.write(BODY, b"x").unwrap();
    assert_eq!(
        ask(&storage, "PUT", "/api/file?path=Books/x.epub", Some(BODY)).0,
        403
    );
    assert!(!storage.exists("Books/x.epub"));
}

#[test]
fn other_methods_and_places_are_not_found() {
    let storage = library();
    for (method, path) in [
        ("DELETE", "/api/file"),
        ("get", "/api/list"),
        ("POST", "/api/file"),
        ("GET", "/api/list/"),
        ("GET", "/api"),
        ("GET", "/../api/list"),
    ] {
        let target = format!("{path}?k={KEY}&path=Books");
        assert_eq!(
            ask(&storage, method, &target, None).0,
            404,
            "{method} {path}"
        );
    }
    assert_eq!(ask(&storage, "GET", "/favicon.ico", None).0, 404);
}

#[test]
fn lists_escape_names_for_json() {
    let storage = MemoryStorage::new().with("Books/a\"b<\\c.epub", b"".to_vec());
    let request = Request {
        number: 1,
        method: "GET",
        target: "/api/list?k=k3y&path=Books",
        body_file: None,
        at: Instant(0),
    };
    let mut guard = Guard::new(KEY.to_string());
    let (response, _) = answer(&storage, &mut guard, "", request);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        r#"{"entries":[{"name":"a\"b\u003c\\c.epub","dir":false,"size":0}]}"#
    );
}

#[test]
fn a_query_with_broken_escapes_is_read_as_it_is() {
    assert_eq!(param("path=%", "path").as_deref(), Some("%"));
    assert_eq!(param("path=%4", "path").as_deref(), Some("%4"));
    assert_eq!(param("path=%%41", "path").as_deref(), Some("%A"));
    // Not UTF-8: replaced, not a panic.
    assert_eq!(param("path=%FF", "path").as_deref(), Some("\u{fffd}"));
    assert_eq!(param("&&path&", "path").as_deref(), Some(""));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// What `files::plain_path` lets through has no way out: no `..`, no hidden part,
    /// no backslash, colon or control character, no empty part; and it
    /// is as safe again.
    #[test]
    fn safe_paths_stay_inside(path in "[a-zA-Z0-9./\\\\:% \t\u{0}-]{0,24}") {
        if let Some(safe_path) = files::plain_path(&path) {
            prop_assert!(!safe_path.starts_with('/') && !safe_path.ends_with('/'));
            for part in safe_path.split('/').filter(|_| !safe_path.is_empty()) {
                prop_assert!(!part.is_empty());
                prop_assert!(!part.starts_with('.'));
                prop_assert!(!part.contains(['\\', ':']));
                prop_assert!(!part.chars().any(char::is_control));
            }
            prop_assert_eq!(files::plain_path(&safe_path), Some(safe_path.clone()));
        }
    }

    /// No target makes a panic, and none touches a hidden path: the
    /// secrets stay, and every file but the body is in reach.
    #[test]
    fn no_target_writes_out_of_reach(
        method in prop::sample::select(alloc::vec!["GET", "PUT", "POST"]),
        place in prop::sample::select(alloc::vec!["/api/file", "/api/mkdir", "/api/delete", "/api/list", "/"]),
        path in "[a-zA-Z./\\\\%0-9]{0,20}",
    ) {
        let storage = library();
        storage.write(BODY, b"body").unwrap();
        let target = format!("{place}?k={KEY}&path={path}");
        let (status, _) = ask(&storage, method, &target, Some(BODY));
        prop_assert!([200, 400, 403, 404, 500].contains(&status), "{}", status);
        prop_assert_eq!(storage.get(".paperpaw/secrets.txt").unwrap(), b"secret".to_vec());
        for (file, _) in files(&storage) {
            if file != BODY && file != ".paperpaw/secrets.txt" {
                prop_assert!(!files::is_hidden(&file), "{}", file);
            }
        }
    }
}
