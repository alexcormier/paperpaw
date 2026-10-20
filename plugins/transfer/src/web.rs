//! The requests of the web page: the page itself, and listing,
//! downloading, uploading, making folders and deleting.
//!
//! Every request carries the key shown on the device (`k=`); without it
//! the answer is *403*, and after a few wrong keys *429* for a while (see
//! [`access`](crate::access)). Paths are relative to the top of the
//! storage and found part by part in the folders' listings ([`find`]):
//! hidden ones (e.g. PaperPaw's own data) are out of reach, by any
//! spelling.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::files;
use paperpaw_sdk::{Instant, ServerResponse, Storage, StorageError};

use crate::access::{Access, Guard};

/// What a request changed, to tell on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Received(String),
    Sent(String),
    Folder(String),
    Deleted(String),
    /// An upload that failed: said on the device too, so the last success
    /// is not taken for it.
    NotReceived(String),
}

/// A request to answer.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub number: u32,
    pub method: &'a str,
    pub target: &'a str,
    /// Where its body was saved, if it had one.
    pub body_file: Option<&'a str>,
    /// When it came.
    pub at: Instant,
}

/// Answers `request`, if `guard` lets it in; `page` is the web page to
/// serve.
pub fn answer(
    storage: &dyn Storage,
    guard: &mut Guard,
    page: &str,
    request: Request<'_>,
) -> (ServerResponse, Option<Done>) {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target, ""));
    let number = request.number;
    let text = |status, body: &str| {
        (
            ServerResponse::new(number, status, "text/plain; charset=utf-8", body.into()),
            None,
        )
    };
    if path == "/favicon.ico" {
        return text(404, "not found");
    }
    match guard.check(param(query, "k").as_deref(), request.at) {
        Access::Granted => {}
        Access::Denied => {
            return text(
                403,
                "The key is missing or wrong: open the address shown on the device.",
            );
        }
        Access::Locked => {
            return text(429, "Too many wrong keys: try again in a minute.");
        }
    }
    let wanted = param(query, "path").unwrap_or_default();
    let refused = |refused: Refused| match refused {
        Refused::Bad => text(400, "bad path"),
        Refused::NotFound => text(404, "not found"),
    };
    let failed = |e: StorageError| {
        let status = match e {
            StorageError::NotFound => 404,
            StorageError::InvalidPath => 400,
            _ => 500,
        };
        text(status, &format!("{e:?}"))
    };
    let ok = |done: Option<Done>| {
        (
            ServerResponse::new(number, 200, "application/json", b"{\"ok\":true}".to_vec()),
            done,
        )
    };
    match (request.method, path) {
        ("GET", "/") => (
            ServerResponse::new(
                number,
                200,
                "text/html; charset=utf-8",
                page.as_bytes().to_vec(),
            ),
            None,
        ),
        ("GET", "/api/list") => {
            let dir = match find(storage, &wanted, false) {
                Ok(Found {
                    path,
                    is_dir: Some(true),
                }) => path,
                Ok(_) => return text(404, "not found"),
                Err(e) => return refused(e),
            };
            match files::list(storage, &dir, false) {
                Ok(entries) => {
                    let mut json = String::from("[");
                    for (i, entry) in entries.iter().enumerate() {
                        if i > 0 {
                            json.push(',');
                        }
                        json.push_str(&format!(
                            "{{\"name\":{},\"dir\":{},\"size\":{}}}",
                            json_string(&entry.name),
                            entry.is_dir,
                            entry.size
                        ));
                    }
                    json.push(']');
                    (
                        ServerResponse::new(number, 200, "application/json", json.into_bytes()),
                        None,
                    )
                }
                Err(e) => failed(e),
            }
        }
        ("GET", "/api/file") => {
            let file = match find(storage, &wanted, false) {
                Ok(Found {
                    path,
                    is_dir: Some(false),
                }) => path,
                Ok(_) => return text(404, "not found"),
                Err(e) => return refused(e),
            };
            let mut response =
                ServerResponse::new(number, 200, "application/octet-stream", Vec::new());
            response.body_from = Some(file.clone());
            (response, Some(Done::Sent(files::name(&file).to_string())))
        }
        ("PUT", "/api/file") => {
            let name = files::name(wanted.trim_matches('/')).to_string();
            let not_received = |(response, _): (ServerResponse, Option<Done>)| {
                (response, Some(Done::NotReceived(name.clone())))
            };
            let target = match find(storage, &wanted, true) {
                Ok(found) if !found.path.is_empty() => found.path,
                Ok(_) => return not_received(text(400, "bad path")),
                Err(Refused::NotFound) => {
                    return not_received(text(400, "bad path: the folder is not there"));
                }
                Err(e) => return not_received(refused(e)),
            };
            let at = files::free_path(storage, files::parent(&target), files::name(&target));
            let result = match request.body_file {
                Some(body) => files::move_to(storage, body, &at),
                None => storage.write(&at, &[]),
            };
            match result {
                Ok(()) => ok(Some(Done::Received(files::name(&at).to_string()))),
                Err(e) => not_received(failed(e)),
            }
        }
        ("POST", "/api/mkdir") => {
            let target = match find(storage, &wanted, true) {
                Ok(found) if !found.path.is_empty() => found.path,
                Ok(_) => return text(400, "bad path"),
                Err(e) => return refused(e),
            };
            let at = files::free_path(storage, files::parent(&target), files::name(&target));
            match storage.create_dir(&at) {
                Ok(()) => ok(Some(Done::Folder(files::name(&at).to_string()))),
                Err(e) => failed(e),
            }
        }
        ("POST", "/api/delete") => {
            let target = match find(storage, &wanted, false) {
                // Not everything at once.
                Ok(found) if found.path.is_empty() => return text(404, "not found"),
                Ok(found) => found.path,
                Err(e) => return refused(e),
            };
            match files::delete(storage, &target) {
                Ok(()) => ok(Some(Done::Deleted(files::name(&target).to_string()))),
                Err(e) => failed(e),
            }
        }
        _ => text(404, "not found"),
    }
}

