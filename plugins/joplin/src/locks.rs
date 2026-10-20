//! Joplin's sync locks: a client upgrading a sync target to a new
//! version of its format holds an exclusive lock on it meanwhile, and
//! nothing is to be written then. PaperPaw looks for one before it
//! writes, and waits while one is held.
//!
//! Locks are kept as files `locks/<type>_<client type>_<client id>.json`
//! in a WebDAV folder (type 2 is exclusive), and listed at `api/locks` by
//! Joplin Server, each with when it was renewed (`updatedTime`); one not
//! renewed for 3 minutes has run out (`defaultLockTtl` in Joplin's
//! `LockHandler`). The times are the server's, compared with this
//! device's clock.
//!
//! PaperPaw takes no shared sync lock of its own, as Joplin's clients
//! once did while syncing: Joplin no longer uses them. Since Joplin 3.1
//! they could be switched off (issue laurent22/joplin#10407, "Disable
//! sync lock support"), and in Joplin 3.7.1 `LockHandler` takes and reads
//! none at all (`enabled_ = false`; its `acquireLock`, `locks` and
//! `releaseLock` return at once), neither when syncing nor when
//! upgrading. Only older clients still lock, so looking for their
//! exclusive locks is kept; a shared lock would cost two more requests
//! with every sending, radio time on small devices, for no one to read.

use alloc::string::String;

use paperpaw_json::stream::{self, Event as Json};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::{HttpRequest, Storage, files};
use paperpaw_xml::{Token, Tokenizer, local_name};

use crate::service::{self, Auth, Service};

/// How long a lock holds unless renewed, in ms.
pub const TTL: i64 = 3 * 60 * 1000;
/// The type of an exclusive lock.
const EXCLUSIVE: &str = "2";
/// Longest `<href>` or `<getlastmodified>` kept from a WebDAV listing, in
/// bytes; the rest of a longer one is left out (no lock has such a name).
const MAX_FIELD: usize = 1024;
/// What a WebDAV folder is asked for: when each file changed.
const PROPFIND: &str = "<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\">\
    <d:prop><d:getlastmodified/></d:prop></d:propfind>";

/// The request listing the locks at `base`, its answer saved to the file
/// `save_to`.
pub fn request(
    service: Service,
    base: &str,
    auth: Auth<'_>,
    save_to: String,
    reply: &Event<HttpReply>,
) -> HttpRequest {
    let url = service.locks_url(base);
    let mut request = match service {
        Service::WebDav => {
            let mut request = service::request(&url, "PROPFIND", auth, reply);
            request
                .headers
                .push((String::from("Depth"), String::from("1")));
            request.headers.push((
                String::from("Content-Type"),
                String::from("application/xml"),
            ));
            request.body = Some(String::from(PROPFIND));
            request
        }
        _ => service::request(&url, "GET", auth, reply),
    };
    request.save_to = Some(save_to);
    request
}

/// Whether the list of locks in the file `path` (the answer to
/// [`request`]) holds an exclusive lock still held at `now` (ms since
/// 1970): a client upgrading the target.
pub fn upgrading(service: Service, storage: &dyn Storage, path: &str, now: i64) -> bool {
    let held = |renewed: Option<i64>| renewed.is_some_and(|at| now - at < TTL);
    match service {
        Service::WebDav => webdav_upgrading(storage, path, &held),
        _ => server_upgrading(storage, path, &held),
    }
}

/// Joplin Server's list: `{"items": [{"type": 2, "updatedTime": …}]}`.
fn server_upgrading(storage: &dyn Storage, path: &str, held: &dyn Fn(Option<i64>) -> bool) -> bool {
    let (mut found, mut exclusive, mut renewed) = (false, false, None);
    stream::read_file(storage, path, &mut |event| match event {
        Json::Open([items, item]) if items == "items" && item == "[]" => {
            (exclusive, renewed) = (false, None);
        }
        Json::Value([items, item, key], value) if items == "items" && item == "[]" => {
            match key.as_str() {
                "type" => exclusive = value == EXCLUSIVE,
                "updatedTime" => renewed = value.parse().ok(),
                _ => {}
            }
        }
        Json::Close([items, item]) if items == "items" && item == "[]" => {
            found |= exclusive && held(renewed);
        }
        _ => {}
    });
    found
}

