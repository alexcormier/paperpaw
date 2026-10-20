//! The server side: request heads read with [`parse_request`] from odd and
//! hostile clients (no blank line, bare line feeds, missing colons, huge
//! heads, broken or conflicting lengths, invalid UTF-8, targets that look
//! like path traversal), and answer heads made with [`response_head`].

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests panic on purpose when something is wrong.

use paperpaw_http::server::{MAX_REQUEST_HEAD, RequestHead, parse_request, reason, response_head};
use paperpaw_http::{Error, Parser};

fn read_head(request: &[u8]) -> RequestHead {
    parse_request(request).unwrap().unwrap().0
}

#[test]
fn nothing_yet_is_no_head_yet() {
    assert_eq!(parse_request(b""), Ok(None));
    assert_eq!(parse_request(b"GET / HTTP/1.1\r\nHost: x\r\n"), Ok(None));
}

#[test]
fn bare_line_feeds_never_end_a_head() {
    assert_eq!(parse_request(b"GET / HTTP/1.1\nHost: x\n\n"), Ok(None));
    let mut long = b"GET / HTTP/1.1\n".to_vec();
    long.resize(MAX_REQUEST_HEAD, b'\n');
    assert_eq!(parse_request(&long), Err(Error::Malformed));
}

#[test]
fn the_body_after_the_head_is_not_taken() {
    let request = b"POST /api HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody";
    let (head, used) = parse_request(request).unwrap().unwrap();
    assert_eq!(&request[used..], b"body");
    assert_eq!(head.length, 4);
}

