//! HTTP for the emulator: each request runs the system's `curl` on a
//! thread of its own (Windows, macOS and Linux ship it), so the emulator
//! needs no TLS library. A shell may route requests elsewhere instead
//! ([`set_route`]): the browser's emulator answers them from the page's
//! recorded answers, or has the page fetch them.

use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
// `curl` on a thread, where there are processes and threads.
#[cfg(not(target_arch = "wasm32"))]
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
};

use paperpaw_core::sdk::HttpError;
use paperpaw_core::{HttpCall, HttpResponse};

/// Where requests go instead of the system's network ([`set_route`]).
pub trait Route {
    /// Takes a request: its answer now, or `None` where it comes later
    /// ([`Route::later`]).
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse>;
    /// An answer that came later than its request (a delayed or fetched
    /// one), if one is ready: the route's own, so that answers reach the
    /// device that asked.
    fn later(&mut self) -> Option<HttpResponse>;
}

thread_local! {
    /// The route set, the thread's (a device runs on one thread).
    static ROUTE: RefCell<Option<Box<dyn Route>>> = const { RefCell::new(None) };
}

/// Sends this thread's requests to `route` (`None`: the system's network
/// again, where there is one).
pub fn set_route(route: Option<Box<dyn Route>>) {
    ROUTE.with_borrow_mut(|set| *set = route);
}

/// Whether this thread's requests go to a route ([`set_route`]).
pub fn routed() -> bool {
    ROUTE.with_borrow(Option::is_some)
}

/// How much of an answer's body came: the request's id, the bytes so
/// far, and in all where the answer said.
pub type Progress = (u32, u64, Option<u64>);

/// Requests in flight and their answers.
#[derive(Debug)]
pub struct Http {
    sender: Sender<HttpResponse>,
    receiver: Receiver<HttpResponse>,
    /// How far the answers that ask for it have come.
    progress: (Sender<Progress>, Receiver<Progress>),
    pub in_flight: usize,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            receiver,
            progress: channel(),
            in_flight: 0,
        }
    }

    /// Starts a request: on the route set, else on the system's network.
    pub fn start(&mut self, call: HttpCall) {
        self.in_flight += 1;
        let mut call = Some(call);
        // Taken by the route, if one is set: its answer now, or later.
        let routed = ROUTE.with_borrow_mut(|route| Some(route.as_mut()?.start(call.take()?)));
        match (routed, call) {
            (Some(Some(answer)), _) => {
                let _ = self.sender.send(answer);
            }
            (Some(None), _) => {}
            (None, Some(call)) => self.system(call),
            (None, None) => {}
        }
    }

    /// Runs a request with `curl` on a thread of its own.
    #[cfg(not(target_arch = "wasm32"))]
    fn system(&mut self, call: HttpCall) {
        let sender = self.sender.clone();
        let progress = call.progress.then(|| self.progress.0.clone());
        thread::spawn(move || {
            let _ = sender.send(run(call, progress.as_ref()));
        });
    }

    /// Answers a request at once as failed: in a browser the emulator
    /// has no network of its own.
    #[cfg(target_arch = "wasm32")]
    fn system(&mut self, call: HttpCall) {
        let reason = "no network in the browser's emulator";
        let failed = HttpResponse::failed(call.id, HttpError::NoNetwork, reason);
        let _ = self.sender.send(failed);
    }

    /// How far an answer that asks for it has come since the last call,
    /// if it did.
    pub fn next_progress(&mut self) -> Option<Progress> {
        self.progress.1.try_recv().ok()
    }

    /// An answer that has arrived, if any; waits up to `wait`.
    pub fn next(&mut self, wait: Option<Duration>) -> Option<HttpResponse> {
        // In a browser nothing is waited for: there is no clock to wait
        // by, and answers come at once (see `start`).
        #[cfg(target_arch = "wasm32")]
        let wait: Option<Duration> = wait.and(None);
        let later = ROUTE.with_borrow_mut(|route| route.as_mut()?.later());
        let response = later.or_else(|| match wait {
            Some(wait) => self.receiver.recv_timeout(wait).ok(),
            None => self.receiver.try_recv().ok(),
        });
        if response.is_some() {
            self.in_flight -= 1;
        }
        response
    }
}

/// Makes the request with `curl`; status 0 with the reason if it fails.
#[cfg(not(target_arch = "wasm32"))]
fn run(call: HttpCall, progress: Option<&Sender<Progress>>) -> HttpResponse {
    let failed = |error, reason: String| HttpResponse::failed(call.id, error, &reason);
    let mut command = Command::new("curl");
    let seconds = call.timeout_ms.div_ceil(1000).max(1).to_string();
    // The status, the ETag, the content type, the final address and the
    // headers asked for, a line each after the body.
    let mut written =
        String::from("\n%{http_code}\n%header{etag}\n%{content_type}\n%{url_effective}");
    for name in &call.want_headers {
        written.push_str(&format!("\n%header{{{name}}}"));
    }
    command
        .args(["-sS", "-L", "--max-redirs", "10", "--max-time", &seconds])
        .args(["-X", &call.method])
        .args(["-w", &written]);
    for (name, value) in &call.headers {
        command.args(["-H", &format!("{name}: {value}")]);
    }
    if call.body.is_some() {
        command.args(["--data-binary", "@-"]);
    }
    // Where its progress is told: the headers in a file of their own,
    // read for the size while the body comes.
    let headers = progress.map(|_| {
        std::env::temp_dir().join(format!(
            "paperpaw-emu-{}-{}.headers",
            std::process::id(),
            call.id
        ))
    });
    if let Some(path) = &headers {
        command.arg("-D").arg(path);
    }
    command
        .arg(&call.url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for each request.
        command.creation_flags(0x0800_0000);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return failed(HttpError::Io, format!("curl: {e}")),
    };
    if let (Some(body), Some(mut stdin)) = (&call.body, child.stdin.take()) {
        let _ = stdin.write_all(body);
    }
    let output = match (progress, headers, child.stdout.take()) {
        (Some(progress), Some(headers), Some(stdout)) => {
            let counted = counting(child, stdout, (call.id, &headers), progress);
            let _ = std::fs::remove_file(&headers);
            counted
        }
        (_, _, stdout) => {
            child.stdout = stdout;
            child.wait_with_output()
        }
    };
    let output = match output {
        Ok(output) => output,
        Err(e) => return failed(HttpError::Io, format!("curl: {e}")),
    };
    if !output.status.success() {
        let error = curl_error(output.status.code());
        return failed(
            error,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        );
    }
    answer(&call, output.stdout)
}

