//! The browser's emulator's network: plugins' requests answered from the
//! page's recorded answers (fixtures), offline and the same each time, and
//! each request told to the page; where the page allows it (a local
//! setup), the requests no fixture answers fetched by the page itself.
//! Nothing goes through a server of PaperPaw's.
//!
//! - A **fixture** answers requests whose method (any, where none is
//!   named) and address match: the address a pattern where `*` stands for
//!   any text (`https://api.example.com/v1/*`). The first that matches
//!   answers, with its status, headers and body, after its delay (the
//!   device's time).
//! - **Unmatched**, a request fails as without a network (status 0), or,
//!   with real fetches allowed ([`Network::set_real`]), waits in the
//!   outbox for the page to fetch it and hand its answer back.
//! - **Every request** is noted for the page ([`Network::take_seen`]):
//!   what was asked, and how it was answered.

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_dev::report::json_string;
use paperpaw_emulator::http::{self, Route};

/// A recorded answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fixture {
    /// The method it answers (`GET` …); `None`: any.
    pub method: Option<String>,
    /// The addresses it answers: `*` stands for any text.
    pub url: String,
    pub status: u16,
    /// Its headers (`content-type`, `etag` …), names in any case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// How long it takes, in milliseconds of the device's time.
    pub delay_ms: u64,
}

impl Fixture {
    fn answers(&self, call: &HttpCall) -> bool {
        self.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(&call.method))
            && matches(&self.url, &call.url)
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any text.
pub fn matches(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        // No `*`: the whole text.
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// How a request was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// By a fixture, with its status.
    Fixture(u16),
    /// By none: failed as without a network.
    Unmatched,
    /// Given to the page to fetch.
    Fetch,
}

/// A request noted for the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub id: u32,
    pub method: String,
    pub url: String,
    pub source: Source,
}

/// The requests and their answers.
#[derive(Debug, Default)]
pub struct Network {
    fixtures: Vec<Fixture>,
    real: bool,
    seen: Vec<Seen>,
    /// Requests for the page to fetch, not handed over yet.
    outbox: Vec<HttpCall>,
    /// Requests the page fetches: what their answers must keep to.
    fetching: Vec<HttpCall>,
    /// Fixtures' answers and when they are due (the device's ms).
    delayed: Vec<(u64, HttpResponse)>,
    /// The device's time, as last told.
    now_ms: u64,
}

impl Network {
    /// Shared: what the device's route and the emulator both hold.
    pub fn shared() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self::default()))
    }

    pub fn set_fixtures(&mut self, fixtures: Vec<Fixture>) {
        self.fixtures = fixtures;
    }

    /// Adds a recorded answer after those there are.
    pub fn add_fixture(&mut self, fixture: Fixture) {
        self.fixtures.push(fixture);
    }

    /// Lets the page fetch what no fixture answers (`true`), or not.
    pub fn set_real(&mut self, real: bool) {
        self.real = real;
    }

    /// The requests noted since asked last.
    pub fn take_seen(&mut self) -> Vec<Seen> {
        std::mem::take(&mut self.seen)
    }

    /// The requests for the page to fetch since asked last.
    pub fn take_outbox(&mut self) -> Vec<HttpCall> {
        let calls = std::mem::take(&mut self.outbox);
        self.fetching.extend(calls.iter().cloned());
        calls
    }

    /// The device's time is `now_ms`: the answers due by then, given to
    /// the device.
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed)
            .into_iter()
            .partition(|(at, _)| *at <= now_ms);
        self.delayed = later;
        for (_, response) in due {
            http::answer_later(response);
        }
    }

    /// The page's answer to request `id` it fetched: its status (0:
    /// failed, the reason in `body`), headers (`name: value` lines) and
    /// body. `false` if no such request waits.
    pub fn fetched(&mut self, id: u32, status: u16, headers: &str, body: Vec<u8>) -> bool {
        let Some(at) = self.fetching.iter().position(|c| c.id == id) else {
            return false;
        };
        let call = self.fetching.remove(at);
        let headers: Vec<(String, String)> = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
            .collect();
        http::answer_later(response(&call, status, &headers, body));
        true
    }

    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        let mut seen = Seen {
            id: call.id,
            method: call.method.clone(),
            url: call.url.clone(),
            source: Source::Unmatched,
        };
        let answer = if let Some(fixture) = self.fixtures.iter().find(|f| f.answers(&call)) {
            seen.source = Source::Fixture(fixture.status);
            let answer = response(
                &call,
                fixture.status,
                &fixture.headers,
                fixture.body.clone(),
            );
            if fixture.delay_ms > 0 {
                self.delayed.push((self.now_ms + fixture.delay_ms, answer));
                None
            } else {
                Some(answer)
            }
        } else if self.real {
            seen.source = Source::Fetch;
            self.outbox.push(call);
            None
        } else {
            let reason = format!(
                "no recorded answer for {} {} (no network in the browser's emulator)",
                call.method, call.url
            );
            Some(HttpResponse::failed(call.id, &reason))
        };
        self.seen.push(seen);
        answer
    }
}

/// The answer to `call`, as a shell gives it: the headers it asked for,
/// the body cut to the most it takes (failed for a file instead, which is
/// never saved cut short).
fn response(
    call: &HttpCall,
    status: u16,
    headers: &[(String, String)],
    mut body: Vec<u8>,
) -> HttpResponse {
    if body.len() > call.max_response {
        if call.save_to.is_some() {
            return HttpResponse::failed(call.id, "the answer is larger than the request takes");
        }
        body.truncate(call.max_response);
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    HttpResponse {
        id: call.id,
        status,
        body,
        etag: header("etag"),
        content_type: header("content-type"),
        headers: call
            .want_headers
            .iter()
            .filter_map(|name| Some((name.to_ascii_lowercase(), header(name)?)))
            .collect(),
        ..HttpResponse::default()
    }
}

/// The device's route to a [`Network`].
#[derive(Debug)]
pub struct Routed(pub Rc<RefCell<Network>>);

impl Route for Routed {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        self.0.borrow_mut().start(call)
    }
}

