//! Plugins' HTTP requests (the kernel's `HttpCall`s), made over the
//! network of `net` without stopping the X4: each one moves on a step at a
//! time from the main loop (finding the host, connecting, sending,
//! receiving), the protocol by `quireink_http`. A body meant for a file
//! goes there as it comes (by way of a `.part` file, renamed once whole),
//! so a large download never needs to fit in memory.
//!
//! Plain `http://` only, for now.

use alloc::string::String;
use alloc::vec::Vec;

use quireink_core::{HttpCall, HttpResponse};
use quireink_http::{Parser, Url};
use quireink_platform::Storage;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::{dns, tcp};

use crate::net::{self, Net, Resolve};

/// How long a request waits for the network (e.g. while a network is
/// joined after a start), and at most without any progress.
const OFFLINE_MS: u64 = 20_000;
const STALL_MS: u64 = 20_000;
/// Redirects followed at most.
const MAX_REDIRECTS: u32 = 10;
/// Bytes gathered before they are appended to a file: few, larger writes
/// to the card.
const FILE_BUF: usize = 4096;

fn now_ms() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis()
}

/// Where a body goes.
enum Sink {
    Memory(Vec<u8>),
    File {
        /// The `.part` file written.
        part: String,
        buf: Vec<u8>,
        started: bool,
    },
}

/// A file sent as the body, read from storage in parts as it goes.
struct Upload {
    path: String,
    size: u64,
    /// Bytes sent so far.
    at: u64,
}

/// Bytes of a file read for sending at a time.
const UPLOAD_PART: usize = 1024;

enum Phase {
    /// Waiting for the network, or for a free connection.
    Waiting,
    Resolving(dns::QueryHandle, u16),
    Exchanging {
        socket: SocketHandle,
        /// The head and body to send, and how much went.
        request: Vec<u8>,
        sent: usize,
        /// A file sent after them.
        upload: Option<Upload>,
        parser: Parser,
    },
}

struct Call {
    call: HttpCall,
    /// The address asked now (after redirects).
    url: String,
    redirects: u32,
    phase: Phase,
    sink: Sink,
    /// Bytes of the body so far.
    got: u64,
    /// Until when it may wait for the network, or for progress.
    until: u64,
}

/// The requests going on.
#[derive(Default)]
pub struct Calls {
    calls: Vec<Call>,
}

impl Calls {
    pub fn add(&mut self, call: HttpCall) {
        let sink = match &call.save_to {
            Some(path) => Sink::File {
                part: alloc::format!("{path}.part"),
                buf: Vec::new(),
                started: false,
            },
            None => Sink::Memory(Vec::new()),
        };
        self.calls.push(Call {
            url: call.url.clone(),
            call,
            redirects: 0,
            phase: Phase::Waiting,
            sink,
            got: 0,
            until: now_ms() + OFFLINE_MS,
        });
    }

