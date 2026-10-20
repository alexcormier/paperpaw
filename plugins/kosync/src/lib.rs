//! Reading progress synced with KOReader's sync server ("kosync"), so a
//! book opens where one stopped on another device, PaperPaw or KOReader.
//!
//! - Books are identified as KOReader does (a partial MD5 of the file), so
//!   the same file matches on every device.
//! - Positions are KOReader's XPointers where the format has them (EPUB),
//!   besides the percentage.
//! - The progress is sent at most once a minute while reading (where
//!   PaperPaw runs the radio itself, only while it is on anyway: turning
//!   pages never keeps it on), and when a book is closed and the device
//!   sleeps or the app goes to the background; it is fetched when a book
//!   opens and when the app comes back. If another device is further, a dialog offers to
//!   go there (once for each position).
//! - Nothing is sent for a book while its progress is being fetched, nor
//!   while the dialog is open: a further position on the server is never
//!   overwritten before the user chose to stay.
//! - *Sync progress* in the reading menu's *More* fetches the latest, then
//!   sends this device's position unless another device is further.
//! - The account (settings → *Progress sync*) keeps the user name in the
//!   settings and only the MD5 of the password, as KOReader does. Its
//!   screen also sets the server: KOReader's or any compatible one, also
//!   below a path (`https://example.org/kosync`) or over plain `http` in a
//!   home network. Another server signs out, however it was set: an
//!   account's key belongs to its server.
//!
//! Needs a device with network ([`Capabilities::network`](paperpaw_sdk::Capabilities)).

#![no_std]

extern crate alloc;

mod account;
mod confirm;

use paperpaw_digest::md5;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_json as json;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::work::{Moments, Work};
use paperpaw_sdk::{
    ActionSpec, Arg, Command, Env, HttpRequest, Id, Instant, Manifest, MenuEntry, Params, Plugin,
    RegisterError, Registrar, SDK_VERSION, ScreenSpec, SettingGroupSpec, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("kosync");
/// The account's user name (empty when signed out); edited on the account
/// screen.
pub const USER: Setting<Text> = Setting::new("kosync.user");
/// The sync server.
pub const SERVER: Setting<Text> = Setting::new("kosync.server");
/// Send and fetch the progress by itself.
pub const AUTO: Setting<bool> = Setting::new("kosync.auto");
/// Sends the progress of the open book and fetches the latest now.
pub const SYNC_NOW: Id = Id::new_static("kosync.sync-now");

const GROUP: Id = Id::new_static("kosync.group");
const ACCOUNT_SCREEN: Id = Id::new_static("kosync.account");
const CONFIRM_SCREEN: Id = Id::new_static("kosync.confirm");
/// Emitted by the account screen when one signed in or out.
const ACCOUNT_CHANGED: Event = Event::new("kosync.account-changed");
/// Replies of the server.
const PULLED: Event<HttpReply> = Event::new("kosync.pulled");
const PUSHED: Event<HttpReply> = Event::new("kosync.pushed");
/// Declared work: the progress, sent while reading at the next moment
/// the radio is on anyway, at the latest after [`PUSH_WITHIN`] (ADR 0025).
const PUSH_DUE: Event = Event::new("kosync.push-due");
/// At the latest this long after a page turned, the progress goes out
/// (longer in Saving); closing the book or sleeping sends it before.
const PUSH_WITHIN: core::time::Duration = core::time::Duration::from_secs(30 * 60);
/// Books switched from while Offline, their progress sent once allowed.
const MOST_UNSENT: usize = 4;

/// Whether sending now keeps no radio on that would be off otherwise: the
/// system runs Wi-Fi, or PaperPaw's radio is in a network already.
fn radio_on_anyway(env: &Env<'_>) -> bool {
    env.wifi.is_none_or(|w| w.state.is_online())
}

/// KOReader's public server.
const DEFAULT_SERVER: &str = "https://sync.koreader.rocks";
/// The progress is sent at most this often while reading.
const PUSH_EVERY_MS: u64 = 60_000;
/// The reader is shown again often (after every menu); the progress is
/// fetched again for the same book only after this long.
const PULL_EVERY_MS: u64 = 60_000;
/// Another device counts as further from this much more progress (0..1).
const FURTHER: f64 = 0.002;
/// Parameters of a fetch: the local progress (0..1) and whether the user
/// asked for it.
const LOCAL: &str = "local";
const MANUAL: &str = "manual";

/// Where the reader is in which book.
#[derive(Debug, Clone, PartialEq)]
struct Position {
    path: String,
    at: Locator,
    /// Reading progress from 0 to 1, as the reader shows it.
    progress: f64,
}

/// Sending held back for a book while its progress on the server is being
/// fetched and, if another device is further there, until the user chose
/// whether to go there: sending first would overwrite the further position.
#[derive(Debug, Clone, PartialEq)]
struct Hold {
    path: String,
    /// Send this device's position when the hold ends with it kept.
    send: bool,
    /// Send it even if it was sent already (*Sync progress*).
    force: bool,
}

#[derive(Debug, Default)]
struct State {
    loaded: bool,
    device_id: String,
    current: Option<Position>,
    /// The position last sent.
    sent: Option<Position>,
    last_push: Option<Instant>,
    /// The book last fetched for, and when.
    last_pull: Option<(String, Instant)>,
    hold: Option<Hold>,
    /// A position the user chose not to go to: book and progress.
    declined: Option<(String, String)>,
    /// Book fingerprints already computed.
    digests: Vec<(String, String)>,
    /// Positions of books switched from while Offline, not sent yet.
    unsent: Vec<Position>,
    /// The server the account belongs to; another one signs out.
    server: Option<String>,
}

type Shared = Rc<RefCell<State>>;

fn dir() -> String {
    Env::plugin_dir(PLUGIN.as_str())
}

/// The account's key (the MD5 of its password), kept with the other
/// passwords and keys (see `paperpaw_sdk::secrets`).
pub(crate) const SECRET: &str = "kosync.key";

fn key<'a>(env: &Env<'a>) -> Option<&'a str> {
    env.secrets.get(SECRET).filter(|k| !k.is_empty())
}

