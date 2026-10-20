//! The update page: under the header, what there is in large type with a
//! few words below it, and up to two buttons at the bottom (the first one
//! always the way on). While installing, a bar shows how far it is.
//!
//! A newer update on the card comes first; without one, the page offers
//! to search online, and shows an update the server named (see
//! [`online`]), then an older one on the card, which it says is older.
//! An update that leaves out plugins built in now says which; one not
//! signed by PaperPaw shows its key, which the reader accepts at each
//! install.
//!
//! Buttons: up and down (or left and right) move between the buttons,
//! *Select* chooses, *Back* closes (not while installing). Touch: tap.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::firmware::{
    FirmwareCommand, FirmwareState, InstallFailure, Signer, Update, fingerprint,
};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, files,
};

use crate::online::{self, Offer};
use crate::{INSTALLING, found, write_pair};

/// Wi-Fi transfer, offered to put an update on the card.
const TRANSFER: Id = Id::new_static("transfer.open");
/// How long "Restarting" shows before the restart.
const RESTART_MS: u64 = 1500;
/// Lines of the explanation at most.
const HELP_LINES: usize = 8;

/// What the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum View {
    /// No update file on the card.
    Nothing,
    /// Files, but none for this device.
    Unfit(InstallFailure),
    /// An update to install, whether it is newer than the running
    /// firmware, and the plugin crates it leaves out.
    Ready {
        update: Update,
        signer: Signer,
        newer: bool,
        leaves: Vec<String>,
    },
    /// Asking the update server.
    Checking,
    /// The server knows no newer update.
    UpToDate,
    /// The server could not be asked (no answer, for this reason), or it
    /// answered with this error status.
    Unreachable {
        status: u16,
        reason: String,
    },
    /// An update the server named, and the plugin crates it leaves out.
    Offer {
        offer: Offer,
        leaves: Vec<String>,
    },
    /// Downloading the update of this version.
    Downloading(String),
    Installing {
        done: u64,
        total: u64,
    },
    Installed,
    Failed(InstallFailure),
}

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Install,
    Check,
    Download,
    Transfer,
    Close,
}

pub(crate) struct UpdatePage {
    style: UiStyle,
    header: ui::Header,
    view: View,
    /// The button chosen with the buttons.
    focus: usize,
    /// Whether the focus shows (once a button was used; at once without
    /// touch).
    shown: bool,
}

/// The text for why an update cannot be installed.
fn reason(failure: InstallFailure) -> &'static str {
    match failure {
        InstallFailure::Unreadable => "update.reason.unreadable",
        InstallFailure::NotAnUpdate => "update.reason.not-an-update",
        InstallFailure::NewerFormat => "update.reason.newer-format",
        InstallFailure::OtherDevice => "update.reason.other-device",
        InstallFailure::TooLarge => "update.reason.too-large",
        InstallFailure::Untrusted => "update.reason.untrusted",
        InstallFailure::NotNewer => "update.reason.not-newer",
        InstallFailure::Damaged => "update.reason.damaged",
        InstallFailure::Write => "update.reason.write",
    }
}