    /// Requests are going on.
    pub fn busy(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Moves every request on; the answers of those done. `net` is `None`
    /// while the radio is off.
    pub fn poll(&mut self, mut net: Option<&mut Net>, storage: &dyn Storage) -> Vec<HttpResponse> {
        let mut answers = Vec::new();
        let mut at = 0;
        while at < self.calls.len() {
            let open = self
                .calls
                .iter()
                .filter(|c| !matches!(c.phase, Phase::Waiting))
                .count();
            let call = &mut self.calls[at];
            let outcome = match net.as_deref_mut() {
                None => Some(Err("no network")),
                Some(net) => call.step(net, storage, open < net::MAX_CALLS),
            };
            match outcome {
                None => at += 1,
                Some(result) => {
                    let mut call = self.calls.remove(at);
                    if let Some(net) = net.as_deref_mut() {
                        call.stop(net);
                    }
                    answers.push(match result {
                        Ok(answer) => call.finish(answer, storage),
                        Err(reason) => {
                            call.discard(storage);
                            HttpResponse::failed(call.call.id, reason)
                        }
                    });
                }
            }
        }
        answers
    }
}

impl Call {
    /// One step on; the outcome once there is one.
    fn step(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        may_open: bool,
    ) -> Option<Result<HttpResponse, &'static str>> {
        let now = now_ms();
        match &mut self.phase {
            Phase::Waiting => {
                if net.address().is_none() || !may_open {
                    return (now > self.until).then_some(Err("no network"));
                }
                let Some(url) = Url::parse(&self.url) else {
                    return Some(Err("not an address"));
                };
                if url.https {
                    return Some(Err("HTTPS is not supported on this device yet"));
                }
                let port = url.port;
                match net.resolve(url.host) {
                    Ok(Resolve::Done(address)) => self.connect(net, storage, address, port),
                    Ok(Resolve::Asking(query)) => {
                        self.phase = Phase::Resolving(query, port);
                        self.until = now + STALL_MS;
                        None
                    }
                    Err(_) => Some(Err("no such host")),
                }
            }
            Phase::Resolving(query, port) => {
                let (query, port) = (*query, *port);
                let resolved = net.resolved(query);
                if resolved.is_some() {
                    // The lookup is over (smoltcp freed it): nothing to stop.
                    self.phase = Phase::Waiting;
                }
                match resolved {
                    None => (now > self.until).then_some(Err("no such host")),
                    Some(Ok(address)) => self.connect(net, storage, address, port),
                    Some(Err(_)) => Some(Err("no such host")),
                }
            }
            Phase::Exchanging {
                socket,
                request,
                sent,
                upload,
                parser,
            } => {
                let socket = net.socket(*socket);
                let mut progress = false;
                if *sent < request.len() && socket.can_send() {
                    match socket.send_slice(&request[*sent..]) {
                        Ok(n) => {
                            *sent += n;
                            progress |= n > 0;
                        }
                        Err(_) => return Some(Err("connection lost")),
                    }
                }
                // Then the file, a part at a time, as the socket takes it.
                while *sent == request.len()
                    && let Some(upload) = upload.as_mut()
                    && upload.at < upload.size
                    && socket.can_send()
                {
                    let mut part = [0u8; UPLOAD_PART];
                    let want = (upload.size - upload.at).min(UPLOAD_PART as u64) as usize;
                    let Ok(read) = storage.read_at(&upload.path, upload.at, &mut part[..want])
                    else {
                        return Some(Err("could not read the file"));
                    };
                    if read == 0 {
                        return Some(Err("could not read the file"));
                    }
                    match socket.send_slice(&part[..read]) {
                        Ok(0) => break,
                        Ok(n) => {
                            upload.at += n as u64;
                            progress = true;
                        }
                        Err(_) => return Some(Err("connection lost")),
                    }
                }
                let mut problem = None;
                // Body bytes that come with the head, held until the head
                // tells whether they are wanted (not a redirect's).
                let mut early = Vec::new();
                while socket.can_recv() && problem.is_none() && !parser.done() {
                    let received = socket.recv(|data| {
                        let had_head = parser.head().is_some();
                        let sink = &mut self.sink;
                        let got = &mut self.got;
                        let max = self.call.max_response as u64;
                        let fed = parser.feed(data, &mut |piece| {
                            if had_head {
                                if let Err(e) = keep(sink, got, max, piece, storage) {
                                    problem.get_or_insert(e);
                                }
                            } else {
                                early.extend_from_slice(piece);
                            }
                        });
                        if fed.is_err() {
                            problem.get_or_insert("not an HTTP answer");
                        }
                        (data.len(), ())
                    });
                    if received.is_err() {
                        problem.get_or_insert("connection lost");
                    }
                    progress = true;
                    if let Some(head) = parser.head()
                        && head.is_redirect()
                    {
                        break;
                    }
                    if !early.is_empty() {
                        let max = self.call.max_response as u64;
                        if let Err(e) = keep(&mut self.sink, &mut self.got, max, &early, storage) {
                            problem.get_or_insert(e);
                        }
                        early.clear();
                    }
                }
                if let Some(problem) = problem {
                    return Some(Err(problem));
                }
                if let Some(head) = parser.head()
                    && head.is_redirect()
                {
                    return self.redirect(net);
                }
                let state = net.socket(match &self.phase {
                    Phase::Exchanging { socket, .. } => *socket,
                    _ => return None,
                });
                let ended = matches!(
                    state.state(),
                    tcp::State::CloseWait | tcp::State::Closed | tcp::State::TimeWait
                );
                let Phase::Exchanging {
                    parser,
                    sent,
                    request,
                    upload,
                    ..
                } = &mut self.phase
                else {
                    return None;
                };
                let all_sent =
                    *sent == request.len() && upload.as_ref().is_none_or(|u| u.at == u.size);
                if parser.done() || (ended && parser.close().is_ok() && all_sent) {
                    let head = parser.head().cloned().unwrap_or_default();
                    return Some(Ok(HttpResponse {
                        id: self.call.id,
                        status: head.status,
                        etag: head.etag,
                        content_type: head.content_type,
                        url: (self.url != self.call.url).then(|| self.url.clone()),
                        ..HttpResponse::default()
                    }));
                }
                if ended {
                    return Some(Err("connection lost"));
                }
                if progress {
                    self.until = now + STALL_MS;
                    None
                } else {
                    (now > self.until).then_some(Err("timed out"))
                }
            }
        }
    }

