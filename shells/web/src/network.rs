//! The browser's emulator's network: plugins' requests answered from the
//! page's recorded answers (fixtures), offline and the same each time, and
//! each request told to the page; where the page allows it (a local
//! setup), the requests no fixture answers fetched by the page itself.
//! Nothing goes through a server of PaperPaw's.
//!
//! - A **fixture** answers requests whose method (any, where none is
//!   named) and address match. The pattern matches the whole address, its
//!   query too (`https://api.example.com/v1/feed*` for `…/feed?page=2`),
//!   `*` standing for any text; scheme and host are compared in any case,
//!   the rest as written. The first that matches answers, with its status,
//!   headers and body, after its delay (the device's time), or fails as
//!   timed out where the delay is longer than the request waits.
//! - A **redirect** (3xx with a `location`) is followed through the table,
//!   as a device's shell follows it (at most ten): the answer is the last
//!   fixture's, with the address it came from.
//! - **Unmatched**, a request fails as without a network (status 0), or,
//!   with real fetches allowed ([`Network::set_real`]), waits in the
//!   outbox for the page to fetch it and hand its answer back.
//! - **Every request** is noted for the page ([`Network::take_seen`]):
//!   what was asked, and how it was answered.
//!
//! Answers wait in the network that took the request ([`Route::later`]):
//! each device (a snapshot's too) has its own.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use paperpaw_core::sdk::HttpError;
use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_dev::report::json_string;
use paperpaw_emulator::http::Route;

/// Redirects followed at most, as `curl -L --max-redirs 10`.
const MAX_REDIRECTS: usize = 10;

/// A recorded answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fixture {
    /// The method it answers (`GET` …); `None`: any.
    pub method: Option<String>,
    /// The addresses it answers: `*` stands for any text.
    pub url: String,
    pub status: u16,
    /// Its headers (`content-type`, `etag`, `location` …), names in any
    /// case.
    pub headers: Vec<(String, String)>,
    /// Shared by every answer made of it.
    pub body: Rc<[u8]>,
    /// How long it takes, in milliseconds of the device's time.
    pub delay_ms: u64,
}

impl Fixture {
    fn answers(&self, method: &str, url: &str) -> bool {
        self.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(method))
            && matches(&self.url, url)
    }

    fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `url` with its scheme and host in lower case (they are compared in any
/// case), the rest as it is.
fn host_lower(url: &str) -> Cow<'_, str> {
    let Some(start) = url.find("://").map(|at| at + 3) else {
        return Cow::Borrowed(url);
    };
    let end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |at| start + at);
    if url[..end].bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(format!(
            "{}{}",
            url[..end].to_ascii_lowercase(),
            &url[end..]
        ))
    } else {
        Cow::Borrowed(url)
    }
}

/// Whether the address `text` matches `pattern`, where `*` stands for any
/// text: the whole address, scheme and host in any case.
pub fn matches(pattern: &str, text: &str) -> bool {
    glob(&host_lower(pattern), &host_lower(text))
}