/// Where a request's path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The path, as the storage lists its parts; `""` for the top.
    pub path: String,
    /// Whether a folder is there; `None` where nothing is yet.
    pub is_dir: Option<bool>,
}

/// Why a request's path is not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Not a path a request may name (*400*).
    Bad,
    /// Not there (*404*).
    NotFound,
}

/// Finds `path`, from a request, part by part: each part is a plain name
/// ([`files::is_plain_name`]: nothing hidden, no `..`, no dot or space at
/// the end) of an entry its folder lists by exactly that name, a folder
/// but for the last. With `new`, the last may also be a name not there
/// yet, where the storage finds nothing by it either. So nothing is
/// reached by another spelling a storage takes for it: another case, a
/// short name (`PAPERP~1` for `.paperpaw`), a dot at the end.
pub fn find(storage: &dyn Storage, path: &str, new: bool) -> Result<Found, Refused> {
    let path = path.trim_matches('/');
    let mut found = Found {
        path: String::new(),
        is_dir: Some(true),
    };
    if path.is_empty() {
        return Ok(found);
    }
    let path = files::plain_path(path).ok_or(Refused::Bad)?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        if found.is_dir != Some(true) {
            return Err(Refused::NotFound);
        }
        let mut listed = None;
        storage
            .list(&found.path, &mut |entry| {
                if entry.name == part {
                    listed = Some(entry.is_dir);
                }
            })
            .map_err(|_| Refused::NotFound)?;
        let here = files::join(&found.path, part);
        found = match listed {
            Some(is_dir) => Found {
                path: here,
                is_dir: Some(is_dir),
            },
            None if new && parts.peek().is_none() => {
                if storage.exists(&here) || files::is_dir(storage, &here) {
                    return Err(Refused::Bad);
                }
                Found {
                    path: here,
                    is_dir: None,
                }
            }
            None => return Err(Refused::NotFound),
        };
    }
    Ok(found)
}

/// Parameter `name` of a query (`a=1&b=x%20y`), decoded.
pub fn param(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| decode(value))
}

/// Decodes `%XX` and `+` of a query value.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| Some((hex(*bytes.get(i + 1)?)?, hex(*bytes.get(i + 2)?)?)))
            .flatten();
        match (bytes[i], escaped) {
            (_, Some((high, low))) => {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (byte, _) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `text` as a JSON string.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
