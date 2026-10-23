//! `paperpaw-emu --stdio`: the emulator driven by another program (the
//! workshop, a test) over its standard input and output, one JSON object
//! a line each way. The program sends requests; the emulator answers and
//! tells what happens: a frame only when the panel changed, errors,
//! warnings, the requests plugins make.
//!
//! [`PROTOCOL`] is the protocol's version, said first (`ready`); a field
//! is only added, and a request or event whose meaning changes gets a new
//! name. Version 2 added `slot`, `set`, `setting`, `emit`, `screens`,
//! `actions`, `wait`, `until`, `expect`, `network`, `fixture` and the
//! `screen` of `described`, after the X4's remote control
//! (`tools/x4-remote.py`); what version 1 had is as it was.
//!
//! Requests (`op`):
//!
//! | `op` | Fields | Does |
//! |------|--------|------|
//! | `start` | `profile`, `library` (a folder), `settings` (INI text, optional), `trust_code` (optional) | starts a device of that profile on the folder (setup done), with `trust_code` the code plugins built for this computer there installed (`native::trust`); `started` follows, then a `frame` |
//! | `button` | `name` (`confirm`, `page-forward`, …), `pressed` | a button pressed or let go |
//! | `pointer` | `phase` (`down`, `move`, `up`, `cancel`), `x`, `y` | a finger on a touch screen, at the panel's pixels |
//! | `reload` | | starts the device again on its folder: plugins changed there are read anew |
//! | `describe` | | `described`: what the screen shows, part by part |
//! | `run` | `action` | runs an action, as a button bound to it would |
//! | `open` | `screen`, `params` (an object of texts, optional) | opens a screen, as a plugin would |
//! | `shot` | `path` (optional) | `shot`: the panel as a PNG, written there or in the event |
//! | `slot` | `slot`, `params` (optional) | opens the screen bound to a slot, as a plugin would (`core.settings`) |
//! | `set` | `id`, `value` (as configuration files write it) | `set`: a setting set, as the settings screen would |
//! | `setting` | `id` | `setting`: its value as configuration files write it (`null` if there is none, `•` for a secret) |
//! | `emit` | `event`, `params` (optional) | sends an event, as a plugin would |
//! | `screens` | | `screens`: the screens open (`open`, the one shown last), `shown`, and every screen plugins added (`known`) |
//! | `actions` | `prefix` (optional) | `actions`: every action, its `id` and `title` |
//! | `wait` | `ms` | lets that much time pass, the device going on; `waited` follows |
//! | `until` | `text` or `screen`, `timeout` (ms, 30 000 if not given) | waits until the screen shows that text (as `tap-on` finds it) or is that screen; `until` with `found` |
//! | `expect` | `text`, `not` (optional) | `expect` with `ok`: whether the screen shows that text now (or not) |
//! | `network` | `mode`: `real`, `recorded` or `offline` | where plugins' requests go: the network, the recorded answers (`fixture`; none recorded: failed), nowhere (failed); `network` follows |
//! | `fixture` | `url` (a `*` at its end matches what starts so), `method`, `status` (200), `body` (text) or `file` (its bytes), `content_type` | records an answer for `recorded`; `clear: true` forgets them all |
//! | `quit` | | ends |
//!
//! Events (`event`): `ready` (`protocol`, `paperpaw`, `profiles`, and
//! `fingerprint` and `toolchain`, which code plugins must be built for),
//! `started` (`profile`, `width`, `height`, `buttons`, `touch`), `frame`
//! (`width`, `height`, `png` in base64), `described` (`screen`, `parts`:
//! `role`, `label`, `x`, `y`, `width`, `height`, `focused`, `on`), `shot`
//! (`path` or `png`), `request` (`method`, `url`, and `answer`: `network`,
//! `recorded` or `none`), `set`, `setting`, `screens`, `actions`,
//! `waited`, `until`, `expect`, `network`, `fixture`, `warning`
//! (`message`), `error` (`message`, and `op` where a request caused it).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use paperpaw_core::sdk::json::{self, Value, string};
use paperpaw_platform::{ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool};

use paperpaw_core::sdk::Id;

use crate::device::{self, Device};
use crate::profiles::{self, PROFILES};

/// The protocol's version.
pub const PROTOCOL: u32 = 2;

/// How long `until` waits where it is not told, in milliseconds.
const UNTIL_MS: u64 = 30_000;

/// How long the loop waits at most between looks when the device asks
/// for nothing sooner (a clock, an animation).
const IDLE_MS: u64 = 50;
/// The longest request line read, in bytes: a longer one is said to be
/// too long and passed over, up to its end.
pub const MAX_LINE: usize = 16 << 20;

/// The next line of `input` (without its end), at most `max` bytes: `None`
/// at the input's end, `Err` for one longer (read to its end and dropped,
/// so the next one is read as it is) or not UTF-8.
pub fn read_line(
    input: &mut impl BufRead,
    max: usize,
) -> std::io::Result<Option<Result<String, String>>> {
    let mut line = Vec::new();
    let mut long = false;
    let mut any = false;
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        any = true;
        let (part, end) = match buf.iter().position(|&b| b == b'\n') {
            Some(at) => (&buf[..at], Some(at + 1)),
            None => (buf, None),
        };
        if !long && line.len() + part.len() > max {
            long = true;
            line = Vec::new();
        }
        if !long {
            line.extend_from_slice(part);
        }
        let used = end.unwrap_or(buf.len());
        input.consume(used);
        if end.is_some() {
            break;
        }
    }
    if !any {
        return Ok(None);
    }
    if long {
        return Ok(Some(Err(format!("a request longer than {max} bytes"))));
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Some(
        String::from_utf8(line).map_err(|_| String::from("a request not in UTF-8")),
    ))
}

