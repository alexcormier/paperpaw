//! Setting up Wi-Fi with a phone: the device looks for networks, then
//! opens a network of its own (`WifiCommand::Host`) and shows two QR
//! codes: one joins it, the other opens the page it serves there. On the
//! page the reader chooses the network and types its password with the
//! phone's keyboard; the device closes its own network and joins that
//! one, keeping the password once joined (as the Wi-Fi page does).
//!
//! The own network has a password drawn anew each time, shown with its
//! code, so only who sees the screen joins it. Joining fails: the own
//! network opens again, to try once more from the phone.

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::replies::ServerNotice;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::wifi::{WifiCommand, WifiState};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, ServerResponse,
};

use crate::Shared;

const SERVER: Id = Id::new_static("wifi.phone-server");
const SERVER_EVENT: Event<ServerNotice> = Event::new("wifi.phone-server-event");
/// The page's port: the address alone opens it.
const PORT: u16 = 80;
/// The page around what it shows.
const PAGE: &str = include_str!("phone.html");
/// How long after the page answered the device leaves its own network,
/// so that the answer reaches the phone first.
const LEAVE_MS: u64 = 1500;
/// How long "Connected" shows before the screen closes.
const DONE_MS: u64 = 2500;
/// Longest form taken, in bytes.
const MAX_FORM: usize = 1024;
/// Without a request from the phone this long, the own network closes
/// (and the screen with it): it is not left open by mistake.
const IDLE_MS: u64 = 10 * 60 * 1000;

/// Not `Debug`: it holds a password.
enum State {
    /// Looking for networks, to list them on the page.
    Searching,
    /// The own network opens.
    Opening,
    /// The own network is open; the page's address once it is served.
    Hosting {
        page: Option<(String, Option<QrCode>)>,
    },
    /// The network chosen on the phone, joined once the answer went.
    Leaving {
        ssid: String,
        password: String,
    },
    Joining(String),
    Joined(String),
    Failed(String),
}

/// The own network: its name and password, and its code to join it.
struct Own {
    ssid: String,
    password: String,
    qr: Option<QrCode>,
}

pub(crate) struct PhoneScreen {
    policy: Shared,
    style: UiStyle,
    header: ui::Header,
    state: State,
    own: Option<Own>,
    /// Why joining failed last, said above the codes.
    problem: Option<String>,
    /// Whether the user had switched the radio on before (it stays on
    /// while this screen is open).
    manual: bool,
    started: bool,
    /// Drawn anew each time, in the page's form: a form sent from another
    /// page in the phone's browser does not have it.
    token: String,
    /// When the phone asked last (or the own network opened).
    active: paperpaw_sdk::Instant,
}

impl PhoneScreen {
    pub(crate) fn new(env: &Env<'_>, policy: Shared) -> Self {
        let style = UiStyle::new(env);
        let own = env
            .random
            .token(4)
            .zip(env.random.token(10))
            .map(|(id, password)| {
                let ssid = format!("PaperPaw-{id}");
                let qr = QrCode::new(format!("WIFI:T:WPA;S:{ssid};P:{password};;").as_bytes());
                Own { ssid, password, qr }
            });
        let manual = policy.borrow().manual;
        Self {
            policy,
            header: ui::Header::new(env, &style, true),
            style,
            state: State::Searching,
            own,
            problem: None,
            manual,
            started: false,
            token: env.random.token(13).unwrap_or_default(),
            active: env.now,
        }
    }

    /// The phone did something (or the own network opened): the idle
    /// time counts from now.
    fn touched(&mut self, cx: &mut Cx<'_, '_>) {
        self.active = cx.env().now;
        cx.wake_at(self.active.add_millis(IDLE_MS));
    }

