//! The HTTP/1.1 client protocol, without I/O: for a shell that has its own
//! network stack (the X4's, on smoltcp) and passes bytes in and out.
//!
//! - [`Url::parse`] splits an address; [`request`] builds what to send;
//! - a [`Parser`] takes the response as it comes, in pieces of any size,
//!   and passes the body on in pieces (by `Content-Length`, chunked, or up
//!   to the connection's end), so a large one never needs to fit in
//!   memory;
//! - [`redirect`] tells where a redirect leads.
//!
//! One request per connection (`Connection: close`).

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// What went wrong with a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Not HTTP as expected (or a head longer than [`MAX_HEAD`]).
    Malformed,
    /// The connection ended before the whole body came.
    Cut,
}

/// The longest response head taken.
pub const MAX_HEAD: usize = 16 * 1024;

/// An address split up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Url<'a> {
    pub https: bool,
    pub host: &'a str,
    pub port: u16,
    /// From the first `/` on, with the query (`/` if none).
    pub path: &'a str,
}

impl<'a> Url<'a> {
    /// Splits `http://host[:port]/path` or `https://…`; `None` for anything
    /// else.
    pub fn parse(url: &'a str) -> Option<Self> {
        let (https, rest) = if let Some(rest) = strip_prefix_ignore_case(url, "http://") {
            (false, rest)
        } else {
            (true, strip_prefix_ignore_case(url, "https://")?)
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        // Credentials in the address are not sent.
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        let path = path.split('#').next().unwrap_or("");
        let path = if path.is_empty() { "/" } else { path };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.ends_with(']') || port.is_empty() => {
                (host, port.parse().ok()?)
            }
            _ => (authority, if https { 443 } else { 80 }),
        };
        (!host.is_empty()).then_some(Self {
            https,
            host,
            port,
            path,
        })
    }

