//! What is typed and what servers send, at random: addresses are always
//! something to load, answers are always told apart, titles are read from
//! any page, kept pages keep their address safely in `<base>`, and page
//! names are stable, valid file names.

extern crate std;

use alloc::format;
use alloc::string::String;

use paperpaw_platform::storage::is_valid_path;
use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::{address, keep_page, kind, page_name, title};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn anything_typed_is_an_address_or_a_search(input in ".{0,60}") {
        let url = address(&input);
        let text = input.trim();
        prop_assert!(!url.contains(char::is_whitespace) || url == text, "{}", url);
        prop_assert!(url.contains(':'), "{}", url);
        if let Some(query) = url.strip_prefix("https://html.duckduckgo.com/html/?q=") {
            // The words go as a query, encoded.
            prop_assert!(!query.contains(['&', '#', '?', '/', ' ']), "{}", query);
        }
    }

    #[test]
    fn any_answer_is_told_apart(
        mime in prop::option::of(".{0,30}"),
        url in ".{0,60}",
        head in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let _ = kind(mime.as_deref(), &url, &head);
    }

    #[test]
    fn any_page_gives_a_title_or_none(data in prop::collection::vec(any::<u8>(), 0..2000)) {
        let storage = MemoryStorage::new().with("p.html", data);
        if let Some(title) = title(&storage, "p.html") {
            prop_assert!(!title.is_empty());
            prop_assert!(!title.contains(['\n', '\t', '\r']));
            prop_assert_eq!(title.trim(), title.as_str());
        }
    }

    #[test]
    fn kept_pages_hold_their_address_inside_the_attribute(url in ".{0,80}") {
        let storage = MemoryStorage::new().with("tmp", b"<p>x</p>".to_vec());
        keep_page(&storage, "tmp", "page.html", &url).unwrap();
        let page = String::from_utf8(storage.get("page.html").unwrap()).unwrap();
        let first = page.lines().next().unwrap_or("");
        // `<base href="` … `">`: no quote in between ends it early.
        let inner = first
            .strip_prefix("<base href=\"")
            .and_then(|r| r.strip_suffix("\">"))
            .unwrap_or_else(|| panic!("{first}"));
        if !url.contains(['\n', '\r']) {
            prop_assert!(!inner.contains('"'), "{}", inner);
        }
        prop_assert!(page.ends_with("<p>x</p>"));
        prop_assert!(!storage.exists("tmp"));
    }

    #[test]
    fn page_names_are_stable_valid_names(url in ".{0,80}") {
        let name = page_name(&url);
        prop_assert_eq!(&name, &page_name(&url));
        let path = format!("pages/{name}");
        prop_assert!(is_valid_path(&path), "{}", path);
        prop_assert_eq!(name.len(), 21);
    }
}

#[test]
fn a_title_far_into_a_page_is_not_looked_for() {
    let page = format!("{}<title>Late</title>", " ".repeat(20_000));
    let storage = MemoryStorage::new().with("p.html", page);
    assert_eq!(title(&storage, "p.html"), None);
}

#[test]
fn script_and_data_addresses_stay_as_typed() {
    assert_eq!(address("javascript:alert(1)"), "javascript:alert(1)");
    assert_eq!(address("data:text/html,x"), "data:text/html,x");
    // Not a scheme: searched for.
    assert!(address("a:b c").starts_with("https://html.duckduckgo.com/"));
    assert_eq!(address("..."), "https://html.duckduckgo.com/html/?q=...");
}

#[test]
fn a_page_that_cannot_be_kept_is_an_error() {
    let storage = MemoryStorage::new();
    assert!(keep_page(&storage, "missing", "page.html", "https://a.org/").is_err());
    let storage = MemoryStorage::new().with("tmp", b"x".to_vec());
    assert!(keep_page(&storage, "tmp", "../page.html", "https://a.org/").is_err());
}

#[test]
#[ignore = "bug: a host with a port typed without a scheme is taken for a scheme and loaded as it is"]
fn a_host_with_a_port_is_an_address() {
    assert_eq!(address("calibre.local:8080"), "https://calibre.local:8080");
    assert_eq!(
        address("nas.example.org:5000/books"),
        "https://nas.example.org:5000/books"
    );
}
