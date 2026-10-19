//! The plugins of the library fit for this device, under the headings of
//! their groups, each with what it does and whether it is here; *Fonts*
//! with the looks. Choosing one shows its page. The list kept from the
//! last visit shows at once, while a fresh one is fetched.
//!
//! Only the plugins' groups and where their lines are in the index are
//! held (8 bytes each); those of the page shown are read from it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::ui::{self, ListChange, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, HttpRequest, Id, Params,
    Rect, RefreshMode, Screen, files,
};

use crate::catalog::{self, Entry, GROUPS, Index};
use crate::{CATALOG, DOWNLOADED, FONTS, INSTALLER, PAGE_SCREEN};

/// The answer with the list.
const LOADED: Event<HttpReply> = Event::new("store.loaded");

/// The group *Fonts* goes with.
const FONTS_GROUP: usize = 5;

fn fetched_path() -> String {
    format!("{}.part", catalog::path())
}

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    None,
    Loading,
    Offline,
    Failed,
}

/// A row: a group's heading, *Fonts*, or a plugin of the list (4 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Heading(u8),
    Fonts,
    Plugin(u16),
}

pub(crate) struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the rows go, and the line at the bottom.
    area: Rect,
    status: Rect,
    /// The list read; `None` until one was.
    plugins: Option<Index>,
    /// The plugins of the page shown, read from the index.
    page: Vec<(usize, Entry)>,
    shown: Vec<Row>,
    rows: ui::Rows,
    trouble: Trouble,
    /// The focus is shown: always with buttons only, on touch screens
    /// once a button moved it.
    keys: bool,
    /// A fresh list was asked for since the screen opened.
    asked: bool,
}

