//! A small world without a network: answers to the requests of
//! PaperPaw's own plugins (the plugin library and the other servers they
//! ask), from the files in `fixtures/network`, so that the screens that
//! show what a server sent can be drawn offline and alike on every
//! computer (`paperpaw-emu --network fixtures`, the snapshot tests and
//! `paperpaw-dev`). A request it has no answer for fails as refused and
//! is kept ([`unanswered`]), so a tool can tell a screen that waited for
//! data from one with no way in.

use std::cell::RefCell;

use paperpaw_core::sdk::HttpError;
use paperpaw_core::sdk::native::FINGERPRINT;
use paperpaw_core::{HttpCall, HttpResponse};

use crate::http::Route;

/// An answer: the address it answers (ending in `*`, every one that
/// starts so; the first that fits is taken), its content type and body.
struct Answer {
    url: &'static str,
    content_type: &'static str,
    body: &'static [u8],
}

const JSON: &str = "application/json";
const PNG: &str = "image/png";
const TEXT: &str = "text/plain";

macro_rules! answer {
    ($url:expr, $type:expr, $file:literal) => {
        Answer {
            url: $url,
            content_type: $type,
            body: include_bytes!(concat!("../fixtures/network/", $file)),
        }
    };
}

/// The plugin library's address (the store's `OFFICIAL_LIST` without its
/// file).
#[cfg(test)]
const LIBRARY: &str = "https://paperpaw.org/api/v1/plugins";

const ANSWERS: &[Answer] = &[
    // The plugin library: the list, a page a plugin, their files and a
    // plugin's screenshots. A data plugin with a long summary and
    // description, screenshots and a source; a code plugin (its builds
    // for this emulator's code, see `ABI`); a short one, a long name.
    answer!(
        "https://paperpaw.org/api/v1/plugins.json?*",
        JSON,
        "store/plugins.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/reading-aids.json?*",
        JSON,
        "store/reading-aids.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/reading-aids/1.2.0/plugin.ini",
        TEXT,
        "store/reading-aids.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/shots/reading-aids-1.png",
        PNG,
        "store/reading-aids-1.png"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/shots/reading-aids-2.png",
        PNG,
        "store/reading-aids-2.png"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/word-clock.json?*",
        JSON,
        "store/word-clock.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/word-clock/1.0.0/plugin.ini",
        TEXT,
        "store/word-clock.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/airy-theme.json?*",
        JSON,
        "store/airy-theme.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/airy-theme/1.0.0/plugin.ini",
        TEXT,
        "store/airy-theme.ini"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/long-notes.json?*",
        JSON,
        "store/long-notes.json"
    ),
    answer!(
        "https://paperpaw.org/api/v1/plugins/long-notes/0.3.1/plugin.ini",
        TEXT,
        "store/long-notes.ini"
    ),
];

/// Where a body says which build of PaperPaw's code a code plugin was
/// made for: this emulator's, so that it is listed.
const ABI: &str = "{abi}";

thread_local! {
    /// The requests no answer fitted, the thread's (a device runs on one).
    static UNANSWERED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The addresses asked for that had no answer since the last call, and
/// forgets them.
pub fn unanswered() -> Vec<String> {
    UNANSWERED.with_borrow_mut(std::mem::take)
}

/// The answer to a request to `url`, if the world has one.
fn find(url: &str) -> Option<&'static Answer> {
    ANSWERS.iter().find(|a| match a.url.strip_suffix('*') {
        Some(start) => url.starts_with(start),
        None => url == a.url,
    })
}

/// The network as the world answers it (see [`crate::http::set_route`]).
#[derive(Debug, Default)]
pub struct Offline;

impl Route for Offline {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        let Some(answer) = find(&call.url).filter(|_| call.method == "GET") else {
            UNANSWERED.with_borrow_mut(|kept| kept.push(call.url.clone()));
            let reason = "no answer in the emulator's fixtures";
            return Some(HttpResponse::failed(call.id, HttpError::Refused, reason));
        };
        let mut body = answer.body.to_vec();
        if answer.content_type == JSON {
            let text = String::from_utf8_lossy(&body);
            if text.contains(ABI) {
                body = text.replace(ABI, &FINGERPRINT.to_string()).into_bytes();
            }
        }
        if body.len() > call.max_response {
            let reason = format!("the answer is larger than {} bytes", call.max_response);
            return Some(HttpResponse::failed(call.id, HttpError::Refused, &reason));
        }
        Some(HttpResponse {
            id: call.id,
            status: 200,
            body,
            content_type: Some(answer.content_type.to_string()),
            ..HttpResponse::default()
        })
    }

    fn later(&mut self) -> Option<HttpResponse> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> HttpCall {
        HttpCall {
            id: 1,
            method: String::from("GET"),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: usize::MAX,
            timeout_ms: 1000,
            want_headers: Vec::new(),
            save_to: None,
        }
    }

    #[test]
    fn every_page_of_the_list_and_its_files_are_answered() {
        let list = String::from_utf8_lossy(ANSWERS[0].body);
        let ids: Vec<&str> = list
            .split("\"id\": \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect();
        assert!(ids.len() >= 4, "{ids:?}");
        for id in ids {
            let page = find(&format!("{LIBRARY}/{id}.json?device=x4&lang=en"))
                .unwrap_or_else(|| panic!("no page for {id}"));
            let text = String::from_utf8_lossy(page.body);
            for url in text.split("\"url\": \"").skip(1) {
                let url = url.split('"').next().unwrap_or_default();
                if url.starts_with("http") || url.ends_with(".ppn") {
                    continue;
                }
                assert!(find(&format!("{LIBRARY}/{url}")).is_some(), "{url}");
            }
        }
    }

    #[test]
    fn a_code_plugin_is_made_for_this_build() {
        let answer = Offline.start(get(&format!("{LIBRARY}.json?device=x4&lang=en")));
        let answer = answer.unwrap();
        let body = String::from_utf8(answer.body).unwrap();
        assert!(!body.contains(ABI));
        assert!(body.contains(&FINGERPRINT.to_string()));
    }

    #[test]
    fn what_has_no_answer_fails_and_is_kept() {
        let _ = unanswered();
        let answer = Offline.start(get("https://example.org/feed")).unwrap();
        assert_eq!(answer.status, 0);
        assert_eq!(unanswered(), ["https://example.org/feed"]);
        assert!(unanswered().is_empty());
    }
}