fn read_value(storage: &dyn paperpaw_sdk::Storage, file: &str, key: &str) -> Option<String> {
    let data = paperpaw_sdk::read_file(storage, file, 4096).ok()?;
    let text = core::str::from_utf8(&data).ok()?;
    paperpaw_sdk::config::parse(text)
        .filter_map(Result::ok)
        .find(|e| e.key == key)
        .map(|e| e.value.as_str().into_owned())
}

/// Keeps the key of a signed-in account; `None` signs out. It cannot be
/// typed anew in the list of passwords and keys: it is no password.
pub(crate) fn save_key(key: Option<&str>) -> Command {
    Command::SetSecret(match key {
        Some(key) => SecretChange {
            changeable: false,
            ..SecretChange::keep(SECRET, "kosync.secret", key)
        },
        None => SecretChange::remove(SECRET),
    })
}

/// Signs out: the account's key and user name go.
pub(crate) fn sign_out() -> [Command; 3] {
    [
        save_key(None),
        Command::set(&USER, Cow::Borrowed("")),
        ACCOUNT_CHANGED.emit(()),
    ]
}

/// Moves a key kept in the plugin's folder by earlier versions to the
/// other passwords and keys.
fn move_key(env: &Env<'_>, out: &mut Vec<Command>) {
    let file = format!("{}/account.ini", dir());
    let Some(old) = read_value(env.storage, &file, "key") else {
        return;
    };
    if !old.is_empty() && key(env).is_none() {
        out.push(save_key(Some(&old)));
    }
    let _ = env.storage.remove(&file);
}

impl State {
    fn load(&mut self, env: &Env<'_>) {
        self.server.get_or_insert_with(|| server(env));
        if self.loaded {
            return;
        }
        self.loaded = true;
        if !self.device_id.is_empty() {
            return;
        }
        let device = format!("{}/device.ini", dir());
        self.device_id = match read_value(env.storage, &device, "id") {
            Some(id) => id,
            None => {
                // Different on each installation: the time and where memory
                // happened to be.
                let probe = Box::new(0u8);
                let seed = format!("{}{}{:p}", env.now.millis(), env.device.id, &*probe);
                let id = md5::of(&seed).to_uppercase();
                let text = format!("[device]\nid = {id}\n");
                let _ = env.storage.write(&device, text.as_bytes());
                id
            }
        };
    }

    /// Whether to sync by itself now.
    fn active(&self, env: &Env<'_>) -> bool {
        env.caps.network && env.settings.read(&AUTO) && self.signed_in(env)
    }

    /// Whether to sync by itself now, and the power mode lets the network
    /// be used (not Offline): otherwise what is not sent waits, silently.
    fn may_send(&self, env: &Env<'_>) -> bool {
        self.active(env) && env.power.online()
    }