    /// The `Host` header's value: the host, with the port if not the
    /// scheme's own.
    pub fn host_header(&self) -> String {
        let default = if self.https { 443 } else { 80 };
        if self.port == default {
            self.host.to_string()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// The bytes of a request's head (the body, if any, goes after them).
/// `headers` may override `User-Agent`; `Host`, `Connection` and
/// `Content-Length` are set here.
pub fn request(
    method: &str,
    url: &Url<'_>,
    headers: &[(String, String)],
    body_len: Option<usize>,
) -> Vec<u8> {
    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        url.path,
        url.host_header()
    );
    let ours = ["host", "connection", "content-length"];
    for (name, value) in headers {
        if ours.iter().any(|o| name.eq_ignore_ascii_case(o)) || has_break(name) || has_break(value)
        {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
    {
        head.push_str("User-Agent: QuireInk\r\n");
    }
    if let Some(len) = body_len {
        head.push_str(&format!("Content-Length: {len}\r\n"));
    }
    head.push_str("\r\n");
    head.into_bytes()
}

fn has_break(text: &str) -> bool {
    text.contains(['\r', '\n'])
}

/// Where a redirect from `base` to `location` leads (absolute, or relative
/// to `base`).
pub fn redirect(base: &str, location: &str) -> Option<String> {
    let location = location.trim();
    if Url::parse(location).is_some() {
        return Some(location.to_string());
    }
    let url = Url::parse(base)?;
    let origin = format!(
        "{}://{}",
        if url.https { "https" } else { "http" },
        url.host_header()
    );
    if let Some(rest) = location.strip_prefix("//") {
        return Some(format!(
            "{}://{rest}",
            if url.https { "https" } else { "http" }
        ));
    }
    if location.starts_with('/') {
        return Some(format!("{origin}{location}"));
    }
    let path = url.path.split('?').next().unwrap_or("/");
    let dir = &path[..path.rfind('/').map_or(0, |at| at + 1)];
    Some(format!("{origin}{dir}{location}"))
}

/// What the head of a response said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub location: Option<String>,
    /// `Content-Length`, if given.
    pub length: Option<u64>,
    pub chunked: bool,
}

impl Head {
    /// A redirect that says where to.
    pub fn is_redirect(&self) -> bool {
        matches!(self.status, 301 | 302 | 303 | 307 | 308) && self.location.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chunk {
    /// Reading a chunk's size line.
    Size,
    /// Bytes of the chunk left.
    Data(u64),
    /// The line break after a chunk's data.
    DataEnd,
    /// The lines after the last chunk, until an empty one.
    Trailer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    /// Gathering the head.
    Head,
    /// Bytes of the body left (by `Content-Length`).
    Length(u64),
    Chunked(Chunk),
    /// Up to the connection's end.
    ToClose,
    Done,
}

/// Takes a response as it comes.
#[derive(Debug)]
pub struct Parser {
    state: State,
    /// The head so far, or a chunked body's current line.
    line: Vec<u8>,
    head: Option<Head>,
    /// The request was `HEAD`: no body follows.
    no_body: bool,
}

impl Parser {
    /// A parser for the answer to a request with `method`.
    pub fn new(method: &str) -> Self {
        Self {
            state: State::Head,
            line: Vec::new(),
            head: None,
            no_body: method.eq_ignore_ascii_case("HEAD"),
        }
    }

    /// The head, once it came.
    pub fn head(&self) -> Option<&Head> {
        self.head.as_ref()
    }

    /// The whole response came.
    pub fn done(&self) -> bool {
        self.state == State::Done
    }

    /// Takes the next bytes received; the body's go to `body`, in pieces.
    /// Bytes after the end are ignored.
    pub fn feed(&mut self, mut data: &[u8], body: &mut dyn FnMut(&[u8])) -> Result<(), Error> {
        while !data.is_empty() {
            match self.state.clone() {
                State::Head => data = self.take_head(data)?,
                State::Length(left) => {
                    let n = data.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                    body(&data[..n]);
                    data = &data[n..];
                    let left = left - n as u64;
                    self.state = if left == 0 {
                        State::Done
                    } else {
                        State::Length(left)
                    };
                }
                State::ToClose => {
                    body(data);
                    data = &[];
                }
                State::Chunked(chunk) => data = self.take_chunked(chunk, data, body)?,
                State::Done => data = &[],
            }
        }
        Ok(())
    }

    /// The connection ended: fine if the response was whole, or one that
    /// ends with the connection.
    pub fn close(&mut self) -> Result<(), Error> {
        match self.state {
            State::Done => Ok(()),
            State::ToClose => {
                self.state = State::Done;
                Ok(())
            }
            _ => Err(Error::Cut),
        }
    }

    fn take_head<'d>(&mut self, data: &'d [u8]) -> Result<&'d [u8], Error> {
        let from = self.line.len().saturating_sub(3);
        self.line.extend_from_slice(data);
        let Some(end) = find(&self.line[from..], b"\r\n\r\n").map(|at| from + at) else {
            if self.line.len() > MAX_HEAD {
                return Err(Error::Malformed);
            }
            return Ok(&[]);
        };
        let rest_len = self.line.len() - (end + 4);
        let head = parse_head(&self.line[..end]).ok_or(Error::Malformed)?;
        self.line.clear();
        let rest = &data[data.len() - rest_len..];
        // An interim answer (e.g. 100 Continue): the real one follows.
        if (100..200).contains(&head.status) {
            return Ok(rest);
        }
        self.state = if self.no_body || head.status == 204 || head.status == 304 {
            State::Done
        } else if head.chunked {
            State::Chunked(Chunk::Size)
        } else {
            match head.length {
                Some(0) => State::Done,
                Some(len) => State::Length(len),
                None => State::ToClose,
            }
        };
        self.head = Some(head);
        Ok(rest)
    }

    fn take_chunked<'d>(
        &mut self,
        chunk: Chunk,
        data: &'d [u8],
        body: &mut dyn FnMut(&[u8]),
    ) -> Result<&'d [u8], Error> {
        match chunk {
            Chunk::Data(left) => {
                let n = data.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                body(&data[..n]);
                let left = left - n as u64;
                self.state = State::Chunked(if left == 0 {
                    Chunk::DataEnd
                } else {
                    Chunk::Data(left)
                });
                Ok(&data[n..])
            }
            Chunk::Size | Chunk::DataEnd | Chunk::Trailer => {
                let Some(at) = data.iter().position(|&b| b == b'\n') else {
                    self.line.extend_from_slice(data);
                    if self.line.len() > 1024 {
                        return Err(Error::Malformed);
                    }
                    return Ok(&[]);
                };
                self.line.extend_from_slice(&data[..at]);
                let line = core::mem::take(&mut self.line);
                let line = line.strip_suffix(b"\r").unwrap_or(&line);
                self.state = match chunk {
                    Chunk::DataEnd if line.is_empty() => State::Chunked(Chunk::Size),
                    Chunk::DataEnd => return Err(Error::Malformed),
                    Chunk::Trailer if line.is_empty() => State::Done,
                    Chunk::Trailer => State::Chunked(Chunk::Trailer),
                    _ => {
                        let size = core::str::from_utf8(line)
                            .ok()
                            .map(|l| l.split(';').next().unwrap_or("").trim())
                            .and_then(|hex| u64::from_str_radix(hex, 16).ok())
                            .ok_or(Error::Malformed)?;
                        State::Chunked(if size == 0 {
                            Chunk::Trailer
                        } else {
                            Chunk::Data(size)
                        })
                    }
                };
                Ok(&data[at + 1..])
            }
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(bytes: &[u8]) -> Option<Head> {
    let text = core::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next()?;
    let mut words = status_line.split_ascii_whitespace();
    if !words.next()?.starts_with("HTTP/") {
        return None;
    }
    let mut head = Head {
        status: words.next()?.parse().ok()?,
        ..Head::default()
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-type" => head.content_type = Some(value.to_string()),
            "etag" => head.etag = Some(value.to_string()),
            "location" => head.location = Some(value.to_string()),
            "content-length" => head.length = Some(value.parse().ok()?),
            "transfer-encoding" => {
                head.chunked = value
                    .rsplit(',')
                    .next()
                    .is_some_and(|last| last.trim().eq_ignore_ascii_case("chunked"));
            }
            _ => {}
        }
    }
    Some(head)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `response` in pieces of `step` bytes; the head and the body.
    fn run(method: &str, response: &[u8], step: usize, close: bool) -> (Head, Vec<u8>, bool) {
        let mut parser = Parser::new(method);
        let mut body = Vec::new();
        for piece in response.chunks(step) {
            parser
                .feed(piece, &mut |b| body.extend_from_slice(b))
                .unwrap();
        }
        let ok = if close {
            parser.close().is_ok()
        } else {
            parser.done()
        };
        (parser.head().cloned().unwrap(), body, ok)
    }

    #[test]
    fn urls() {
        let url = Url::parse("http://Example.org:8080/a/b?c=1#top").unwrap();
        assert_eq!(
            url,
            Url {
                https: false,
                host: "Example.org",
                port: 8080,
                path: "/a/b?c=1"
            }
        );
        assert_eq!(url.host_header(), "Example.org:8080");
        let url = Url::parse("HTTPS://user:pw@host?q").unwrap();
        assert_eq!((url.https, url.host, url.port, url.path), (true, "host", 443, "?q"));
        assert_eq!(Url::parse("http://host").unwrap().path, "/");
        assert_eq!(Url::parse("ftp://host"), None);
        assert_eq!(Url::parse("http://"), None);
        assert_eq!(Url::parse("http://host:x/"), None);
    }

    #[test]
    fn requests() {
        let url = Url::parse("http://h.org/p?q").unwrap();
        let headers = [
            (String::from("Accept"), String::from("*/*")),
            (String::from("Host"), String::from("evil")),
            (String::from("X-Bad"), String::from("a\r\nInjected: 1")),
        ];
        let head = String::from_utf8(request("POST", &url, &headers, Some(3))).unwrap();
        assert_eq!(
            head,
            "POST /p?q HTTP/1.1\r\nHost: h.org\r\nConnection: close\r\nAccept: */*\r\n\
             User-Agent: QuireInk\r\nContent-Length: 3\r\n\r\n"
        );
    }

    #[test]
    fn redirects() {
        let base = "http://h.org/a/b?x";
        assert_eq!(redirect(base, "https://o.org/").unwrap(), "https://o.org/");
        assert_eq!(redirect(base, "/c").unwrap(), "http://h.org/c");
        assert_eq!(redirect(base, "c?y").unwrap(), "http://h.org/a/c?y");
        assert_eq!(redirect(base, "//o.org/d").unwrap(), "http://o.org/d");
    }

    #[test]
    fn bodies_by_length_in_any_pieces() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nETag: \"e\"\r\n\
                         Content-Length: 5\r\n\r\nhelloEXTRA";
        for step in [1, 2, 7, 100] {
            let (head, body, done) = run("GET", response, step, false);
            assert_eq!(head.status, 200);
            assert_eq!(head.content_type.as_deref(), Some("text/plain"));
            assert_eq!(head.etag.as_deref(), Some("\"e\""));
            assert_eq!(body, b"hello");
            assert!(done);
        }
    }

    #[test]
    fn chunked_bodies() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n\
                         4;ext=1\r\nWiki\r\n5\r\npedia\r\n0\r\nTrailer: x\r\n\r\n";
        for step in [1, 3, 11, 1000] {
            let (head, body, done) = run("GET", response, step, false);
            assert!(head.chunked);
            assert_eq!(body, b"Wikipedia");
            assert!(done);
        }
    }

    #[test]
    fn bodies_to_the_end_and_cut_ones() {
        let (_, body, ok) = run("GET", b"HTTP/1.0 200 OK\r\n\r\nall of it", 4, true);
        assert_eq!(body, b"all of it");
        assert!(ok);
        let (_, _, ok) = run("GET", b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\npart", 4, true);
        assert!(!ok);
    }

    #[test]
    fn no_bodies() {
        let (head, body, done) = run("HEAD", b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n", 5, false);
        assert_eq!((head.status, body.len(), done), (200, 0, true));
        let (head, _, done) = run("GET", b"HTTP/1.1 304 Not Modified\r\n\r\n", 5, false);
        assert_eq!((head.status, done), (304, true));
        let (head, body, done) = run(
            "GET",
            b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 302 Found\r\nLocation: /x\r\nContent-Length: 0\r\n\r\n",
            3,
            false,
        );
        assert!(head.is_redirect());
        assert_eq!((head.location.as_deref(), body.len(), done), (Some("/x"), 0, true));
    }

    #[test]
    fn malformed_heads() {
        let mut parser = Parser::new("GET");
        assert_eq!(
            parser.feed(b"SSH-2.0 hello\r\n\r\n", &mut |_| {}),
            Err(Error::Malformed)
        );
        let mut parser = Parser::new("GET");
        let long = alloc::vec![b'a'; MAX_HEAD + 10];
        assert_eq!(parser.feed(&long, &mut |_| {}), Err(Error::Malformed));
    }
}
