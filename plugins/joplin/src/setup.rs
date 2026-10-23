//! The set-up: where the notes go, the account, the notebook and *Send
//! now*, a row each; below them what the row chosen is for, and at the
//! bottom how the last sending went or how many books wait. Rows are
//! chosen by a tap or with the buttons; text is typed on the keyboard
//! (or a phone, where offered).

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::SecretChange;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use crate::send::notebook_name;
use crate::{ADDRESS, DONE, NOTEBOOK, PASSWORD, SEND, SERVICE, Service, USER, kept};

/// Typed on the keyboard; parameter [`FIELD`].
pub(crate) const ENTERED: Event<Typed> = Event::new("joplin.entered");
const FIELD: &str = "field";

/// A row of the set-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Service,
    Address,
    User,
    Password,
    Notebook,
    Send,
}

const ALL: [Row; 6] = [
    Row::Service,
    Row::Address,
    Row::User,
    Row::Password,
    Row::Notebook,
    Row::Send,
];
/// Joplin Cloud has its address.
const CLOUD: [Row; 5] = [
    Row::Service,
    Row::User,
    Row::Password,
    Row::Notebook,
    Row::Send,
];

impl Row {
    /// The rows for `service`.
    fn of(service: Service) -> &'static [Row] {
        if service.has_address() { &ALL } else { &CLOUD }
    }

    fn title(self, service: Service) -> &'static str {
        match self {
            Row::Service => "joplin.service",
            Row::Address => "joplin.address",
            Row::User if service.signs_in() => "joplin.email",
            Row::User => "joplin.user",
            Row::Password => "joplin.password",
            Row::Notebook => "joplin.notebook",
            Row::Send => "joplin.send",
        }
    }

    /// What it is for.
    fn help(self) -> &'static str {
        match self {
            Row::Service => "joplin.service.help",
            Row::Address => "joplin.address.help",
            Row::User => "joplin.user.help",
            Row::Password => "joplin.password.help",
            Row::Notebook => "joplin.notebook.help",
            Row::Send => "joplin.send.help",
        }
    }

    /// Its text setting, for those typed in.
    fn setting(self) -> Option<Setting<Text>> {
        match self {
            Row::Address => Some(ADDRESS),
            Row::User => Some(USER),
            Row::Notebook => Some(NOTEBOOK),
            _ => None,
        }
    }

    /// The name of what is typed for it.
    fn field(self) -> &'static str {
        match self {
            Row::Address => "address",
            Row::User => "user",
            Row::Password => "password",
            Row::Notebook => "notebook",
            Row::Service | Row::Send => "",
        }
    }

    fn of_field(field: &str) -> Option<Self> {
        ALL.into_iter()
            .find(|r| !field.is_empty() && r.field() == field)
    }
}

/// Keeps what was typed for a row of the set-up; whether the password
/// changed (the settings say themselves when they change).
pub(crate) fn entered(typed: &Typed, out: &mut Vec<Command>) -> bool {
    let Some(row) = typed.context.get(FIELD).and_then(Row::of_field) else {
        return false;
    };
    let text = typed.text.trim();
    if let Some(setting) = row.setting() {
        out.push(Command::set(&setting, Cow::Owned(text.to_string())));
        return false;
    }
    // An empty answer keeps the password kept: a confirm by mistake on
    // the empty field (a password is never shown in it) loses nothing.
    if text.is_empty() {
        return false;
    }
    out.push(Command::SetSecret(SecretChange::keep(
        PASSWORD,
        "joplin.secret",
        text,
    )));
    true
}

/// How many books wait to be sent, in words, or what happens to notes.
fn waiting(env: &Env<'_>) -> String {
    let count = kept::waiting(env.storage).len() as i64;
    if count == 0 {
        return env.i18n.tr("joplin.idle").to_string();
    }
    env.i18n
        .plural("joplin.waiting", count, &[("count", Arg::Int(count))])
}

/// The set-up screen.
pub(crate) struct SetupScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    help: Rect,
    status: Rect,
    service: Service,
    /// How the last sending went, or how many books wait.
    message: String,
}