    /// Sends what waited while Offline: the books switched from, the open
    /// one's progress, and fetches its latest.
    fn allowed(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        if !self.may_send(env) {
            return;
        }
        for position in core::mem::take(&mut self.unsent) {
            self.send(env, out, position, false);
        }
        // The latest first: what this device sends waits for it (`Hold`).
        if let Some(path) = self.current.as_ref().map(|c| c.path.clone()) {
            self.pull(env, out, &path, false);
        }
        self.push(env, out, false);
    }

    fn signed_in(&self, env: &Env<'_>) -> bool {
        !env.settings.read(&USER).is_empty() && key(env).is_some()
    }

    fn digest(&mut self, env: &Env<'_>, path: &str) -> Option<String> {
        if let Some((_, digest)) = self.digests.iter().find(|(p, _)| p == path) {
            return Some(digest.clone());
        }
        let digest = md5::partial(env.storage, path)?;
        self.digests.push((path.to_string(), digest.clone()));
        Some(digest)
    }

    /// A request to the server with the account's credentials.
    fn request(
        &self,
        env: &Env<'_>,
        method: &str,
        path: &str,
        body: Option<String>,
        reply: &Event<HttpReply>,
        params: Params,
    ) -> Option<Command> {
        let user = env.settings.read(&USER);
        let auth = Some((user, key(env)?));
        Some(Command::Http(server_request(
            env, method, path, body, reply, params, auth,
        )))
    }

    /// Sends the current position, unless it was sent already or is held
    /// back (see [`Hold`]).
    fn push(&mut self, env: &Env<'_>, out: &mut Vec<Command>, force: bool) {
        let Some(position) = self.current.clone() else {
            return;
        };
        if let Some(hold) = self.hold.as_mut().filter(|h| h.path == position.path) {
            // Sent when the hold ends, if this position is kept.
            hold.send = true;
            hold.force |= force;
            return;
        }
        if !force && self.sent.as_ref() == Some(&position) {
            return;
        }
        self.send(env, out, position, true);
    }

    /// Sends `position`; `current` when it is the open book's, which is
    /// then known as sent.
    fn send(&mut self, env: &Env<'_>, out: &mut Vec<Command>, position: Position, current: bool) {
        // Not worth opening the book for.
        if !self.signed_in(env) {
            return;
        }
        let Some(digest) = self.digest(env, &position.path) else {
            return;
        };
        let document = env.formats.open(env.storage, &position.path).ok();
        let xpointer = document
            .as_ref()
            .and_then(|d| d.xpointer(env.storage, position.at))
            .unwrap_or_default();
        let progress = document.map_or(position.progress, |d| f64::from(d.progress(position.at)));
        let body = format!(
            "{{\"document\":{},\"progress\":{},\"percentage\":{:.4},\"device\":{},\"device_id\":{}}}",
            json::string(&digest),
            json::string(&xpointer),
            progress,
            json::string(&device_name(env)),
            json::string(&self.device_id),
        );
        let params = Params::new().with(params::PATH, position.path.clone());
        let target = "/syncs/progress";
        if let Some(command) = self.request(env, "PUT", target, Some(body), &PUSHED, params) {
            out.push(command);
            if current {
                self.sent = Some(position);
                self.last_push = Some(env.now);
            }
        }
    }

    /// Asks for the latest position of the book at `path`, holding back
    /// its sending until the answer; *Sync progress* (`manual`) sends after
    /// it unless another device is further.
    fn pull(&mut self, env: &Env<'_>, out: &mut Vec<Command>, path: &str, manual: bool) {
        let Some(digest) = self.digest(env, path) else {
            return;
        };
        let local = self
            .current
            .as_ref()
            .filter(|c| c.path == path)
            .map_or(0.0, |c| c.progress);
        let params = Params::new()
            .with(params::PATH, path.to_string())
            .with(LOCAL, format!("{local}"))
            .with(MANUAL, if manual { "1" } else { "0" });
        let target = format!("/syncs/progress/{digest}");
        if let Some(command) = self.request(env, "GET", &target, None, &PULLED, params) {
            out.push(command);
            self.last_pull = Some((path.to_string(), env.now));
            let held = self.hold.take().filter(|h| h.path == path);
            self.hold = Some(Hold {
                path: path.to_string(),
                send: manual || held.as_ref().is_some_and(|h| h.send),
                force: manual || held.is_some_and(|h| h.force),
            });
        }
    }

