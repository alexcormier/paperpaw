//! A plugin's page: what it does, who made it, its licence, version and
//! size, and what can be done with it: install it, stop installing it,
//! update or remove it.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::Package;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::choice::{self, Picked};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, files,
};

use crate::catalog::{self, Entry};
use crate::{CHOICE, DOWNLOADED, INSTALLER, failed};

/// The answer to *Remove?*.
const REMOVE: Event<Picked> = Event::new("store.remove");

/// Lines of the summary at most.
const SUMMARY_LINES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Install,
    Stop,
    Update,
    Remove,
}

pub(crate) struct PageScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the summary and the facts begin.
    top: u16,
    list: ListView,
    status: Rect,
    plugin: Option<(Entry, Package)>,
}

impl PageScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let line = (style.row as u16) * 3 / 2;
        let status = Rect::new(0, display.height - line, display.width, line);
        // The rows go below the text, where it ends (see `draw`).
        let area = Rect::new(0, top, display.width, status.y.saturating_sub(top));
        let id = params.get(params::ID).unwrap_or_default();
        Self {
            list: ListView::new(area, style.row).focus_on_keys(env),
            top,
            status,
            plugin: catalog::find(env, id),
            style,
            header,
        }
    }

    fn rows(&self, env: &Env<'_>) -> Vec<Row> {
        let Some((entry, _)) = &self.plugin else {
            return Vec::new();
        };
        let job = INSTALLER.progress(env.storage);
        if job.is_some_and(|j| j.plugin == entry.id) {
            return alloc::vec![Row::Stop];
        }
        if entry.installed(env).is_none() {
            return alloc::vec![Row::Install];
        }
        let mut rows = Vec::new();
        if entry.newer(env) {
            rows.push(Row::Update);
        }
        rows.push(Row::Remove);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some((entry, package)) = &self.plugin else {
            return;
        };
        match self.rows(&env).get(index).copied() {
            Some(Row::Install | Row::Update) => {
                if !env.caps.network {
                    let text = env.i18n.tr("store.offline").to_string();
                    return cx.command(Command::Notify(text));
                }
                // One at a time: another one going on is said, not
                // stopped.
                if let Some(other) = INSTALLER.progress(env.storage) {
                    let text = env
                        .i18n
                        .format("store.busy", &[("name", Arg::Str(&other.name))]);
                    return cx.command(Command::Notify(text));
                }
                let mut out = Vec::new();
                if !INSTALLER.start(&env, package, &mut out) {
                    out.push(failed(&env, &entry.name));
                }
                for command in out {
                    cx.command(command);
                }
                self.list.focus = 0;
                self.redraw(cx);
            }
            Some(Row::Stop) => {
                INSTALLER.cancel(env.storage);
                self.list.focus = 0;
                self.redraw(cx);
            }
            Some(Row::Remove) => {
                let title = env
                    .i18n
                    .format("store.remove-question", &[("name", Arg::Str(&entry.name))]);
                cx.command(choice::open(
                    CHOICE,
                    &REMOVE,
                    &title,
                    &entry.id,
                    "store.option.",
                    &["remove", "keep"],
                ));
            }
            None => {}
        }
    }

    /// A row's name and what it says at its end.
    fn row_text(env: &Env<'_>, entry: &Entry, row: Row) -> (String, String) {
        let decimal = env.i18n.tr("store.decimal");
        match row {
            Row::Install => (
                env.i18n.tr("store.install").to_string(),
                files::size(entry.size, decimal),
            ),
            Row::Stop => {
                let job = INSTALLER.progress(env.storage).unwrap_or_default();
                (
                    env.i18n.tr("store.stop").to_string(),
                    format!("{} / {}", job.done, job.total),
                )
            }
            Row::Update => (
                env.i18n.tr("store.update").to_string(),
                entry.version.clone(),
            ),
            Row::Remove => (env.i18n.tr("store.remove").to_string(), String::new()),
        }
    }

    /// The facts below the summary: by whom, under which licence, which
    /// version and how large, which version is here.
    fn facts(env: &Env<'_>, entry: &Entry) -> Vec<String> {
        let i18n = env.i18n;
        let mut facts = Vec::new();
        let mut first = Vec::new();
        if !entry.author.is_empty() {
            first.push(i18n.format("store.by", &[("author", Arg::Str(&entry.author))]));
        }
        if !entry.licence.is_empty() {
            first.push(i18n.format("store.licence", &[("licence", Arg::Str(&entry.licence))]));
        }
        if !first.is_empty() {
            facts.push(first.join(" · "));
        }
        let size = files::size(entry.size, i18n.tr("store.decimal"));
        facts.push(i18n.format(
            "store.version",
            &[
                ("version", Arg::Str(&entry.version)),
                ("size", Arg::Str(&size)),
            ],
        ));
        if let Some(installed) = entry.installed(env) {
            facts.push(i18n.format(
                "store.installed-version",
                &[("version", Arg::Str(&installed.to_string()))],
            ));
        }
        facts
    }
}

