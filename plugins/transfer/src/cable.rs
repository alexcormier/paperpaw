//! Transfer over the cable (USB): a computer at the other end of the
//! cable (the workshop, a page in a browser) installs a plugin or adds
//! books, after the reader allowed it on the device. The screen is bound
//! to [`slots::CABLE`]: PaperPaw opens it when the computer speaks, and
//! it serves on [`CABLE_PORT`] while it is open. Nothing is held before.
//!
//! The requests (framed as lines by the kernel, see
//! `docs/usb-transfer.md`):
//!
//! - `GET /api/hello`: `{"protocol": 1, "device", "name", "version",
//!   "fingerprint"}`, the fingerprint of the code plugins this PaperPaw
//!   runs (`null` where it runs none);
//! - `GET /api/plugins`: the plugins installed, as over Wi-Fi;
//! - `POST /api/offer`, its body lines `plugin <id>`, `<path> <size>
//!   <sha256>` for each of its files, `book <name> <size> <sha256>`: what
//!   the computer means to send, at most [`MAX_OFFER`] bytes. The device
//!   asks the reader, with the names and sizes, and answers *202*
//!   `{"state": "asking"}`; *507* where there is not room for it all;
//!   *409* while it asks about another; `GET /api/offer` says `asking`,
//!   `allowed` or `refused`. The answer counts for that offer only;
//! - once allowed, and only what was offered, each file exactly as
//!   offered (its size and SHA-256): `PUT /api/plugin/<id>/<path>` and
//!   `POST /api/plugin/<id>` (installed in one step, as over Wi-Fi; its
//!   code runs, as the reader allowed), `PUT /api/book/<name>` (into the
//!   library folder, under a free name: never over a book). Each is
//!   taken once.
//!
//! Nothing else is read or written: no other files, no settings. The
//! permission holds while the screen is open; *Back* ends it, and so does
//! a while without a request.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::local::{Commit, Refusal, Staging, file_sha256};
use paperpaw_sdk::install::{self};
use paperpaw_sdk::replies::{Confirmed, ServerNotice};
use paperpaw_sdk::served::{CABLE_PORT, json_string, param};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Handled, Id, Instant, Params, Rect, RefreshMode, Screen,
    ServerResponse, files,
};

use crate::plugins;

const SERVER: Id = Id::new_static("transfer.cable-server");
pub(crate) const EVENTS: Event<ServerNotice> = Event::new("transfer.cable-events");
const ALLOWED: Event<Confirmed> = Event::new("transfer.cable-allowed");
/// The parameter of the question that names the offer it asks about.
const OFFER: &str = "offer";
/// Without a request this long, the screen closes and the permission
/// with it.
const IDLE_MS: u64 = 5 * 60 * 1000;
/// Files of an offer at most.
const MAX_FILES: usize = 64;
/// The longest offer read: room for [`MAX_FILES`] lines.
const MAX_OFFER: usize = 8 * 1024;
/// Books named on the device at most; the others are counted.
const NAMED: usize = 8;

/// A file offered: its name or path, size and SHA-256.
type Offered = (String, u64, [u8; 32]);

/// What the computer means to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Offer {
    /// A plugin: its id and files.
    plugin: Option<(String, Vec<Offered>)>,
    books: Vec<Offered>,
}

