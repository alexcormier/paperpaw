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
    ActionSpec, Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest,
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
/// Without a request this long, the reader's own network closes (and
/// the screen with it): it is not left open by mistake.
const OWN_IDLE_MS: u64 = 10 * 60 * 1000;

/// The web page; its texts come from the translations.
const PAGE: &str = include_str!("page.html");

/// The transfer plugin.
#[derive(Debug, Default)]
pub struct Transfer;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Transfer {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Transfer", Version::new(0, 1, 0))
            .description(
                "Put books and other files on the device from a browser in the same network.",
            )
            .how(&HOW)
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
    /// Offline mode: the offer to go online for the transfer.
    Offline(ui::OfflineOffer),
}

/// The reader's own network, for a transfer without a router: its name
/// and password, and the code to join it.
struct OwnNetwork {
    ssid: String,
    password: String,
    qr: Option<QrCode>,
}

struct TransferScreen {
    style: UiStyle,
    header: ui::Header,
    state: State,
    /// Without a router: the reader's own network (once chosen), and when
    /// a request came last.
    own: Option<OwnNetwork>,
    active: paperpaw_sdk::Instant,
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
            own: None,
            active: env.now,
            guard: None,
            page: page(env),
            last: None,
            started: false,
        }
    }

    /// What the reader chose on the offer to go online: served now, or
    /// without a network as without one.
    fn offered(&mut self, chosen: Option<ui::Offer>, cx: &mut Cx<'_, '_>) {
        match chosen {
            Some(ui::Offer::Online) => {
                self.state = State::Starting;
                cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
            }
            Some(ui::Offer::NotNow) => self.state = State::NoNetwork,
            None => return,
        }
        self.redraw(cx);
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// The *Without a router* button at the bottom: where PaperPaw runs
    /// Wi-Fi, before it was chosen.
    fn own_button(&self, env: &Env<'_>) -> Option<Rect> {
        if env.wifi.is_none() || self.own.is_some() || !env.random.available() {
            return None;
        }
        let area = self.content(env);
        let style = &self.style;
        let row = style.touch_row(style.control_row) as u16;
        let margin = style.margin as u16;
        let width = area.width.saturating_sub(2 * margin);
        // A touch row, higher where its words take more lines.
        let label = env.i18n.tr("transfer.without-router");
        let slot = ui::button_slot_height(env, style, width, label);
        let bottom = area.y + area.height - ui::edge_room(style, row);
        Some(ui::button_in_slot(
            style,
            Rect::new(margin, bottom.saturating_sub(slot), width, slot),
        ))
    }

    /// Opens the reader's own network and serves the page there: for a
    /// transfer where there is no router (or the computer is in another
    /// network).
    fn without_router(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Letters not taken for one another: read off the screen, typed.
        let Some(password) = env.random.readable(15) else {
            return;
        };
        // Named as the device is, for the phone to find it.
        let ssid = paperpaw_sdk::device_name::ssid(&env.device_name());
        let qr = QrCode::new(paperpaw_sdk::wifi::join_code(&ssid, &password).as_bytes());
        cx.command(Command::StopServing(SERVER));
        if !env.wifi.is_some_and(|w| w.state.is_on()) {
            cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::On));
        }
        cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::Host {
            ssid: ssid.clone(),
            password: password.clone(),
        }));
        self.own = Some(OwnNetwork { ssid, password, qr });
        self.guard = None;
        self.state = State::Starting;
        self.touched(cx);
        self.redraw(cx);
    }

    /// A request came (or the own network opened): the idle time counts
    /// from now.
    fn touched(&mut self, cx: &mut Cx<'_, '_>) {
        self.active = cx.env().now;
        if self.own.is_some() {
            cx.wake_at(self.active.add_millis(OWN_IDLE_MS));
        }
    }

    /// Closes the screen, and the reader's own network if it is open.
    fn close(&mut self, cx: &mut Cx<'_, '_>) {
        if self.own.is_some() && cx.env().wifi.is_some_and(|w| w.state.is_hosting()) {
            cx.command(Command::Wifi(paperpaw_sdk::wifi::WifiCommand::Leave));
        }
        cx.command(Command::Back);
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
                self.touched(cx);
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
    // The page's title: the device, as it is called.
    json.push_str(&format!(
        ",name:{}",
        served::json_string(&env.device_name())
    ));
    json.push('}');
    PAGE.replace("/*STRINGS*/{}", &json)
}