    /// Handles the server's answer to a fetch.
    fn pulled(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Vec<Command>) {
        let manual = reply.context.get(MANUAL) == Some("1");
        let path = reply.context.get(params::PATH).unwrap_or_default();
        let hold = self.hold.take_if(|h| h.path == path);
        // The book was closed meanwhile.
        if self.current.as_ref().is_none_or(|c| c.path != path) {
            return;
        }
        let status = reply.status.to_string();
        let status = status.as_str();
        if reply.status != 200 {
            if manual {
                out.push(message(env, failure(status), status));
            }
            return;
        }
        let body = json::object(&reply.body);
        let Some(remote) = remote_progress(&reply.body, &body) else {
            // Not knowing the server's progress, nothing may overwrite it.
            if manual {
                out.push(message(env, "kosync.unreadable", status));
            }
            return;
        };
        let local: f64 = reply
            .context
            .get(LOCAL)
            .and_then(|l| l.parse().ok())
            .unwrap_or(0.0);
        let from_here = json::text(&body, "device_id") == Some(self.device_id.as_str());
        let declined = self
            .declined
            .as_ref()
            .is_some_and(|(p, r)| p == path && *r == format!("{remote}"));
        if !from_here && remote > local + FURTHER && (manual || !declined) {
            // Further on another device: offer to go there, and send
            // nothing until the user chose.
            let device = json::text(&body, "device").unwrap_or("?");
            let xpointer = json::text(&body, "progress").unwrap_or_default();
            out.push(Command::OpenScreen(
                CONFIRM_SCREEN,
                Params::new()
                    .with(params::PATH, path.to_string())
                    .with(confirm::XPOINTER, xpointer.to_string())
                    .with(confirm::PERCENTAGE, format!("{remote}"))
                    .with(confirm::DEVICE, device.to_string()),
            ));
            self.hold = Some(hold.unwrap_or(Hold {
                path: path.to_string(),
                send: false,
                force: false,
            }));
            return;
        }
        if manual {
            out.push(message(env, "kosync.up-to-date", status));
        }
        if let Some(hold) = hold.filter(|h| h.send) {
            self.push(env, out, hold.force);
        }
    }

    /// The user closed the offer to go where another device is.
    fn answered(&mut self, env: &Env<'_>, answer: &confirm::Answered, out: &mut Vec<Command>) {
        self.hold.take_if(|h| h.path == answer.path);
        // Gone there: the reader tells the new position as it shows it.
        if !answer.stay {
            return;
        }
        self.declined = Some((answer.path.clone(), answer.percentage.clone()));
        // This device's position is the one to keep.
        if self.current.as_ref().is_some_and(|c| c.path == answer.path) {
            self.push(env, out, true);
        }
    }

    /// The server setting changed: the account's key must not go to
    /// another server.
    fn server_changed(&mut self, env: &Env<'_>, out: &mut Vec<Command>) {
        let now = server(env);
        let before = self.server.replace(now.clone());
        if before.as_ref() != Some(&now) && self.signed_in(env) {
            out.extend(sign_out());
        }
    }
}

/// The progress (0..1) in a sync server's answer `text`, read as `body`:
/// 0 for a book it has none of (an object without one, e.g. `{}`); `None`
/// if the answer is no object or its progress no number from 0 to 1.
fn remote_progress(text: &str, body: &[(String, json::Value)]) -> Option<f64> {
    if body.is_empty() {
        let inner = text
            .trim()
            .strip_prefix('{')
            .and_then(|t| t.strip_suffix('}'));
        return inner.is_some_and(|i| i.trim().is_empty()).then_some(0.0);
    }
    if !body.iter().any(|(key, _)| key == "percentage") {
        return Some(0.0);
    }
    let progress = json::number(body, "percentage")
        .or_else(|| json::text(body, "percentage").and_then(|p| p.trim().parse().ok()))?;
    (0.0..=1.0).contains(&progress).then_some(progress)
}

/// How this device shows up on others.
fn device_name(env: &Env<'_>) -> String {
    format!("PaperPaw ({})", env.device.name)
}

/// The server's base URL.
pub(crate) fn server(env: &Env<'_>) -> String {
    normalize_server(env.settings.read(&SERVER))
}

/// A server address as entered, as a base URL: `https://` unless another
/// scheme is given, no `/` at the end, KOReader's server if empty.
pub(crate) fn normalize_server(text: &str) -> String {
    let server = paperpaw_sdk::web::base_url(text);
    if server.is_empty() {
        DEFAULT_SERVER.to_string()
    } else {
        server
    }
}