    fn redraw(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens the own network.
    fn host(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(own) = &self.own else {
            return;
        };
        self.state = State::Opening;
        cx.command(Command::Wifi(WifiCommand::Host {
            ssid: own.ssid.clone(),
            password: own.password.clone(),
        }));
    }

    fn on_wifi(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(wifi) = env.wifi else {
            return;
        };
        match (&self.state, &wifi.state) {
            (State::Opening, WifiState::Hosting { .. }) => {
                self.state = State::Hosting { page: None };
                cx.command(Command::serve(SERVER, PORT, SERVER_EVENT.into()));
                self.touched(cx);
            }
            (State::Joining(ssid), WifiState::Online { ssid: joined, .. }) if ssid == joined => {
                self.state = State::Joined(joined.clone());
                cx.wake_at(env.now.add_millis(DONE_MS));
            }
            _ => return,
        }
        Self::redraw(cx);
    }

    fn on_server(&mut self, notice: ServerNotice, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match notice {
            ServerNotice::Started { address, .. } => {
                if let State::Hosting { page } = &mut self.state {
                    // Without the port where it is the usual one.
                    let address = address.strip_suffix(":80").unwrap_or(&address);
                    let url = format!("http://{address}/");
                    let qr = QrCode::new(url.as_bytes());
                    *page = Some((url, qr));
                }
            }
            ServerNotice::Failed { reason, .. } => {
                self.state = State::Failed(
                    env.i18n
                        .format("wifi.phone.failed", &[("reason", Arg::Str(&reason))]),
                );
            }
            ServerNotice::Request {
                request,
                method,
                target,
                body_file,
                ..
            } => {
                self.touched(cx);
                let path = target.split_once('?').map_or(target.as_str(), |(p, _)| p);
                let form = body_file
                    .as_deref()
                    .and_then(|file| paperpaw_sdk::read_file(env.storage, file, MAX_FORM).ok())
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                let token = self.token.as_str();
                let (status, body, chosen) = match (method.as_str(), path) {
                    // Only the page's own form: not one another page sent.
                    ("POST", "/join")
                        if token.is_empty()
                            || form.as_deref().and_then(|f| field(f, "t")).as_deref()
                                != Some(token) =>
                    {
                        (403, choose_page(&env, true, token), None)
                    }
                    ("POST", "/join") => match form.as_deref().and_then(chosen) {
                        Some((ssid, password)) => {
                            let text = env
                                .i18n
                                .format("wifi.phone.joining-page", &[("ssid", Arg::Str(&ssid))]);
                            let body = format!("<h1>{}</h1>", escape(&text));
                            (200, page(&env, &body), Some((ssid, password)))
                        }
                        None => (400, choose_page(&env, true, token), None),
                    },
                    ("GET", "/") => (200, choose_page(&env, false, token), None),
                    _ => (404, page(&env, "<h1>404</h1>"), None),
                };
                cx.command(Command::Respond(ServerResponse::new(
                    request,
                    status,
                    "text/html; charset=utf-8",
                    body.into_bytes(),
                )));
                let Some((ssid, password)) = chosen else {
                    return;
                };
                self.state = State::Leaving { ssid, password };
                cx.wake_at(env.now.add_millis(LEAVE_MS));
            }
            _ => return,
        }
        Self::redraw(cx);
    }

    fn on_failed(
        &mut self,
        ssid: &str,
        reason: paperpaw_sdk::wifi::JoinFailure,
        cx: &mut Cx<'_, '_>,
    ) {
        let env = *cx.env();
        // The own network could not open.
        if matches!(self.state, State::Opening)
            && self.own.as_ref().is_some_and(|own| own.ssid == ssid)
        {
            self.state = State::Failed(env.i18n.tr("wifi.phone.not-opened").into());
            self.touched(cx);
            return Self::redraw(cx);
        }
        if !matches!(&self.state, State::Joining(joining) if joining == ssid) {
            return;
        }
        self.problem = Some(crate::failure_text(&env, ssid, reason));
        self.host(cx);
        Self::redraw(cx);
    }

    /// Closes the screen: the page no longer served, the own network
    /// closed, the radio as the user left it.
    fn close(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::StopServing(SERVER));
        if cx.env().wifi.is_some_and(|w| w.state.is_hosting()) {
            cx.command(Command::Wifi(WifiCommand::Leave));
        }
        let mut policy = self.policy.borrow_mut();
        policy.manual = self.manual;
        policy.phone = false;
        policy.goal = crate::policy::Goal::Nothing;
        drop(policy);
        cx.command(Command::Back);
    }

