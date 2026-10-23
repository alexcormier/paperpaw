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
//! - `POST /api/offer`, its body lines `plugin <id>`, `<path> <size>` for
//!   each of its files, `book <name> <size>`: what the computer means to
//!   send. The device asks the reader, with the names and sizes, and
//!   answers *202* `{"state": "asking"}`; *507* where there is not room
//!   for it all; `GET /api/offer` says `asking`, `allowed` or `refused`;
//! - once allowed, and only what was offered, at the size offered:
//!   `PUT /api/plugin/<id>/<path>?sha256=<hex>` and `POST
//!   /api/plugin/<id>` (installed in one step, as over Wi-Fi; its code
//!   runs, as the reader allowed), `PUT /api/book/<name>?sha256=<hex>`
//!   (into the library folder, under a free name: never over a book).
//!   Each is taken once.
//!
//! Nothing else is read or written: no other files, no settings. The
//! permission holds while the screen is open; *Back* ends it, and so does
//! a while without a request.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
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
/// Without a request this long, the screen closes and the permission
/// with it.
const IDLE_MS: u64 = 5 * 60 * 1000;
/// Files of an offer at most.
const MAX_FILES: usize = 256;
/// The longest offer read.
const MAX_OFFER: usize = 32 * 1024;
/// Books named on the device at most; the others are counted.
const NAMED: usize = 8;

/// What the computer means to send.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Offer {
    /// A plugin: its id and files, each with its size.
    plugin: Option<(String, Vec<(String, u64)>)>,
    books: Vec<(String, u64)>,
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
            match word {
                "plugin" if offer.plugin.is_none() => {
                    let id = rest.trim();
                    Staging::new(id)?;
                    offer.plugin = Some((String::from(id), Vec::new()));
                }
                "book" => {
                    let (name, size) = rest.rsplit_once(' ')?;
                    let name = name.trim();
                    if !is_book_name(name) {
                        return None;
                    }
                    offer.books.push((String::from(name), size.parse().ok()?));
                }
                path => {
                    let (_, plugin_files) = offer.plugin.as_mut()?;
                    if !install::is_safe_path(path) {
                        return None;
                    }
                    plugin_files.push((String::from(path), rest.trim().parse().ok()?));
                }
            }
        }
        let empty = offer.plugin.as_ref().is_none_or(|(_, f)| f.is_empty());
        (!(empty && offer.books.is_empty())).then_some(offer)
    }

    fn size(&self) -> u64 {
        let plugin: u64 = self
            .plugin
            .iter()
            .flat_map(|(_, f)| f.iter().map(|(_, s)| *s))
            .sum();
        plugin + self.books.iter().map(|(_, s)| *s).sum::<u64>()
    }

    /// Whether the plugin offered has code.
    fn code(&self) -> bool {
        self.plugin.as_ref().is_some_and(|(_, files)| {
            files
                .iter()
                .any(|(path, _)| paperpaw_sdk::native::CODE_FILES.contains(&path.as_str()))
        })
    }

    fn plugin_file(&self, id: &str, path: &str) -> Option<u64> {
        let (offered, files) = self.plugin.as_ref()?;
        (offered == id).then_some(())?;
        files.iter().find(|(p, _)| p == path).map(|(_, s)| *s)
    }

    fn book(&self, name: &str) -> Option<u64> {
        self.books.iter().find(|(n, _)| n == name).map(|(_, s)| *s)
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
        let text = body
            .and_then(|path| paperpaw_sdk::read_file(env.storage, path, MAX_OFFER).ok())
            .and_then(|bytes| String::from_utf8(bytes).ok());
        let Some(offer) = text.as_deref().and_then(Offer::parse) else {
            return json(400, "{\"why\":\"not an offer\"}");
        };
        if env
            .storage
            .free_space()
            .is_some_and(|free| offer.size() > free)
        {
            return json(507, "{\"why\":\"not enough room\"}");
        }
        let (question, text) = question(env, &offer);
        cx.command(
            paperpaw_sdk::ask::confirm(question, &ALLOWED)
                .text(text)
                .yes(env.i18n.tr("transfer.cable.allow"))
                .command(),
        );
        self.offer = Some(offer);
        self.permission = Permission::Asking;
        json(202, "{\"state\":\"asking\"}")
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
        let (Some(size), Some(body), Some(staging)) = (offered, body, Staging::new(id)) else {
            return json(403, String::from("{\"why\":\"not offered\"}"));
        };
        if env.storage.size(body).ok() != Some(size) {
            return json(422, String::from("{\"why\":\"not the size offered\"}"));
        }
        let Some(sha256) = param(query, "sha256").and_then(|h| digest(&h)) else {
            return json(400, String::from("{\"why\":\"sha256\"}"));
        };
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
        // The reader allowed its code with the offer.
        let commit = if self.offer.as_ref().is_some_and(Offer::code) {
            Commit::approved()
        } else {
            Commit::default()
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
        let (Some(size), Some(body)) = (offered, body) else {
            return json(403, String::from("{\"why\":\"not offered\"}"));
        };
        if env.storage.size(body).ok() != Some(size) {
            return json(422, String::from("{\"why\":\"not the size offered\"}"));
        }
        let wanted = param(query, "sha256").and_then(|h| digest(&h));
        if wanted.is_none() || file_sha256(env.storage, body) != wanted {
            return json(422, String::from("{\"why\":\"damaged\"}"));
        }
        let folder = paperpaw_sdk::library::folder(env);
        let _ = env.storage.create_dir(folder);
        let at = files::free_path(env.storage, folder, &name);
        match files::move_to(env.storage, body, &at) {
            Ok(()) => {
                // Offered once, taken once.
                if let Some(offer) = &mut self.offer {
                    offer.books.retain(|(n, _)| *n != name);
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
        let size: u64 = files.iter().map(|(_, s)| *s).sum();
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
    for (name, size) in offer.books.iter().take(NAMED) {
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
            cx.command(Command::Back);
        } else {
            cx.wake_at(self.active.add_millis(IDLE_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            // The permission ends with the screen.
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
        {
            self.permission = if answer.yes {
                Permission::Allowed
            } else {
                self.offer = None;
                Permission::Refused
            };
            self.redraw(cx);
        }
    }
}