    /// Opens the connection and sends the request.
    fn connect(
        &mut self,
        net: &mut Net,
        storage: &dyn Storage,
        address: smoltcp::wire::IpAddress,
        port: u16,
    ) -> Option<Result<HttpResponse, &'static str>> {
        let Some(url) = Url::parse(&self.url) else {
            return Some(Err("not an address"));
        };
        let upload = match &self.call.body_from {
            Some(path) => match storage.size(path) {
                Ok(size) => Some(Upload {
                    path: path.clone(),
                    size,
                    at: 0,
                }),
                Err(_) => return Some(Err("could not read the file")),
            },
            None => None,
        };
        let body = self.call.body.as_deref();
        let body_len = match &upload {
            Some(upload) => Some(upload.size as usize),
            None => body.map(<[u8]>::len),
        };
        let mut request =
            quireink_http::request(&self.call.method, &url, &self.call.headers, body_len);
        request.extend_from_slice(body.unwrap_or_default());
        let Ok(socket) = net.open((address, port)) else {
            return Some(Err("could not connect"));
        };
        self.phase = Phase::Exchanging {
            socket,
            request,
            sent: 0,
            upload,
            parser: Parser::new(&self.call.method),
        };
        self.until = now_ms() + STALL_MS;
        None
    }

    /// Follows a redirect: the same request (a `GET` without body after a
    /// 303, or a 301 or 302 of a `POST`) to where it leads.
    fn redirect(&mut self, net: &mut Net) -> Option<Result<HttpResponse, &'static str>> {
        let Phase::Exchanging { parser, .. } = &self.phase else {
            return None;
        };
        let head = parser.head().cloned().unwrap_or_default();
        self.redirects += 1;
        if self.redirects > MAX_REDIRECTS {
            return Some(Err("too many redirects"));
        }
        let Some(to) = head
            .location
            .as_deref()
            .and_then(|location| quireink_http::redirect(&self.url, location))
        else {
            return Some(Err("not an address"));
        };
        if head.status == 303
            || (matches!(head.status, 301 | 302) && self.call.method.eq_ignore_ascii_case("POST"))
        {
            self.call.method = String::from("GET");
            self.call.body = None;
            self.call.body_from = None;
        }
        self.stop(net);
        self.url = to;
        self.phase = Phase::Waiting;
        self.until = now_ms() + OFFLINE_MS;
        None
    }

    /// Lets the connection or lookup go.
    fn stop(&mut self, net: &mut Net) {
        match core::mem::replace(&mut self.phase, Phase::Waiting) {
            Phase::Waiting => {}
            Phase::Resolving(query, _) => net.forget(query),
            Phase::Exchanging { socket, .. } => net.release(socket),
        }
    }

    /// The answer of a request done: the body in memory, or the file
    /// made whole.
    fn finish(self, mut answer: HttpResponse, storage: &dyn Storage) -> HttpResponse {
        match self.sink {
            Sink::Memory(body) => answer.body = body,
            Sink::File { part, buf, started } => {
                let path = self.call.save_to.as_deref().unwrap_or_default();
                let written = if started {
                    storage.append(&part, &buf)
                } else {
                    storage.write(&part, &buf)
                };
                let _ = storage.remove(path);
                if written.and_then(|()| storage.rename(&part, path)).is_err() {
                    let _ = storage.remove(&part);
                    return HttpResponse::failed(self.call.id, "could not write the file");
                }
                answer.saved = true;
            }
        }
        answer
    }

    /// Removes what a failed request wrote.
    fn discard(&self, storage: &dyn Storage) {
        if let Sink::File {
            part,
            started: true,
            ..
        } = &self.sink
        {
            let _ = storage.remove(part);
        }
    }
}

/// Takes a piece of the body: into memory (cut at `max`) or towards the
/// file (failing past `max`).
fn keep(
    sink: &mut Sink,
    got: &mut u64,
    max: u64,
    piece: &[u8],
    storage: &dyn Storage,
) -> Result<(), &'static str> {
    *got += piece.len() as u64;
    match sink {
        Sink::Memory(body) => {
            let room = (max as usize).saturating_sub(body.len());
            body.extend_from_slice(&piece[..piece.len().min(room)]);
        }
        Sink::File { part, buf, started } => {
            if *got > max {
                return Err("too large");
            }
            buf.extend_from_slice(piece);
            if buf.len() >= FILE_BUF {
                let written = if *started {
                    storage.append(part, buf)
                } else {
                    storage.write(part, buf)
                };
                written.map_err(|_| "could not write the file")?;
                *started = true;
                buf.clear();
            }
        }
    }
    Ok(())
}
