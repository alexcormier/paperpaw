//! The response [`Parser`] on hostile and odd input: heads without the
//! blank line, bare line feeds, header lines without a colon, oversized
//! heads, conflicting or broken lengths, chunked bodies with bad sizes,
//! cut bodies and trailers, interim answers and the end of the connection.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests panic on purpose when something is wrong.

use paperpaw_http::{Error, Head, MAX_HEAD, Parser};

/// What came of feeding `response` in pieces of `step` bytes.
struct Outcome {
    head: Option<Head>,
    body: Vec<u8>,
    /// The first error `feed` returned, if any.
    error: Option<Error>,
    done: bool,
}

fn feed(method: &str, response: &[u8], step: usize) -> Outcome {
    let mut parser = Parser::new(method);
    let mut body = Vec::new();
    let mut error = None;
    for piece in response.chunks(step.max(1)) {
        if let Err(e) = parser.feed(piece, &mut |b| body.extend_from_slice(b)) {
            error = Some(e);
            break;
        }
    }
    Outcome {
        head: parser.head().cloned(),
        body,
        error,
        done: parser.done(),
    }
}

/// Feeds `response` at once, then ends the connection.
fn feed_and_close(response: &[u8]) -> (Result<(), Error>, Vec<u8>) {
    let mut parser = Parser::new("GET");
    let mut body = Vec::new();
    let fed = parser.feed(response, &mut |b| body.extend_from_slice(b));
    (fed.and_then(|()| parser.close()), body)
}

#[test]
fn an_empty_connection_is_cut() {
    let mut parser = Parser::new("GET");
    parser.feed(b"", &mut |_| panic!("no body")).unwrap();
    assert!(parser.head().is_none());
    assert!(!parser.done());
    assert_eq!(parser.close(), Err(Error::Cut));
}

#[test]
fn a_head_without_the_blank_line_waits_and_then_is_cut() {
    let outcome = feed("GET", b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n", 3);
    assert_eq!(outcome.error, None);
    assert!(outcome.head.is_none(), "the head is not complete yet");
    let (closed, _) = feed_and_close(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n");
    assert_eq!(closed, Err(Error::Cut));
}

#[test]
fn a_head_with_bare_line_feeds_only_never_completes() {
    // Without CRLF CRLF the head never ends; the connection's end says so.
    let (closed, body) = feed_and_close(b"HTTP/1.1 200 OK\nContent-Length: 2\n\nhi");
    assert_eq!(closed, Err(Error::Cut));
    assert!(body.is_empty());
}

#[test]
fn bare_line_feeds_that_fill_the_limit_are_malformed() {
    let mut response = b"HTTP/1.1 200 OK\n".to_vec();
    while response.len() <= MAX_HEAD {
        response.extend_from_slice(b"X-Filler: aaaaaaaaaaaaaaaa\n");
    }
    let outcome = feed("GET", &response, 512);
    assert_eq!(outcome.error, Some(Error::Malformed));
}

#[test]
fn status_lines_that_are_not_http_are_malformed() {
    for bad in [
        &b"\r\n\r\n"[..],
        b"HTTP/1.1\r\n\r\n",
        b"HTTP/1.1 OK 200\r\n\r\n",
        b"HTTP/1.1 70000 Too Big\r\n\r\n",
        b"HTTP/1.1 -200 Negative\r\n\r\n",
        b"http2 200 OK\r\n\r\n",
        b"ICY 200 OK\r\n\r\n",
        b"<html>\r\n\r\n",
    ] {
        for step in [1, 5, bad.len()] {
            let outcome = feed("GET", bad, step);
            assert_eq!(
                outcome.error,
                Some(Error::Malformed),
                "{:?} in pieces of {step}",
                String::from_utf8_lossy(bad)
            );
            assert!(outcome.head.is_none());
        }
    }
}

#[test]
fn a_status_line_without_a_reason_is_taken() {
    let outcome = feed("GET", b"HTTP/1.1 404\r\nContent-Length: 0\r\n\r\n", 4);
    assert_eq!(outcome.head.unwrap().status, 404);
    assert!(outcome.done);
}

#[test]
fn header_lines_without_a_colon_are_skipped() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nthis is no header\r\nContent-Length: 3\r\n\r\nabc",
        7,
    );
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.head.unwrap().length, Some(3));
    assert_eq!(outcome.body, b"abc");
    assert!(outcome.done);
}

#[test]
fn header_names_are_matched_without_regard_to_case_or_spaces() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\ncontent-TYPE:text/html\r\n  LOCATION  :  /x  \r\nEtag: W/\"1\"\r\n\
          CONTENT-LENGTH:2\r\n\r\nok",
        3,
    );
    let head = outcome.head.unwrap();
    assert_eq!(head.content_type.as_deref(), Some("text/html"));
    assert_eq!(head.location.as_deref(), Some("/x"));
    assert_eq!(head.etag.as_deref(), Some("W/\"1\""));
    assert_eq!(outcome.body, b"ok");
}