/// A base URL as shown: without `https://` (but with `http://`, which is
/// worth knowing).
pub(crate) fn server_label(url: &str) -> &str {
    url.strip_prefix("https://").unwrap_or(url)
}

/// A request to the sync server, authenticated as user and key (the MD5 of
/// the password) if given.
pub(crate) fn server_request(
    env: &Env<'_>,
    method: &str,
    path: &str,
    body: Option<String>,
    reply: &Event<HttpReply>,
    params: Params,
    auth: Option<(&str, &str)>,
) -> HttpRequest {
    let mut headers = Vec::from([
        (
            "Accept".to_string(),
            "application/vnd.koreader.v1+json".to_string(),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ]);
    if let Some((user, key)) = auth {
        headers.push(("x-auth-user".to_string(), user.to_string()));
        headers.push(("x-auth-key".to_string(), key.to_string()));
    }
    let mut request =
        HttpRequest::new(method, &format!("{}{path}", server(env)), reply).with(params);
    request.headers = headers;
    request.body = body;
    request
}

/// The position in the parameters of a reading event.
fn position(reading: &events::Reading) -> Position {
    Position {
        path: reading.path.clone(),
        at: reading.at(),
        progress: f64::from(reading.progress) / 100.0,
    }
}

/// The translation key telling why a request with HTTP status `status`
/// failed (0: no answer).
pub(crate) fn failure(status: &str) -> &'static str {
    match status {
        "0" => "kosync.unreachable",
        "401" | "403" => "kosync.wrong",
        _ => "kosync.server-error",
    }
}

/// A message about a request in the confirmation overlay.
fn message(env: &Env<'_>, key: &str, status: &str) -> Command {
    let text = env.i18n.format(key, &[("status", Arg::Str(status))]);
    Command::OpenScreen(CONFIRM_SCREEN, Params::new().with(params::TEXT, text))
}

/// The kosync plugin.
#[derive(Debug, Default)]
pub struct Kosync;

/// The account, signed in on a screen of its own.
static ACCOUNT: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor(ACCOUNT_SCREEN);

/// The account, whether books sync by themselves, and the server.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::fixed_text(USER.static_str(), "kosync.setting.user", "", GROUP)
        .with_extra(&ACCOUNT)
        .described("kosync.setting.user.help"),
    SettingSpec::fixed_switch(AUTO.static_str(), "kosync.setting.auto", true, GROUP)
        .described("kosync.setting.auto.help"),
    // Set on the account screen too, which tells that it signs out.
    SettingSpec::fixed_text(
        SERVER.static_str(),
        "kosync.setting.server",
        DEFAULT_SERVER,
        GROUP,
    )
    .with_extra(&ACCOUNT)
    .advanced()
    .described("kosync.setting.server.help"),
];

impl Plugin for Kosync {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Progress sync"),
            description: Cow::Borrowed(
                "Syncs the reading position with KOReader's sync server, also with KOReader.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
            uses: paperpaw_sdk::Uses::NETWORK,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state: Shared = Rc::new(RefCell::new(State::default()));
        r.add_setting_group(SettingGroupSpec {
            id: GROUP,
            title: Cow::Borrowed("kosync.group"),
            order: 28,
            page: None,
        })?;
        r.add_icon(GROUP, Cow::Borrowed("devices"))?;
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec {
            id: ACCOUNT_SCREEN,
            title: Cow::Borrowed("kosync.group"),
            factory: Box::new(|env, _| Box::new(account::AccountScreen::new(env))),
        })?;
        r.add_screen(ScreenSpec {
            id: CONFIRM_SCREEN,
            title: Cow::Borrowed("kosync.group"),
            factory: Box::new(|env, params| Box::new(confirm::ConfirmScreen::new(env, params))),
        })?;

