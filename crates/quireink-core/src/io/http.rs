//! HTTP requests that plugins ask for and shells make ([`Io::Http`],
//! answered with `Kernel::http_response`).
//!
//! [`Io::Http`]: super::Io::Http

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use quireink_platform::{Storage, StorageError};
use quireink_sdk::event::EventData;
use quireink_sdk::{Command, HttpReply, HttpRequest, Id, Params};

/// Longest response body passed on to plugins.
pub const MAX_RESPONSE: usize = 1 << 20;
/// Longest response body saved to a file, and longest file sent.
pub const MAX_FILE: usize = 64 << 20;

/// A request for the shell to make, e.g. on a background thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    /// Passed back with the response.
    pub id: u32,
    /// `GET`, `PUT`, `POST`, …
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// A file in storage to send as the body instead of `body` (e.g. a
    /// database to upload): the shell reads it, in parts where it can, so
    /// it never needs to fit in memory.
    pub body_from: Option<String>,
    /// The longest response body wanted; shells cut longer ones.
    pub max_response: usize,
    /// The file in storage the body is for, if it is one (e.g. a
    /// download). A shell may write it there itself as it comes, for one
    /// too large for memory, and say so with [`HttpResponse::saved`];
    /// otherwise the kernel writes the body it gets.
    pub save_to: Option<String>,
}

impl HttpCall {
    /// Reads [`body_from`](Self::body_from) into `body`, for shells with
    /// memory to spare (others send the file in parts).
    pub fn load_body(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        if let Some(path) = self.body_from.take() {
            let size = storage.size(&path)?;
            let mut body = alloc::vec![0; usize::try_from(size).unwrap_or(usize::MAX)];
            let mut at = 0;
            while at < body.len() {
                let n = storage.read_at(&path, at as u64, &mut body[at..])?;
                if n == 0 {
                    break;
                }
                at += n;
            }
            body.truncate(at);
            self.body = Some(body);
        }
        Ok(())
    }
}

/// What came back. `status` 0 means the request failed (no network, no
/// such host, timeout); `body` then holds a short reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    pub id: u32,
    pub status: u16,
    pub body: Vec<u8>,
    /// The `ETag` header, if any.
    pub etag: Option<String>,
    /// The `Content-Type` header, if any.
    pub content_type: Option<String>,
    /// Where the answer came from after redirects, if elsewhere.
    pub url: Option<String>,
    /// The body was written to [`HttpCall::save_to`] by the shell (and
    /// is not in `body`).
    pub saved: bool,
}

impl HttpResponse {
    /// The answer of a request that failed (status 0), and why.
    pub fn failed(id: u32, reason: &str) -> Self {
        Self {
            id,
            status: 0,
            body: reason.as_bytes().to_vec(),
            ..Self::default()
        }
    }
}

/// A request made and not answered yet.
#[derive(Debug)]
struct Awaiting {
    id: u32,
    reply: Id,
    context: Params,
    save_to: Option<String>,
}

/// Plugins' requests: those for the shell, and those it has not answered.
#[derive(Debug, Default)]
pub(crate) struct Http {
    calls: Vec<HttpCall>,
    awaiting: Vec<Awaiting>,
    next: u32,
}

impl Http {
    /// Queues a plugin's request for the shell, or fails it at once
    /// (without a network, or a file to send that is not there).
    pub fn request(
        &mut self,
        request: HttpRequest,
        network: bool,
        storage: &dyn Storage,
        out: &mut Vec<Command>,
    ) {
        if !network {
            return reply(
                request.reply,
                HttpReply::failed("no network", request.params),
                out,
            );
        }
        // A file to send must be there; the shell reads it.
        if let Some(path) = &request.body_from
            && let Err(e) = storage.size(path)
        {
            let failed = HttpReply::failed(format!("{e}"), request.params);
            return reply(request.reply, failed, out);
        }
        let body = match request.body_from {
            Some(_) => None,
            None => request.body.map(String::into_bytes),
        };
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        let max_response = if request.save_to.is_some() {
            MAX_FILE
        } else {
            MAX_RESPONSE
        };
        self.calls.push(HttpCall {
            id,
            method: request.method,
            url: request.url,
            headers: request.headers,
            body,
            body_from: request.body_from,
            max_response,
            save_to: request.save_to.clone(),
        });
        self.awaiting.push(Awaiting {
            id,
            reply: request.reply,
            context: request.params,
            save_to: request.save_to,
        });
    }

    /// Turns a request's answer into the plugin's event; a body for a
    /// file is written there.
    pub fn answer(
        &mut self,
        response: HttpResponse,
        storage: &dyn Storage,
        out: &mut Vec<Command>,
    ) {
        let Some(at) = self.awaiting.iter().position(|a| a.id == response.id) else {
            return;
        };
        let awaiting = self.awaiting.remove(at);
        let mut status = response.status;
        let body = match awaiting.save_to {
            // Answers that are files: written where asked (a failure has
            // its reason as the body).
            Some(path) if response.status != 0 => {
                if !response.saved && storage.write(&path, &response.body).is_err() {
                    status = 0;
                }
                String::new()
            }
            _ => String::from_utf8_lossy(&response.body).into_owned(),
        };
        let answer = HttpReply {
            status,
            body,
            etag: response.etag,
            content_type: response.content_type,
            final_url: response.url,
            context: awaiting.context,
        };
        reply(awaiting.reply, answer, out);
    }

    /// The requests for the shell.
    pub fn take(&mut self) -> Vec<HttpCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether requests wait for the shell (for a network).
    pub fn waiting(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Whether requests are not answered yet.
    pub fn busy(&self) -> bool {
        !self.awaiting.is_empty()
    }

    /// Fails the requests still waiting for the shell, for `reason`.
    pub fn fail_waiting(&mut self, reason: &str, storage: &dyn Storage, out: &mut Vec<Command>) {
        for call in self.take() {
            self.answer(HttpResponse::failed(call.id, reason), storage, out);
        }
    }
}

fn reply(event: Id, answer: HttpReply, out: &mut Vec<Command>) {
    out.push(Command::Emit(event, answer.to_params()));
}