/// A WebDAV listing of the folder `locks`: a `<response>` per file, its
/// `<href>` and `<getlastmodified>`.
fn webdav_upgrading(storage: &dyn Storage, path: &str, held: &dyn Fn(Option<i64>) -> bool) -> bool {
    let (mut found, mut href, mut modified) = (false, String::new(), String::new());
    let mut field: Option<bool> = None;
    let mut tokenizer = Tokenizer::new();
    let mut emit = |token: Token<'_>| match token {
        Token::Start {
            name, self_closing, ..
        } => match local_name(name) {
            "response" => {
                href.clear();
                modified.clear();
            }
            "href" if !self_closing => field = Some(true),
            "getlastmodified" if !self_closing => field = Some(false),
            _ => {}
        },
        Token::Text(text) => match field {
            Some(true) => push_bounded(&mut href, &paperpaw_xml::decode(&text)),
            Some(false) => push_bounded(&mut modified, &paperpaw_xml::decode(&text)),
            None => {}
        },
        Token::End { name } => match local_name(name) {
            "href" | "getlastmodified" => field = None,
            "response" => {
                let file = href.trim().trim_end_matches('/');
                let file = file.rsplit('/').next().unwrap_or_default();
                let exclusive = file.starts_with(&alloc::format!("{EXCLUSIVE}_"))
                    && files::has_extension(file, "json");
                let renewed = paperpaw_sdk::web::http_date(&modified).map(|s| s * 1000);
                found |= exclusive && held(renewed);
            }
            _ => {}
        },
        _ => {}
    };
    let mut piece = [0u8; 512];
    let mut at = 0u64;
    while let Ok(n) = storage.read_at(path, at, &mut piece) {
        if n == 0 {
            break;
        }
        tokenizer.feed(&piece[..n], &mut emit);
        at += n as u64;
    }
    tokenizer.finish(&mut emit);
    found
}

/// Adds `text` to `field`, as much as fits in [`MAX_FIELD`] bytes.
fn push_bounded(field: &mut String, text: &str) {
    let room = MAX_FIELD.saturating_sub(field.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    field.push_str(&text[..end]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// 2026-10-20 08:00:00 UTC.
    const NOW: i64 = 1_792_483_200_000;

    fn listing(responses: &[(&str, &str)]) -> String {
        let mut xml = String::from(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
             <d:response><d:href>/dav/Joplin/locks/</d:href>\
             <d:propstat><d:prop><d:getlastmodified>Tue, 20 Oct 2026 07:59:00 GMT\
             </d:getlastmodified></d:prop></d:propstat></d:response>",
        );
        for (name, modified) in responses {
            xml.push_str(&alloc::format!(
                "<d:response><d:href>/dav/Joplin/locks/{name}</d:href><d:propstat><d:prop>\
                 <d:getlastmodified>{modified}</d:getlastmodified></d:prop></d:propstat>\
                 </d:response>"
            ));
        }
        xml.push_str("</d:multistatus>");
        xml
    }

    fn webdav(xml: &str) -> bool {
        let storage = MemoryStorage::new().with("locks.xml", xml);
        upgrading(Service::WebDav, &storage, "locks.xml", NOW)
    }

    #[test]
    fn exclusive_locks_in_a_webdav_folder() {
        let fresh = "Tue, 20 Oct 2026 07:58:30 GMT";
        let old = "Tue, 20 Oct 2026 07:56:30 GMT";
        assert!(webdav(&listing(&[("2_1_abc.json", fresh)])));
        // Run out, shared, or no lock.
        assert!(!webdav(&listing(&[("2_1_abc.json", old)])));
        assert!(!webdav(&listing(&[("1_1_abc.json", fresh)])));
        assert!(!webdav(&listing(&[("2_1_abc.txt", fresh)])));
        // Named in upper case, as a FAT or Windows server may give it.
        assert!(webdav(&listing(&[("2_1_ABC.JSON", fresh)])));
        assert!(!webdav(&listing(&[])));
        assert!(!webdav(""));
        // Among others.
        assert!(webdav(&listing(&[
            ("1_2_m.json", fresh),
            ("2_3_cli.json", fresh)
        ])));
    }

    #[test]
    fn long_listing_fields_are_cut() {
        let fresh = "Tue, 20 Oct 2026 07:58:30 GMT";
        let long = "x".repeat(64 * 1024);
        let mut href = String::new();
        push_bounded(&mut href, &long);
        push_bounded(&mut href, "y");
        assert_eq!(href.len(), MAX_FIELD);
        // Cut between the bytes of a character: the character is left out.
        let mut text = "x".repeat(MAX_FIELD - 1);
        push_bounded(&mut text, "\u{e4}");
        assert_eq!(text.len(), MAX_FIELD - 1);
        // A huge name and date do not hide the lock that follows.
        let xml = listing(&[(long.as_str(), long.as_str()), ("2_1_abc.json", fresh)]);
        assert!(webdav(&xml));
    }

    fn server(json: &str) -> bool {
        let storage = MemoryStorage::new().with("locks.json", json);
        upgrading(Service::Server, &storage, "locks.json", NOW)
    }

    #[test]
    fn exclusive_locks_of_joplin_server() {
        let lock = |kind: u8, ago: i64| {
            alloc::format!(
                "{{\"id\":\"x\",\"type\":{kind},\"clientType\":1,\"clientId\":\"c\",\
                 \"updatedTime\":{}}}",
                NOW - ago
            )
        };
        let list = |locks: &[String]| {
            alloc::format!("{{\"items\":[{}],\"has_more\":false}}", locks.join(","))
        };
        assert!(server(&list(&[lock(2, 30_000)])));
        assert!(!server(&list(&[lock(2, TTL + 1)])));
        assert!(!server(&list(&[lock(1, 30_000)])));
        assert!(server(&list(&[lock(1, 30_000), lock(2, 60_000)])));
        assert!(!server(&list(&[])));
        assert!(!server("not json"));
    }
}
