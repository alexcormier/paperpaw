//! Plugins' HTTP servers over loopback sockets: what reaches the kernel as
//! `ServerEvent`s, what clients get back for `ServerCall::Respond` (bodies
//! and files sent in parts), refused and dropped requests (chunked
//! uploads, broken heads, oversized heads, clients that leave), several
//! connections at once, and the choice of port. Every server listens on a
//! free port (port 0) or one found free just before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use paperpaw_core::{ServedFile, ServerCall, ServerEvent};
use paperpaw_http::Parser;
use paperpaw_http::server::MAX_REQUEST_HEAD;
use paperpaw_serve::Servers;

/// The next event, waiting up to five seconds.
fn wait(servers: &Servers) -> ServerEvent {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(event) = servers.next() {
            return event;
        }
        assert!(Instant::now() < until, "no event");
        thread::sleep(Duration::from_millis(2));
    }
}

/// Asserts that no event comes for a while.
fn quiet(servers: &Servers, millis: u64) {
    let until = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < until {
        if let Some(event) = servers.next() {
            panic!("unexpected {event:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// The next event within `millis`, if any.
fn servers_next_within(servers: &Servers, millis: u64) -> Option<ServerEvent> {
    let until = Instant::now() + Duration::from_millis(millis);
    while Instant::now() < until {
        if let Some(event) = servers.next() {
            return Some(event);
        }
        thread::sleep(Duration::from_millis(5));
    }
    None
}

/// Starts server 1 on a free port; its port.
fn start(servers: &mut Servers) -> u16 {
    servers.apply(ServerCall::Listen { server: 1, port: 0 });
    match wait(servers) {
        ServerEvent::Started { server: 1, address } => {
            address.rsplit(':').next().unwrap().parse().unwrap()
        }
        other => panic!("not started: {other:?}"),
    }
}

/// A client that sends `request` (then stops sending) and reads all that
/// comes back until the server closes.
fn client(port: u16, request: Vec<u8>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        // A server that drops the connection early may make writing fail.
        let _ = stream.write_all(&request);
        let _ = stream.shutdown(Shutdown::Write);
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
        answer
    })
}

/// The status, head text and body of an answer, read with the client's
/// own parser; also whether it came whole.
fn read_answer(answer: &[u8]) -> (u16, String, Vec<u8>, bool) {
    let split = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("an answer head")
        + 4;
    let mut parser = Parser::new("GET");
    let mut body = Vec::new();
    parser
        .feed(answer, &mut |b| body.extend_from_slice(b))
        .unwrap();
    let whole = parser.close().is_ok();
    let status = parser.head().unwrap().status;
    let head = String::from_utf8(answer[..split].to_vec()).unwrap();
    (status, head, body, whole)
}

fn respond(servers: &mut Servers, request: u32, status: u16, body: &[u8]) {
    servers.apply(ServerCall::Respond {
        request,
        status,
        content_type: String::from("text/plain"),
        headers: Vec::new(),
        body: body.to_vec(),
        file: None,
    });
}

#[test]
fn port_zero_listens_on_a_free_port() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    assert!(!servers.any());
    let port = start(&mut servers);
    assert_ne!(port, 0);
    assert!(servers.any());
    TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    servers.apply(ServerCall::Stop { server: 1 });
    assert!(!servers.any());
}

#[test]
fn a_taken_port_moves_on_to_a_free_one_near_it() {
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port();
    if port > u16::MAX - 20 {
        return; // no room above it to move to
    }
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    servers.apply(ServerCall::Listen { server: 7, port });
    let ServerEvent::Started { server: 7, address } = wait(&servers) else {
        panic!("not started");
    };
    let got: u16 = address.rsplit(':').next().unwrap().parse().unwrap();
    assert!(got > port && got < port + 20, "{got} near {port}");
    servers.apply(ServerCall::Stop { server: 7 });
}

#[test]
fn the_highest_port_can_be_listened_on() {
    match TcpListener::bind((Ipv4Addr::LOCALHOST, u16::MAX)) {
        Ok(probe) => drop(probe),
        Err(_) => return, // taken by something else here
    }
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    servers.apply(ServerCall::Listen {
        server: 1,
        port: u16::MAX,
    });
    let event = wait(&servers);
    assert!(
        matches!(&event, ServerEvent::Started { address, .. } if address.ends_with(":65535")),
        "{event:?}"
    );
    servers.apply(ServerCall::Stop { server: 1 });
}

#[test]
fn a_request_reaches_the_kernel_and_its_answer_the_client() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(
        port,
        b"GET /index.html?lang=en HTTP/1.1\r\nHost: x\r\n\r\n".to_vec(),
    );
    let ServerEvent::Request {
        server,
        request,
        method,
        target,
        body,
    } = wait(&servers)
    else {
        panic!("no request");
    };
    assert_eq!(
        (server, method.as_str(), target.as_str(), body),
        (1, "GET", "/index.html?lang=en", false)
    );
    servers.apply(ServerCall::Respond {
        request,
        status: 404,
        content_type: String::from("text/html; charset=utf-8"),
        headers: vec![
            (String::from("X-Plugin"), String::from("transfer")),
            (String::from("X-Evil"), String::from("1\r\nSet-Cookie: a=b")),
        ],
        body: b"<p>gone</p>".to_vec(),
        file: None,
    });
    let (status, head, body, whole) = read_answer(&client.join().unwrap());
    assert_eq!(status, 404);
    assert!(whole);
    assert_eq!(body, b"<p>gone</p>");
    assert!(head.contains("\r\nContent-Type: text/html; charset=utf-8\r\n"));
    assert!(head.contains("\r\nX-Plugin: transfer\r\n"));
    assert!(!head.contains("Set-Cookie"), "{head}");
    quiet(&servers, 50);
}

