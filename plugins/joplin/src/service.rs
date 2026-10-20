//! Where Joplin keeps the notes: its sync target, the same files for
//! each of them (see [`item`](crate::item)), reached in two ways:
//!
//! - Joplin Cloud and Joplin Server: signed in with the e-mail and
//!   password (`POST /api/sessions`, its `id` then in `X-API-AUTH`), the
//!   files under `/api/items/root:/<name>:/content`.
//! - WebDAV (e.g. Nextcloud): the folder Joplin syncs to, with the user
//!   name and password on every request.
//!
//! Other ways to Joplin (another sync target, or the desktop app's Web
//! Clipper service) would be another [`Service`].

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::web::basic_auth;
use paperpaw_sdk::{Env, HttpRequest};

use crate::{ADDRESS, SERVICE};

/// Joplin Cloud's address.
pub const CLOUD: &str = "https://api.joplincloud.com";

/// The kinds of Joplin sync targets reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Cloud,
    Server,
    WebDav,
}

impl Service {
    /// All of them, in the order offered.
    pub const ALL: [Service; 3] = [Service::Cloud, Service::Server, Service::WebDav];
    /// The setting's values (and their translation keys).
    pub const KEYS: [&'static str; 3] = [
        "joplin.service.cloud",
        "joplin.service.server",
        "joplin.service.webdav",
    ];

    /// The setting's value for it.
    pub fn key(self) -> &'static str {
        match self {
            Service::Cloud => Self::KEYS[0],
            Service::Server => Self::KEYS[1],
            Service::WebDav => Self::KEYS[2],
        }
    }

    /// The service chosen.
    pub fn of(env: &Env<'_>) -> Service {
        let key = env.settings.read(&SERVICE);
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or(Service::Cloud)
    }

    /// The one after it, for choosing by going through them.
    pub fn next(self) -> Service {
        match self {
            Service::Cloud => Service::Server,
            Service::Server => Service::WebDav,
            Service::WebDav => Service::Cloud,
        }
    }

    /// Whether it is reached at an address the user gives.
    pub fn has_address(self) -> bool {
        self != Service::Cloud
    }

    /// Whether it signs in first, for a session.
    pub fn signs_in(self) -> bool {
        self != Service::WebDav
    }

    /// Its address: `https://` unless another scheme is given, no `/` at
    /// the end; empty if none is set.
    pub fn base(self, env: &Env<'_>) -> String {
        if self == Service::Cloud {
            return String::from(CLOUD);
        }
        paperpaw_sdk::web::base_url(env.settings.read(&ADDRESS))
    }

    /// The address of the file `name` (e.g. `info.json`) at `base`.
    pub fn url(self, base: &str, name: &str) -> String {
        match self {
            Service::WebDav => format!("{base}/{name}"),
            _ => format!("{base}/api/items/root:/{name}:/content"),
        }
    }

    /// The address of the sync locks at `base` (see the module `locks`).
    pub fn locks_url(self, base: &str) -> String {
        match self {
            Service::WebDav => format!("{base}/locks/"),
            _ => format!("{base}/api/locks"),
        }
    }
}

/// The request signing in to Joplin Cloud or Joplin Server at `base`.
pub fn sign_in(base: &str, email: &str, password: &str, reply: &Event<HttpReply>) -> HttpRequest {
    let mut request = HttpRequest::new("POST", &format!("{base}/api/sessions"), reply);
    request.headers.push((
        String::from("Content-Type"),
        String::from("application/json"),
    ));
    request.body = Some(format!(
        "{{\"email\":{},\"password\":{}}}",
        paperpaw_json::string(email),
        paperpaw_json::string(password)
    ));
    request
}

/// How a request proves who sends it: the session where there is one,
/// else the user name and password.
#[derive(Debug, Clone, Copy)]
pub enum Auth<'a> {
    Session(&'a str),
    Basic { user: &'a str, password: &'a str },
}

/// A request of `method` for `url` (see [`Service::url`]), answered as
/// `reply`.
pub fn request(url: &str, method: &str, auth: Auth<'_>, reply: &Event<HttpReply>) -> HttpRequest {
    let mut request = HttpRequest::new(method, url, reply);
    let header = match auth {
        Auth::Session(id) => (String::from("X-API-AUTH"), id.to_string()),
        Auth::Basic { user, password } => {
            (String::from("Authorization"), basic_auth(user, password))
        }
    };
    request.headers.push(header);
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: Event<HttpReply> = Event::new("joplin.test");

    #[test]
    fn addresses_of_files() {
        assert_eq!(
            Service::Cloud.url(CLOUD, "info.json"),
            "https://api.joplincloud.com/api/items/root:/info.json:/content"
        );
        assert_eq!(
            Service::WebDav.url(
                "https://cloud.example/remote.php/dav/files/me/Joplin",
                "a.md"
            ),
            "https://cloud.example/remote.php/dav/files/me/Joplin/a.md"
        );
        assert_eq!(
            Service::Server.locks_url("https://j"),
            "https://j/api/locks"
        );
        assert_eq!(
            Service::WebDav.locks_url("https://d/J"),
            "https://d/J/locks/"
        );
        let mut s = Service::Cloud;
        for _ in Service::ALL {
            s = s.next();
        }
        assert_eq!(s, Service::Cloud);
    }

    #[test]
    fn signs_in_with_json_and_proves_it() {
        let r = sign_in("https://joplin.example", "me@example.org", "p\"w", &REPLY);
        assert_eq!(r.url, "https://joplin.example/api/sessions");
        assert_eq!(
            r.body.as_deref(),
            Some("{\"email\":\"me@example.org\",\"password\":\"p\\\"w\"}")
        );
        let r = request("https://j/x", "GET", Auth::Session("S"), &REPLY);
        assert!(
            r.headers
                .contains(&(String::from("X-API-AUTH"), String::from("S")))
        );
        let basic = Auth::Basic {
            user: "me",
            password: "pw",
        };
        let r = request("https://d/x.md", "PUT", basic, &REPLY);
        assert_eq!(r.headers[0].0, "Authorization");
        assert!(r.headers[0].1.starts_with("Basic "));
    }
}