#[test]
fn invalid_utf8_in_a_head_is_malformed() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nContent-Type: text/\xff\xfe\r\nContent-Length: 0\r\n\r\n",
        8,
    );
    assert_eq!(outcome.error, Some(Error::Malformed));
}

#[test]
fn a_head_of_exactly_the_limit_is_taken() {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n".to_vec();
    let filler = MAX_HEAD - response.len() - "X-Pad: \r\n\r\n".len();
    response.extend_from_slice(b"X-Pad: ");
    response.extend(std::iter::repeat_n(b'p', filler));
    response.extend_from_slice(b"\r\n\r\n");
    assert_eq!(response.len(), MAX_HEAD);
    response.push(b'!');
    for step in [1, 100, 4096, response.len()] {
        let outcome = feed("GET", &response, step);
        assert_eq!(outcome.error, None, "pieces of {step}");
        assert_eq!(outcome.body, b"!");
        assert!(outcome.done);
    }
}

#[test]
fn heads_with_huge_header_counts_hit_the_limit_in_small_pieces() {
    let mut response = b"HTTP/1.1 200 OK\r\n".to_vec();
    for i in 0..5000 {
        response.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
    }
    response.extend_from_slice(b"\r\n");
    let outcome = feed("GET", &response, 1024);
    assert_eq!(outcome.error, Some(Error::Malformed));
}

#[test]
fn a_head_longer_than_the_limit_is_malformed_however_it_comes() {
    let mut response = b"HTTP/1.1 200 OK\r\n".to_vec();
    response.extend_from_slice(b"X-Pad: ");
    response.extend(std::iter::repeat_n(b'p', MAX_HEAD));
    response.extend_from_slice(b"\r\nContent-Length: 0\r\n\r\n");
    // In small pieces the limit holds ...
    assert_eq!(feed("GET", &response, 1024).error, Some(Error::Malformed));
    // ... and so it must when the same bytes come in one piece.
    assert_eq!(
        feed("GET", &response, response.len()).error,
        Some(Error::Malformed)
    );
}

#[test]
fn content_lengths_that_are_not_numbers_are_malformed() {
    for bad in [
        "-1",
        "abc",
        "1.5",
        "0x10",
        "18446744073709551616",
        "99999999999999999999999",
        "",
        "1 2",
    ] {
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {bad}\r\n\r\n");
        let outcome = feed("GET", response.as_bytes(), 9);
        assert_eq!(outcome.error, Some(Error::Malformed), "{bad:?}");
    }
}

#[test]
fn the_largest_content_length_is_taken_and_cut_at_the_end() {
    let (closed, body) =
        feed_and_close(b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\nsome");
    assert_eq!(body, b"some");
    assert_eq!(closed, Err(Error::Cut));
}

#[test]
fn chunked_wins_over_a_content_length() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n\
                     5\r\nhello\r\n0\r\n\r\n";
    for step in [1, 4, response.len()] {
        let outcome = feed("GET", response, step);
        let head = outcome.head.unwrap();
        assert!(head.chunked);
        assert_eq!(outcome.body, b"hello", "not cut after three bytes");
        assert!(outcome.done);
    }
}

#[test]
fn chunked_only_counts_when_it_is_the_last_coding() {
    // `chunked, gzip` is not chunked framing: the body runs to the end.
    let (closed, body) =
        feed_and_close(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked, gzip\r\n\r\n3\r\nabc");
    assert_eq!(closed, Ok(()));
    assert_eq!(body, b"3\r\nabc");
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding:  GZIP , Chunked \r\n\r\n0\r\n\r\n",
        2,
    );
    assert!(outcome.head.unwrap().chunked);
    assert!(outcome.done);
}

#[test]
fn chunk_sizes_that_are_not_hex_are_malformed() {
    for bad in ["", "g", "-1", "0x5", "5 5", "\u{e9}"] {
        let response = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{bad}\r\n");
        let outcome = feed("GET", response.as_bytes(), 3);
        assert_eq!(outcome.error, Some(Error::Malformed), "{bad:?}");
    }
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\xff\r\n",
        3,
    );
    assert_eq!(outcome.error, Some(Error::Malformed), "invalid UTF-8");
}

#[test]
fn chunk_sizes_beyond_64_bits_are_malformed() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10000000000000000\r\nx",
        5,
    );
    assert_eq!(outcome.error, Some(Error::Malformed));
}

#[test]
fn the_largest_chunk_size_is_taken_and_cut_at_the_end() {
    let (closed, body) = feed_and_close(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFFF\r\npart",
    );
    assert_eq!(body, b"part");
    assert_eq!(closed, Err(Error::Cut));
}

#[test]
fn chunk_sizes_in_upper_case_with_extensions_and_padding_are_read() {
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     A ; name=\"x;y\"\r\n0123456789\r\n00000\r\n\r\n";
    let outcome = feed("GET", response, 2);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.body, b"0123456789");
    assert!(outcome.done);
}

#[test]
fn chunk_data_longer_than_its_size_is_malformed() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcdef\r\n0\r\n\r\n",
        4,
    );
    assert_eq!(outcome.error, Some(Error::Malformed));
    assert_eq!(outcome.body, b"abc", "the chunk's own bytes went on");
}

