//! Transfer by browser: while the screen is open, the device serves a
//! web page to the local network for putting books and other files on
//! it, fetching them and tidying up, from a phone or a computer, no
//! cable or app needed.
//!
//! The screen shows the page's address as text and as a QR code. The
//! address carries a key drawn anew each time from the platform's random
//! source, so others in the network cannot get at the files; a few wrong
//! keys, and no request is answered for a while (see `paperpaw_sdk::served`). Closing
//! the screen ends the transfer.
//!
//! The same way, the plugin lets one type text on a phone for the
//! keyboard (slot `core.remote-keyboard`, see `phone`).

#![no_std]

extern crate alloc;

mod phone;
mod web;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_qr::QrCode;
use paperpaw_sdk::builtin::{actions, events, menus, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::ServerNotice;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest,
    MenuEntry, Params, Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar, Screen,
    ScreenSpec, Version,
};

use paperpaw_sdk::served::{self, Guard};

pub const PLUGIN: Id = Id::new_static("transfer");
/// Opens the transfer screen.
pub const OPEN: Id = Id::new_static("transfer.open");
const SCREEN: Id = Id::new_static("transfer.screen");
const SERVER: Id = Id::new_static("transfer.server");
const SERVER_EVENT: Event<ServerNotice> = Event::new("transfer.server-event");
/// Typing on a phone for the keyboard.
const PHONE_SCREEN: Id = Id::new_static("transfer.phone");

/// The port asked for; the next free one is taken if it is in use.
pub(crate) const PORT: u16 = 8080;

/// The web page; its texts come from the translations.
const PAGE: &str = include_str!("page.html");

/// The transfer plugin.
#[derive(Debug, Default)]
pub struct Transfer;

impl Plugin for Transfer {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Transfer", Version::new(0, 1, 0))
            .description(
                "Put books and other files on the device from a browser in the same network.",
            )
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "transfer.title",
            Box::new(|env, _| Box::new(TransferScreen::new(env))),
        ))?;
        r.add_action(ActionSpec::new(OPEN, "transfer.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            },
        ))?;
        r.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN).order(46))?;
        r.add_icon(OPEN, Cow::Borrowed("transfer"))?;
        r.add_screen(ScreenSpec::new(
            PHONE_SCREEN,
            "transfer.phone.title",
            Box::new(|env, params| Box::new(phone::PhoneScreen::new(env, params))),
        ))?;
        r.bind_slot(slots::REMOTE_KEYBOARD, PHONE_SCREEN, Priority::DEFAULT)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[derive(Debug)]
enum State {
    Starting,
    /// Serving at `url`.
    Serving {
        url: String,
        qr: Option<QrCode>,
    },
    Failed(String),
    /// Without a network: how to get one.
    NoNetwork,
}

struct TransferScreen {
    style: UiStyle,
    header: ui::Header,
    state: State,
    /// The key in the address, drawn once the server runs.
    guard: Option<Guard>,
    /// The web page with its texts.
    page: String,
    /// What happened last.
    last: Option<String>,
    started: bool,
}