#[test]
fn targets_reach_the_kernel_as_sent() {
    // Decoding and keeping requests inside a folder is up to the plugin;
    // the server must not decode or "fix" the target on its way.
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    for target in [
        "/%2e%2e/%2e%2e/secret",
        "/../../etc/passwd",
        "/a%00b",
        "/files?path=..%2F..%2Fx",
    ] {
        let client = client(port, format!("GET {target} HTTP/1.1\r\n\r\n").into_bytes());
        let ServerEvent::Request {
            request,
            target: got,
            ..
        } = wait(&servers)
        else {
            panic!("no request");
        };
        assert_eq!(got, target);
        respond(&mut servers, request, 403, b"");
        assert_eq!(read_answer(&client.join().unwrap()).0, 403);
    }
}

#[test]
fn bodies_come_in_parts_in_order() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let up: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let mut request = format!(
        "POST /api/upload?name=a.epub HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        up.len()
    )
    .into_bytes();
    request.extend_from_slice(&up);
    let client = client(port, request);
    let ServerEvent::Request {
        request: id, body, ..
    } = wait(&servers)
    else {
        panic!("no request");
    };
    assert!(body);
    let mut received = Vec::new();
    loop {
        let ServerEvent::Body {
            request,
            data,
            last,
        } = wait(&servers)
        else {
            panic!("no body");
        };
        assert_eq!(request, id);
        assert!(!data.is_empty());
        received.extend(data);
        if last {
            break;
        }
    }
    assert_eq!(received, up);
    respond(&mut servers, id, 201, b"saved");
    let (status, _, body, _) = read_answer(&client.join().unwrap());
    assert_eq!((status, body.as_slice()), (201, &b"saved"[..]));
}

#[test]
fn chunked_uploads_are_refused_without_the_kernel() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(
        port,
        b"POST /up HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec(),
    );
    let (status, _, body, _) = read_answer(&client.join().unwrap());
    assert_eq!(status, 411);
    assert!(body.is_empty());
    quiet(&servers, 100);
}

