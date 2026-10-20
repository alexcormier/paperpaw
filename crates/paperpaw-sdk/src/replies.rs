//! What comes back to a plugin, as typed events: the answer to an HTTP
//! request ([`HttpReply`]), what happens to its server ([`ServerNotice`])
//! or its sockets ([`SocketNotice`]), and what the reader typed or chose
//! when asked ([`Typed`], [`Chosen`], see [`ask`](crate::ask)).
//!
//! A plugin names the event that brings it, e.g.
//! `const FETCHED: Event<HttpReply> = Event::new("news.fetched")`, passes
//! it with the request, and hears it with
//! [`Registrar::on`](crate::Registrar::on) or reads it on a screen with
//! [`Event::read`](crate::event::Event::read). The parameters it gave the
//! request come back as [`HttpReply::context`] (and so on), to tell its
//! requests apart.

use alloc::string::{String, ToString};

use crate::builtin::params;
use crate::command::{http_params, server_params, socket_params};
use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// The parameters of `params` but `known`, as a plugin gave them.
fn context(params: &Params, known: &[&str]) -> Params {
    let mut context = Params::new();
    for (key, value) in params.iter().filter(|(k, _)| !known.contains(k)) {
        context.set(String::from(key), value);
    }
    context
}

fn with_context(mut params: Params, context: &Params) -> Params {
    for (key, value) in context.iter() {
        params.set(String::from(key), value);
    }
    params
}

fn text(params: &Params, key: &str) -> Option<String> {
    params.get(key).map(String::from)
}

/// The answer to an HTTP request ([`Command::Http`](crate::Command::Http)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    /// The status, e.g. 200; 0 if there was no answer (no network, a
    /// refused certificate …), with the reason as the body.
    pub status: u16,
    /// The body as text, unless it was saved to a file
    /// ([`HttpRequest::save_to`](crate::HttpRequest)).
    pub body: String,
    /// The `ETag` header, if there was one.
    pub etag: Option<String>,
    /// The `Content-Type` header, if there was one.
    pub content_type: Option<String>,
    /// Where the answer came from, if it was redirected.
    pub final_url: Option<String>,
    /// The request's own parameters
    /// ([`HttpRequest::params`](crate::HttpRequest)).
    pub context: Params,
    /// Why it was not sent, if it was not (status 0): the power mode, the
    /// reader switched the plugin's network off, too soon again, and when
    /// to try again (see [`work`](crate::work)).
    pub refused: Option<crate::work::Refusal>,
    /// The headers asked for that the answer had
    /// ([`HttpOptions::want_headers`](crate::HttpOptions)), their names in
    /// lower case.
    pub headers: alloc::vec::Vec<(String, String)>,
}

impl HttpReply {
    const KEYS: [&'static str; 7] = [
        http_params::STATUS,
        http_params::BODY,
        http_params::ETAG,
        http_params::CONTENT_TYPE,
        http_params::FINAL_URL,
        crate::work::params::REFUSED,
        crate::work::params::RETRY,
    ];

    /// No answer (status 0), for `reason`.
    pub fn failed(reason: impl Into<String>, context: Params) -> Self {
        Self {
            status: 0,
            body: reason.into(),
            etag: None,
            content_type: None,
            final_url: None,
            context,
            refused: None,
            headers: alloc::vec::Vec::new(),
        }
    }

    /// Not sent, for `refusal` (status 0, its reason as the body).
    pub fn refused(refusal: crate::work::Refusal, context: Params) -> Self {
        Self {
            refused: Some(refusal),
            ..Self::failed(refusal.reason.as_str(), context)
        }
    }

    /// The header `name` of the answer, if it was asked for and came.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Whether the status says it succeeded (2xx).
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl EventData for HttpReply {
    fn to_params(&self) -> Params {
        let mut params = with_context(Params::new(), &self.context)
            .with(http_params::STATUS, self.status.to_string())
            .with(http_params::BODY, self.body.clone());
        for (key, value) in [
            (http_params::ETAG, &self.etag),
            (http_params::CONTENT_TYPE, &self.content_type),
            (http_params::FINAL_URL, &self.final_url),
        ] {
            if let Some(value) = value {
                params.set(key, value.clone());
            }
        }
        if let Some(refusal) = &self.refused {
            refusal.write(&mut params);
        }
        for (name, value) in &self.headers {
            params.set(
                alloc::format!("{}{}", http_params::HEADER, name.to_ascii_lowercase()),
                value.clone(),
            );
        }
        params
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            status: params.get(http_params::STATUS)?.parse().ok()?,
            body: text(params, http_params::BODY).unwrap_or_default(),
            etag: text(params, http_params::ETAG),
            content_type: text(params, http_params::CONTENT_TYPE),
            final_url: text(params, http_params::FINAL_URL),
            context: context(params, &Self::KEYS)
                .iter()
                .filter(|(k, _)| !k.starts_with(http_params::HEADER))
                .fold(Params::new(), |p, (k, v)| p.with(String::from(k), v)),
            refused: crate::work::Refusal::read(params),
            headers: params
                .iter()
                .filter_map(|(k, v)| {
                    let name = k.strip_prefix(http_params::HEADER)?;
                    Some((String::from(name), String::from(v)))
                })
                .collect(),
        })
    }
}