impl TransferScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Starting,
            guard: None,
            page: page(env),
            last: None,
            started: false,
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// The area below the header.
    fn content(&self, env: &Env<'_>) -> Rect {
        let display = env.device.display;
        let top = self.header.area.height + self.style.gap as u16;
        Rect::new(0, top, display.width, display.height.saturating_sub(top))
    }

    fn on_server_event(&mut self, notice: ServerNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            ServerNotice::Started { address, .. } => {
                // Drawn now: on the X4, the radio makes it fully random.
                self.state = match served::key(&env.random) {
                    Some(key) => {
                        let url = format!("http://{address}/?k={key}");
                        self.guard = Some(Guard::new(key));
                        let qr = QrCode::new(url.as_bytes());
                        State::Serving { url, qr }
                    }
                    None => {
                        cx.command(Command::StopServing(SERVER));
                        State::Failed(no_random(&env))
                    }
                };
                self.redraw(cx);
            }
            ServerNotice::Failed { reason, .. } => {
                // Before it served, or after (e.g. the network went).
                let key = if matches!(self.state, State::Serving { .. }) {
                    "transfer.stopped"
                } else {
                    "transfer.failed"
                };
                self.state = if reason == "no network" {
                    State::NoNetwork
                } else {
                    State::Failed(env.i18n.format(key, &[("reason", Arg::Str(&reason))]))
                };
                self.redraw(cx);
            }
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                let Some(guard) = &mut self.guard else {
                    return;
                };
                let request = web::Request {
                    number: request,
                    method: &method,
                    target: &target,
                    body_file: body_file.as_deref(),
                    at: env.now,
                };
                let (response, done) = web::answer(env.storage, guard, &self.page, request);
                cx.command(Command::Respond(response));
                if let Some(done) = done {
                    // The library follows: a book added, a book removed (what
                    // plugins keep for it goes too; a folder removed, its
                    // books with it).
                    match &done {
                        web::Done::Received(path) => {
                            cx.command(paperpaw_sdk::library::added(path, "transfer"));
                        }
                        web::Done::Deleted(path) => {
                            let removed = events::BookRemoved::new(path.clone());
                            cx.command(events::BOOK_REMOVED.emit(removed));
                            cx.command(paperpaw_sdk::library::changed());
                        }
                        web::Done::Folder(_) | web::Done::Sent(_) | web::Done::NotReceived(_) => {}
                    }
                    let (key, name) = match &done {
                        web::Done::Received(path) => {
                            ("transfer.received", paperpaw_sdk::files::name(path))
                        }
                        web::Done::Sent(name) => ("transfer.sent", name.as_str()),
                        web::Done::Folder(name) => ("transfer.folder", name.as_str()),
                        web::Done::Deleted(path) => {
                            ("transfer.deleted", paperpaw_sdk::files::name(path))
                        }
                        web::Done::NotReceived(name) => ("transfer.not-received", name.as_str()),
                    };
                    self.last = Some(env.i18n.format(key, &[("name", Arg::Str(name))]));
                    let area = self.content(&env);
                    cx.request_redraw_area(area);
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            _ => {}
        }
    }
}

/// What the screen says where no key can be drawn: the platform has no
/// source of random numbers, and nothing is served.
pub(crate) fn no_random(env: &Env<'_>) -> String {
    env.i18n.tr("transfer.no-random").into()
}

/// The web page, with its texts in the device's language.
fn page(env: &Env<'_>) -> String {
    let texts = [
        ("title", "transfer.web.title"),
        ("device", "transfer.web.device"),
        ("upload", "transfer.web.upload"),
        ("drop", "transfer.web.drop"),
        ("newFolder", "transfer.web.new-folder"),
        ("folderName", "transfer.web.folder-name"),
        ("delete", "transfer.web.delete"),
        ("deleteQuestion", "transfer.web.delete-question"),
        ("empty", "transfer.web.empty"),
        ("uploading", "transfer.web.uploading"),
        ("done", "transfer.web.done"),
        ("failed", "transfer.web.failed"),
    ];
    let mut json = String::from("{");
    for (i, (name, key)) in texts.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("{name}:{}", served::json_string(env.i18n.tr(key))));
    }
    json.push('}');
    PAGE.replace("/*STRINGS*/{}", &json)
}

impl Screen for TransferScreen {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        // Only back does anything here.
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("transfer.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let area = self.content(env);
        let (url, qr) = match &self.state {
            State::Starting => {
                return ui::draw_message(
                    canvas,
                    env,
                    &style,
                    area,
                    env.i18n.tr("transfer.starting"),
                );
            }
            State::Failed(message) => {
                return ui::draw_message(canvas, env, &style, area, message);
            }
            State::NoNetwork => {
                // Where PaperPaw runs Wi-Fi, its settings join a network;
                // elsewhere the system's.
                let text = if env.wifi.is_some() {
                    "transfer.no-network"
                } else {
                    "transfer.no-network.system"
                };
                return ui::draw_empty(
                    canvas,
                    env,
                    &style,
                    area,
                    "wifi-off",
                    env.i18n.tr("transfer.no-network.title"),
                    env.i18n.tr(text),
                );
            }
            State::Serving { url, qr } => (url, qr),
        };
        let hint = env.i18n.tr("transfer.hint");
        ui::draw_address(
            canvas,
            env,
            &style,
            area,
            url,
            qr.as_ref(),
            hint,
            self.last.as_deref(),
        );
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            if !cx.env().random.available() {
                self.state = State::Failed(no_random(cx.env()));
                self.redraw(cx);
                return;
            }
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            // Closing the screen also ends the transfer.
            cx.command(Command::Back);
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server_event(notice, cx);
        }
    }
}

#[cfg(test)]
mod tests;