#[test]
fn a_client_leaving_mid_body_closes_the_request() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(
        port,
        b"PUT /f HTTP/1.1\r\nContent-Length: 1000\r\n\r\nonly a little".to_vec(),
    );
    let ServerEvent::Request { request, body, .. } = wait(&servers) else {
        panic!("no request");
    };
    assert!(body);
    // What came with the head is passed on, never as the last part; then
    // the request is closed.
    let mut received = Vec::new();
    loop {
        match wait(&servers) {
            ServerEvent::Body {
                request: of,
                data,
                last: false,
            } if of == request => received.extend(data),
            ServerEvent::Closed { request: of } if of == request => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(b"only a little".starts_with(&received), "{received:?}");
    assert!(client.join().unwrap().is_empty(), "no answer");
    // An answer coming late goes nowhere.
    respond(&mut servers, request, 200, b"late");
    quiet(&servers, 50);
}

#[test]
fn answers_to_unknown_requests_are_ignored() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    servers.apply(ServerCall::Respond {
        request: 12345,
        status: 200,
        content_type: String::from("application/octet-stream"),
        headers: Vec::new(),
        body: Vec::new(),
        file: Some(ServedFile {
            path: String::from("books/a.epub"),
            size: 10,
        }),
    });
    assert!(!servers.any(), "no file is read for nobody");
    servers.pump(|_, _, _| panic!("nothing to read"));
}

#[test]
fn request_lines_without_a_target_are_refused_without_the_kernel() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    for bad in [&b"GET\r\n\r\n"[..], b"\r\n\r\n"] {
        let answer = client(port, bad.to_vec()).join().unwrap();
        assert_eq!(
            read_answer(&answer).0,
            400,
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    // Nothing at all is no request: the connection is dropped.
    assert!(client(port, Vec::new()).join().unwrap().is_empty());
    quiet(&servers, 100);
}

#[test]
fn heads_cut_before_their_end_are_dropped() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let answer = client(port, b"GET / HTTP/1.1\r\nHost: x\r\n".to_vec())
        .join()
        .unwrap();
    assert!(answer.is_empty());
    quiet(&servers, 100);
}

#[test]
fn heads_with_bare_line_feeds_never_reach_the_kernel() {
    // The head is read by `paperpaw_http`'s parser, as on every shell,
    // which takes only CR LF line ends: this head never ends.
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(port, b"PUT /lf HTTP/1.1\nContent-Length: 2\n\nok".to_vec());
    assert!(client.join().unwrap().is_empty());
    quiet(&servers, 100);
}

#[test]
fn heads_beyond_the_limit_never_reach_the_kernel() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let mut request = b"GET / HTTP/1.1\r\n".to_vec();
    for i in 0..4000 {
        request.extend_from_slice(format!("X-Header-{i}: some value here\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    assert!(request.len() > MAX_REQUEST_HEAD);
    let answer = client(port, request).join().unwrap();
    // Answered 431; the server closes with the rest of the head unread,
    // so a reset may take the answer away from the client.
    assert!(
        answer.is_empty() || answer.starts_with(b"HTTP/1.1 431 "),
        "{:?}",
        String::from_utf8_lossy(&answer)
    );
    quiet(&servers, 100);
}

#[test]
fn request_lines_count_towards_the_head_limit() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let target = format!("/{}", "a".repeat(256 * 1024));
    let client = client(port, format!("GET {target} HTTP/1.1\r\n\r\n").into_bytes());
    let event = servers_next_within(&servers, 2000);
    if let Some(ServerEvent::Request { request, .. }) = &event {
        respond(&mut servers, *request, 200, b"");
    }
    let _ = client.join();
    assert!(
        !matches!(event, Some(ServerEvent::Request { .. })),
        "a 256 KiB request line reached the kernel"
    );
}

#[test]
fn content_lengths_that_are_no_numbers_are_refused() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    for bad in ["-5", "abc", "18446744073709551616"] {
        let client = client(
            port,
            format!("PUT /books/a.epub HTTP/1.1\r\nContent-Length: {bad}\r\n\r\nsome book")
                .into_bytes(),
        );
        let event = servers_next_within(&servers, 1000);
        if let Some(ServerEvent::Request { request, .. }) = &event {
            respond(&mut servers, *request, 201, b"");
        }
        let _ = client.join();
        assert!(
            !matches!(event, Some(ServerEvent::Request { .. })),
            "{bad:?}: the upload reached the plugin as an empty one"
        );
    }
}