    fn draw_codes(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, own: &Own) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let margin = style.margin as u16;
        let width = area.width.saturating_sub(2 * margin);
        let line = font.line_metrics(style.text).line_height as u16;
        let small = font.line_metrics(style.small).line_height as u16;
        let mut y = area.y;
        if let Some(problem) = &self.problem {
            let rect = Rect::new(margin, y, width, line * 2);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(margin),
                rect,
                problem,
                Color::BLACK,
            );
            y += line * 2;
        }
        let page = match &self.state {
            State::Hosting { page } => page.as_ref(),
            _ => None,
        };
        let half = area.bottom().saturating_sub(u32::from(y)) as u16 / 2;
        let join = env.i18n.tr("wifi.phone.step-join");
        let note = env.i18n.format(
            "wifi.phone.network",
            &[
                ("ssid", Arg::Str(&own.ssid)),
                ("password", Arg::Str(&own.password)),
            ],
        );
        let open = env.i18n.tr("wifi.phone.step-open");
        let steps = [
            (join, note, own.qr.as_ref()),
            (
                open,
                page.map_or_else(String::new, |(url, _)| url.clone()),
                page.and_then(|(_, qr)| qr.as_ref()),
            ),
        ];
        for (n, (title, note, qr)) in steps.into_iter().enumerate() {
            let top = y + half * n as u16;
            let title_row = Rect::new(margin, top, width, line);
            ui::draw_label(
                canvas,
                &font,
                style.text,
                f32::from(margin),
                title_row,
                title,
                Color::BLACK,
            );
            let note_row = Rect::new(margin, top + half - small, width, small);
            let faint = style.faint(60);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(margin),
                note_row,
                &note,
                faint,
            );
            let room = Rect::new(margin, top + line, width, half.saturating_sub(line + small));
            match qr {
                Some(qr) => {
                    ui::draw_qr(canvas, qr, room);
                }
                None => {
                    ui::draw_message(canvas, env, &style, room, env.i18n.tr("wifi.phone.opening"))
                }
            }
        }
    }
}

/// Field `name` of a form sent, decoded.
fn field(form: &str, name: &str) -> Option<String> {
    form.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| paperpaw_sdk::web::percent_decode(&value.replace('+', " ")))
}

/// The network and password sent from the page: a network listed, or
/// one named in the field below them.
fn chosen(form: &str) -> Option<(String, String)> {
    let field = |name: &str| field(form, name);
    let other = field("other").unwrap_or_default();
    let ssid = if other.trim().is_empty() {
        field("ssid")?
    } else {
        String::from(other.trim())
    };
    (!ssid.is_empty() && ssid.len() <= 32).then(|| (ssid, field("password").unwrap_or_default()))
}

/// `text` safe in HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    paperpaw_sdk::xml::escape(text, &mut out);
    out
}

/// A page with `body`.
fn page(env: &Env<'_>, body: &str) -> String {
    PAGE.replace("{lang}", env.i18n.language())
        .replace("{title}", &escape(env.i18n.tr("wifi.phone.title")))
        .replace("{body}", body)
}

/// The page to choose a network: those found, the strongest first, a
/// field for another one, the password. `again` after a form without a
/// network.
fn choose_page(env: &Env<'_>, again: bool, token: &str) -> String {
    let tr = |key| escape(env.i18n.tr(key));
    let mut body = format!("<h1>{}</h1>", tr("wifi.phone.title"));
    if again {
        body.push_str(&format!(
            "<p class=\"note\">{}</p>",
            tr("wifi.phone.choose-one")
        ));
    }
    body.push_str(&format!(
        "<form method=\"post\" action=\"/join\"><input type=\"hidden\" name=\"t\" value=\"{}\">",
        escape(token)
    ));
    let networks = env.wifi.map(|w| w.networks.as_slice()).unwrap_or_default();
    for network in networks {
        let lock = if network.secured { "" } else { " ·" };
        let open = if network.secured {
            String::new()
        } else {
            tr("wifi.network.open")
        };
        body.push_str(&format!(
            "<label class=\"network\"><input type=\"radio\" name=\"ssid\" value=\"{name}\"> {name}<small>{open}{lock}</small></label>",
            name = escape(&network.ssid),
        ));
    }
    body.push_str(&format!(
        "<p><label>{}<input type=\"text\" name=\"other\" autocomplete=\"off\"></label></p>\
         <p><label>{}<input type=\"password\" name=\"password\" autocomplete=\"off\"></label></p>\
         <button type=\"submit\">{}</button></form><p class=\"note\">{}</p>",
        tr("wifi.phone.other"),
        tr("wifi.phone.password"),
        tr("wifi.phone.join"),
        tr("wifi.phone.page-note"),
    ));
    page(env, &body)
}

