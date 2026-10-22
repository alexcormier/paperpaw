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

/// Requests in flight and their answers.
#[derive(Debug)]
pub struct Http {
    sender: Sender<HttpResponse>,
    receiver: Receiver<HttpResponse>,
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
        thread::spawn(move || {
            let _ = sender.send(run(call));
        });
    }

    /// Answers a request at once as failed: in a browser the emulator
    /// has no network of its own.
    #[cfg(target_arch = "wasm32")]
    fn system(&mut self, call: HttpCall) {
        let failed = HttpResponse::failed(call.id, "no network in the browser's emulator");
        let _ = self.sender.send(failed);
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
fn run(call: HttpCall) -> HttpResponse {
    let failed = |reason: String| HttpResponse {
        id: call.id,
        status: 0,
        body: reason.into_bytes(),
        ..HttpResponse::default()
    };
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
        Err(e) => return failed(format!("curl: {e}")),
    };
    if let (Some(body), Some(mut stdin)) = (&call.body, child.stdin.take()) {
        let _ = stdin.write_all(body);
    }
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(e) => return failed(format!("curl: {e}")),
    };
    if !output.status.success() {
        return failed(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    // The body, then lines with the status, the ETag, the content type,
    // the final address and the headers asked for (`-w`), read from the
    // end.
    let mut out = output.stdout;
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
    out.truncate(call.max_response);
    HttpResponse {
        id: call.id,
        status,
        body: out,
        etag,
        content_type,
        url,
        saved: false,
        headers,
    }
}