/// A request, as read from a line.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Start {
        profile: String,
        library: PathBuf,
        settings: Option<String>,
        trust_code: bool,
    },
    Button {
        name: String,
        pressed: bool,
    },
    Pointer {
        phase: PointerPhase,
        x: i32,
        y: i32,
    },
    Reload,
    Describe,
    Run {
        action: String,
    },
    Open {
        screen: String,
        params: Vec<(String, String)>,
    },
    Shot {
        path: Option<PathBuf>,
    },
    Slot {
        slot: String,
        params: Vec<(String, String)>,
    },
    Set {
        id: String,
        value: String,
    },
    Setting {
        id: String,
    },
    Emit {
        event: String,
        params: Vec<(String, String)>,
    },
    Screens,
    Actions {
        prefix: String,
    },
    Wait {
        ms: u64,
    },
    Until {
        text: Option<String>,
        screen: Option<String>,
        timeout: u64,
    },
    Expect {
        text: String,
        not: bool,
    },
    Network {
        mode: Network,
    },
    Fixture(Option<Fixture>),
    Quit,
}

/// Where plugins' requests go (`network`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// To the network, as without the protocol.
    Real,
    /// To the answers recorded (`fixture`); one with none, failed.
    Recorded,
    /// Nowhere: every one fails, as without a network.
    Offline,
}

impl Network {
    fn name(self) -> &'static str {
        match self {
            Network::Real => "real",
            Network::Recorded => "recorded",
            Network::Offline => "offline",
        }
    }
}

/// An answer recorded for plugins' requests (`fixture`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// The address it answers; ending in `*`, every one that starts so.
    pub url: String,
    /// The method it answers (any where `None`).
    pub method: Option<String>,
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

impl Fixture {
    fn answers(&self, method: &str, url: &str) -> bool {
        let address = match self.url.strip_suffix('*') {
            Some(start) => url.starts_with(start),
            None => url == self.url,
        };
        address
            && self
                .method
                .as_deref()
                .is_none_or(|m| m.eq_ignore_ascii_case(method))
    }
}

/// An `until` request's text or screen and timeout.
fn until(members: &[(String, Value)]) -> Result<Request, String> {
    let text = json::text(members, "text").map(String::from);
    let screen = json::text(members, "screen").map(String::from);
    if text.is_none() && screen.is_none() {
        return Err(String::from("`until` waits for a `text` or a `screen`"));
    }
    Ok(Request::Until {
        text,
        screen,
        timeout: json::number(members, "timeout").map_or(UNTIL_MS, |n| n.max(0.0) as u64),
    })
}

/// A `network` request's mode.
fn network(mode: &str) -> Result<Network, String> {
    match mode {
        "real" => Ok(Network::Real),
        "recorded" => Ok(Network::Recorded),
        "offline" => Ok(Network::Offline),
        other => Err(format!("no network mode `{other}`")),
    }
}

/// A `fixture` request's answer.
fn fixture(members: &[(String, Value)]) -> Result<Fixture, String> {
    let text = |key: &str| json::text(members, key).map(String::from);
    Ok(Fixture {
        url: text("url").ok_or_else(|| String::from("`url` is missing"))?,
        method: text("method"),
        status: json::number(members, "status").map_or(200, |n| n as u16),
        body: match (text("body"), text("file")) {
            (Some(body), _) => body.into_bytes(),
            (None, Some(file)) => std::fs::read(&file).map_err(|e| format!("{file}: {e}"))?,
            (None, None) => Vec::new(),
        },
        content_type: text("content_type"),
    })
}

/// The members of the object `key` of `line` as texts (numbers as they
/// are written), for parameters.
fn texts(line: &str, key: &str) -> Vec<(String, String)> {
    json::raw_member(line, key)
        .map(json::object)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(key, value)| match value {
            Value::Str(text) => Some((key, text)),
            Value::Num(n) => Some((key, n.to_string())),
            Value::Other => None,
        })
        .collect()
}