#[test]
fn chunk_lines_with_bare_line_feeds_are_taken() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\nok\n0\n\n",
        1,
    );
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.body, b"ok");
    assert!(outcome.done);
}

#[test]
fn chunk_size_lines_longer_than_a_kilobyte_in_pieces_are_malformed() {
    let mut response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;".to_vec();
    response.extend(std::iter::repeat_n(b'e', 2048));
    response.extend_from_slice(b"\r\nhello\r\n0\r\n\r\n");
    assert_eq!(feed("GET", &response, 100).error, Some(Error::Malformed));
}

#[test]
fn truncated_chunked_bodies_are_cut() {
    let full: &[u8] =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n0\r\n\r\n";
    let head_end = full.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    for cut in head_end..full.len() {
        let (closed, _) = feed_and_close(&full[..cut]);
        assert_eq!(closed, Err(Error::Cut), "cut after {cut} bytes");
    }
    let (closed, body) = feed_and_close(full);
    assert_eq!((closed, body.as_slice()), (Ok(()), &b"Wiki"[..]));
}

#[test]
fn trailers_are_read_and_dropped() {
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                     3\r\nabc\r\n0\r\nX-Sum: 1\r\nX-More: two\r\n\r\nAFTER";
    for step in [1, 6, response.len()] {
        let outcome = feed("GET", response, step);
        assert_eq!(outcome.body, b"abc", "trailers and later bytes are no body");
        assert!(outcome.done);
    }
}

#[test]
fn a_body_by_length_cut_short_is_cut() {
    let (closed, body) = feed_and_close(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n12345");
    assert_eq!(body, b"12345");
    assert_eq!(closed, Err(Error::Cut));
}

#[test]
fn bytes_after_a_complete_response_are_ignored() {
    let mut parser = Parser::new("GET");
    let mut body = Vec::new();
    parser
        .feed(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nokHTTP/1.1 500 X\r\n\r\n",
            &mut |b| body.extend_from_slice(b),
        )
        .unwrap();
    parser
        .feed(b"more and more", &mut |b| body.extend_from_slice(b))
        .unwrap();
    assert_eq!(body, b"ok");
    assert_eq!(parser.head().unwrap().status, 200);
    assert_eq!(parser.close(), Ok(()));
}

#[test]
fn duplicate_content_lengths_take_the_last() {
    let outcome = feed(
        "GET",
        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 3\r\n\r\nabcdef",
        4,
    );
    assert_eq!(outcome.body, b"abc");
}

#[test]
fn several_interim_answers_come_before_the_real_one() {
    let response = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\n\
                     HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi";
    for step in [1, 13, response.len()] {
        let outcome = feed("GET", response, step);
        assert_eq!(outcome.head.unwrap().status, 200);
        assert_eq!(outcome.body, b"hi");
        assert!(outcome.done);
    }
}

#[test]
fn a_connection_ending_after_an_interim_answer_is_cut() {
    let (closed, _) = feed_and_close(b"HTTP/1.1 100 Continue\r\n\r\n");
    assert_eq!(closed, Err(Error::Cut));
}

#[test]
fn no_content_and_head_answers_have_no_body_whatever_they_say() {
    for (method, response) in [
        (
            "GET",
            &b"HTTP/1.1 204 No Content\r\nContent-Length: 5\r\n\r\nhello"[..],
        ),
        (
            "GET",
            b"HTTP/1.1 304 Not Modified\r\nTransfer-Encoding: chunked\r\n\r\n5\r\n",
        ),
        (
            "head",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
        ),
        ("HEAD", b"HTTP/1.0 200 OK\r\n\r\n"),
    ] {
        let outcome = feed(method, response, 3);
        assert!(
            outcome.done,
            "{method} {:?}",
            String::from_utf8_lossy(response)
        );
        assert!(outcome.body.is_empty());
    }
}

#[test]
fn a_zero_length_body_is_done_at_once() {
    let outcome = feed(
        "POST",
        b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n",
        100,
    );
    assert!(outcome.done);
    assert!(outcome.body.is_empty());
}

#[test]
fn redirects_need_a_location_and_a_redirect_status() {
    let head = |status, location: Option<&str>| Head {
        status,
        location: location.map(String::from),
        ..Head::default()
    };
    for status in [301, 302, 303, 307, 308] {
        assert!(head(status, Some("/x")).is_redirect(), "{status}");
        assert!(
            !head(status, None).is_redirect(),
            "{status} without Location"
        );
    }
    for status in [200, 300, 304, 305, 306, 309] {
        assert!(!head(status, Some("/x")).is_redirect(), "{status}");
    }
}

#[test]
fn feeding_after_an_error_does_not_panic() {
    let mut parser = Parser::new("GET");
    assert!(parser.feed(b"garbage\r\n\r\n", &mut |_| {}).is_err());
    for _ in 0..3 {
        let _ = parser.feed(b"HTTP/1.1 200 OK\r\n\r\nbody", &mut |_| {});
    }
    let _ = parser.close();
}
