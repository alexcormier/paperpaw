//! The Wi-Fi page (the settings group's own page): the radio's switch
//! with where it is, the networks in reach (choose one to join; the menu
//! button forgets a saved one), the setup with a phone, and the
//! plugin's settings.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::wifi::{self, WifiCommand, WifiNetwork, WifiState};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::{IDLE_OFF, IDLE_OFF_CHOICES, ON_DEMAND, PASSWORD_ENTERED, Shared, WHILE_CHARGING};

/// A row of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Radio,
    /// Setting it up with a phone.
    Phone,
    Network(WifiNetwork),
    Search,
    OnDemand,
    IdleOff,
    WhileCharging,
}

pub(crate) struct WifiPage {
    policy: Shared,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl core::fmt::Debug for WifiPage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WifiPage").finish_non_exhaustive()
    }
}

/// Where the radio is, in words.
fn state_text(env: &Env<'_>, state: &WifiState, scanning: bool) -> String {
    match state {
        WifiState::Off => env.i18n.tr("wifi.state.off").to_string(),
        WifiState::On if scanning => env.i18n.tr("wifi.state.searching").to_string(),
        WifiState::On => env.i18n.tr("wifi.state.on").to_string(),
        WifiState::Joining { ssid } => env
            .i18n
            .format("wifi.state.joining", &[("ssid", Arg::Str(ssid))]),
        WifiState::Online { ssid, address } => env.i18n.format(
            "wifi.state.online",
            &[("ssid", Arg::Str(ssid)), ("address", Arg::Str(address))],
        ),
        _ => env.i18n.tr("wifi.state.on").to_string(),
    }
}

/// What is known of a network, in words: joined or saved, its signal,
/// open or not.
fn network_note(env: &Env<'_>, network: &WifiNetwork, state: &WifiState, saved: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    match state {
        WifiState::Online { ssid, .. } if *ssid == network.ssid => {
            parts.push(env.i18n.tr("wifi.network.connected"));
        }
        WifiState::Joining { ssid } if *ssid == network.ssid => {
            parts.push(env.i18n.tr("wifi.network.connecting"));
        }
        _ if saved => parts.push(env.i18n.tr("wifi.network.saved")),
        _ => {}
    }
    let signal = match network.bars() {
        4 => "wifi.signal.excellent",
        3 => "wifi.signal.good",
        2 => "wifi.signal.fair",
        _ => "wifi.signal.weak",
    };
    parts.push(env.i18n.tr(signal));
    if !network.secured {
        parts.push(env.i18n.tr("wifi.network.open"));
    }
    parts.join(" · ")
}

impl WifiPage {
    pub(crate) fn new(env: &Env<'_>, policy: Shared) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height - top);
        Self {
            policy,
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
        }
    }

    fn rows(env: &Env<'_>) -> Vec<Row> {
        let mut rows = alloc::vec![Row::Radio];
        if let Some(wifi) = env.wifi
            && wifi.state.is_on()
        {
            rows.extend(wifi.networks.iter().cloned().map(Row::Network));
            rows.push(Row::Search);
        }
        rows.push(Row::Phone);
        rows.extend([Row::OnDemand, Row::IdleOff, Row::WhileCharging]);
        rows
    }

    fn changed(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Looks for networks if the radio is on and nothing else goes on.
    fn search(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(wifi) = env.wifi
            && wifi.state.is_on()
            && !wifi.scanning
        {
            cx.command(Command::Wifi(WifiCommand::Scan));
        }
    }

    /// What choosing a row does.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(wifi) = env.wifi else {
            return;
        };
        let Some(row) = Self::rows(&env).into_iter().nth(index) else {
            return;
        };
        let mut policy = self.policy.borrow_mut();
        match row {
            Row::Radio if wifi.state.is_on() => {
                policy.manual = false;
                cx.command(Command::Wifi(WifiCommand::Off));
            }
            Row::Radio => {
                policy.manual = true;
                cx.command(Command::Wifi(WifiCommand::On));
            }
            Row::Network(network) => {
                let known = wifi::known(env.secrets.iter());
                let saved = known.iter().find(|(ssid, _)| *ssid == network.ssid);
                if wifi.state.ssid() == Some(&network.ssid) {
                    // Joined already (or joining).
                } else if let Some((ssid, password)) = saved {
                    for command in policy.choose(&wifi.state, ssid, password) {
                        cx.command(Command::Wifi(command));
                    }
                } else if network.secured {
                    let title = env
                        .i18n
                        .format("wifi.password", &[("ssid", Arg::Str(&network.ssid))]);
                    cx.command(
                        paperpaw_sdk::ask::text(title, &PASSWORD_ENTERED)
                            .password()
                            .with(params::ID, network.ssid.clone())
                            .command(),
                    );
                } else {
                    // Not kept: an open network is not joined by itself
                    // again (see `policy::best_known`).
                    for command in policy.choose(&wifi.state, &network.ssid, "") {
                        cx.command(Command::Wifi(command));
                    }
                }
            }
            Row::Search => {
                drop(policy);
                self.search(cx);
            }
            Row::Phone => {
                cx.command(Command::OpenScreen(crate::PHONE, Params::new()));
                return;
            }
            Row::OnDemand | Row::WhileCharging => {
                let id = if row == Row::OnDemand {
                    ON_DEMAND
                } else {
                    WHILE_CHARGING
                };
                let on = env.settings.read(&id);
                cx.command(Command::set(&id, !on));
            }
            Row::IdleOff => {
                let now = env.settings.read(&IDLE_OFF);
                let at = IDLE_OFF_CHOICES
                    .iter()
                    .position(|(choice, _)| *choice == now)
                    .unwrap_or(0);
                let next = IDLE_OFF_CHOICES[(at + 1) % IDLE_OFF_CHOICES.len()].0;
                cx.command(Command::set(&IDLE_OFF, Cow::Borrowed(next)));
            }
        }
        Self::changed(cx);
    }

    /// Forgets the saved network in row `index`, leaving it if joined.
    fn forget(&self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        let Some(Row::Network(network)) = Self::rows(&env).into_iter().nth(index) else {
            return false;
        };
        if !wifi::known(env.secrets.iter())
            .iter()
            .any(|(ssid, _)| *ssid == network.ssid)
        {
            return false;
        }
        cx.command(Command::SetSecret(wifi::forget(&network.ssid)));
        if env.wifi.and_then(|w| w.state.ssid()) == Some(&network.ssid) {
            cx.command(Command::Wifi(WifiCommand::Leave));
        }
        cx.command(Command::Notify(
            env.i18n
                .format("wifi.forgotten", &[("ssid", Arg::Str(&network.ssid))]),
        ));
        Self::changed(cx);
        true
    }
}