impl Offer {
    /// The offer in `text`, if every line is understood and names only
    /// what may be written.
    fn parse(text: &str) -> Option<Offer> {
        let mut offer = Offer::default();
        let mut files = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            files += 1;
            if files > MAX_FILES {
                return None;
            }
            let (word, rest) = line.split_once(' ')?;
            // `<name> <size> <sha256>`: a book's name may hold spaces.
            let sized = |rest: &str| -> Option<Offered> {
                let (rest, sha256) = rest.rsplit_once(' ')?;
                let (name, size) = rest.rsplit_once(' ')?;
                Some((
                    String::from(name.trim()),
                    size.parse().ok()?,
                    digest(sha256)?,
                ))
            };
            match word {
                "plugin" if offer.plugin.is_none() => {
                    let id = rest.trim();
                    Staging::new(id)?;
                    offer.plugin = Some((String::from(id), Vec::new()));
                }
                "book" => {
                    let book = sized(rest)?;
                    if !is_book_name(&book.0) {
                        return None;
                    }
                    offer.books.push(book);
                }
                path => {
                    let (_, plugin_files) = offer.plugin.as_mut()?;
                    let file = sized(line)?;
                    if file.0 != path || !install::is_safe_path(path) {
                        return None;
                    }
                    plugin_files.push(file);
                }
            }
        }
        let empty = offer.plugin.as_ref().is_none_or(|(_, f)| f.is_empty());
        (!(empty && offer.books.is_empty())).then_some(offer)
    }

    /// The bytes of all of it; `None` past what can be counted.
    fn size(&self) -> Option<u64> {
        let plugin = self.plugin.iter().flat_map(|(_, files)| files.iter());
        plugin
            .chain(self.books.iter())
            .try_fold(0u64, |sum, (_, size, _)| sum.checked_add(*size))
    }

    /// Whether the plugin offered has code.
    fn code(&self) -> bool {
        self.plugin.as_ref().is_some_and(|(_, files)| {
            files
                .iter()
                .any(|(path, ..)| paperpaw_sdk::native::CODE_FILES.contains(&path.as_str()))
        })
    }

    /// The size and SHA-256 offered of file `path` of plugin `id`.
    fn plugin_file(&self, id: &str, path: &str) -> Option<(u64, [u8; 32])> {
        let (offered, files) = self.plugin.as_ref()?;
        (offered == id).then_some(())?;
        files
            .iter()
            .find(|(p, ..)| p == path)
            .map(|(_, size, sha)| (*size, *sha))
    }

    /// The size and SHA-256 offered of book `name`.
    fn book(&self, name: &str) -> Option<(u64, [u8; 32])> {
        let found = self.books.iter().find(|(n, ..)| n == name);
        found.map(|(_, size, sha)| (*size, *sha))
    }
}

/// A name a book may be put under in the library folder: no folders, no
/// hidden file.
fn is_book_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

/// Where the reader stands on the offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Permission {
    None,
    Asking,
    Allowed,
    Refused,
}

pub(crate) struct CableScreen {
    style: UiStyle,
    header: ui::Header,
    offer: Option<Offer>,
    /// The number of the offer, given anew to each: the reader's answer
    /// counts for the one it was asked about only.
    serial: u32,
    permission: Permission,
    /// What happened last, said on the screen.
    last: Option<String>,
    active: Instant,
    started: bool,
}