impl UpdatePage {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let mut page = Self {
            header: ui::Header::new(env, &style, true),
            style,
            view: View::Nothing,
            focus: 0,
            shown: !env.device.input.touch,
        };
        page.view = page.look(env);
        page
    }

    /// What there is now: an install going on, or the files on the card.
    fn look(&self, env: &Env<'_>) -> View {
        let Some(firmware) = env.firmware else {
            return View::Nothing;
        };
        match &firmware.state {
            FirmwareState::Installing { done, total } => {
                return View::Installing {
                    done: *done,
                    total: *total,
                };
            }
            FirmwareState::Installed => return View::Installed,
            FirmwareState::Idle | FirmwareState::Failed(_) => {}
        }
        let card = found::find(env.storage, firmware);
        let ready = |update: Update| {
            let leaves = update
                .leaves_out(firmware)
                .into_iter()
                .map(String::from)
                .collect();
            View::Ready {
                signer: update.signer(firmware),
                newer: update.is_newer(firmware),
                update,
                leaves,
            }
        };
        let card = match card {
            Ok(update) if update.is_newer(firmware) => return ready(update),
            card => card,
        };
        if let Some(offer) = online::offer(env.storage, firmware) {
            let leaves = offer
                .leaves_out(firmware)
                .into_iter()
                .map(String::from)
                .collect();
            return View::Offer { offer, leaves };
        }
        match card {
            // Older, or the same again: offered as such.
            Ok(update) => ready(update),
            Err(InstallFailure::Unreadable) => View::Nothing,
            Err(why) => View::Unfit(why),
        }
    }

    /// Whether the page waits for an answer or a download (kept when it
    /// shows again).
    fn waiting(&self) -> bool {
        matches!(self.view, View::Checking | View::Downloading(_))
    }

    /// The plugins `crates` leave out, by their names, and why the server
    /// says they are missing.
    fn plugin_names(&self, env: &Env<'_>, crates: &[String]) -> String {
        let offer = match &self.view {
            View::Offer { offer, .. } => Some(offer),
            _ => None,
        };
        let names: Vec<String> = crates
            .iter()
            .map(|name| {
                // First-party crates are named after their plugin.
                let key = name
                    .strip_prefix("paperpaw-plugin-")
                    .map(|id| alloc::format!("{id}.plugin.name"));
                let shown = key
                    .as_deref()
                    .and_then(|key| env.i18n.get(key))
                    .unwrap_or(name);
                match offer.and_then(|o| o.why_missing(name)) {
                    Some(why @ ("unknown" | "not-ready")) => alloc::format!(
                        "{shown} ({})",
                        env.i18n.tr(&alloc::format!("update.missing.{why}"))
                    ),
                    _ => String::from(shown),
                }
            })
            .collect();
        names.join(", ")
    }

    /// `help`, followed by what cannot come along if anything.
    fn with_leaves(&self, env: &Env<'_>, help: String, leaves: &[String]) -> String {
        if leaves.is_empty() {
            return help;
        }
        let names = self.plugin_names(env, leaves);
        // Before a download, the choice is to download without them.
        let key = if matches!(self.view, View::Offer { .. }) {
            "update.leaves.download"
        } else {
            "update.leaves"
        };
        alloc::format!(
            "{help} {}",
            env.i18n.format(key, &[("plugins", Arg::Str(&names))])
        )
    }

    fn buttons(&self, env: &Env<'_>) -> Vec<Button> {
        match &self.view {
            View::Nothing => {
                let mut buttons = Vec::new();
                if env.caps.network {
                    buttons.push(Button::Check);
                }
                if env.actions.get(TRANSFER.as_str()).is_some() {
                    buttons.push(Button::Transfer);
                }
                buttons
            }
            View::Ready { .. } => alloc::vec![Button::Install, Button::Close],
            View::Offer { .. } => alloc::vec![Button::Download, Button::Close],
            View::Unreachable { .. } => alloc::vec![Button::Check, Button::Close],
            View::Unfit(_) | View::Failed(_) | View::UpToDate => alloc::vec![Button::Close],
            View::Installing { .. } | View::Installed | View::Checking | View::Downloading(_) => {
                Vec::new()
            }
        }
    }

    fn label<'a>(&self, env: &'a Env<'_>, button: Button) -> &'a str {
        env.i18n.tr(match (button, &self.view) {
            (
                Button::Install,
                View::Ready {
                    signer: Signer::Unknown,
                    ..
                },
            ) => "update.trust-install",
            (Button::Install, View::Ready { newer: false, .. }) => "update.install-older",
            (Button::Install, View::Ready { leaves, .. }) if !leaves.is_empty() => {
                "update.install-without"
            }
            (Button::Install, _) => "update.install",
            (Button::Download, View::Offer { leaves, .. }) if !leaves.is_empty() => {
                "update.download-without"
            }
            (Button::Download, _) => "update.download",
            (Button::Check, View::Unreachable { .. }) => "update.retry",
            (Button::Check, _) => "update.check",
            (Button::Transfer, _) => "update.transfer",
            (Button::Close, View::Ready { .. } | View::Offer { .. }) => "update.later",
            (Button::Close, _) => "update.ok",
        })
    }

    /// The large line and the words below it.
    fn texts(&self, env: &Env<'_>) -> (String, String) {
        let current = env.firmware.map_or("", |f| f.version.as_str());
        let tr = |key: &str| env.i18n.tr(key).to_string();
        let with = |key: &str, args: &[(&str, Arg<'_>)]| env.i18n.format(key, args);
        match &self.view {
            View::Nothing => (
                tr("update.nothing.title"),
                with("update.nothing.help", &[("current", Arg::Str(current))]),
            ),
            View::Unfit(why) => (tr("update.unfit.title"), tr(reason(*why))),
            View::Ready {
                update,
                signer,
                newer,
                leaves,
            } => {
                let version = Arg::Str(&update.info.version);
                let current = &[("current", Arg::Str(current))];
                let (title, mut help) = if *signer == Signer::Unknown {
                    // Its groups kept together on one line.
                    let key = fingerprint(&update.trailer.key).replace(' ', "\u{a0}");
                    (
                        with("update.unknown.title", &[("version", version)]),
                        with("update.unknown.help", &[("key", Arg::Str(&key))]),
                    )
                } else if *newer {
                    (
                        with("update.ready.title", &[("version", version)]),
                        with("update.ready.help", current),
                    )
                } else {
                    (
                        with("update.older.title", &[("version", version)]),
                        with("update.older.help", current),
                    )
                };
                if *signer == Signer::Unknown && !*newer {
                    help.push(' ');
                    help.push_str(&with("update.older.note", current));
                }
                (title, self.with_leaves(env, help, leaves))
            }
            View::Checking => (
                tr("update.checking.title"),
                with(
                    "update.checking.help",
                    &[("server", Arg::Str(&online::server_name(env)))],
                ),
            ),
            View::UpToDate => (
                tr("update.up-to-date.title"),
                with("update.up-to-date.help", &[("current", Arg::Str(current))]),
            ),
            View::Unreachable { status, reason } => {
                let server = online::server_name(env);
                let status = alloc::format!("{status}");
                (
                    tr("update.unreachable.title"),
                    if reason.is_empty() {
                        with(
                            "update.server-error.help",
                            &[("server", Arg::Str(&server)), ("status", Arg::Str(&status))],
                        )
                    } else {
                        with(
                            "update.unreachable.help",
                            &[("server", Arg::Str(&server)), ("reason", Arg::Str(reason))],
                        )
                    },
                )
            }
            View::Offer { offer, leaves } => {
                let size = files::size(offer.size, env.i18n.tr("update.decimal"));
                let help = with(
                    if offer.size > 0 {
                        "update.offer.help"
                    } else {
                        "update.offer.help-unsized"
                    },
                    &[("current", Arg::Str(current)), ("size", Arg::Str(&size))],
                );
                (
                    with(
                        "update.offer.title",
                        &[("version", Arg::Str(&offer.version))],
                    ),
                    self.with_leaves(env, help, leaves),
                )
            }
            View::Downloading(version) => (
                with(
                    "update.downloading.title",
                    &[("version", Arg::Str(version))],
                ),
                tr("update.downloading.help"),
            ),
            View::Installing { .. } => {
                (tr("update.installing.title"), tr("update.installing.help"))
            }
            View::Installed => (tr("update.installed.title"), tr("update.installed.help")),
            View::Failed(why) => (
                tr("update.failed.title"),
                with(
                    "update.failed.help",
                    &[
                        ("reason", Arg::Str(env.i18n.tr(reason(*why)))),
                        ("current", Arg::Str(current)),
                    ],
                ),
            ),
        }
    }

    /// The button areas, the first one above the second, at the bottom.
    fn button_rects(&self, env: &Env<'_>, count: usize) -> Vec<Rect> {
        let style = &self.style;
        let display = env.device.display;
        let margin = style.margin as u16 * 2;
        let gap = style.gap as u16;
        let width = display.width.saturating_sub(2 * margin);
        let height = style.row as u16;
        let mut bottom = display.height.saturating_sub(margin);
        let mut rects: Vec<Rect> = (0..count)
            .map(|_| {
                let rect = Rect::new(margin, bottom.saturating_sub(height), width, height);
                bottom = rect.y.saturating_sub(gap * 2);
                rect
            })
            .collect();
        rects.reverse();
        rects
    }

    fn redraw(&mut self, cx: &mut Cx<'_, '_>, mode: RefreshMode) {
        cx.request_redraw();
        cx.request_refresh(mode);
    }

    fn choose(&mut self, button: Button, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match button {
            Button::Close => cx.command(Command::Back),
            Button::Transfer => cx.command(Command::RunAction(TRANSFER)),
            Button::Check => {
                let mut out = Vec::new();
                online::check(&env, false, &mut out);
                out.into_iter().for_each(|command| cx.command(command));
                self.view = View::Checking;
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            Button::Download => {
                let View::Offer { offer, .. } = &self.view else {
                    return;
                };
                let mut out = Vec::new();
                online::download(&env, offer, &mut out);
                out.into_iter().for_each(|command| cx.command(command));
                self.view = View::Downloading(offer.version.clone());
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            Button::Install => {
                let View::Ready {
                    update,
                    signer,
                    newer,
                    ..
                } = &self.view
                else {
                    return;
                };
                write_pair(env.storage, INSTALLING, &update.path, &update.info.version);
                // What the reader saw and chose: the key shown, an older
                // version said to be older.
                cx.command(Command::Firmware(FirmwareCommand::Install {
                    path: update.path.clone(),
                    key: (*signer == Signer::Unknown).then_some(update.trailer.key),
                    older: !*newer,
                }));
                self.view = View::Installing {
                    done: 0,
                    total: u64::from(update.trailer.image_len),
                };
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let count = self.buttons(cx.env()).len();
        if count == 0 {
            return;
        }
        if !self.shown {
            self.shown = true;
        } else {
            self.focus = ui::step_focus(self.focus, delta, count);
        }
        self.redraw(cx, RefreshMode::Fast);
    }

    fn draw_progress(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        area: Rect,
        done: u64,
        total: u64,
    ) {
        let style = &self.style;
        let percent = (done * 100).checked_div(total).unwrap_or(0).min(100);
        let border = ui::button_border(style);
        let (x, y, w, h) = (
            i32::from(area.x),
            i32::from(area.y),
            u32::from(area.width),
            u32::from(area.height),
        );
        canvas.stroke_round_rect(x, y, w, h, border, style.corner, Color::BLACK);
        let inset = border * 2;
        let inner = w.saturating_sub(2 * inset);
        let filled = (u64::from(inner) * percent / 100) as u32;
        if filled > 0 {
            canvas.fill_rect(
                x + inset as i32,
                y + inset as i32,
                filled,
                h.saturating_sub(2 * inset),
                Color::BLACK,
            );
        }
        let text = alloc::format!("{percent} %");
        canvas.describe(Role::Text, &text, area, false, None);
        if let Some(font) = style.font(env) {
            let label = Rect::new(
                area.x,
                area.y + area.height + style.gap as u16,
                area.width,
                style.row as u16,
            );
            let tx =
                f32::from(area.x) + (f32::from(area.width) - font.measure(style.text, &text)) / 2.0;
            ui::draw_label(canvas, &font, style.text, tx, label, &text, Color::BLACK);
        }
    }
}

/// Draws `text` left-aligned in `area`, wrapped, at most `max` lines and
/// as many as `area` holds; returns the height used.
#[allow(clippy::too_many_arguments)]
fn paragraph(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    size: f32,
    text: &str,
    max: usize,
    color: Color,
) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let height = font.line_metrics(size).line_height as u16;
    let max = max.min(usize::from(area.height / height.max(1)));
    let lines = font.wrap(size, text, f32::from(area.width));
    let used = height * lines.len().min(max) as u16;
    canvas.describe(
        Role::Text,
        text,
        Rect::new(area.x, area.y, area.width, used),
        false,
        None,
    );
    let mut top = area.y;
    for range in lines.into_iter().take(max) {
        let row = Rect::new(area.x, top, area.width, height);
        ui::draw_label(
            canvas,
            &font,
            size,
            f32::from(area.x),
            row,
            &text[range],
            color,
        );
        top += height;
    }
    top - area.y
}

impl Screen for UpdatePage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        canvas.clear(Color::WHITE);
        let style = self.style;
        self.header
            .draw(canvas, env, &style, env.i18n.tr("update.title"), None, None);
        let margin = style.margin as u16 * 2;
        let gap = style.gap as u16;
        let width = env.device.display.width.saturating_sub(2 * margin);
        let (title, help) = self.texts(env);
        let mut y = self.header.area.height + gap * 4;
        let area = Rect::new(
            margin,
            y,
            width,
            env.device.display.height.saturating_sub(y),
        );
        y += paragraph(
            canvas,
            env,
            &style,
            area,
            style.title * 1.3,
            &title,
            3,
            Color::BLACK,
        ) + gap;
        let buttons = self.buttons(env);
        let rects = self.button_rects(env, buttons.len());
        // The words keep clear of the buttons.
        let bottom = rects
            .first()
            .map_or(env.device.display.height, |r| r.y.saturating_sub(gap));
        let area = Rect::new(margin, y, width, bottom.saturating_sub(y));
        y += paragraph(
            canvas,
            env,
            &style,
            area,
            style.small,
            &help,
            HELP_LINES,
            style.faint(60),
        ) + gap * 4;
        if let View::Installing { done, total } = self.view {
            let bar = Rect::new(margin, y, width, gap * 3);
            self.draw_progress(canvas, env, bar, done, total);
        }
        for (i, (button, rect)) in buttons.iter().zip(rects).enumerate() {
            let label = self.label(env, *button);
            let focused = self.shown && self.focus == i;
            ui::draw_button(canvas, env, &style, rect, label, i == 0, focused);
        }
    }

    fn hint(&self, _: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        // Nothing can be done while it installs.
        if matches!(self.view, View::Installing { .. } | View::Installed) {
            paperpaw_sdk::Hint::Hidden
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Back from Wi-Fi transfer: a file may have come.
        let env = *cx.env();
        if !matches!(self.view, View::Installing { .. } | View::Installed) && !self.waiting() {
            self.view = self.look(&env);
            self.focus = 0;
        }
        self.redraw(cx, RefreshMode::Partial);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if matches!(self.view, View::Installing { .. } | View::Installed) {
            // Held: the install must not be left half way.
            return Handled::Yes;
        }
        if *a == actions::BACK {
            cx.command(Command::Back);
        } else if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            self.move_focus(-1, cx);
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            self.move_focus(1, cx);
        } else if *a == actions::CONFIRM {
            let buttons = self.buttons(cx.env());
            if let Some(button) = buttons.get(self.focus).copied() {
                if self.shown {
                    self.choose(button, cx);
                } else {
                    self.shown = true;
                    self.redraw(cx, RefreshMode::Fast);
                }
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap
            || matches!(self.view, View::Installing { .. } | View::Installed)
        {
            return Handled::No;
        }
        let env = *cx.env();
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, None) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        self.shown = false;
        let buttons = self.buttons(&env);
        let rects = self.button_rects(&env, buttons.len());
        match buttons
            .iter()
            .zip(rects)
            .position(|(_, rect)| rect.contains(gesture.start))
        {
            Some(i) => {
                self.focus = i;
                self.choose(buttons[i], cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        // The plugin kept the answer or the file before the page hears of
        // it (see `online`).
        if let Some(reply) = online::CHECKED.read(event, params) {
            if self.view == View::Checking {
                self.view = if !(200..300).contains(&reply.status) {
                    View::Unreachable {
                        status: reply.status,
                        reason: if reply.status == 0 {
                            reply.body.clone()
                        } else {
                            String::new()
                        },
                    }
                } else {
                    match self.look(&env) {
                        View::Nothing | View::Unfit(_) => View::UpToDate,
                        view => view,
                    }
                };
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            return;
        }
        if online::DOWNLOADED.read(event, params).is_some() {
            if matches!(self.view, View::Downloading(_)) {
                self.view = self.look(&env);
                self.focus = 0;
                self.redraw(cx, RefreshMode::Partial);
            }
            return;
        }
        if *event != events::FIRMWARE_CHANGED {
            return;
        }
        let Some(firmware) = env.firmware else {
            return;
        };
        let (view, mode) = match &firmware.state {
            FirmwareState::Installing { done, total } => {
                // Only the bar and its number change.
                (
                    View::Installing {
                        done: *done,
                        total: *total,
                    },
                    RefreshMode::Fast,
                )
            }
            FirmwareState::Installed => {
                cx.wake_at(env.now.add_millis(RESTART_MS));
                (View::Installed, RefreshMode::Partial)
            }
            FirmwareState::Failed(why) => (View::Failed(*why), RefreshMode::Partial),
            FirmwareState::Idle => return,
        };
        if view != self.view {
            self.view = view;
            self.focus = 0;
            self.redraw(cx, mode);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        if self.view == View::Installed {
            cx.command(Command::Firmware(FirmwareCommand::Restart));
        }
    }
}