impl SetupScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let gap = style.gap as u16;
        let top = header.area.height + gap;
        let height = style.row as u16 * ALL.len() as u16;
        let line = style.row as u16 * 3 / 2;
        let status = Rect::new(0, display.height.saturating_sub(line), display.width, line);
        let help_top = top + height + gap;
        Self {
            list: ListView::new(Rect::new(0, top, display.width, height), style.row)
                .focus_on_keys(env),
            help: Rect::new(
                0,
                help_top,
                display.width,
                status.y.saturating_sub(help_top + gap),
            ),
            status,
            service: Service::of(env),
            message: waiting(env),
            style,
            header,
        }
    }

    fn rows(&self) -> &'static [Row] {
        Row::of(self.service)
    }

    fn focused(&self) -> Row {
        let rows = self.rows();
        rows[self.list.focus().min(rows.len() - 1)]
    }

    fn activate(&mut self, row: Row, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match row {
            Row::Service => {
                self.service = self.service.next();
                cx.command(Command::set(&SERVICE, Cow::Borrowed(self.service.key())));
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            Row::Send => cx.command(Command::RunAction(SEND)),
            _ => {
                let initial = row.setting().map_or("", |s| env.settings.read(&s));
                let question =
                    paperpaw_sdk::ask::text(env.i18n.tr(row.title(self.service)), &ENTERED)
                        .initial(initial)
                        .with(FIELD, row.field());
                let question = if row == Row::Password {
                    question.password()
                } else {
                    question
                };
                cx.command(question.command());
            }
        }
    }

    /// What a row shows on the right.
    fn value(&self, row: Row, env: &Env<'_>) -> String {
        match row {
            Row::Service => env.i18n.tr(self.service.key()).to_string(),
            Row::Address => {
                let url = self.service.base(env);
                url.strip_prefix("https://").unwrap_or(&url).to_string()
            }
            Row::User => env.settings.read(&USER).to_string(),
            Row::Password if env.secrets.get(PASSWORD).is_some() => String::from("••••••"),
            Row::Password => String::new(),
            Row::Notebook => notebook_name(env).to_string(),
            Row::Send => String::from("›"),
        }
    }
}

impl Screen for SetupScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("joplin.title");
        self.header.draw(canvas, env, &style, title, None, None);
        let rows = self.rows();
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let row = rows[index];
                let title = env.i18n.tr(row.title(self.service));
                let value = self.value(row, env);
                ui::draw_value_row(canvas, env, &style, rect, focused, title, &value);
            },
        );
        // The help in small letters, to fit on small screens.
        let help = env.i18n.tr(self.focused().help());
        let mut small = style;
        small.text = style.small;
        ui::draw_message(canvas, env, &small, self.help, help);
        ui::draw_note_line(canvas, env, &style, self.status, &self.message);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.activate(self.focused(), cx);
            return Handled::Yes;
        }
        let Some(change) = self.list.navigate(action, self.rows().len()) else {
            return Handled::No;
        };
        if change != ListChange::None {
            // The help below follows the focus.
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.rows().len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        cx.request_redraw();
        self.activate(self.rows()[index], cx);
        Handled::Yes
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Back from the keyboard: the help of the row chosen by a tap.
        cx.request_redraw_area(self.help);
    }

    fn on_event(&mut self, event: &Id, p: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(finished) = DONE.read(event, p) {
            self.message = finished.text;
        } else if let Some(typed) = ENTERED.read(event, p) {
            // On to the next row.
            if let Some(row) = typed.context.get(FIELD).and_then(Row::of_field) {
                let rows = self.rows();
                let at = rows.iter().position(|r| *r == row).unwrap_or(0);
                self.list.set_focus((at + 1).min(rows.len() - 1));
            }
        } else if event == events::SETTING_CHANGED.id() {
            self.service = Service::of(&env);
            self.list.set_len(self.rows().len());
        } else {
            return;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_by_field_and_service() {
        for row in ALL {
            if !row.field().is_empty() {
                assert_eq!(Row::of_field(row.field()), Some(row));
            }
        }
        assert_eq!(Row::of_field(""), None);
        assert!(!Row::of(Service::Cloud).contains(&Row::Address));
        assert!(Row::of(Service::WebDav).contains(&Row::Address));
        assert_eq!(Row::User.title(Service::Server), "joplin.email");
        assert_eq!(Row::User.title(Service::WebDav), "joplin.user");
    }
}