impl CableScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            offer: None,
            serial: 0,
            permission: Permission::None,
            last: None,
            active: env.now,
            started: false,
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn said(&mut self, text: String, cx: &mut Cx<'_, '_>) {
        self.last = Some(text);
        self.redraw(cx);
    }

    /// Answers a request of the computer's.
    fn request(
        &mut self,
        number: u32,
        method: &str,
        target: &str,
        body: Option<&str>,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        self.active = env.now;
        cx.wake_at(env.now.add_millis(IDLE_MS));
        // A question closed without an answer (Back): not allowed.
        let asked = env
            .stack
            .last()
            .is_some_and(|top| top.slot == Some(slots::CONFIRM));
        if self.permission == Permission::Asking && !asked {
            self.forget_offer(&env);
            self.permission = Permission::Refused;
        }
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let json = |status, body: String| {
            ServerResponse::new(number, status, "application/json", body.into_bytes())
        };
        let mut out = Vec::new();
        let response = match (method, path) {
            ("GET", "/api/hello") => json(200, hello(&env)),
            ("GET", "/api/plugins") => json(200, plugins::listing(&env)),
            ("POST", "/api/offer") => self.offered(&env, body, number, cx),
            ("GET", "/api/offer") => json(200, self.permission_json()),
            _ if self.permission != Permission::Allowed => {
                json(403, String::from("{\"why\":\"not allowed on the device\"}"))
            }
            ("PUT", path) if path.starts_with("/api/plugin/") => {
                self.plugin_file(&env, path, query, body, number, cx)
            }
            ("POST", path) if path.starts_with("/api/plugin/") => {
                self.install(&env, path, number, &mut out, cx)
            }
            ("PUT", path) if path.starts_with("/api/book/") => {
                self.book(&env, path, query, body, number, &mut out, cx)
            }
            _ => json(404, String::from("{\"why\":\"not found\"}")),
        };
        // A body not taken goes (the kernel removes it once answered).
        out.push(Command::Respond(response));
        for command in out {
            cx.command(command);
        }
    }

    fn permission_json(&self) -> String {
        let state = match self.permission {
            Permission::None => "none",
            Permission::Asking => "asking",
            Permission::Allowed => "allowed",
            Permission::Refused => "refused",
        };
        format!("{{\"state\":{}}}", json_string(state))
    }

    /// An offer came: the reader is asked.
    fn offered(
        &mut self,
        env: &Env<'_>,
        body: Option<&str>,
        number: u32,
        cx: &mut Cx<'_, '_>,
    ) -> ServerResponse {
        let json = |status, body: &str| {
            ServerResponse::new(number, status, "application/json", body.as_bytes().to_vec())
        };
        // One question at a time: an answer is never taken for another.
        if self.permission == Permission::Asking {
            return json(409, "{\"why\":\"asking about another offer\"}");
        }
        let text = body
            .and_then(|path| paperpaw_sdk::read_file(env.storage, path, MAX_OFFER).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(offer) = text.as_deref().and_then(Offer::parse) else {
            return json(400, "{\"why\":\"not an offer\"}");
        };
        let room = env.storage.free_space();
        match offer.size() {
            Some(size) if room.is_none_or(|free| size <= free) => {}
            _ => return json(507, "{\"why\":\"not enough room\"}"),
        }
        // Nothing sent before counts: a plugin's files are taken anew.
        self.forget_offer(env);
        if let Some(staging) = offer.plugin.as_ref().and_then(|(id, _)| Staging::new(id)) {
            staging.abort(env.storage);
        }
        self.serial = self.serial.wrapping_add(1);
        let (question, text) = question(env, &offer);
        cx.command(
            paperpaw_sdk::ask::confirm(question, &ALLOWED)
                .text(text)
                .yes(env.i18n.tr("transfer.cable.allow"))
                .with(OFFER, format!("{}", self.serial))
                .command(),
        );
        self.offer = Some(offer);
        self.permission = Permission::Asking;
        json(202, "{\"state\":\"asking\"}")
    }

    /// Lets go of the offer, and of what was sent for its plugin.
    fn forget_offer(&mut self, env: &Env<'_>) {
        let id = self
            .offer
            .take()
            .and_then(|offer| offer.plugin)
            .map(|(id, _)| id);
        if let Some(staging) = id.as_deref().and_then(Staging::new) {
            staging.abort(env.storage);
        }
    }

    fn plugin_file(
        &mut self,
        env: &Env<'_>,
        path: &str,
        query: &str,
        body: Option<&str>,
        number: u32,
        cx: &mut Cx<'_, '_>,
    ) -> ServerResponse {
        let json = |status, body: String| {
            ServerResponse::new(number, status, "application/json", body.into_bytes())
        };
        let rest = path.trim_start_matches("/api/plugin/");
        let Some((id, file)) = rest.split_once('/') else {
            return json(404, String::from("{\"why\":\"not found\"}"));
        };
        let offered = self.offer.as_ref().and_then(|o| o.plugin_file(id, file));
        let (Some((size, sha256)), Some(body), Some(staging)) = (offered, body, Staging::new(id))
        else {
            return json(403, String::from("{\"why\":\"not offered\"}"));
        };
        if env.storage.size(body).ok() != Some(size) || !same_digest(query, &sha256) {
            return json(422, String::from("{\"why\":\"not as offered\"}"));
        }
        match staging.put(env.storage, file, body, &sha256) {
            Ok(()) => {
                let text = env
                    .i18n
                    .format("transfer.cable.receiving", &[("name", Arg::Str(id))]);
                self.said(text, cx);
                json(200, String::from("{\"ok\":true}"))
            }
            Err(refusal) => json(422, refused(refusal)),
        }
    }

    fn install(
        &mut self,
        env: &Env<'_>,
        path: &str,
        number: u32,
        out: &mut Vec<Command>,
        cx: &mut Cx<'_, '_>,
    ) -> ServerResponse {
        let json = |status, body: String| {
            ServerResponse::new(number, status, "application/json", body.into_bytes())
        };
        let id = path.trim_start_matches("/api/plugin/");
        let offered = self
            .offer
            .as_ref()
            .and_then(|o| o.plugin.as_ref())
            .is_some_and(|(p, _)| p == id);
        let (true, Some(staging)) = (offered, Staging::new(id)) else {
            return json(403, String::from("{\"why\":\"not offered\"}"));
        };
        // The reader allowed its code with the offer: the code sent is the
        // one offered (its SHA-256 checked as it came).
        let code = staging.code(env.storage);
        let commit = match code {
            Some(sha256) if self.offer.as_ref().is_some_and(Offer::code) => {
                Commit::approved(sha256)
            }
            _ => Commit::default(),
        };
        match staging.commit(env, commit, out) {
            Ok(done) => {
                if let Some(offer) = &mut self.offer {
                    offer.plugin = None;
                }
                let key = if done.code {
                    "transfer.cable.installed-code"
                } else {
                    "transfer.plugin-installed"
                };
                self.said(env.i18n.format(key, &[("name", Arg::Str(id))]), cx);
                let restart = done.code;
                json(
                    200,
                    format!("{{\"state\":\"installed\",\"restart\":{restart}}}"),
                )
            }
            Err(refusal) => {
                let text = env
                    .i18n
                    .format("transfer.cable.not-installed", &[("name", Arg::Str(id))]);
                self.said(text, cx);
                json(422, refused(refusal))
            }
        }
    }

    #[expect(clippy::too_many_arguments, reason = "a request's parts, passed on")]
    fn book(
        &mut self,
        env: &Env<'_>,
        path: &str,
        query: &str,
        body: Option<&str>,
        number: u32,
        out: &mut Vec<Command>,
        cx: &mut Cx<'_, '_>,
    ) -> ServerResponse {
        let json = |status, body: String| {
            ServerResponse::new(number, status, "application/json", body.into_bytes())
        };
        let name = paperpaw_sdk::web::percent_decode(path.trim_start_matches("/api/book/"));
        let offered = self.offer.as_ref().and_then(|o| o.book(&name));
        let (Some((size, sha256)), Some(body)) = (offered, body) else {
            return json(403, String::from("{\"why\":\"not offered\"}"));
        };
        if env.storage.size(body).ok() != Some(size)
            || !same_digest(query, &sha256)
            || file_sha256(env.storage, body) != Some(sha256)
        {
            return json(422, String::from("{\"why\":\"not as offered\"}"));
        }
        let folder = paperpaw_sdk::library::folder(env);
        let _ = env.storage.create_dir(folder);
        let at = files::free_path(env.storage, folder, &name);
        match files::move_to(env.storage, body, &at) {
            Ok(()) => {
                // Offered once, taken once.
                if let Some(offer) = &mut self.offer {
                    offer.books.retain(|(n, ..)| *n != name);
                }
                out.push(paperpaw_sdk::library::added(&at, "transfer"));
                let text = env
                    .i18n
                    .format("transfer.received", &[("name", Arg::Str(files::name(&at)))]);
                self.said(text, cx);
                json(200, format!("{{\"path\":{}}}", json_string(&at)))
            }
            Err(_) => json(500, String::from("{\"why\":\"storage\"}")),
        }
    }
}