#[test]
fn several_connections_at_once_get_their_own_answers() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let clients: Vec<_> = (0..6)
        .map(|i| client(port, format!("GET /n/{i} HTTP/1.1\r\n\r\n").into_bytes()))
        .collect();
    let mut requests = Vec::new();
    while requests.len() < clients.len() {
        let ServerEvent::Request {
            request, target, ..
        } = wait(&servers)
        else {
            panic!("no request");
        };
        requests.push((request, target));
    }
    let mut ids: Vec<u32> = requests.iter().map(|r| r.0).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), clients.len(), "each request has its own number");
    // Answered in the other order: each still gets its own.
    for (request, target) in requests.iter().rev() {
        respond(&mut servers, *request, 200, target.as_bytes());
    }
    for (i, client) in clients.into_iter().enumerate() {
        let (_, _, body, whole) = read_answer(&client.join().unwrap());
        assert!(whole);
        assert_eq!(body, format!("/n/{i}").as_bytes());
    }
}

#[test]
fn files_are_sent_in_parts_as_the_shell_reads_them() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let file: Vec<u8> = (0..300_001u32).map(|i| (i * 13 % 256) as u8).collect();
    let client = client(port, b"GET /books/big.epub HTTP/1.1\r\n\r\n".to_vec());
    let ServerEvent::Request { request, .. } = wait(&servers) else {
        panic!("no request");
    };
    servers.apply(ServerCall::Respond {
        request,
        status: 200,
        content_type: String::from("application/epub+zip"),
        headers: Vec::new(),
        body: Vec::new(),
        file: Some(ServedFile {
            path: String::from("books/big.epub"),
            size: file.len() as u64,
        }),
    });
    let mut asked = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while !client.is_finished() {
        assert!(Instant::now() < until, "the file is not sent");
        servers.pump(|path, at, buffer| {
            assert_eq!(path, "books/big.epub");
            asked.push(at);
            let at = at as usize;
            let n = buffer.len().min(file.len() - at);
            buffer[..n].copy_from_slice(&file[at..at + n]);
            Some(n)
        });
        thread::sleep(Duration::from_millis(2));
    }
    let (status, head, body, whole) = read_answer(&client.join().unwrap());
    assert_eq!(status, 200);
    assert!(head.contains(&format!("Content-Length: {}", file.len())));
    assert!(whole);
    assert!(body == file, "the file arrives whole");
    assert!(asked.windows(2).all(|w| w[0] < w[1]), "read front to back");
}

#[test]
fn a_file_gone_while_sending_ends_the_answer_early() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(port, b"GET /f HTTP/1.1\r\n\r\n".to_vec());
    let ServerEvent::Request { request, .. } = wait(&servers) else {
        panic!("no request");
    };
    servers.apply(ServerCall::Respond {
        request,
        status: 200,
        content_type: String::from("application/octet-stream"),
        headers: Vec::new(),
        body: Vec::new(),
        file: Some(ServedFile {
            path: String::from("f"),
            size: 1_000_000,
        }),
    });
    // The first part is there, then the file is gone.
    let mut reads = 0;
    servers.pump(|_, _, buffer| {
        reads += 1;
        if reads > 1 {
            return None;
        }
        buffer.fill(b'x');
        Some(buffer.len())
    });
    assert_eq!(reads, 2);
    // Nothing is read for it any more.
    servers.pump(|_, _, _| panic!("the file is still being sent"));
    let (status, _, body, whole) = read_answer(&client.join().unwrap());
    assert_eq!(status, 200);
    assert!(!whole, "the client sees the answer cut");
    assert!(body.len() < 1_000_000 && body.iter().all(|&b| b == b'x'));
    servers.apply(ServerCall::Stop { server: 1 });
    assert!(!servers.any(), "nothing left to send");
}