#[test]
fn request_lines_that_are_not_http_1_are_malformed() {
    for bad in [
        &b"\r\n\r\n"[..],
        b" / HTTP/1.1\r\n\r\n",
        b"GET  HTTP/1.1\r\n\r\n",
        b"GET  / HTTP/1.1\r\n\r\n",
        b"GET /\r\n\r\n",
        b"GET / HTTP/2\r\n\r\n",
        b"GET / http/1.1\r\n\r\n",
        b"GET\t/\tHTTP/1.1\r\n\r\n",
    ] {
        assert_eq!(
            parse_request(bad),
            Err(Error::Malformed),
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
}

#[test]
fn header_lines_without_a_colon_are_malformed() {
    for bad in [
        &b"GET / HTTP/1.1\r\nHost x\r\n\r\n"[..],
        // Folded (obsolete) continuation lines are no headers either.
        b"GET / HTTP/1.1\r\nX-A: 1\r\n  continued\r\n\r\n",
        // A bare line feed inside a header line joins two lines into one
        // with a colon; the line without one is still caught.
        b"GET / HTTP/1.1\r\nX-A: 1\nno colon\r\nno colon either\r\n\r\n",
    ] {
        assert_eq!(parse_request(bad), Err(Error::Malformed));
    }
}

#[test]
fn invalid_utf8_in_a_head_is_malformed() {
    assert_eq!(
        parse_request(b"GET /\xff HTTP/1.1\r\n\r\n"),
        Err(Error::Malformed)
    );
    assert_eq!(
        parse_request(b"GET / HTTP/1.1\r\nX-Name: \xc3\x28\r\n\r\n"),
        Err(Error::Malformed)
    );
}

#[test]
fn content_lengths_that_are_not_numbers_are_malformed() {
    for bad in [
        "-1",
        "abc",
        "",
        "1.0",
        "0x10",
        "18446744073709551616",
        "5, 5",
    ] {
        let request = format!("PUT /f HTTP/1.1\r\nContent-Length: {bad}\r\n\r\n");
        assert_eq!(
            parse_request(request.as_bytes()),
            Err(Error::Malformed),
            "{bad:?}"
        );
    }
}

#[test]
fn the_largest_content_length_is_read() {
    let head = read_head(b"PUT /f HTTP/1.1\r\ncontent-LENGTH :  18446744073709551615 \r\n\r\n");
    assert_eq!(head.length, u64::MAX);
}

#[test]
fn chunked_uploads_are_flagged_even_with_a_length() {
    let head =
        read_head(b"POST / HTTP/1.1\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n");
    assert!(head.chunked, "the server answers 411");
    for coding in ["gzip", "gzip, chunked", "CHUNKED"] {
        let request = format!("POST / HTTP/1.1\r\nTransfer-Encoding: {coding}\r\n\r\n");
        assert!(
            parse_request(request.as_bytes())
                .unwrap()
                .unwrap()
                .0
                .chunked,
            "{coding}"
        );
    }
    let head = read_head(b"POST / HTTP/1.1\r\nTransfer-Encoding: Identity\r\n\r\n");
    assert!(!head.chunked);
}

#[test]
fn a_request_without_headers_has_no_body() {
    let head = read_head(b"DELETE /x HTTP/1.0\r\n\r\n");
    assert_eq!(
        (head.method.as_str(), head.target.as_str(), head.length),
        ("DELETE", "/x", 0)
    );
}

#[test]
fn targets_are_kept_as_sent_without_decoding() {
    // Decoding and refusing what climbs out of a folder is up to the
    // plugin that serves files; the head passes the target on untouched.
    for target in [
        "/%2e%2e/%2e%2e/secret",
        "/../../etc/passwd",
        "/a%00b",
        "/files?path=..%2F..%2Fx&y=%ZZ",
        "*",
        "http://other.host/abs",
        "/\u{e4}\u{f6}\u{fc}",
    ] {
        let request = format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(read_head(request.as_bytes()).target, target);
    }
}

#[test]
fn heads_at_and_beyond_the_limit() {
    let mut request = b"GET / HTTP/1.1\r\nX-Pad: ".to_vec();
    let filler = MAX_REQUEST_HEAD - request.len() - 4;
    request.extend(std::iter::repeat_n(b'p', filler));
    request.extend_from_slice(b"\r\n\r\n");
    assert_eq!(request.len(), MAX_REQUEST_HEAD);
    assert!(
        parse_request(&request).unwrap().is_some(),
        "exactly the limit"
    );
    // One byte more is too long, whether or not the head is complete.
    let mut longer = request.clone();
    longer.insert(20, b'p');
    assert_eq!(parse_request(&longer), Err(Error::Malformed));
    assert_eq!(
        parse_request(&longer[..longer.len() - 1]),
        Err(Error::Malformed)
    );
}

#[test]
fn huge_header_counts_hit_the_limit() {
    let mut request = b"GET / HTTP/1.1\r\n".to_vec();
    for i in 0..2000 {
        request.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    assert_eq!(parse_request(&request), Err(Error::Malformed));
}

#[test]
fn every_status_has_reason_words() {
    for status in 0..=999u16 {
        let words = reason(status);
        assert!(!words.is_empty());
        assert!(!words.contains(['\r', '\n']));
    }
    assert_eq!(reason(413), "Content Too Large");
    assert_eq!(reason(299), "Status");
}

#[test]
fn answer_heads_keep_line_breaks_out() {
    let headers = [
        (String::from("X-Name\r\nX-Evil"), String::from("1")),
        (String::from("X-Lf"), String::from("a\nb")),
        (String::from("X-Cr"), String::from("a\rb")),
        (String::from("X-Ok"), String::from("fine")),
    ];
    let head = response_head(200, "text/html\r\nSet-Cookie: x=1", &headers, 0);
    let text = String::from_utf8(head).unwrap();
    assert_eq!(
        text,
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 0\r\n\
         Connection: close\r\nCache-Control: no-store\r\nX-Ok: fine\r\n\r\n"
    );
}

#[test]
fn answer_heads_read_back_as_sent() {
    let headers = [
        (String::from("ETag"), String::from("\"v1\"")),
        (String::from("Location"), String::from("/next")),
    ];
    let mut bytes = response_head(302, "text/plain; charset=utf-8", &headers, 3);
    bytes.extend_from_slice(b"abc");
    let mut parser = Parser::new("GET");
    let mut body = Vec::new();
    parser
        .feed(&bytes, &mut |b| body.extend_from_slice(b))
        .unwrap();
    let head = parser.head().unwrap();
    assert_eq!(head.status, 302);
    assert_eq!(
        head.content_type.as_deref(),
        Some("text/plain; charset=utf-8")
    );
    assert_eq!(head.etag.as_deref(), Some("\"v1\""));
    assert!(head.is_redirect());
    assert_eq!(head.length, Some(3));
    assert_eq!(body, b"abc");
    assert!(parser.done());
}