/// What the device asks about `offer`: the question, and the names and
/// sizes below it.
fn question(env: &Env<'_>, offer: &Offer) -> (String, String) {
    let i18n = env.i18n;
    let decimal = i18n.tr("transfer.cable.decimal");
    let key = match (offer.plugin.is_some(), offer.books.is_empty()) {
        (true, true) => "transfer.cable.ask-plugin",
        (true, false) => "transfer.cable.ask-both",
        (false, _) => "transfer.cable.ask-books",
    };
    let mut lines: Vec<String> = Vec::new();
    if let Some((id, files)) = &offer.plugin {
        let size = files
            .iter()
            .fold(0u64, |sum, (_, s, _)| sum.saturating_add(*s));
        lines.push(i18n.format(
            "transfer.cable.plugin",
            &[
                ("name", Arg::Str(id)),
                ("size", Arg::Str(&files::size(size, decimal))),
            ],
        ));
        if offer.code() {
            lines.push(i18n.tr("transfer.cable.code").to_string());
        }
    }
    for (name, size, _) in offer.books.iter().take(NAMED) {
        lines.push(format!("{name} ({})", files::size(*size, decimal)));
    }
    if offer.books.len() > NAMED {
        let more = (offer.books.len() - NAMED) as i64;
        lines.push(i18n.format("transfer.cable.more", &[("count", Arg::Int(more))]));
    }
    (i18n.tr(key).to_string(), lines.join("\n"))
}