impl ListScreen {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let line = (style.row as u16) * 3 / 2;
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + line),
        );
        let mut screen = Self {
            area,
            status,
            plugins: catalog::kept(env),
            page: Vec::new(),
            shown: Vec::new(),
            rows: ui::Rows::new(area, Vec::new(), Vec::new()),
            trouble: Trouble::None,
            keys: !env.device.input.touch,
            asked: false,
            style,
            header,
        };
        screen.arrange(env);
        screen
    }

    /// The rows anew from the list, the focus kept where it was.
    fn arrange(&mut self, env: &Env<'_>) {
        let focused = self.rows.focus;
        let count = self.plugins.as_ref().map_or(0, Index::len);
        let fonts = self.plugins.is_some() && env.actions.contains(FONTS.as_str());
        let mut shown = Vec::new();
        for group in 0..GROUPS.len() {
            let start = shown.len();
            if fonts && group == FONTS_GROUP {
                shown.push(Row::Fonts);
            }
            if let Some(index) = &self.plugins {
                shown.extend(
                    (0..count)
                        .filter(|&n| index.group(n) == group)
                        .map(|n| Row::Plugin(n as u16)),
                );
            }
            if shown.len() > start {
                shown.insert(start, Row::Heading(group as u8));
            }
        }
        let heading = ui::heading_height(env, &self.style);
        let entry = self.style.noted_row() as u16;
        let heights = shown
            .iter()
            .map(|row| match row {
                Row::Heading(_) => heading,
                _ => entry,
            })
            .collect();
        let headings = shown
            .iter()
            .map(|row| matches!(row, Row::Heading(_)))
            .collect();
        shown.shrink_to_fit();
        self.rows = ui::Rows::new(self.area, heights, headings);
        if focused < shown.len() && !self.rows.is_heading(focused) {
            self.rows.focus = focused;
        }
        self.shown = shown;
        self.page.clear();
    }

    /// The plugins of the page shown, read where they are not yet.
    fn read_page(&mut self, env: &Env<'_>) {
        let Some(index) = &self.plugins else {
            return;
        };
        let wanted: Vec<usize> = self
            .rows
            .visible()
            .into_iter()
            .filter_map(|(row, _)| match self.shown.get(row) {
                Some(Row::Plugin(n)) => Some(usize::from(*n)),
                _ => None,
            })
            .collect();
        self.page.retain(|(n, _)| wanted.contains(n));
        for n in wanted {
            if !self.page.iter().any(|(m, _)| *m == n)
                && let Some(entry) = index.entry(env.storage, n)
            {
                self.page.push((n, entry));
            }
        }
    }

    /// Plugin `n` of the list, from the page read or the index.
    fn entry(&self, env: &Env<'_>, n: usize) -> Option<Entry> {
        self.page
            .iter()
            .find(|(m, _)| *m == n)
            .map(|(_, entry)| entry.clone())
            .or_else(|| self.plugins.as_ref()?.entry(env.storage, n))
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Fetches the list anew.
    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !env.caps.network {
            self.trouble = Trouble::Offline;
            return self.redraw(cx);
        }
        let url = env.settings.read(&CATALOG);
        let mut request = HttpRequest::new("GET", url.trim(), LOADED);
        request.save_to = Some(fetched_path());
        cx.command(Command::Http(request));
        if self.plugins.is_none() {
            self.trouble = Trouble::Loading;
            self.redraw(cx);
        }
    }

    fn loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let fetched = fetched_path();
        let setting = env.settings.read(&CATALOG);
        let address = reply.final_url.as_deref().unwrap_or(setting.trim());
        let plugins = (200..300)
            .contains(&reply.status)
            .then(|| catalog::take(&env, &fetched, address))
            .flatten();
        match plugins {
            Some(plugins) => {
                self.plugins = Some(plugins);
                self.trouble = Trouble::None;
                self.arrange(&env);
            }
            None => {
                let _ = env.storage.remove(&fetched);
                // The list kept still serves; no answer at all is no
                // connection.
                if self.plugins.is_none() {
                    self.trouble = if reply.status == 0 {
                        Trouble::Offline
                    } else {
                        Trouble::Failed
                    };
                }
            }
        }
        self.redraw(cx);
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        match self.shown.get(index) {
            Some(Row::Fonts) => cx.command(Command::RunAction(FONTS)),
            Some(&Row::Plugin(n)) => {
                if let Some(entry) = self.entry(cx.env(), usize::from(n)) {
                    cx.command(Command::OpenScreen(
                        PAGE_SCREEN,
                        Params::new().with(params::ID, entry.id.clone()),
                    ));
                }
            }
            Some(Row::Heading(_)) => {}
            None => {
                if self.retry() {
                    self.fetch(cx);
                }
            }
        }
    }

    /// Whether *Select* tries again: nothing to show for want of the
    /// list.
    fn retry(&self) -> bool {
        self.shown.is_empty() && matches!(self.trouble, Trouble::Offline | Trouble::Failed)
    }

    fn paging(&self) -> Option<(usize, usize)> {
        let (page, pages) = self.rows.paging();
        Some((page, pages))
    }

    /// What the row of `entry` says at its end.
    fn state(env: &Env<'_>, entry: &Entry) -> String {
        if let Some(job) = INSTALLER
            .progress(env.storage)
            .filter(|j| j.plugin == entry.id)
        {
            return format!("{} / {}", job.done, job.total);
        }
        let key = if entry.newer(env) {
            "store.state.update"
        } else if entry.installed(env).is_some() {
            "store.state.installed"
        } else {
            return files::size(entry.size, env.i18n.tr("store.decimal"));
        };
        env.i18n.tr(key).to_string()
    }
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("store.title");
        self.header
            .draw(canvas, env, &style, title, None, self.paging());
        if self.shown.is_empty() {
            let (icon, title, text) = match self.trouble {
                Trouble::Offline => ("wifi-off", "store.offline", "store.offline.help"),
                Trouble::Failed => ("cloud", "store.unreachable", "store.unreachable.help"),
                _ if self.plugins.is_some() => {
                    ("cloud-download", "store.empty", "store.empty.help")
                }
                _ => ("cloud-download", "store.loading", ""),
            };
            let text = if text.is_empty() {
                ""
            } else {
                ui::tr_input(env, text)
            };
            let area = Rect::new(
                0,
                self.area.y,
                self.area.width,
                self.area.height + self.status.height,
            );
            ui::draw_empty(canvas, env, &style, area, icon, env.i18n.tr(title), text);
            return;
        }
        self.read_page(env);
        let (page, shown) = (&self.page, &self.shown);
        self.rows.draw(
            canvas,
            &style,
            self.keys,
            |canvas, index, rect, focused| match shown[index] {
                Row::Heading(group) => {
                    let key = format!("store.group.{}", GROUPS[usize::from(group)]);
                    ui::draw_heading(canvas, env, &style, rect, env.i18n.tr(&key));
                }
                Row::Fonts => ui::draw_noted_row(
                    canvas,
                    env,
                    &style,
                    rect,
                    focused,
                    env.i18n.tr("store.fonts"),
                    "›",
                    env.i18n.tr("store.fonts.summary"),
                ),
                Row::Plugin(n) => {
                    let Some((_, entry)) = page.iter().find(|(m, _)| *m == usize::from(n)) else {
                        return;
                    };
                    let state = ListScreen::state(env, entry);
                    ui::draw_noted_row(
                        canvas,
                        env,
                        &style,
                        rect,
                        focused,
                        &entry.name,
                        &state,
                        &entry.summary,
                    );
                }
            },
        );
        canvas.fill(self.status, Color::WHITE);
        let note = ui::tr_input(env, "store.hint");
        ui::draw_note_line(canvas, env, &style, self.status, note);
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM && self.retry() {
            Hint::Text(env.i18n.tr("store.retry").to_string())
        } else {
            Hint::Default
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Back from a plugin's page: what changed there shows.
        let env = *cx.env();
        self.arrange(&env);
        if !self.asked {
            self.asked = true;
            self.fetch(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let back = [actions::UP, actions::LEFT, actions::PREVIOUS];
        let forward = [actions::DOWN, actions::RIGHT, actions::NEXT];
        if (back.contains(a) || forward.contains(a)) && !self.keys {
            // The first button press shows where the focus is.
            self.keys = true;
            self.redraw(cx);
            return Handled::Yes;
        }
        let change = if back.contains(a) {
            self.rows.step(false)
        } else if forward.contains(a) {
            self.rows.step(true)
        } else if *a == actions::NEXT_PAGE {
            self.rows.turn(true)
        } else if *a == actions::PREV_PAGE {
            self.rows.turn(false)
        } else if *a == actions::CONFIRM {
            let focus = if self.shown.is_empty() {
                usize::MAX
            } else {
                self.rows.focus
            };
            self.activate(focus, cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if change == ListChange::Page {
            self.redraw(cx);
        } else {
            self.rows.request(change, cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, self.paging())
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        if self.shown.is_empty() {
            if self.area.contains(gesture.start) {
                self.activate(usize::MAX, cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(index) = self.rows.hit(gesture.start) else {
            return Handled::No;
        };
        self.rows.focus = index;
        self.activate(index, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = LOADED.read(event, params) {
            self.loaded(&reply, cx);
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            // Progress, or a plugin installed or removed.
            self.redraw(cx);
        }
    }
}