impl Screen for PageScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows(env);
        let title = self.plugin.as_ref().map_or("", |(e, _)| e.name.as_str());
        self.header.draw(
            canvas,
            env,
            &style,
            title,
            None,
            self.list.paging(rows.len()),
        );
        let Some((entry, _)) = &self.plugin else {
            ui::draw_empty(
                canvas,
                env,
                &style,
                self.list.area,
                "cloud",
                env.i18n.tr("store.gone"),
                env.i18n.tr("store.gone.help"),
            );
            return;
        };
        let Some(font) = style.font(env) else {
            return;
        };
        // What it does, then the facts in small print, then what can be
        // done, each right below the other.
        let margin = style.margin as f32;
        let width = f32::from(self.list.area.width) - 2.0 * margin;
        let mut y = f32::from(self.top);
        let mut paragraph = |y: &mut f32, text: &str, size: f32, color: Color, most: usize| {
            let metrics = font.line_metrics(size);
            let lines = font.wrap(size, text, width);
            let shown = lines.len().min(most);
            let area = Rect::new(
                margin as u16,
                *y as u16,
                width as u16,
                (metrics.line_height * shown as f32) as u16,
            );
            canvas.describe(Role::Text, text, area, false, None);
            for (n, range) in lines.iter().take(shown).enumerate() {
                let text = if n + 1 == shown && lines.len() > shown {
                    ui::fit(&font, size, text[range.start..].trim_end(), width)
                } else {
                    text[range.clone()].trim_end().to_string()
                };
                let baseline = *y + metrics.ascent;
                font.draw(canvas, size, margin, baseline as i32, &text, color);
                *y += metrics.line_height;
            }
        };
        paragraph(
            &mut y,
            &entry.summary,
            style.text,
            Color::BLACK,
            SUMMARY_LINES,
        );
        y += style.gap as f32;
        for fact in PageScreen::facts(env, entry) {
            paragraph(&mut y, &fact, style.small, style.faint(80), 2);
        }
        let gap = style.gap as u16;
        self.list.area.y = y as u16 + gap;
        self.list.area.height = self.status.y.saturating_sub(self.list.area.y);
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let (name, value) = PageScreen::row_text(env, entry, rows[index]);
                ui::draw_value_row(canvas, env, &style, rect, focused, &name, &value);
            },
        );
        canvas.fill(self.status, Color::WHITE);
        let job = INSTALLER.progress(env.storage);
        let note = match job {
            Some(job) if job.plugin == entry.id => env.i18n.tr("store.downloading").to_string(),
            Some(job) => env
                .i18n
                .format("store.busy", &[("name", Arg::Str(&job.name))]),
            None => String::new(),
        };
        if !note.is_empty() {
            ui::draw_note_line(canvas, env, &style, self.status, &note);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows(cx.env()).len();
        if let Some(change) = self.list.navigate(action, len) {
            self.list.request(change, cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.rows(cx.env()).len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.focus = index;
        self.activate(index, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = REMOVE.read(event, params) {
            if picked.option == "remove"
                && let Ok(plugin) = Id::parse(&picked.path)
            {
                let env = *cx.env();
                let mut out = Vec::new();
                INSTALLER.remove(&env, &plugin, &mut out);
                let name = self.plugin.as_ref().map_or("", |(e, _)| e.name.as_str());
                out.push(Command::Notify(
                    env.i18n
                        .format("store.removed", &[("name", Arg::Str(name))]),
                ));
                for command in out {
                    cx.command(command);
                }
                self.list.focus = 0;
            }
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            let len = self.rows(cx.env()).len();
            self.list.focus = self.list.focus.min(len.saturating_sub(1));
            self.redraw(cx);
        }
    }
}