/// `line` as a request, or why it is none.
pub fn parse(line: &str) -> Result<Request, String> {
    let members = json::object(line);
    if members.is_empty() {
        return Err(String::from("a request is a JSON object with an `op`"));
    }
    let text = |key: &str| json::text(&members, key).map(String::from);
    let need = |key: &str| text(key).ok_or_else(|| format!("`{key}` is missing"));
    let number = |key: &str| {
        json::number(&members, key)
            .map(|n| n as i32)
            .ok_or_else(|| format!("`{key}` is missing"))
    };
    let op = need("op")?;
    Ok(match op.as_str() {
        "start" => Request::Start {
            profile: need("profile")?,
            library: PathBuf::from(need("library")?),
            settings: text("settings"),
            trust_code: json::raw_member(line, "trust_code").map(str::trim) == Some("true"),
        },
        "button" => Request::Button {
            name: need("name")?,
            pressed: json::raw_member(line, "pressed").map(str::trim) == Some("true"),
        },
        "pointer" => Request::Pointer {
            phase: match need("phase")?.as_str() {
                "down" => PointerPhase::Down,
                "move" => PointerPhase::Move,
                "up" => PointerPhase::Up,
                "cancel" => PointerPhase::Cancel,
                other => return Err(format!("no phase `{other}`")),
            },
            x: number("x")?,
            y: number("y")?,
        },
        "reload" => Request::Reload,
        "describe" => Request::Describe,
        "run" => Request::Run {
            action: need("action")?,
        },
        "open" => Request::Open {
            screen: need("screen")?,
            params: texts(line, "params"),
        },
        "shot" => Request::Shot {
            path: text("path").map(PathBuf::from),
        },
        "slot" => Request::Slot {
            slot: need("slot")?,
            params: texts(line, "params"),
        },
        "set" => Request::Set {
            id: need("id")?,
            value: text("value")
                .or_else(|| json::number(&members, "value").map(|n| n.to_string()))
                .or_else(|| json::raw_member(line, "value").map(|raw| raw.trim().to_string()))
                .ok_or_else(|| String::from("`value` is missing"))?,
        },
        "setting" => Request::Setting { id: need("id")? },
        "emit" => Request::Emit {
            event: need("event")?,
            params: texts(line, "params"),
        },
        "screens" => Request::Screens,
        "actions" => Request::Actions {
            prefix: text("prefix").unwrap_or_default(),
        },
        "wait" => Request::Wait {
            ms: json::number(&members, "ms").map_or(0, |n| n.max(0.0) as u64),
        },
        "until" => until(&members)?,
        "expect" => Request::Expect {
            text: need("text")?,
            not: json::raw_member(line, "not").map(str::trim) == Some("true"),
        },
        "network" => Request::Network {
            mode: network(&need("mode")?)?,
        },
        "fixture" if json::raw_member(line, "clear").map(str::trim) == Some("true") => {
            Request::Fixture(None)
        }
        "fixture" => Request::Fixture(Some(fixture(&members)?)),
        "quit" => Request::Quit,
        other => return Err(format!("no request `{other}`")),
    })
}

/// The device a program drives, and what it was last shown.
#[derive(Default)]
pub struct Session {
    device: Option<Device>,
    /// Where plugins' requests go, and the answers recorded for them,
    /// shared with the route the device's requests take (`network`).
    network: std::rc::Rc<std::cell::RefCell<(Option<Network>, Vec<Fixture>)>>,
    /// What the device was started with, for `reload`.
    started: Option<Started>,
    shown: Vec<u8>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// The first event: who answers.
    pub fn ready() -> String {
        let profiles: Vec<String> = PROFILES.iter().map(|p| string(p.device.id)).collect();
        let native = crate::native::native();
        format!(
            "{{\"event\": \"ready\", \"protocol\": {PROTOCOL}, \"paperpaw\": {}, \"profiles\": [{}], \"fingerprint\": \"{:016x}\", \"toolchain\": {}}}",
            string(env!("CARGO_PKG_VERSION")),
            profiles.join(", "),
            native.fingerprint.0,
            string(&native.toolchain)
        )
    }

    /// Does `request`; the events it makes. `None` to end.
    pub fn handle(&mut self, request: Request) -> Option<Vec<String>> {
        let op = op_name(&request);
        match self.carry_out(request) {
            Ok(Some(events)) => Some(events),
            Ok(None) => None,
            Err(message) => Some(vec![format!(
                "{{\"event\": \"error\", \"op\": {}, \"message\": {}}}",
                string(op),
                string(&message)
            )]),
        }
    }

    fn carry_out(&mut self, request: Request) -> Result<Option<Vec<String>>, String> {
        let mut events = Vec::new();
        match request {
            Request::Start {
                profile,
                library,
                settings,
                trust_code,
            } => {
                self.stop();
                let start = Started {
                    profile,
                    library,
                    settings,
                    trust_code,
                };
                let device = start.device()?;
                events.push(started(&device));
                self.device = Some(device);
                self.started = Some(start);
            }
            Request::Reload => {
                let start = self
                    .started
                    .clone()
                    .ok_or_else(|| String::from("no device started"))?;
                self.stop();
                let device = start.device()?;
                events.push(started(&device));
                self.device = Some(device);
            }
            Request::Quit => {
                self.stop();
                return Ok(None);
            }
            Request::Network { mode } => {
                self.network.borrow_mut().0 = (mode != Network::Real).then_some(mode);
                crate::http::set_route((mode != Network::Real).then(|| {
                    Box::new(Answers(self.network.clone())) as Box<dyn crate::http::Route>
                }));
                let recorded = self.network.borrow().1.len();
                events.push(format!(
                    "{{\"event\": \"network\", \"mode\": \"{}\", \"fixtures\": {recorded}}}",
                    mode.name()
                ));
            }
            Request::Fixture(fixture) => {
                let mut network = self.network.borrow_mut();
                match fixture {
                    Some(fixture) => network.1.push(fixture),
                    None => network.1.clear(),
                }
                events.push(format!(
                    "{{\"event\": \"fixture\", \"fixtures\": {}}}",
                    network.1.len()
                ));
            }
            request => {
                let device = self
                    .device
                    .as_mut()
                    .ok_or_else(|| String::from("no device started"))?;
                if let Some(event) = drive(device, request)? {
                    events.push(event);
                }
            }
        }
        Ok(Some(events))
    }