/// The code to join the reader's own network in `area`, its name and
/// password below it, to type where the code cannot be scanned.
fn draw_join(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    own: &OwnNetwork,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let margin = style.margin as u16;
    let width = area.width.saturating_sub(2 * margin);
    let line = font.line_metrics(style.text).line_height as u16;
    let small = font.line_metrics(style.small).line_height as u16;
    let title = env.i18n.tr("transfer.own.join");
    ui::draw_label(
        canvas,
        &font,
        style.text,
        f32::from(margin),
        Rect::new(margin, area.y, width, line),
        title,
        Color::BLACK,
    );
    let note = env.i18n.format(
        "transfer.own.network",
        &[
            ("ssid", Arg::Str(&own.ssid)),
            (
                "password",
                Arg::Str(&paperpaw_sdk::random::grouped(&own.password)),
            ),
        ],
    );
    let bottom = area.y + area.height;
    ui::draw_label(
        canvas,
        &font,
        style.small,
        f32::from(margin),
        Rect::new(margin, bottom.saturating_sub(small), width, small),
        &note,
        style.faint(60),
    );
    if let Some(qr) = &own.qr {
        let room = Rect::new(
            margin,
            area.y + line,
            width,
            area.height.saturating_sub(line + small),
        );
        ui::draw_qr(canvas, qr, room);
    }
}

impl Screen for TransferScreen {
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        // Back, and Select for the transfer without a router.
        if *action == actions::CONFIRM && self.own_button(env).is_some() {
            return paperpaw_sdk::Hint::Text(env.i18n.tr("transfer.without-router").into());
        }
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("transfer.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let area = self.content(env);
        let own_button = self.own_button(env);
        let (url, qr) = match &mut self.state {
            State::Starting => {
                // While it waits for a network: or without a router.
                let button = self.own_button(env);
                let room = match button {
                    Some(b) => Rect::new(area.x, area.y, area.width, b.y.saturating_sub(area.y)),
                    None => area,
                };
                ui::draw_message(canvas, env, &style, room, env.i18n.tr("transfer.starting"));
                if let Some(button) = button {
                    let label = env.i18n.tr("transfer.without-router");
                    let focused = !env.device.input.touch;
                    ui::draw_button(canvas, env, &style, button, label, false, focused);
                }
                return;
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
                // Or without a router, through the reader's own network.
                let button = self.own_button(env);
                let room = match button {
                    Some(b) => Rect::new(area.x, area.y, area.width, b.y.saturating_sub(area.y)),
                    None => area,
                };
                ui::draw_empty(
                    canvas,
                    env,
                    &style,
                    room,
                    "wifi-off",
                    env.i18n.tr("transfer.no-network.title"),
                    env.i18n.tr(text),
                );
                if let Some(button) = button {
                    let label = env.i18n.tr("transfer.without-router");
                    let focused = !env.device.input.touch;
                    ui::draw_button(canvas, env, &style, button, label, false, focused);
                }
                return;
            }
            State::Offline(offer) => return offer.draw(canvas, env, &style, area),
            State::Serving { url, qr } => (url, qr),
        };
        // Without a router: first the code to join the reader's network.
        let area = match &self.own {
            Some(own) => {
                let half = area.height / 2;
                draw_join(
                    canvas,
                    env,
                    &style,
                    Rect::new(area.x, area.y, area.width, half),
                    own,
                );
                Rect::new(area.x, area.y + half, area.width, area.height - half)
            }
            None => area,
        };
        let button = own_button;
        let area = match button {
            Some(button) => Rect::new(
                area.x,
                area.y,
                area.width,
                button.y.saturating_sub(area.y + style.gap as u16),
            ),
            None => area,
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
        if let Some(button) = button {
            let label = env.i18n.tr("transfer.without-router");
            let focused = !env.device.input.touch;
            ui::draw_button(canvas, env, &style, button, label, false, focused);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        // The own network left alone: closed.
        if self.own.is_some() {
            if cx.env().now >= self.active.add_millis(OWN_IDLE_MS) {
                self.close(cx);
            } else {
                cx.wake_at(self.active.add_millis(OWN_IDLE_MS));
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.started {
            self.started = true;
            if !cx.env().random.available() {
                self.state = State::Failed(no_random(cx.env()));
                self.redraw(cx);
                return;
            }
            let offer = ui::OfflineOffer::new();
            if offer.shows(cx.env()) {
                self.state = State::Offline(offer);
                self.redraw(cx);
                return;
            }
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            // Closing the screen also ends the transfer.
            self.close(cx);
            Handled::Yes
        } else if *action == actions::CONFIRM && self.own_button(cx.env()).is_some() {
            self.without_router(cx);
            Handled::Yes
        } else if let State::Offline(offer) = &mut self.state {
            let chosen = offer.on_action(action, cx);
            self.offered(chosen, cx);
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None)
        {
            if action == actions::BACK {
                self.close(cx);
            } else {
                cx.command(Command::RunAction(action));
            }
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap
            && self
                .own_button(cx.env())
                .is_some_and(|b| b.contains(gesture.start))
        {
            self.without_router(cx);
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap
            && let State::Offline(offer) = &mut self.state
        {
            let chosen = offer.on_tap(gesture.start, cx);
            self.offered(chosen, cx);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server_event(notice, cx);
        } else if *event == events::WIFI_CHANGED
            && self.own.is_some()
            && matches!(self.state, State::Starting)
            && cx.env().wifi.is_some_and(|w| w.state.is_hosting())
        {
            // The own network open: the page served there.
            cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
        }
    }
}

#[cfg(test)]
mod tests;
