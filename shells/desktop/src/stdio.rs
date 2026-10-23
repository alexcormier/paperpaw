//! `paperpaw-emu --stdio`: the emulator driven by another program (the
//! workshop, a test) over its standard input and output, one JSON object
//! a line each way. The program sends requests; the emulator answers and
//! tells what happens: a frame only when the panel changed, errors,
//! warnings, the requests plugins make.
//!
//! [`PROTOCOL`] is the protocol's version, said first (`ready`); a field
//! is only added, and a request or event whose meaning changes gets a new
//! name.
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
//! | `quit` | | ends |
//!
//! Events (`event`): `ready` (`protocol`, `paperpaw`, `profiles`, and
//! `fingerprint` and `toolchain`, which code plugins must be built for),
//! `started` (`profile`, `width`, `height`, `buttons`, `touch`), `frame`
//! (`width`, `height`, `png` in base64), `described` (`parts`: `role`,
//! `label`, `x`, `y`, `width`, `height`, `focused`, `on`), `shot` (`path`
//! or `png`), `request` (`method`, `url`), `warning` (`message`), `error`
//! (`message`, and `op` where a request caused it).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

use paperpaw_core::sdk::json::{self, Value, string};
use paperpaw_platform::{ButtonState, InputEvent, Point, PointerEvent, PointerPhase, PointerTool};

use crate::device::{self, Device};
use crate::profiles::{self, PROFILES};

/// The protocol's version.
pub const PROTOCOL: u32 = 1;

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
    Quit,
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
            params: json::raw_member(line, "params")
                .map(json::object)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|(key, value)| match value {
                    Value::Str(text) => Some((key, text)),
                    Value::Num(n) => Some((key, n.to_string())),
                    Value::Other => None,
                })
                .collect(),
        },
        "shot" => Request::Shot {
            path: text("path").map(PathBuf::from),
        },
        "quit" => Request::Quit,
        other => return Err(format!("no request `{other}`")),
    })
}

/// The device a program drives, and what it was last shown.
#[derive(Default)]
pub struct Session {
    device: Option<Device>,
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
        for (method, url) in device.requests.iter_mut().flat_map(std::mem::take) {
            events.push(format!(
                "{{\"event\": \"request\", \"method\": {}, \"url\": {}}}",
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
        Request::Quit => "quit",
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
            let screen = paperpaw_core::sdk::Id::parse(&screen).map_err(|e| e.to_string())?;
            let mut given = paperpaw_core::sdk::Params::new();
            for (key, value) in params {
                given = given.with(key, value);
            }
            device.open_screen(screen, given);
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
        Request::Start { .. } | Request::Reload | Request::Quit => {}
    }
    Ok(None)
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
    format!(
        "{{\"event\": \"described\", \"parts\": [{}]}}",
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