#[test]
fn stopping_a_server_ends_the_files_it_sends() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(port, b"GET /f HTTP/1.1\r\n\r\n".to_vec());
    let ServerEvent::Request { request, .. } = wait(&servers) else {
        panic!("no request");
    };
    servers.apply(ServerCall::Respond {
        request,
        status: 200,
        content_type: String::from("application/octet-stream"),
        headers: Vec::new(),
        body: Vec::new(),
        file: Some(ServedFile {
            path: String::from("f"),
            size: 1_000_000,
        }),
    });
    servers.apply(ServerCall::Stop { server: 1 });
    assert!(!servers.any(), "nothing left to send");
    servers.pump(|_, _, _| panic!("a file is read for a stopped server"));
    // The connection ends with the server: at most a head, never the
    // whole answer.
    let answer = client.join().unwrap();
    if answer.windows(4).any(|w| w == b"\r\n\r\n") {
        assert!(!read_answer(&answer).3, "the client sees the answer cut");
    }
}

#[test]
fn an_empty_file_is_an_empty_answer() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    let client = client(port, b"GET /empty HTTP/1.1\r\n\r\n".to_vec());
    let ServerEvent::Request { request, .. } = wait(&servers) else {
        panic!("no request");
    };
    servers.apply(ServerCall::Respond {
        request,
        status: 200,
        content_type: String::from("text/plain"),
        headers: Vec::new(),
        body: Vec::new(),
        file: Some(ServedFile {
            path: String::from("empty"),
            size: 0,
        }),
    });
    servers.pump(|_, _, _| panic!("nothing to read for an empty file"));
    let (status, head, body, whole) = read_answer(&client.join().unwrap());
    assert_eq!(status, 200);
    assert!(head.contains("Content-Length: 0\r\n"));
    assert!(body.is_empty() && whole);
}

#[test]
fn a_stopped_server_takes_no_more_requests() {
    let mut servers = Servers::new(|| {}).listening_on(Ipv4Addr::LOCALHOST.into());
    let port = start(&mut servers);
    servers.apply(ServerCall::Stop { server: 1 });
    // Stopping a server twice, or one never started, is harmless.
    servers.apply(ServerCall::Stop { server: 1 });
    servers.apply(ServerCall::Stop { server: 99 });
    // The listening thread sees the stop within its poll interval.
    thread::sleep(Duration::from_millis(200));
    if let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) {
        let _ = stream.write_all(b"GET / HTTP/1.1\r\n\r\n");
    }
    quiet(&servers, 200);
}

#[test]
fn the_shell_is_woken_for_events() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let woken = Arc::new(AtomicUsize::new(0));
    let mut servers = Servers::new({
        let woken = woken.clone();
        move || {
            woken.fetch_add(1, Ordering::Relaxed);
        }
    });
    let port = start(&mut servers);
    assert!(woken.load(Ordering::Relaxed) >= 1, "for Started");
    let before = woken.load(Ordering::Relaxed);
    let client = client(port, b"GET / HTTP/1.1\r\n\r\n".to_vec());
    let ServerEvent::Request { request, .. } = wait(&servers) else {
        panic!("no request");
    };
    assert!(woken.load(Ordering::Relaxed) > before, "for the request");
    respond(&mut servers, request, 200, b"");
    client.join().unwrap();
}