/// What `curl` writes, read as it comes: how much came (with the size
/// from the headers it wrote to `headers`, once there) told to
/// `progress` after each piece.
#[cfg(not(target_arch = "wasm32"))]
fn counting(
    child: std::process::Child,
    mut stdout: std::process::ChildStdout,
    (id, headers): (u32, &std::path::Path),
    progress: &Sender<Progress>,
) -> std::io::Result<std::process::Output> {
    use std::io::Read as _;
    let mut body = Vec::new();
    let mut piece = vec![0; 16 << 10];
    let mut total = None;
    loop {
        let n = stdout.read(&mut piece)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&piece[..n]);
        if total.is_none() {
            total = std::fs::read_to_string(headers)
                .ok()
                .and_then(|text| content_length(&text));
        }
        let _ = progress.send((id, body.len() as u64, total));
    }
    let mut output = child.wait_with_output()?;
    output.stdout = body;
    Ok(output)
}

/// The size of the body that the last answer of `headers` names (after
/// redirects, each answer's headers follow the one before).
#[cfg(not(target_arch = "wasm32"))]
fn content_length(headers: &str) -> Option<u64> {
    let last = headers.rsplit("HTTP/").next()?;
    last.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })
}

/// The answer to `call` from what `curl` wrote: the body, then lines with
/// the status, the ETag, the content type, the final address and the
/// headers asked for (`-w`), read from the end.
#[cfg(not(target_arch = "wasm32"))]
fn answer(call: &HttpCall, mut out: Vec<u8>) -> HttpResponse {
    let mut line = || {
        let at = out.iter().rposition(|&b| b == b'\n').unwrap_or(0);
        let text = String::from_utf8_lossy(&out[at..]).trim().to_string();
        out.truncate(at);
        (!text.is_empty()).then_some(text)
    };
    let mut headers = Vec::new();
    for name in call.want_headers.iter().rev() {
        if let Some(value) = line() {
            headers.push((name.to_ascii_lowercase(), value));
        }
    }
    headers.reverse();
    let url = line().filter(|url| *url != call.url);
    let content_type = line();
    let etag = line();
    let status = line().and_then(|s| s.parse().ok()).unwrap_or(0);
    if out.len() > call.max_response && call.save_to.is_some() {
        // A file cut short would be saved as if whole.
        let reason = format!("the answer is larger than {} bytes", call.max_response);
        return HttpResponse::failed(call.id, HttpError::Refused, &reason);
    }
    let truncated = out.len() > call.max_response;
    out.truncate(call.max_response);
    HttpResponse {
        id: call.id,
        status,
        body: out,
        etag,
        content_type,
        url,
        headers,
        truncated,
        ..HttpResponse::default()
    }
}

/// Why `curl` failed, by its exit code.
#[cfg(not(target_arch = "wasm32"))]
fn curl_error(code: Option<i32>) -> HttpError {
    match code {
        Some(6) => HttpError::Dns,
        Some(7 | 47) => HttpError::Refused,
        Some(28) => HttpError::Timeout,
        Some(35 | 51 | 53 | 54 | 58 | 59 | 60 | 66 | 77 | 80 | 82 | 83 | 90 | 91) => HttpError::Tls,
        _ => HttpError::Io,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn call(save_to: Option<&str>) -> HttpCall {
        HttpCall {
            id: 7,
            method: String::from("GET"),
            url: String::from("https://a.example/book.epub"),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: 4,
            timeout_ms: 1000,
            want_headers: Vec::new(),
            save_to: save_to.map(String::from),
            progress: false,
        }
    }

    #[test]
    fn the_size_is_the_last_answers() {
        let headers = "HTTP/1.1 302 Found\r\nContent-Length: 5\r\nLocation: /b\r\n\r\n\
                       HTTP/2 200\r\ncontent-length: 1234\r\n\r\n";
        assert_eq!(content_length(headers), Some(1234));
        assert_eq!(content_length("HTTP/2 200\r\nserver: x\r\n\r\n"), None);
    }

    /// An answer longer than wanted is cut, unless it is a file: then it
    /// fails, so no file is saved cut short as if whole.
    #[test]
    fn answers_too_long_for_a_file_fail() {
        let written = b"PK book
200

application/epub+zip
https://a.example/book.epub"
            .to_vec();
        let cut = answer(&call(None), written.clone());
        assert_eq!((cut.status, cut.body.as_slice()), (200, &b"PK b"[..]));
        let file = answer(&call(Some("Books/a.epub")), written);
        assert_eq!(file.status, 0, "{file:?}");
        let whole = b"PK
200


"
        .to_vec();
        let fits = answer(&call(Some("Books/a.epub")), whole);
        assert_eq!((fits.status, fits.body.as_slice()), (200, &b"PK"[..]));
    }
}