fn glob(pattern: &str, text: &str) -> bool {
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

/// Where a redirect's `location` points from `url`.
fn resolve(url: &str, location: &str) -> String {
    if location.contains("://") {
        return location.to_string();
    }
    let start = url.find("://").map_or(0, |at| at + 3);
    let host_end = url[start..].find('/').map_or(url.len(), |at| start + at);
    if location.starts_with('/') {
        return format!("{}{location}", &url[..host_end]);
    }
    // Relative to the address's folder.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let folder = path
        .rfind('/')
        .filter(|at| *at >= host_end)
        .map_or(host_end, |at| at + 1);
    let base = &url[..folder];
    if base.ends_with('/') {
        format!("{base}{location}")
    } else {
        format!("{base}/{location}")
    }
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

/// What the table says for a request: the fixture that answers it, the
/// address it answers at (after redirects) and the time it takes in all.
enum Lookup<'a> {
    Answer(&'a Fixture, String, u64),
    Unmatched(String),
}

/// The requests and their answers, a device's.
#[derive(Debug, Default)]
pub struct Network {
    fixtures: Vec<Fixture>,
    real: bool,
    seen: Vec<Seen>,
    /// Requests for the page to fetch, not handed over yet.
    outbox: Vec<HttpCall>,
    /// Requests the page fetches: what their answers must keep to.
    fetching: Vec<HttpCall>,
    /// Answers and when they are due (the device's ms).
    delayed: Vec<(u64, HttpResponse)>,
    /// Answers ready for the device.
    later: VecDeque<HttpResponse>,
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

    /// The recorded answers (their bodies shared), e.g. for another
    /// device's network.
    pub fn fixtures(&self) -> Vec<Fixture> {
        self.fixtures.clone()
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

    /// The device's time is `now_ms`: the answers due by then made ready.
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.delayed)
            .into_iter()
            .partition(|(at, _)| *at <= now_ms);
        self.delayed = later;
        self.later
            .extend(due.into_iter().map(|(_, response)| response));
    }

    /// When the next delayed answer is due (the device's ms), if one is.
    pub fn next_due(&self) -> Option<u64> {
        self.delayed.iter().map(|(at, _)| *at).min()
    }

    /// The page's answer to request `id` it fetched: its status (0:
    /// failed, the reason in `body`), headers (`name: value` lines), body
    /// and the address it came from (after redirects). `false` if no such
    /// request waits.
    pub fn fetched(
        &mut self,
        id: u32,
        status: u16,
        headers: &str,
        body: Vec<u8>,
        url: &str,
    ) -> bool {
        let Some(at) = self.fetching.iter().position(|c| c.id == id) else {
            return false;
        };
        let call = self.fetching.remove(at);
        let headers: Vec<(String, String)> = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
            .collect();
        let from = (!url.is_empty()).then_some(url);
        let answer = if status == 0 {
            // The page could not fetch it: no network, or the address
            // refused the page (its CORS).
            let reason = String::from_utf8_lossy(&body);
            HttpResponse::failed(call.id, HttpError::Refused, &reason)
        } else {
            response(&call, status, &headers, Cow::Owned(body), from)
        };
        self.later.push_back(answer);
        true
    }

    /// The fixture that answers `method` `url`, redirects followed.
    fn lookup(&self, method: &str, url: &str) -> Lookup<'_> {
        let (mut method, mut url, mut delay) = (method.to_string(), url.to_string(), 0u64);
        for _ in 0..=MAX_REDIRECTS {
            let Some(fixture) = self.fixtures.iter().find(|f| f.answers(&method, &url)) else {
                return Lookup::Unmatched(url);
            };
            delay = delay.saturating_add(fixture.delay_ms);
            match (fixture.status, fixture.header("location")) {
                (301 | 302 | 303 | 307 | 308, Some(location)) => {
                    if fixture.status == 303 {
                        method = String::from("GET");
                    }
                    url = resolve(&url, location);
                }
                _ => return Lookup::Answer(fixture, url, delay),
            }
        }
        Lookup::Unmatched(url)
    }

    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        let mut seen = Seen {
            id: call.id,
            method: call.method.clone(),
            url: call.url.clone(),
            source: Source::Unmatched,
        };
        let (answer, delay) = match self.lookup(&call.method, &call.url) {
            Lookup::Answer(fixture, url, delay) => {
                seen.source = Source::Fixture(fixture.status);
                let timeout = u64::from(call.timeout_ms);
                if delay > timeout {
                    let reason = "timed out (the recorded answer takes longer)";
                    (Some(HttpResponse::failed(call.id, HttpError::Timeout, reason)), timeout)
                } else {
                    let from = (url != call.url).then_some(url.as_str());
                    let body = Cow::Borrowed(&*fixture.body);
                    let answer = response(&call, fixture.status, &fixture.headers, body, from);
                    (Some(answer), delay)
                }
            }
            Lookup::Unmatched(_) if self.real => {
                seen.source = Source::Fetch;
                self.outbox.push(call);
                (None, 0)
            }
            Lookup::Unmatched(url) => {
                let reason = format!(
                    "no recorded answer for {} {url} (no network in the browser's emulator)",
                    call.method
                );
                (Some(HttpResponse::failed(call.id, HttpError::NoNetwork, &reason)), 0)
            }
        };
        self.seen.push(seen);
        match answer {
            Some(answer) if delay > 0 => {
                self.delayed
                    .push((self.now_ms.saturating_add(delay), answer));
                None
            }
            answer => answer,
        }
    }
}