/// What happened to a plugin's server ([`Command::Serve`](crate::Command::Serve)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerNotice {
    /// It serves at `address`, e.g. `192.168.1.20:8080`.
    Started {
        server: Id,
        address: String,
    },
    /// A request came: answer it with
    /// [`Command::Respond`](crate::Command::Respond) and its `request`
    /// number.
    Request {
        server: Id,
        request: u32,
        /// `GET`, `PUT`, …
        method: String,
        /// Its path and query.
        target: String,
        /// Where its body was saved, if it had one; removed once it is
        /// answered, unless moved.
        body_file: Option<String>,
    },
    Failed {
        server: Id,
        reason: String,
    },
    Stopped {
        server: Id,
    },
}

impl ServerNotice {
    /// Which server.
    pub fn server(&self) -> &Id {
        match self {
            ServerNotice::Started { server, .. }
            | ServerNotice::Request { server, .. }
            | ServerNotice::Failed { server, .. }
            | ServerNotice::Stopped { server } => server,
        }
    }
}

impl EventData for ServerNotice {
    fn to_params(&self) -> Params {
        let p = Params::new().with(params::ID, self.server().as_str().to_string());
        match self {
            ServerNotice::Started { address, .. } => p
                .with(server_params::KIND, server_params::STARTED)
                .with(server_params::ADDRESS, address.clone()),
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                let mut p = p
                    .with(server_params::KIND, server_params::REQUEST)
                    .with(server_params::REQUEST, request.to_string())
                    .with(server_params::METHOD, method.clone())
                    .with(server_params::TARGET, target.clone());
                if let Some(file) = body_file {
                    p.set(server_params::BODY_FILE, file.clone());
                }
                p
            }
            ServerNotice::Failed { reason, .. } => p
                .with(server_params::KIND, server_params::FAILED)
                .with(server_params::REASON, reason.clone()),
            ServerNotice::Stopped { .. } => p.with(server_params::KIND, server_params::STOPPED),
        }
    }

    fn from_params(p: &Params) -> Option<Self> {
        let server = Id::parse(p.get(params::ID)?).ok()?;
        Some(match p.get(server_params::KIND)? {
            server_params::STARTED => ServerNotice::Started {
                server,
                address: text(p, server_params::ADDRESS)?,
            },
            server_params::REQUEST => ServerNotice::Request {
                server,
                request: p.get(server_params::REQUEST)?.parse().ok()?,
                method: text(p, server_params::METHOD)?,
                target: text(p, server_params::TARGET)?,
                body_file: text(p, server_params::BODY_FILE),
            },
            server_params::FAILED => ServerNotice::Failed {
                server,
                reason: text(p, server_params::REASON).unwrap_or_default(),
            },
            server_params::STOPPED => ServerNotice::Stopped { server },
            _ => return None,
        })
    }
}

/// What happened to a plugin's socket ([`Command::Connect`](crate::Command::Connect))
/// or broadcast ([`Command::Broadcast`](crate::Command::Broadcast)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketNotice {
    Connected {
        socket: Id,
    },
    /// Text came; empty when the other side closed.
    Received {
        socket: Id,
        text: String,
    },
    /// A file read is complete.
    Saved {
        socket: Id,
        path: String,
    },
    Closed {
        socket: Id,
        reason: String,
    },
    Failed {
        socket: Id,
        reason: String,
    },
    /// An answer to a broadcast, and the address it came from.
    Answer {
        socket: Id,
        text: String,
        from: String,
    },
    /// No more answers to a broadcast will come; `reason` says why it
    /// ended early (e.g. `no network`), empty if it ran its time.
    Done {
        socket: Id,
        reason: String,
    },
}

impl SocketNotice {
    /// Which socket or broadcast.
    pub fn socket(&self) -> &Id {
        match self {
            SocketNotice::Connected { socket }
            | SocketNotice::Received { socket, .. }
            | SocketNotice::Saved { socket, .. }
            | SocketNotice::Closed { socket, .. }
            | SocketNotice::Failed { socket, .. }
            | SocketNotice::Answer { socket, .. }
            | SocketNotice::Done { socket, .. } => socket,
        }
    }
}