impl Screen for WifiPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = Self::rows(env);
        let len = rows.len();
        let title = env.i18n.tr("wifi.title");
        self.header
            .draw(canvas, env, &style, title, None, self.list.paging(len));
        let Some(wifi) = env.wifi else {
            return;
        };
        let known = wifi::known(env.secrets.iter());
        self.list
            .draw(canvas, &style, len, |canvas, row, rect, focused| {
                let tr = |key| env.i18n.tr(key);
                match &rows[row] {
                    Row::Radio => ui::draw_switch_row(
                        canvas,
                        env,
                        &style,
                        rect,
                        focused,
                        title,
                        &state_text(env, &wifi.state, wifi.scanning),
                        wifi.state.is_on(),
                    ),
                    Row::Network(network) => {
                        let saved = known.iter().any(|(ssid, _)| *ssid == network.ssid);
                        let note = network_note(env, network, &wifi.state, saved);
                        ui::draw_noted_row(
                            canvas,
                            env,
                            &style,
                            rect,
                            focused,
                            &network.ssid,
                            "",
                            &note,
                        );
                    }
                    Row::Search => {
                        let note = if wifi.scanning {
                            tr("wifi.state.searching")
                        } else if wifi.networks.is_empty() {
                            tr("wifi.no-networks")
                        } else {
                            ""
                        };
                        ui::draw_noted_row(
                            canvas,
                            env,
                            &style,
                            rect,
                            focused,
                            tr("wifi.search"),
                            "",
                            note,
                        );
                    }
                    Row::OnDemand | Row::WhileCharging => {
                        let (id, key) = if rows[row] == Row::OnDemand {
                            (ON_DEMAND, "wifi.setting.on-demand")
                        } else {
                            (WHILE_CHARGING, "wifi.setting.while-charging")
                        };
                        let help = alloc::format!("{key}.help");
                        ui::draw_switch_row(
                            canvas,
                            env,
                            &style,
                            rect,
                            focused,
                            tr(key),
                            env.i18n.tr(&help),
                            env.settings.bool(id.as_str()),
                        );
                    }
                    Row::Phone => ui::draw_noted_row(
                        canvas,
                        env,
                        &style,
                        rect,
                        focused,
                        tr("wifi.phone.title"),
                        "›",
                        tr("wifi.phone.note"),
                    ),
                    Row::IdleOff => ui::draw_noted_row(
                        canvas,
                        env,
                        &style,
                        rect,
                        focused,
                        tr("wifi.setting.idle-off"),
                        tr(env.settings.read(&IDLE_OFF)),
                        tr("wifi.setting.idle-off.help"),
                    ),
                }
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Fresh networks, unless joining one is going on.
        if self.policy.borrow().goal == crate::policy::Goal::Nothing {
            self.search(cx);
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        let ours = *event == events::WIFI_CHANGED
            || *event == events::WIFI_SCANNED
            || *event == events::SETTING_CHANGED;
        if !ours {
            return;
        }
        // Switched on here: the networks to choose from.
        let env = *cx.env();
        if *event == events::WIFI_CHANGED
            && env
                .wifi
                .is_some_and(|w| w.state == WifiState::On && w.networks.is_empty())
            && self.policy.borrow().goal == crate::policy::Goal::Nothing
        {
            self.search(cx);
        }
        Self::changed(cx);
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let row = Self::rows(env).into_iter().nth(self.list.focus());
        let saved = |network: &WifiNetwork| {
            wifi::known(env.secrets.iter())
                .iter()
                .any(|(ssid, _)| *ssid == network.ssid)
        };
        match row {
            Some(Row::Network(network)) if *action == actions::MENU && saved(&network) => {
                Hint::Text(env.i18n.tr("wifi.forget").to_string())
            }
            _ => Hint::Default,
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = Self::rows(cx.env()).len();
        let rows = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        } else if *a == actions::MENU && self.forget(self.list.focus(), cx) {
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if self.list.move_focus(delta, len) != ListChange::None {
            Self::changed(cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = Self::rows(cx.env()).len();
        let paging = self.list.paging(len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        match gesture.kind {
            GestureKind::Tap => {
                self.activate(index, cx);
                Handled::Yes
            }
            // A long press forgets a saved network (the menu button's
            // part on devices without touch).
            GestureKind::LongPress if self.forget(index, cx) => Handled::Yes,
            _ => Handled::No,
        }
    }
}