/// The answer to `call`, as a shell gives it: the headers it asked for,
/// the body cut to the most it takes (failed for a file instead, which is
/// never saved cut short), copied no further than that, and the address
/// it came from where elsewhere.
fn response(
    call: &HttpCall,
    status: u16,
    headers: &[(String, String)],
    body: Cow<'_, [u8]>,
    from: Option<&str>,
) -> HttpResponse {
    if body.len() > call.max_response && call.save_to.is_some() {
        let reason = "the answer is larger than the request takes";
        return HttpResponse::failed(call.id, HttpError::Refused, reason);
    }
    let truncated = body.len() > call.max_response;
    let body = match body {
        Cow::Borrowed(bytes) => bytes[..bytes.len().min(call.max_response)].to_vec(),
        Cow::Owned(mut bytes) => {
            bytes.truncate(call.max_response);
            bytes
        }
    };
    let text = |name: &str| header(headers, name).map(String::from);
    HttpResponse {
        id: call.id,
        status,
        body,
        etag: text("etag"),
        content_type: text("content-type"),
        url: from.filter(|url| *url != call.url).map(String::from),
        headers: call
            .want_headers
            .iter()
            .filter_map(|name| Some((name.to_ascii_lowercase(), text(name)?)))
            .collect(),
        truncated,
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

    fn later(&mut self) -> Option<HttpResponse> {
        self.0.borrow_mut().later.pop_front()
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
    fn patterns_match_the_whole_address_its_host_in_any_case() {
        assert!(matches("https://a.org/x", "https://a.org/x"));
        assert!(!matches("https://a.org/x", "https://a.org/xy"));
        assert!(matches("https://a.org/*", "https://a.org/v1/feed?page=2"));
        assert!(matches(
            "https://a.org/v1/feed*",
            "https://a.org/v1/feed?page=2"
        ));
        assert!(
            !matches("https://a.org/v1/feed", "https://a.org/v1/feed?page=2"),
            "the query too"
        );
        assert!(matches("*/feed*", "https://a.org/v1/feed?page=2"));
        assert!(matches("https://*.org/*/feed", "https://a.org/v1/feed"));
        assert!(!matches("https://b.org/*", "https://a.org/v1"));
        assert!(matches(
            "HTTPS://API.Example.com/V1",
            "https://api.example.com/V1"
        ));
        assert!(
            !matches("https://api.example.com/v1", "https://api.example.com/V1"),
            "the path as written"
        );
        assert!(matches("*", ""));
    }

    #[test]
    fn redirects_resolve_against_the_address() {
        assert_eq!(
            resolve("https://a.org/x/y?q", "https://b.org/z"),
            "https://b.org/z"
        );
        assert_eq!(resolve("https://a.org/x/y?q", "/z"), "https://a.org/z");
        assert_eq!(resolve("https://a.org/x/y?q", "z"), "https://a.org/x/z");
        assert_eq!(resolve("https://a.org", "z"), "https://a.org/z");
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

    fn fixture(url: &str, status: u16) -> Fixture {
        Fixture {
            url: url.to_string(),
            status,
            ..Fixture::default()
        }
    }

    #[test]
    fn fixtures_answer_others_fail_or_go_to_the_page() {
        let mut network = Network::default();
        network.set_fixtures(vec![
            Fixture {
                method: Some(String::from("GET")),
                headers: vec![
                    (
                        String::from("Content-Type"),
                        String::from("application/json"),
                    ),
                    (String::from("x-count"), String::from("3")),
                ],
                body: Rc::from(&b"{\"items\":[]}"[..]),
                ..fixture("https://api.example.com/v1/*", 200)
            },
            Fixture {
                delay_ms: 500,
                ..fixture("https://slow.example.com/*", 204)
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
        assert_eq!(answer.url, None, "from where it was asked");
        // Another method: not that fixture's.
        let failed = network
            .start(call(2, "POST", "https://api.example.com/v1/items"))
            .unwrap();
        assert_eq!(failed.status, 0);
        assert!(String::from_utf8_lossy(&failed.body).contains("no recorded answer for POST"));
        // Delayed by the device's time, its due time told.
        network.tick(1000);
        assert!(
            network
                .start(call(3, "GET", "https://slow.example.com/a"))
                .is_none()
        );
        assert_eq!(network.next_due(), Some(1500));
        network.tick(1499);
        assert!(network.later.is_empty());
        network.tick(1500);
        assert_eq!(network.later.pop_front().map(|a| a.status), Some(204));
        assert_eq!(network.next_due(), None);
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
        let from = "https://elsewhere.org/moved";
        let headers = "content-type: text/plain\nx-count: 9";
        assert!(network.fetched(4, 200, headers, b"hi".to_vec(), from));
        let fetched = network.later.pop_front().and_then(|a| a.url);
        assert_eq!(fetched.as_deref(), Some(from));
        assert!(
            !network.fetched(4, 200, "", Vec::new(), ""),
            "answered once"
        );
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
    fn redirects_are_followed_through_the_table_and_delays_keep_to_the_timeout() {
        let mut network = Network::default();
        let moved = |to: &str| Fixture {
            headers: vec![(String::from("Location"), to.to_string())],
            ..fixture("https://a.org/old", 301)
        };
        network.set_fixtures(vec![
            moved("/new"),
            Fixture {
                body: Rc::from(&b"here"[..]),
                ..fixture("https://a.org/new", 200)
            },
            Fixture {
                delay_ms: u64::MAX,
                ..fixture("https://forever.org/*", 200)
            },
        ]);
        let answer = network.start(call(1, "GET", "https://a.org/old")).unwrap();
        assert_eq!((answer.status, answer.body.as_slice()), (200, &b"here"[..]));
        assert_eq!(answer.url.as_deref(), Some("https://a.org/new"));
        // Longer than the request waits: failed at its timeout, no
        // overflow.
        network.tick(u64::MAX - 10);
        assert!(
            network
                .start(call(2, "GET", "https://forever.org/x"))
                .is_none()
        );
        assert_eq!(network.next_due(), Some(u64::MAX));
        network.tick(u64::MAX);
        assert_eq!(network.later.pop_front().map(|a| a.status), Some(0));
        // A loop of redirects: as without a network.
        network.set_fixtures(vec![moved("/old")]);
        let looped = network.start(call(3, "GET", "https://a.org/old"));
        assert_eq!(looped.map(|a| a.status), Some(0));
    }

    #[test]
    fn answers_keep_to_what_the_request_takes() {
        let mut long = call(1, "GET", "u");
        long.max_response = 4;
        let cut = response(&long, 200, &[], Cow::Borrowed(b"123456"), None);
        assert_eq!(cut.body, b"1234");
        long.save_to = Some(String::from("books/a.epub"));
        let refused = response(&long, 200, &[], Cow::Borrowed(b"123456"), None);
        assert_eq!(refused.status, 0);
    }
}
