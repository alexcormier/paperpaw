//! The requests, as read from their lines (the table is the module's
//! above).

use std::path::PathBuf;

use paperpaw_core::sdk::json::{self, Value};
use paperpaw_platform::PointerPhase;

use super::{Fixture, MAX_WAIT_MS, Network, UNTIL_MS};

/// A request, as read from a line.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Start {
        profile: String,
        library: PathBuf,
        settings: Option<String>,
        trust_code: bool,
        /// Online from the start (`"wifi": "online"`), or with the radio
        /// off (`"off"`); `None` as `--wifi` said.
        wifi_online: Option<bool>,
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
    /// A `fixture` whose body is read from `file`.
    FixtureFile {
        fixture: Fixture,
        file: PathBuf,
    },
    Quit,
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
        timeout: json::number(members, "timeout")
            .map_or(UNTIL_MS, |n| n.max(0.0) as u64)
            .min(MAX_WAIT_MS),
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

/// A `fixture` request: its answer, its body read from `file` where it
/// names one (when it is done, in the device's folder).
fn fixture(members: &[(String, Value)]) -> Result<Request, String> {
    let text = |key: &str| json::text(members, key).map(String::from);
    let fixture = Fixture {
        url: text("url").ok_or_else(|| String::from("`url` is missing"))?,
        method: text("method"),
        status: json::number(members, "status").map_or(200, |n| n as u16),
        body: text("body").unwrap_or_default().into_bytes(),
        content_type: text("content_type"),
    };
    Ok(match text("file").filter(|_| text("body").is_none()) {
        Some(file) => Request::FixtureFile {
            fixture,
            file: PathBuf::from(file),
        },
        None => Request::Fixture(Some(fixture)),
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
            wifi_online: match text("wifi").as_deref() {
                None => None,
                Some("off") => Some(false),
                Some("online") => Some(true),
                Some(other) => return Err(format!("no wifi `{other}`: online or off")),
            },
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
            ms: json::number(&members, "ms").map_or(0, |n| (n.max(0.0) as u64).min(MAX_WAIT_MS)),
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
        "fixture" => fixture(&members)?,
        "quit" => Request::Quit,
        other => return Err(format!("no request `{other}`")),
    })
}

/// The name of `request`'s `op`, for errors.
pub(super) fn op_name(request: &Request) -> &'static str {
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
        Request::Fixture(_) | Request::FixtureFile { .. } => "fixture",
        Request::Quit => "quit",
    }
}