impl Screen for PhoneScreen {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        let style = self.style;
        let title = env.i18n.tr("wifi.phone.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let display = env.device.display;
        let top = self.header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let message = |key| env.i18n.tr(key).to_string();
        let text = match (&self.state, &self.own) {
            (_, None) => message("wifi.phone.no-random"),
            (State::Searching, _) => message("wifi.state.searching"),
            (State::Opening, _) if self.problem.is_none() => message("wifi.phone.opening"),
            (State::Opening | State::Hosting { .. }, Some(own)) => {
                return self.draw_codes(canvas, env, area, own);
            }
            (State::Leaving { ssid, .. } | State::Joining(ssid), _) => env
                .i18n
                .format("wifi.state.joining", &[("ssid", Arg::Str(ssid))]),
            (State::Joined(ssid), _) => env
                .i18n
                .format("wifi.phone.joined", &[("ssid", Arg::Str(ssid))]),
            (State::Failed(text), _) => text.clone(),
        };
        ui::draw_message(canvas, env, &style, area, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.started {
            return;
        }
        self.started = true;
        if self.own.is_none() {
            Self::redraw(cx);
            return;
        }
        // On while this screen is open, whatever else wants it; this
        // screen says what it does.
        let mut policy = self.policy.borrow_mut();
        policy.manual = true;
        policy.phone = true;
        policy.goal = crate::policy::Goal::Nothing;
        drop(policy);
        let on = cx.env().wifi.is_some_and(|w| w.state.is_on());
        if !on {
            cx.command(Command::Wifi(WifiCommand::On));
        }
        cx.command(Command::Wifi(WifiCommand::Scan));
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // Left alone: closed.
        let waiting = matches!(
            self.state,
            State::Opening | State::Hosting { .. } | State::Failed(_)
        );
        if waiting && env.now >= self.active.add_millis(IDLE_MS) {
            return self.close(cx);
        }
        match core::mem::replace(&mut self.state, State::Searching) {
            State::Leaving { ssid, password } => {
                cx.command(Command::StopServing(SERVER));
                let state = env.wifi.map(|w| w.state.clone()).unwrap_or_default();
                let mut policy = self.policy.borrow_mut();
                policy.pending = Some((ssid.clone(), password.clone()));
                let commands = policy.choose(&state, &ssid, &password);
                drop(policy);
                for command in commands {
                    cx.command(Command::Wifi(command));
                }
                self.state = State::Joining(ssid);
                Self::redraw(cx);
            }
            State::Joined(ssid) => {
                self.state = State::Joined(ssid);
                self.close(cx);
            }
            state => self.state = state,
        }
        // The screen's one timer: the idle time again, where it counts.
        if matches!(
            self.state,
            State::Opening | State::Hosting { .. } | State::Failed(_)
        ) {
            cx.wake_at(self.active.add_millis(IDLE_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::BACK {
            self.close(cx);
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
        Handled::No
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        use paperpaw_sdk::builtin::events;
        if *event == events::WIFI_SCANNED && matches!(self.state, State::Searching) {
            self.host(cx);
            Self::redraw(cx);
        } else if *event == events::WIFI_CHANGED {
            self.on_wifi(cx);
        } else if let Some(failed) = events::WIFI_FAILED.read(event, params) {
            let (ssid, reason) = (failed.ssid.clone(), failed.reason);
            self.on_failed(&ssid, reason, cx);
        } else if let Some(notice) = SERVER_EVENT.read(event, params)
            && *notice.server() == SERVER
        {
            self.on_server(notice, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_names_a_network_and_its_password() {
        assert_eq!(
            chosen("ssid=Home+Net&other=&password=p%26ss+w"),
            Some((String::from("Home Net"), String::from("p&ss w")))
        );
        // Another network named wins over one listed.
        assert_eq!(
            chosen("ssid=Home&other=+Cafe+&password="),
            Some((String::from("Cafe"), String::new()))
        );
        assert_eq!(chosen("other=&password=x"), None);
        assert_eq!(chosen(&format!("other={}", "n".repeat(33))), None);
    }
}