/// The device, its PaperPaw and the code plugins it runs, as JSON.
fn hello(env: &Env<'_>) -> String {
    let fingerprint = env.native.map_or(String::from("null"), |native| {
        json_string(&format!("{:016x}", native.fingerprint.0))
    });
    format!(
        "{{\"protocol\":1,\"device\":{},\"name\":{},\"version\":{},\"fingerprint\":{fingerprint}}}",
        json_string(env.device.id),
        json_string(env.device.name),
        json_string(&format!("{}", paperpaw_sdk::SDK_VERSION)),
    )
}

fn refused(refusal: Refusal) -> String {
    format!(
        "{{\"state\":\"refused\",\"why\":{}}}",
        json_string(plugins::why(refusal))
    )
}

fn digest(hex: &str) -> Option<[u8; 32]> {
    paperpaw_sdk::digest::hex::decode(hex)?.try_into().ok()
}

/// Whether a `sha256` the request names, if it names one, is `offered`.
fn same_digest(query: &str, offered: &[u8; 32]) -> bool {
    param(query, "sha256").is_none_or(|hex| digest(&hex).as_ref() == Some(offered))
}

impl Screen for CableScreen {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("transfer.cable.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let display = env.device.display;
        let top = self.header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let state = match self.permission {
            Permission::Allowed => "transfer.cable.allowed",
            Permission::Refused => "transfer.cable.refused",
            _ => "transfer.cable.connected",
        };
        let mut text = String::from(env.i18n.tr(state));
        if let Some(last) = &self.last {
            text.push_str("\n\n");
            text.push_str(last);
        }
        ui::draw_message(canvas, env, &style, area, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            cx.command(Command::serve(SERVER, CABLE_PORT, EVENTS.into()));
            cx.wake_at(cx.env().now.add_millis(IDLE_MS));
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if cx.env().now >= self.active.add_millis(IDLE_MS) {
            let env = *cx.env();
            self.forget_offer(&env);
            cx.command(Command::Back);
        } else {
            cx.wake_at(self.active.add_millis(IDLE_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            // The permission ends with the screen, and what was sent for
            // a plugin not installed goes.
            let env = *cx.env();
            self.forget_offer(&env);
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(ServerNotice::Request {
            request,
            method,
            target,
            body_file,
            ..
        }) = EVENTS.read(event, params)
        {
            self.request(request, &method, &target, body_file.as_deref(), cx);
        } else if let Some(answer) = ALLOWED.read(event, params)
            && self.permission == Permission::Asking
            && answer.context.get(OFFER) == Some(format!("{}", self.serial).as_str())
        {
            self.permission = if answer.yes {
                Permission::Allowed
            } else {
                let env = *cx.env();
                self.forget_offer(&env);
                Permission::Refused
            };
            self.redraw(cx);
        }
    }
}
