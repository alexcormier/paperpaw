//! The server's side: a request's head read from the bytes that came, and
//! the head of an answer. One request per connection, as for the client;
//! request bodies need a `Content-Length` (no chunked uploads).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::{Error, is_header};

/// The longest request head taken.
pub const MAX_REQUEST_HEAD: usize = 8 * 1024;

/// What a request's head says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    /// `GET`, `PUT`, `POST`, …
    pub method: String,
    /// The path and query asked for, as sent (e.g. `/api/list?d=books`).
    pub target: String,
    /// The bytes of the body that follows (0 for none).
    pub length: u64,
    /// The body comes in chunks (refused: answer 411).
    pub chunked: bool,
}

/// Reads a request's head from the start of `data`: `None` while it is not
/// all there yet; else the head and how many bytes of `data` it took (the
/// body's first bytes may follow). [`Error::Malformed`] for one that is not
/// HTTP, or longer than [`MAX_REQUEST_HEAD`].
pub fn parse_request(data: &[u8]) -> Result<Option<(RequestHead, usize)>, Error> {
    let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if data.len() >= MAX_REQUEST_HEAD {
            Err(Error::Malformed)
        } else {
            Ok(None)
        };
    };
    if end + 4 > MAX_REQUEST_HEAD {
        return Err(Error::Malformed);
    }
    let text = core::str::from_utf8(&data[..end]).map_err(|_| Error::Malformed)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target), Some(version)) = (first.next(), first.next(), first.next())
    else {
        return Err(Error::Malformed);
    };
    if method.is_empty() || target.is_empty() || !version.starts_with("HTTP/1.") {
        return Err(Error::Malformed);
    }
    let mut head = RequestHead {
        method: method.to_string(),
        target: target.to_string(),
        length: 0,
        chunked: false,
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::Malformed);
        };
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("content-length") {
            head.length = value.parse().map_err(|_| Error::Malformed)?;
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            head.chunked = !value.eq_ignore_ascii_case("identity");
        }
    }
    Ok(Some((head, end + 4)))
}

/// The bytes of an answer's head, for a body of `length` bytes; the
/// connection closes after it. Headers that are not well formed (e.g.
/// with line breaks) are left out.
pub fn response_head(
    status: u16,
    content_type: &str,
    headers: &[(String, String)],
    length: u64,
) -> Vec<u8> {
    let content_type = if !is_header("Content-Type", content_type) {
        "application/octet-stream"
    } else {
        content_type
    };
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        reason(status)
    );
    for (name, value) in headers {
        if is_header(name, value) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    head.into_bytes()
}

/// The words that go with a status.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        507 => "Insufficient Storage",
        _ => "Status",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_in_pieces() {
        let request =
            b"PUT /api/file?p=a%20b HTTP/1.1\r\nHost: x\r\ncontent-length: 5\r\n\r\nhello";
        let end = request.len() - 5;
        for cut in 0..end {
            assert_eq!(parse_request(&request[..cut]), Ok(None), "{cut}");
        }
        let (head, used) = parse_request(request).unwrap().unwrap();
        assert_eq!(used, end);
        assert_eq!(
            head,
            RequestHead {
                method: String::from("PUT"),
                target: String::from("/api/file?p=a%20b"),
                length: 5,
                chunked: false,
            }
        );
    }

    #[test]
    fn chunked_and_bad_heads() {
        let (head, _) = parse_request(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap()
            .unwrap();
        assert!(head.chunked);
        for bad in [
            &b"GET\r\n\r\n"[..],
            b"GET / SPDY\r\n\r\n",
            b"GET / HTTP/1.1\r\nno colon\r\n\r\n",
            b"GET / HTTP/1.1\r\nContent-Length: many\r\n\r\n",
        ] {
            assert_eq!(parse_request(bad), Err(Error::Malformed));
        }
        let long = alloc::vec![b'a'; MAX_REQUEST_HEAD];
        assert_eq!(parse_request(&long), Err(Error::Malformed));
    }

    #[test]
    fn answer_heads() {
        let headers = [
            (String::from("X-A"), String::from("1")),
            (String::from("X-B"), String::from("2\r\nX-Evil: 1")),
        ];
        let head = String::from_utf8(response_head(404, "text/plain", &headers, 9)).unwrap();
        assert_eq!(
            head,
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\nConnection: close\r\nCache-Control: no-store\r\nX-A: 1\r\n\r\n"
        );
    }
}