    /// One look at the device: its ticks and answers, then what it has to
    /// tell (a frame when the panel changed, warnings, requests).
    pub fn step(&mut self) -> Vec<String> {
        let Some(device) = self.device.as_mut() else {
            return Vec::new();
        };
        device.tick_if_due();
        device.poll_http(None);
        device.poll_servers();
        let mut events = Vec::new();
        for warning in device.kernel.take_warnings() {
            events.push(format!(
                "{{\"event\": \"warning\", \"message\": {}}}",
                string(&warning.to_string())
            ));
        }
        let (mode, fixtures) = &*self.network.borrow();
        for (method, url) in device.requests.iter_mut().flat_map(std::mem::take) {
            let answer = match mode {
                None => "network",
                Some(Network::Recorded) if fixtures.iter().any(|f| f.answers(&method, &url)) => {
                    "recorded"
                }
                Some(_) => "none",
            };
            events.push(format!(
                "{{\"event\": \"request\", \"method\": {}, \"url\": {}, \"answer\": \"{answer}\"}}",
                string(&method),
                string(&url)
            ));
        }
        let rgb = device.panel.rgb_bytes();
        if rgb != self.shown {
            let (width, height) = (device.panel.width(), device.panel.height());
            match png(width, height, &rgb) {
                Ok(png) => events.push(format!(
                    "{{\"event\": \"frame\", \"width\": {width}, \"height\": {height}, \"png\": \"{}\"}}",
                    base64(&png)
                )),
                Err(message) => events.push(format!(
                    "{{\"event\": \"error\", \"message\": {}}}",
                    string(&message)
                )),
            }
            self.shown = rgb;
        }
        events
    }

    /// Whether the screen shows `text` (as `tap-on` finds it) or is
    /// `screen`, where given.
    fn shows(&mut self, text: Option<&str>, screen: Option<&str>) -> bool {
        let Some(device) = self.device.as_mut() else {
            return false;
        };
        let is_screen =
            screen.is_none_or(|id| device.kernel.active_screen().map(Id::as_str) == Some(id));
        let has_text = text.is_none_or(|text| {
            let parts = device.kernel.describe();
            paperpaw_core::sdk::semantics::find(&parts, text).is_some()
        });
        is_screen && has_text
    }

    /// Goes on (the device's ticks, what it tells told) until the screen
    /// shows `text` or is `screen`, at most `timeout` milliseconds: the
    /// milliseconds it took, or `None`. With neither, `timeout` passes.
    fn until(
        &mut self,
        text: Option<&str>,
        screen: Option<&str>,
        timeout: u64,
        tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
    ) -> std::io::Result<Option<u64>> {
        let start = std::time::Instant::now();
        let looking = text.is_some() || screen.is_some();
        loop {
            self.step_into(tell)?;
            let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            if looking && self.shows(text, screen) {
                return Ok(Some(ms));
            }
            if ms >= timeout {
                return Ok(None);
            }
            let rest = self.wait().min(timeout - ms).max(1);
            std::thread::sleep(Duration::from_millis(rest));
        }
    }

    /// One look at the device ([`Session::step`]), what it tells told.
    fn step_into(
        &mut self,
        tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        for event in self.step() {
            tell(&event)?;
        }
        Ok(())
    }

    /// How long until the device wants a look.
    fn wait(&self) -> u64 {
        self.device
            .as_ref()
            .and_then(|d| d.next_wake().map(|at| at.since(d.now())))
            .map_or(IDLE_MS, |ms| ms.clamp(1, IDLE_MS))
    }

    fn stop(&mut self) {
        if let Some(mut device) = self.device.take() {
            device.shut_down();
        }
        self.shown.clear();
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

/// What a device was started with (`start`), to start it so again.
#[derive(Debug, Clone)]
struct Started {
    profile: String,
    library: PathBuf,
    settings: Option<String>,
    trust_code: bool,
}

impl Started {
    /// The device, started: the code plugins built for this computer
    /// installed first where asked (also at every `reload`, so a new
    /// build runs).
    fn device(&self) -> Result<Device, String> {
        // Code plugins only where the caller says so, for this device.
        crate::device::set_code_plugins(self.trust_code);
        if self.trust_code {
            crate::native::trust(&self.library)
                .map_err(|e| format!("{}: {e}", self.library.display()))?;
        }
        start(&self.profile, &self.library, self.settings.as_deref())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Requests to the network again, as without the protocol.
        if self.network.borrow().0.is_some() {
            crate::http::set_route(None);
        }
        self.stop();
    }
}

/// The name of `request`'s `op`, for errors.
fn op_name(request: &Request) -> &'static str {
    match request {
        Request::Start { .. } => "start",
        Request::Button { .. } => "button",
        Request::Pointer { .. } => "pointer",
        Request::Reload => "reload",
        Request::Describe => "describe",
        Request::Run { .. } => "run",
        Request::Open { .. } => "open",
        Request::Shot { .. } => "shot",
        Request::Slot { .. } => "slot",
        Request::Set { .. } => "set",
        Request::Setting { .. } => "setting",
        Request::Emit { .. } => "emit",
        Request::Screens => "screens",
        Request::Actions { .. } => "actions",
        Request::Wait { .. } => "wait",
        Request::Until { .. } => "until",
        Request::Expect { .. } => "expect",
        Request::Network { .. } => "network",
        Request::Fixture(_) => "fixture",
        Request::Quit => "quit",
    }
}

/// Answers plugins' requests as the protocol's `network` says: from the
/// answers recorded, or not at all.
struct Answers(std::rc::Rc<std::cell::RefCell<(Option<Network>, Vec<Fixture>)>>);

impl crate::http::Route for Answers {
    fn start(&mut self, call: paperpaw_core::HttpCall) -> Option<paperpaw_core::HttpResponse> {
        let (mode, fixtures) = &*self.0.borrow();
        let failed = |why: &str| paperpaw_core::HttpResponse::failed(call.id, why);
        Some(match mode {
            Some(Network::Recorded) => match fixtures
                .iter()
                .rev()
                .find(|f| f.answers(&call.method, &call.url))
            {
                Some(fixture) => paperpaw_core::HttpResponse {
                    id: call.id,
                    status: fixture.status,
                    body: fixture.body.clone(),
                    content_type: fixture.content_type.clone(),
                    ..paperpaw_core::HttpResponse::default()
                },
                None => failed("no answer recorded for this request"),
            },
            _ => failed("offline"),
        })
    }