impl EventData for SocketNotice {
    fn to_params(&self) -> Params {
        let p = Params::new().with(params::ID, self.socket().as_str().to_string());
        let kind = |p: Params, kind: &'static str| p.with(socket_params::KIND, kind);
        match self {
            SocketNotice::Connected { .. } => kind(p, socket_params::CONNECTED),
            SocketNotice::Received { text, .. } => {
                kind(p, socket_params::RECEIVED).with(socket_params::TEXT, text.clone())
            }
            SocketNotice::Saved { path, .. } => {
                kind(p, socket_params::SAVED).with(socket_params::PATH, path.clone())
            }
            SocketNotice::Closed { reason, .. } => {
                kind(p, socket_params::CLOSED).with(socket_params::REASON, reason.clone())
            }
            SocketNotice::Failed { reason, .. } => {
                kind(p, socket_params::FAILED).with(socket_params::REASON, reason.clone())
            }
            SocketNotice::Answer { text, from, .. } => kind(p, socket_params::ANSWER)
                .with(socket_params::TEXT, text.clone())
                .with(socket_params::FROM, from.clone()),
            SocketNotice::Done { reason, .. } => {
                let p = kind(p, socket_params::DONE);
                if reason.is_empty() {
                    p
                } else {
                    p.with(socket_params::REASON, reason.clone())
                }
            }
        }
    }

    fn from_params(p: &Params) -> Option<Self> {
        let socket = Id::parse(p.get(params::ID)?).ok()?;
        let get = |key| text(p, key).unwrap_or_default();
        Some(match p.get(socket_params::KIND)? {
            socket_params::CONNECTED => SocketNotice::Connected { socket },
            socket_params::RECEIVED => SocketNotice::Received {
                socket,
                text: get(socket_params::TEXT),
            },
            socket_params::SAVED => SocketNotice::Saved {
                socket,
                path: get(socket_params::PATH),
            },
            socket_params::CLOSED => SocketNotice::Closed {
                socket,
                reason: get(socket_params::REASON),
            },
            socket_params::FAILED => SocketNotice::Failed {
                socket,
                reason: get(socket_params::REASON),
            },
            socket_params::ANSWER => SocketNotice::Answer {
                socket,
                text: get(socket_params::TEXT),
                from: get(socket_params::FROM),
            },
            socket_params::DONE => SocketNotice::Done {
                socket,
                reason: get(socket_params::REASON),
            },
            _ => return None,
        })
    }
}

/// What the reader typed when asked ([`ask::text`](crate::ask::text)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Typed {
    pub text: String,
    /// The parameters the question was asked with.
    pub context: Params,
}

impl EventData for Typed {
    fn to_params(&self) -> Params {
        with_context(Params::new(), &self.context).with(params::TEXT, self.text.clone())
    }

    fn from_params(p: &Params) -> Option<Self> {
        Some(Self {
            text: text(p, params::TEXT)?,
            context: context(p, &[params::TEXT]),
        })
    }
}

/// The folder the reader chose when asked ([`ask::folder`](crate::ask::folder)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub path: String,
    /// The parameters the question was asked with.
    pub context: Params,
}

impl EventData for Chosen {
    fn to_params(&self) -> Params {
        with_context(Params::new(), &self.context).with(params::PATH, self.path.clone())
    }

    fn from_params(p: &Params) -> Option<Self> {
        Some(Self {
            path: text(p, params::PATH)?,
            context: context(p, &[params::PATH]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_through_params_and_back() {
        let reply = HttpReply {
            status: 200,
            body: "{}".into(),
            etag: Some("x1".into()),
            content_type: None,
            final_url: None,
            context: Params::new().with("run", "4"),
            refused: None,
            headers: alloc::vec![(String::from("last-modified"), String::from("Tue"))],
        };
        let back = HttpReply::from_params(&reply.to_params()).unwrap();
        assert_eq!(back, reply);
        assert!(back.ok());
        assert_eq!(back.header("Last-Modified"), Some("Tue"));
        let refusal = crate::work::Refusal::new(
            crate::work::Reason::Offline,
            crate::work::Retry::WhenAllowed,
        );
        let refused = HttpReply::refused(refusal, Params::new().with("run", "5"));
        let back = HttpReply::from_params(&refused.to_params()).unwrap();
        assert_eq!(back, refused);
        assert_eq!(back.status, 0);
        assert_eq!(back.context, Params::new().with("run", "5"));
        let request = ServerNotice::Request {
            server: Id::new_static("a.server"),
            request: 7,
            method: "PUT".into(),
            target: "/f?k=1".into(),
            body_file: None,
        };
        assert_eq!(
            ServerNotice::from_params(&request.to_params()),
            Some(request)
        );
        let answer = SocketNotice::Answer {
            socket: Id::new_static("a.find"),
            text: "hello".into(),
            from: "192.168.1.2".into(),
        };
        assert_eq!(SocketNotice::from_params(&answer.to_params()), Some(answer));
        let typed = Typed {
            text: "secret".into(),
            context: Params::new().with("field", "user"),
        };
        assert_eq!(Typed::from_params(&typed.to_params()), Some(typed));
    }
}