/// The requests noted, as JSON for the page: `[{"id", "method", "url",
/// "source": "fixture" | "none" | "fetch", "status"}]`.
pub fn seen_json(seen: &[Seen]) -> String {
    let items: Vec<String> = seen
        .iter()
        .map(|s| {
            let (source, status) = match s.source {
                Source::Fixture(status) => ("fixture", status.to_string()),
                Source::Unmatched => ("none", String::from("0")),
                Source::Fetch => ("fetch", String::from("null")),
            };
            format!(
                "{{\"id\":{},\"method\":{},\"url\":{},\"source\":\"{source}\",\"status\":{status}}}",
                s.id,
                json_string(&s.method),
                json_string(&s.url)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Requests for the page to fetch, as JSON: `[{"id", "method", "url",
/// "headers": [[name, value]], "body": hex or null, "timeout_ms"}]`.
pub fn outbox_json(calls: &[HttpCall]) -> String {
    let items: Vec<String> = calls
        .iter()
        .map(|c| {
            let headers: Vec<String> = c
                .headers
                .iter()
                .map(|(n, v)| format!("[{},{}]", json_string(n), json_string(v)))
                .collect();
            let body = c.body.as_ref().map_or(String::from("null"), |b| {
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("\"{hex}\"")
            });
            format!(
                "{{\"id\":{},\"method\":{},\"url\":{},\"headers\":[{}],\"body\":{body},\"timeout_ms\":{}}}",
                c.id,
                json_string(&c.method),
                json_string(&c.url),
                headers.join(","),
                c.timeout_ms
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_with_stars() {
        assert!(matches("https://a.org/x", "https://a.org/x"));
        assert!(!matches("https://a.org/x", "https://a.org/xy"));
        assert!(matches("https://a.org/*", "https://a.org/v1/feed?page=2"));
        assert!(matches("*/feed*", "https://a.org/v1/feed?page=2"));
        assert!(matches("https://*.org/*/feed", "https://a.org/v1/feed"));
        assert!(!matches("https://b.org/*", "https://a.org/v1"));
        assert!(matches("*", ""));
    }

    fn call(id: u32, method: &str, url: &str) -> HttpCall {
        HttpCall {
            id,
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: 1024,
            timeout_ms: 30_000,
            want_headers: vec![String::from("X-Count")],
            save_to: None,
        }
    }

    #[test]
    fn fixtures_answer_others_fail_or_go_to_the_page() {
        let mut network = Network::default();
        network.set_fixtures(vec![
            Fixture {
                method: Some(String::from("GET")),
                url: String::from("https://api.example.com/v1/*"),
                status: 200,
                headers: vec![
                    (
                        String::from("Content-Type"),
                        String::from("application/json"),
                    ),
                    (String::from("x-count"), String::from("3")),
                ],
                body: b"{\"items\":[]}".to_vec(),
                delay_ms: 0,
            },
            Fixture {
                method: None,
                url: String::from("https://slow.example.com/*"),
                status: 204,
                delay_ms: 500,
                ..Fixture::default()
            },
        ]);
        let answer = network
            .start(call(1, "GET", "https://api.example.com/v1/items"))
            .unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.content_type.as_deref(), Some("application/json"));
        assert_eq!(
            answer.headers,
            [(String::from("x-count"), String::from("3"))]
        );
        // Another method: not that fixture's.
        let failed = network
            .start(call(2, "POST", "https://api.example.com/v1/items"))
            .unwrap();
        assert_eq!(failed.status, 0);
        assert!(String::from_utf8_lossy(&failed.body).contains("no recorded answer for POST"));
        // Delayed by the device's time.
        network.tick(1000);
        assert!(
            network
                .start(call(3, "GET", "https://slow.example.com/a"))
                .is_none()
        );
        assert_eq!(network.delayed.len(), 1);
        network.tick(1499);
        assert_eq!(network.delayed.len(), 1);
        network.tick(1500);
        assert!(network.delayed.is_empty());
        // Real fetches allowed: unmatched ones go to the page.
        network.set_real(true);
        assert!(
            network
                .start(call(4, "GET", "https://elsewhere.org/"))
                .is_none()
        );
        let outbox = network.take_outbox();
        assert_eq!(outbox.len(), 1);
        assert!(outbox_json(&outbox).contains(r#""url":"https://elsewhere.org/""#));
        assert!(network.fetched(
            4,
            200,
            "content-type: text/plain\nx-count: 9",
            b"hi".to_vec()
        ));
        assert!(!network.fetched(4, 200, "", Vec::new()), "answered once");
        let seen = network.take_seen();
        assert_eq!(
            seen.iter().map(|s| s.source).collect::<Vec<_>>(),
            [
                Source::Fixture(200),
                Source::Unmatched,
                Source::Fixture(204),
                Source::Fetch
            ]
        );
        let json = seen_json(&seen);
        assert!(json.contains(r#""source":"none","status":0"#), "{json}");
        assert!(network.take_seen().is_empty());
    }

    #[test]
    fn answers_keep_to_what_the_request_takes() {
        let mut long = call(1, "GET", "u");
        long.max_response = 4;
        let cut = response(&long, 200, &[], b"123456".to_vec());
        assert_eq!(cut.body, b"1234");
        long.save_to = Some(String::from("books/a.epub"));
        assert_eq!(response(&long, 200, &[], b"123456".to_vec()).status, 0);
    }
}