    fn later(&mut self) -> Option<paperpaw_core::HttpResponse> {
        None
    }
}

/// A device of `profile` on the folder `library`, its setup done (it
/// opens on the home screen, not the first-start questions), started.
fn start(profile: &str, library: &Path, settings: Option<&str>) -> Result<Device, String> {
    let profile = profiles::find(profile).ok_or_else(|| format!("no profile `{profile}`"))?;
    let at = |e: std::io::Error| format!("{}: {e}", library.display());
    let setup = library.join(".paperpaw/plugins/setup");
    std::fs::create_dir_all(&setup).map_err(at)?;
    if !setup.join("done").exists() {
        std::fs::write(setup.join("done"), b"").map_err(at)?;
    }
    std::fs::create_dir_all(library.join("books")).map_err(at)?;
    let storage = device::library(library).map_err(at)?;
    let config = settings.map(String::from);
    let mut device = Device::new(profile, config, storage, false)
        .map_err(|e| format!("{}: {e:?}", profile.device.id))?;
    // Frames as they end up, not the panel's flashing on the way.
    device.panel.animate = false;
    device.requests = Some(Vec::new());
    device.start();
    device.tick_if_due();
    Ok(device)
}

fn started(device: &Device) -> String {
    let profile = device.profile;
    let buttons: Vec<String> = profile.buttons.iter().map(|b| string(b)).collect();
    format!(
        "{{\"event\": \"started\", \"profile\": {}, \"width\": {}, \"height\": {}, \"buttons\": [{}], \"touch\": {}}}",
        string(profile.device.id),
        device.panel.width(),
        device.panel.height(),
        buttons.join(", "),
        profile.device.input.touch
    )
}

/// Does what drives the device; an event where the request has one.
fn drive(device: &mut Device, request: Request) -> Result<Option<String>, String> {
    match request {
        Request::Button { name, pressed } => {
            // The device's own name for it: the kernel takes `&'static str`.
            let name = *device
                .profile
                .buttons
                .iter()
                .find(|b| **b == name)
                .ok_or_else(|| format!("`{}` has no button `{name}`", device.profile.device.id))?;
            let state = if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            device.input(InputEvent::Button { name, state });
        }
        Request::Pointer { phase, x, y } => {
            if !device.profile.device.input.touch {
                return Err(format!(
                    "`{}` has no touch screen",
                    device.profile.device.id
                ));
            }
            device.input(InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point: Point::new(x, y),
                tool: PointerTool::Finger,
                pressure: 1000,
            }));
        }
        Request::Describe => return Ok(Some(described(device))),
        Request::Run { action } => {
            device.run_action(paperpaw_core::sdk::Id::parse(&action).map_err(|e| e.to_string())?);
        }
        Request::Open { screen, params } => {
            let screen = Id::parse(&screen).map_err(|e| e.to_string())?;
            device.open_screen(screen, given(params));
        }
        Request::Shot { path } => {
            let rgb = device.panel.rgb_bytes();
            let png = png(device.panel.width(), device.panel.height(), &rgb)?;
            return Ok(Some(match path {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
                    format!(
                        "{{\"event\": \"shot\", \"path\": {}}}",
                        string(&path.display().to_string())
                    )
                }
                None => format!("{{\"event\": \"shot\", \"png\": \"{}\"}}", base64(&png)),
            }));
        }
        request => return ask(device, request),
    }
    Ok(None)
}

