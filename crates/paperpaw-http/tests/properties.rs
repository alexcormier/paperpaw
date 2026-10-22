//! Properties of the HTTP code: arbitrary bytes never panic the response
//! parser, the request head reader or the address parser; an answer head
//! built here reads back the same; a request built here reads back the
//! same on the server side; and a response fed in pieces of any size gives
//! the same head and body as fed at once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

use paperpaw_http::server::{parse_request, response_head};
use paperpaw_http::{Error, Head, Parser, Url, redirect, request};
use proptest::prelude::*;

/// What a whole run of the parser came to.
#[derive(Debug, PartialEq)]
struct Run {
    head: Option<Head>,
    body: Vec<u8>,
    /// The first error, from `feed` or from `close`.
    result: Result<(), Error>,
    done: bool,
}

/// Feeds `bytes` cut at `cuts` (offsets, any order), then closes.
fn run(method: &str, bytes: &[u8], cuts: &[usize]) -> Run {
    let mut points: Vec<usize> = cuts.iter().map(|c| c % (bytes.len() + 1)).collect();
    points.push(0);
    points.push(bytes.len());
    points.sort_unstable();
    points.dedup();
    let mut parser = Parser::new(method);
    let mut body = Vec::new();
    let mut result = Ok(());
    for pair in points.windows(2) {
        result = parser.feed(&bytes[pair[0]..pair[1]], &mut |b| body.extend_from_slice(b));
        if result.is_err() {
            break;
        }
    }
    let done = parser.done();
    if result.is_ok() {
        result = parser.close();
    }
    Run {
        head: parser.head().cloned(),
        body,
        result,
        done,
    }
}

/// A well-formed response: a head with a body framed by length, chunks or
/// the connection's end.
fn response() -> impl Strategy<Value = Vec<u8>> {
    (
        prop_oneof![Just(200u16), Just(201), Just(206), Just(404), Just(500)],
        proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..40), 0..6),
        0..3u8,
        any::<bool>(),
    )
        .prop_map(|(status, chunks, framing, interim)| {
            let mut out = Vec::new();
            if interim {
                out.extend_from_slice(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
            out.extend_from_slice(format!("HTTP/1.1 {status} X\r\nETag: \"t\"\r\n").as_bytes());
            let body: Vec<u8> = chunks.concat();
            match framing {
                0 => {
                    out.extend_from_slice(
                        format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes(),
                    );
                    out.extend_from_slice(&body);
                }
                1 => {
                    out.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
                    for chunk in chunks.iter().filter(|c| !c.is_empty()) {
                        out.extend_from_slice(format!("{:x};e=1\r\n", chunk.len()).as_bytes());
                        out.extend_from_slice(chunk);
                        out.extend_from_slice(b"\r\n");
                    }
                    out.extend_from_slice(b"0\r\nX-Trailer: 1\r\n\r\n");
                }
                _ => {
                    out.extend_from_slice(b"\r\n");
                    out.extend_from_slice(&body);
                }
            }
            out
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn arbitrary_bytes_never_panic_the_response_parser(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
        head_request in any::<bool>(),
    ) {
        let _ = run(if head_request { "HEAD" } else { "GET" }, &bytes, &cuts);
    }

    #[test]
    fn arbitrary_bytes_after_a_valid_head_never_panic(
        tail in proptest::collection::vec(any::<u8>(), 0..512),
        framing in 0..3usize,
        cuts in proptest::collection::vec(any::<usize>(), 0..8),
    ) {
        let heads: [&[u8]; 3] = [
            b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            b"HTTP/1.1 200 OK\r\n\r\n",
        ];
        let mut bytes = heads[framing].to_vec();
        bytes.extend_from_slice(&tail);
        let outcome = run("GET", &bytes, &cuts);
        prop_assert!(outcome.head.is_some());
    }

    #[test]
    fn a_response_in_pieces_reads_as_at_once(
        response in response(),
        cuts in proptest::collection::vec(any::<usize>(), 0..12),
    ) {
        let whole = run("GET", &response, &[]);
        prop_assert_eq!(whole.result, Ok(()));
        prop_assert_eq!(&run("GET", &response, &cuts), &whole);
    }

    #[test]
    fn arbitrary_bytes_never_panic_the_request_reader(
        bytes in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        if let Ok(Some((_, used))) = parse_request(&bytes) {
            prop_assert!(used <= bytes.len());
            prop_assert_eq!(&bytes[used - 4..used], b"\r\n\r\n");
        }
    }

    #[test]
    fn arbitrary_text_never_panics_the_address_parser(
        text in "\\PC{0,40}",
        prefix in prop_oneof![Just(""), Just("http://"), Just("HTTPS://")],
        location in "\\PC{0,20}",
    ) {
        let address = format!("{prefix}{text}");
        if let Some(parsed) = Url::parse(&address) {
            prop_assert!(!parsed.host.is_empty());
            prop_assert!(!parsed.path.contains('#'));
            let _ = parsed.host_header();
            let _ = request("GET", &parsed, &[], Some(0));
        }
        let _ = redirect(&address, &location);
    }

    #[test]
    fn answer_heads_read_back_the_same(
        status in prop_oneof![200u16..=203, 205u16..=299, 400u16..=599],
        content_type in "[a-z]{1,10}/[a-z0-9.+-]{1,12}",
        etag in "\"[a-zA-Z0-9]{0,16}\"",
        body in proptest::collection::vec(any::<u8>(), 0..200),
        cuts in proptest::collection::vec(any::<usize>(), 0..6),
    ) {
        let headers = [(String::from("ETag"), etag.clone())];
        let mut bytes = response_head(status, &content_type, &headers, body.len() as u64);
        bytes.extend_from_slice(&body);
        let outcome = run("GET", &bytes, &cuts);
        prop_assert_eq!(outcome.result, Ok(()));
        prop_assert!(outcome.done);
        let head = outcome.head.unwrap();
        prop_assert_eq!(head.status, status);
        prop_assert_eq!(head.content_type.as_deref(), Some(content_type.as_str()));
        prop_assert_eq!(head.etag, Some(etag));
        prop_assert_eq!(head.length, Some(body.len() as u64));
        prop_assert_eq!(outcome.body, body);
    }

    #[test]
    fn requests_read_back_the_same_on_the_server_side(
        method in "[A-Z]{1,8}",
        host in "[a-z][a-z0-9.-]{0,20}",
        port in 1u16..,
        path in "(/[a-zA-Z0-9._~%!$&'()*+,;=:@-]{0,10}){1,4}(\\?[a-zA-Z0-9=&%]{0,20})?",
        headers in proptest::collection::vec(("[A-Za-z][A-Za-z0-9-]{0,15}", "[ -~]{0,30}"), 0..5),
        body_len in proptest::option::of(0usize..1_000_000),
    ) {
        let address = format!("http://{host}:{port}{path}");
        let parsed = Url::parse(&address).unwrap();
        let bytes = request(&method, &parsed, &headers, body_len);
        let (head, used) = parse_request(&bytes).unwrap().unwrap();
        prop_assert_eq!(used, bytes.len());
        prop_assert_eq!(head.method, method);
        prop_assert_eq!(head.target, path);
        if let Some(len) = body_len {
            prop_assert_eq!(head.length, len as u64);
        }
    }
}