        let s = state.clone();
        r.on(events::STARTED, move |env, _, out| {
            move_key(env, out);
            s.borrow_mut().server = Some(server(env));
        })?;
        let s = state.clone();
        r.on(events::SETTING_CHANGED, move |env, changed, out| {
            if changed.id == *SERVER.id() {
                s.borrow_mut().server_changed(env, out);
            }
        })?;
        let s = state.clone();
        r.on(events::BOOK_OPENED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            let position = position(reading);
            let active = state.active(env);
            // Another book: the last one's position first.
            let other = state
                .current
                .as_ref()
                .is_some_and(|c| c.path != position.path);
            if active && other {
                if state.may_send(env) {
                    state.push(env, out, false);
                } else if let Some(last) = state.current.clone() {
                    // Offline: sent once allowed, silently.
                    state.unsent.retain(|u| u.path != last.path);
                    state.unsent.push(last);
                    let extra = state.unsent.len().saturating_sub(MOST_UNSENT);
                    state.unsent.drain(..extra);
                }
            }
            let path = position.path.clone();
            state.current = Some(position);
            let recent = state
                .last_pull
                .as_ref()
                .is_some_and(|(p, at)| *p == path && env.now.since(*at) < PULL_EVERY_MS);
            if !recent && state.may_send(env) {
                state.pull(env, out, &path, false);
            }
        })?;
        let s = state.clone();
        r.on(events::PAGE_TURNED, move |env, reading, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            state.current = Some(position(reading));
            let due = state
                .last_push
                .is_none_or(|at| env.now.since(at) >= PUSH_EVERY_MS);
            if due && state.may_send(env) && radio_on_anyway(env) {
                state.push(env, out, false);
            } else if state.active(env) && env.work.get(PUSH_DUE.id()).is_none() {
                // When the radio is on anyway, at the latest in a while.
                out.push(Command::Work(
                    Work::network(PUSH_DUE.id().clone())
                        .within(PUSH_WITHIN)
                        .at_moments(Moments::ONLINE),
                ));
            }
        })?;
        let s = state.clone();
        r.subscribe(
            events::BOOK_CLOSED,
            Box::new(move |env, _, out| {
                let mut state = s.borrow_mut();
                state.load(env);
                if state.may_send(env) {
                    state.push(env, out, false);
                }
            }),
        )?;
        let s = state.clone();
        r.on(PUSH_DUE, move |env, (), out| {
            let mut state = s.borrow_mut();
            state.load(env);
            if state.may_send(env) {
                state.push(env, out, false);
            }
        })?;
        let s = state.clone();
        r.on(events::WORK_ALLOWED, move |env, (), out| {
            let mut state = s.borrow_mut();
            state.load(env);
            state.allowed(env, out);
        })?;
        let s = state.clone();
        r.on(events::SUSPEND, move |env, _, out| {
            let mut state = s.borrow_mut();
            state.load(env);
            if state.may_send(env) {
                state.push(env, out, false);
            }
        })?;
        let s = state.clone();
        r.on(events::RESUME, move |env, _, out| {
            // Maybe read on another device meanwhile.
            let mut state = s.borrow_mut();
            state.load(env);
            let path = state.current.as_ref().map(|c| c.path.clone());
            if let Some(path) = path.filter(|_| state.may_send(env)) {
                state.pull(env, out, &path, false);
            }
        })?;
        let s = state.clone();
        r.on(confirm::ANSWERED, move |env, answer, out| {
            s.borrow_mut().answered(env, answer, out);
        })?;
        let s = state.clone();
        r.on(ACCOUNT_CHANGED, move |_, (), _| {
            s.borrow_mut().loaded = false
        })?;
        let s = state.clone();
        r.on(PUSHED, move |_, reply, _| {
            // Not sent after all: again with the next page.
            if reply.status != 200 {
                let mut state = s.borrow_mut();
                state.sent = None;
                state.last_push = None;
            }
        })?;
        let s = state.clone();
        r.on(PULLED, move |env, reply, out| {
            s.borrow_mut().pulled(env, reply, out);
        })?;

        // "Sync progress" in the reading menu's "More".
        let s = state;
        r.add_action(ActionSpec::new(SYNC_NOW, "kosync.sync-now").handler(
            move |env: &Env<'_>, out: &mut Vec<Command>| {
                let mut state = s.borrow_mut();
                state.load(env);
                if !env.caps.network || !state.signed_in(env) {
                    out.push(Command::OpenScreen(ACCOUNT_SCREEN, Params::new()));
                    return;
                }
                let Some(path) = state.current.as_ref().map(|c| c.path.clone()) else {
                    return;
                };
                // The server's first: sent after the answer, unless
                // another device is further and the user goes there.
                state.pull(env, out, &path, true);
            },
        ))?;
        r.add_menu_entry(MenuEntry {
            menu: paperpaw_sdk::builtin::menus::READER_MORE,
            item: SYNC_NOW,
            order: 25,
        })?;
        r.add_icon(SYNC_NOW, Cow::Borrowed("refresh"))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