/// What version 2 asks of a device (`slot`, `set`, `setting`, `emit`,
/// `screens`, `actions`); an event where the request has one.
fn ask(device: &mut Device, request: Request) -> Result<Option<String>, String> {
    match request {
        Request::Slot { slot, params } => {
            let slot = Id::parse(&slot).map_err(|e| e.to_string())?;
            let params = given(params);
            device.call(|kernel, now| kernel.open_slot(now, slot, params));
        }
        Request::Set { id, value } => {
            let mut why = None;
            device.call(|kernel, now| match kernel.set_setting(now, &id, &value) {
                Ok(effects) => effects,
                Err(e) => {
                    why = Some(e);
                    paperpaw_core::Effects::default()
                }
            });
            if let Some(why) = why {
                return Err(format!("{id}: {why}"));
            }
            return Ok(Some(format!(
                "{{\"event\": \"set\", \"id\": {}, \"value\": {}}}",
                string(&id),
                string(&value)
            )));
        }
        Request::Setting { id } => {
            let settings = device.kernel.settings();
            let secret = settings.spec(&id).is_some_and(|spec| spec.extra().secret);
            let value = match settings.get(&id) {
                Some(_) if secret => string("\u{2022}"),
                Some(value) => string(&value.to_config_text()),
                None => String::from("null"),
            };
            return Ok(Some(format!(
                "{{\"event\": \"setting\", \"id\": {}, \"value\": {value}}}",
                string(&id)
            )));
        }
        Request::Emit { event, params } => {
            let event = Id::parse(&event).map_err(|e| e.to_string())?;
            let params = given(params);
            device.call(|kernel, now| kernel.send_event(now, event, params));
        }
        Request::Screens => {
            let ids = |ids: &mut dyn Iterator<Item = &Id>| {
                ids.map(|id| string(id.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let kernel = &device.kernel;
            let shown = kernel
                .active_screen()
                .map_or_else(|| String::from("null"), |id| string(id.as_str()));
            return Ok(Some(format!(
                "{{\"event\": \"screens\", \"open\": [{}], \"shown\": {shown}, \"known\": [{}]}}",
                ids(&mut kernel.screens()),
                ids(&mut kernel.known_screens())
            )));
        }
        Request::Actions { prefix } => {
            let kernel = &device.kernel;
            let mut actions: Vec<_> = kernel
                .actions()
                .iter()
                .filter(|a| a.id.as_str().starts_with(prefix.as_str()))
                .map(|a| {
                    format!(
                        "{{\"id\": {}, \"title\": {}}}",
                        string(a.id.as_str()),
                        string(kernel.translator().tr(&a.title))
                    )
                })
                .collect();
            actions.sort();
            return Ok(Some(format!(
                "{{\"event\": \"actions\", \"actions\": [{}]}}",
                actions.join(", ")
            )));
        }
        _ => {}
    }
    Ok(None)
}

/// `params` as a request's parameters.
fn given(params: Vec<(String, String)>) -> paperpaw_core::sdk::Params {
    let mut given = paperpaw_core::sdk::Params::new();
    for (key, value) in params {
        given = given.with(key, value);
    }
    given
}

/// What the screen shows, part by part.
fn described(device: &mut Device) -> String {
    let parts: Vec<String> = device
        .kernel
        .describe()
        .into_iter()
        .map(|part| {
            let a = part.area;
            let on = part.on.map_or_else(|| String::from("null"), |on| on.to_string());
            format!(
                "{{\"role\": \"{:?}\", \"label\": {}, \"x\": {}, \"y\": {}, \"width\": {}, \"height\": {}, \"focused\": {}, \"on\": {on}}}",
                part.role,
                string(&part.label),
                a.x,
                a.y,
                a.width,
                a.height,
                part.focused
            )
        })
        .collect();
    let screen = device
        .kernel
        .active_screen()
        .map_or_else(|| String::from("null"), |id| string(id.as_str()));
    format!(
        "{{\"event\": \"described\", \"screen\": {screen}, \"parts\": [{}]}}",
        parts.join(", ")
    )
}

/// `rgb` (8 bits a channel) as a PNG.
fn png(width: u16, height: u16, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, u32::from(width), u32::from(height));
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `bytes` in base64 (RFC 4648, with padding).
fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(LETTERS[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Runs the protocol over `input` and `output` until `quit` or the end of
/// the input.
pub fn run(
    mut input: impl BufRead + Send + 'static,
    mut output: impl Write,
) -> std::io::Result<()> {
    let (lines, received) = channel();
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            while let Ok(Some(line)) = read_line(&mut input, MAX_LINE) {
                if lines.send(line).is_err() {
                    break;
                }
            }
        })?;
    let mut session = Session::new();
    writeln!(output, "{}", Session::ready())?;
    output.flush()?;
    serve(&mut session, &received, &mut |event| {
        writeln!(output, "{event}")?;
        output.flush()
    })
}

/// The loop: requests as they come, the device's ticks when due, its
/// events as they happen.
fn serve(
    session: &mut Session,
    received: &Receiver<Result<String, String>>,
    tell: &mut dyn FnMut(&str) -> std::io::Result<()>,
) -> std::io::Result<()> {
    loop {
        match received.recv_timeout(Duration::from_millis(session.wait())) {
            Ok(Err(message)) => tell(&format!(
                "{{\"event\": \"error\", \"message\": {}}}",
                string(&message)
            ))?,
            Ok(Ok(line)) if line.trim().is_empty() => {}
            Ok(Ok(line)) => {
                let events = match parse(&line) {
                    Ok(Request::Wait { ms }) => {
                        session.until(None, None, ms, tell)?;
                        Some(vec![format!("{{\"event\": \"waited\", \"ms\": {ms}}}")])
                    }
                    Ok(Request::Until {
                        text,
                        screen,
                        timeout,
                    }) => {
                        let found =
                            session.until(text.as_deref(), screen.as_deref(), timeout, tell)?;
                        Some(vec![format!(
                            "{{\"event\": \"until\", \"found\": {}, \"ms\": {}}}",
                            found.is_some(),
                            found.unwrap_or(timeout)
                        )])
                    }
                    Ok(Request::Expect { text, not }) => {
                        session.step_into(tell)?;
                        let shows = session.shows(Some(&text), None);
                        Some(vec![format!(
                            "{{\"event\": \"expect\", \"text\": {}, \"ok\": {}}}",
                            string(&text),
                            shows != not
                        )])
                    }
                    Ok(request) => session.handle(request),
                    Err(message) => Some(vec![format!(
                        "{{\"event\": \"error\", \"message\": {}}}",
                        string(&message)
                    )]),
                };
                let Some(events) = events else {
                    return Ok(());
                };
                for event in events {
                    tell(&event)?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        for event in session.step() {
            tell(&event)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read_from_their_lines() {
        assert_eq!(
            parse(r#"{"op": "button", "name": "confirm", "pressed": true}"#),
            Ok(Request::Button {
                name: String::from("confirm"),
                pressed: true
            })
        );
        assert_eq!(
            parse(r#"{"op":"pointer","phase":"down","x":10,"y":20}"#),
            Ok(Request::Pointer {
                phase: PointerPhase::Down,
                x: 10,
                y: 20
            })
        );
        assert_eq!(
            parse(r#"{"op": "open", "screen": "wifi.phone", "params": {"a": "b", "n": 2}}"#),
            Ok(Request::Open {
                screen: String::from("wifi.phone"),
                params: vec![
                    (String::from("a"), String::from("b")),
                    (String::from("n"), String::from("2"))
                ]
            })
        );
        assert!(parse("not json").is_err());
        assert!(parse(r#"{"op": "fly"}"#).is_err());
        assert!(parse(r#"{"op": "button"}"#).is_err());
    }

    /// Lines are read up to their end, a long one said and passed over,
    /// the next one read as it is.
    #[test]
    fn long_lines_are_passed_over_to_their_end() {
        let mut text = format!("{{\"op\": \"quit\"}}\r\n{}\nshort\n", "x".repeat(40)).into_bytes();
        // A byte that is no UTF-8.
        text.extend_from_slice(b"\xff\ntail");
        let mut input = std::io::BufReader::with_capacity(8, text.as_slice());
        let mut lines = Vec::new();
        while let Some(line) = read_line(&mut input, 20).unwrap() {
            lines.push(line);
        }
        assert_eq!(lines[0], Ok(String::from(r#"{"op": "quit"}"#)));
        assert!(
            lines[1]
                .as_ref()
                .is_err_and(|e| e.contains("longer than 20"))
        );
        assert_eq!(lines[2], Ok(String::from("short")));
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[4], Ok(String::from("tail")));
    }

    #[test]
    fn base64_is_rfc_4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// A device started over the protocol says what it is, shows its home
    /// screen in a frame, answers what it shows, and errors are said, not
    /// fatal.
    #[test]
    fn a_device_is_driven_over_lines() {
        let _one = crate::one_device_at_a_time();
        let folder = std::env::temp_dir().join(format!("paperpaw-stdio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let (lines, received) = channel();
        let library = string(&folder.display().to_string());
        for line in [
            String::from(r#"{"op": "describe"}"#),
            format!(r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#),
            String::from(r#"{"op": "button", "name": "confirm", "pressed": true}"#),
            String::from(r#"{"op": "button", "name": "confirm", "pressed": false}"#),
            String::from(r#"{"op": "describe"}"#),
            String::from(r#"{"op": "pointer", "phase": "down", "x": 1, "y": 1}"#),
            String::from(r#"{"op": "shot"}"#),
            String::from(r#"{"op": "quit"}"#),
        ] {
            lines.send(Ok(line)).unwrap();
        }
        let mut events = Vec::new();
        let mut session = Session::new();
        serve(&mut session, &received, &mut |event| {
            events.push(event.to_string());
            Ok(())
        })
        .unwrap();
        let kinds: Vec<String> = events
            .iter()
            .map(|e| {
                json::text(&json::object(e), "event")
                    .unwrap_or("?")
                    .to_string()
            })
            .collect();
        assert_eq!(kinds[0], "error", "{events:?}");
        assert!(events[0].contains("no device started"));
        assert_eq!(kinds[1], "started");
        assert!(events[1].contains("\"width\": 480"));
        assert!(kinds.contains(&String::from("frame")));
        assert!(kinds.contains(&String::from("described")));
        // The X4 has no touch screen: said, and on it goes.
        assert!(events.iter().any(|e| e.contains("no touch screen")));
        let shot = events
            .iter()
            .find(|e| e.contains("\"event\": \"shot\""))
            .unwrap();
        assert!(shot.contains("\"png\": \"iVBOR"), "{shot}");
        assert!(folder.join(".paperpaw/plugins/setup/done").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Runs `lines` through the protocol on a device of the X4, its events
    /// in order.
    fn through(lines: &[String]) -> Vec<String> {
        let (sender, received) = channel();
        for line in lines {
            sender.send(Ok(line.clone())).unwrap();
        }
        sender.send(Ok(String::from(r#"{"op": "quit"}"#))).unwrap();
        let mut events = Vec::new();
        let mut session = Session::new();
        serve(&mut session, &received, &mut |event| {
            events.push(event.to_string());
            Ok(())
        })
        .unwrap();
        events
    }

    /// The event named `name`, the first.
    fn event<'a>(events: &'a [String], name: &str) -> &'a str {
        let key = format!("\"event\": \"{name}\"");
        events
            .iter()
            .find(|e| e.contains(&key))
            .unwrap_or_else(|| panic!("no {name}: {events:?}"))
    }

    /// Version 2's requests, as x4-remote has them: slots, settings,
    /// events, the screens and actions there are, waiting for what shows.
    #[test]
    fn the_device_is_asked_and_waited_for_over_lines() {
        let _one = crate::one_device_at_a_time();
        let folder = crate::private::dir("paperpaw-stdio2").unwrap();
        let library = string(&folder.display().to_string());
        let events = through(&[
            format!(r#"{{"op": "start", "profile": "xteink-x4", "library": {library}}}"#),
            String::from(r#"{"op": "screens"}"#),
            String::from(r#"{"op": "actions", "prefix": "core."}"#),
            String::from(r#"{"op": "slot", "slot": "core.settings"}"#),
            String::from(r#"{"op": "until", "text": "Display", "timeout": 5000}"#),
            String::from(r#"{"op": "expect", "text": "Display"}"#),
            String::from(r#"{"op": "expect", "text": "nothing says this", "not": true}"#),
            String::from(r#"{"op": "describe"}"#),
            String::from(r#"{"op": "set", "id": "core.night-mode", "value": "on"}"#),
            String::from(r#"{"op": "setting", "id": "core.night-mode"}"#),
            String::from(r#"{"op": "set", "id": "core.no-such-thing", "value": "on"}"#),
            String::from(r#"{"op": "emit", "event": "core.library-changed"}"#),
            String::from(r#"{"op": "wait", "ms": 100}"#),
            String::from(r#"{"op": "until", "text": "nothing says this", "timeout": 200}"#),
            String::from(r#"{"op": "network", "mode": "offline"}"#),
            String::from(r#"{"op": "fixture", "url": "https://example.org/*", "body": "hi"}"#),
            String::from(r#"{"op": "network", "mode": "real"}"#),
        ]);
        let screens = event(&events, "screens");
        assert!(
            screens.contains("\"known\": [\"") && screens.contains("\"shown\": \""),
            "{screens}"
        );
        assert!(event(&events, "actions").contains("\"id\": \"core."));
        assert!(
            event(&events, "until").contains("\"found\": true"),
            "{events:?}"
        );
        let expects: Vec<&String> = events
            .iter()
            .filter(|e| e.contains("\"event\": \"expect\""))
            .collect();
        assert!(
            expects.len() == 2 && expects.iter().all(|e| e.contains("\"ok\": true")),
            "{expects:?}"
        );
        let described = event(&events, "described");
        assert!(described.contains("\"screen\": \""), "{described}");
        assert!(event(&events, "set").contains("core.night-mode"));
        assert!(
            event(&events, "setting").contains("\"value\": \"on\""),
            "{events:?}"
        );
        assert!(
            events
                .iter()
                .any(|e| e.contains("core.no-such-thing") && e.contains("\"op\": \"set\""))
        );
        assert!(event(&events, "waited").contains("\"ms\": 100"));
        let untils: Vec<&String> = events
            .iter()
            .filter(|e| e.contains("\"event\": \"until\""))
            .collect();
        assert!(
            untils.last().unwrap().contains("\"found\": false"),
            "{untils:?}"
        );
        assert!(event(&events, "network").contains("\"mode\": \"offline\""));
        assert!(event(&events, "fixture").contains("\"fixtures\": 1"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Recorded answers answer what they were recorded for (the last one
    /// recorded first, `*` a start), others fail, as all do offline.
    #[test]
    fn recorded_answers_answer_their_requests() {
        let call = |method: &str, url: &str| paperpaw_core::HttpCall {
            id: 7,
            method: String::from(method),
            url: String::from(url),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: 1 << 20,
            timeout_ms: 1000,
            want_headers: Vec::new(),
            save_to: None,
        };
        let fixture = |url: &str, method: Option<&str>, body: &str| Fixture {
            url: String::from(url),
            method: method.map(String::from),
            status: 200,
            body: body.as_bytes().to_vec(),
            content_type: Some(String::from("text/plain")),
        };
        let state = std::rc::Rc::new(std::cell::RefCell::new((
            Some(Network::Recorded),
            vec![
                fixture("https://example.org/*", None, "any"),
                fixture("https://example.org/feed", Some("GET"), "feed"),
            ],
        )));
        let mut answers = Answers(state.clone());
        use crate::http::Route as _;
        let answer = |answers: &mut Answers, method, url| answers.start(call(method, url)).unwrap();
        let feed = answer(&mut answers, "GET", "https://example.org/feed");
        assert_eq!(
            (feed.id, feed.status, feed.body.as_slice()),
            (7, 200, &b"feed"[..])
        );
        assert_eq!(
            answer(&mut answers, "POST", "https://example.org/feed").body,
            b"any"
        );
        assert_eq!(answer(&mut answers, "GET", "https://other.org/").status, 0);
        state.borrow_mut().0 = Some(Network::Offline);
        assert_eq!(
            answer(&mut answers, "GET", "https://example.org/feed").status,
            0
        );
    }

    #[test]
    fn ready_says_the_protocol_and_profiles() {
        let ready = Session::ready();
        let members = json::object(&ready);
        assert_eq!(json::text(&members, "event"), Some("ready"));
        assert_eq!(
            json::number(&members, "protocol"),
            Some(f64::from(PROTOCOL))
        );
        assert!(ready.contains("\"xteink-x4-pro\""));
    }
}
