//! A plugin's page: what it does, who made it, its licence, version and
//! size, and what can be done with it: install it, stop installing it,
//! update or remove it. What the list says shows at once; the plugin's
//! own page at the library, with its files, is fetched as the page opens.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Package, Progress};
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::choice::{self, Picked};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, HttpRequest, Id, Params,
    Rect, RefreshMode, Screen, files,
};

use crate::catalog::{self, Entry, Index};
use crate::list::ENTRY;
use crate::{CATALOG, CHOICE, DOWNLOADED, INSTALLER, failed};

/// The answer to *Remove?*.
const REMOVE: Event<Picked> = Event::new("store.remove");
/// The answer with the plugin's page at the library.
const DETAILS: Event<HttpReply> = Event::new("store.details");

/// Lines of the summary at most.
const SUMMARY_LINES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Install,
    Stop,
    Update,
    Remove,
}

/// The plugin's files, from its page at the library.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Details {
    /// Not asked for yet.
    None,
    /// Asked for, at this address.
    Asked(String),
    Here(Package),
    /// Not to be had.
    Failed,
}

pub(crate) struct PageScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the summary and the facts begin.
    top: u16,
    list: ListView,
    status: Rect,
    id: String,
    plugin: Option<Entry>,
    details: Details,
    /// *Install* or *Update* was chosen before the files were known.
    waiting: bool,
    /// The download going on.
    job: Option<Progress>,
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
        let id = params.get(params::ID).unwrap_or_default().to_string();
        // What the list says of it, while its own page is fetched.
        let plugin = params
            .get(ENTRY)
            .and_then(|n| n.parse().ok())
            .zip(Index::kept(env.storage))
            .and_then(|(n, index)| index.entry(env.storage, n))
            .filter(|plugin| plugin.id == id);
        Self {
            list: ListView::new(area, style.row).focus_on_keys(env),
            top,
            status,
            id,
            plugin,
            details: Details::None,
            waiting: false,
            job: INSTALLER.progress(env.storage),
            style,
            header,
        }
    }

    fn rows(&self, env: &Env<'_>) -> Vec<Row> {
        let Some(plugin) = &self.plugin else {
            return Vec::new();
        };
        if self.job.as_ref().is_some_and(|j| j.plugin == plugin.id) {
            return alloc::vec![Row::Stop];
        }
        if plugin.installed(env).is_none() {
            return alloc::vec![Row::Install];
        }
        let mut rows = Vec::new();
        if plugin.newer(env) {
            rows.push(Row::Update);
        }
        rows.push(Row::Remove);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Asks the library for the plugin's page.
    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(device) = env.catalog_device.filter(|_| env.caps.network) else {
            self.details = Details::Failed;
            return;
        };
        let list = env.settings.read(&CATALOG);
        let url = catalog::plugin_url(list.trim(), device, env.i18n.language(), &self.id);
        let mut request = HttpRequest::new("GET", &url, DETAILS);
        request.save_to = Some(catalog::plugin_part());
        request.params = Params::new().with(params::ID, self.id.clone());
        cx.command(Command::Http(request));
        self.details = Details::Asked(url);
    }

    fn fetched(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Details::Asked(url) = &self.details else {
            return;
        };
        let address = reply.final_url.clone().unwrap_or_else(|| url.clone());
        let device = env.catalog_device.unwrap_or_default();
        let fetched = catalog::plugin_part();
        let read = (200..300)
            .contains(&reply.status)
            .then(|| catalog::plugin(env.storage, &fetched, &address, device))
            .flatten();
        let _ = env.storage.remove(&fetched);
        match read {
            Some((plugin, package)) => {
                self.plugin = Some(plugin);
                self.details = Details::Here(package);
                if core::mem::take(&mut self.waiting) {
                    self.install(cx);
                }
            }
            None => {
                self.details = Details::Failed;
                if core::mem::take(&mut self.waiting) {
                    let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
                    cx.command(failed(&env, name));
                }
            }
        }
        self.redraw(cx);
    }

    /// Installs the plugin (or its newer version), once its files are
    /// known.
    fn install(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
        if !env.caps.network {
            let text = env.i18n.tr("store.offline").to_string();
            return cx.command(Command::Notify(text));
        }
        // One at a time: another one going on is said, not stopped.
        if let Some(other) = &self.job {
            let text = env
                .i18n
                .format("store.busy", &[("name", Arg::Str(&other.name))]);
            return cx.command(Command::Notify(text));
        }
        match &self.details {
            Details::Here(package) => {
                let mut out = Vec::new();
                if !INSTALLER.start(&env, package, &mut out) {
                    out.push(failed(&env, name));
                }
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage);
                self.list.focus = 0;
                self.redraw(cx);
            }
            Details::Failed => cx.command(failed(&env, name)),
            Details::None | Details::Asked(_) => {
                self.waiting = true;
                if self.details == Details::None {
                    self.fetch(cx);
                }
                self.redraw(cx);
            }
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(plugin) = &self.plugin else {
            return;
        };
        match self.rows(&env).get(index).copied() {
            Some(Row::Install | Row::Update) => self.install(cx),
            Some(Row::Stop) => {
                INSTALLER.cancel(env.storage);
                self.job = None;
                self.list.focus = 0;
                self.redraw(cx);
            }
            Some(Row::Remove) => {
                let title = env
                    .i18n
                    .format("store.remove-question", &[("name", Arg::Str(&plugin.name))]);
                cx.command(choice::open(
                    CHOICE,
                    &REMOVE,
                    &title,
                    &plugin.id,
                    "store.option.",
                    &["remove", "keep"],
                ));
            }
            None => {}
        }
    }

    /// A row's name and what it says at its end.
    fn row_text(&self, env: &Env<'_>, plugin: &Entry, row: Row) -> (String, String) {
        let tr = |key: &str| env.i18n.tr(key).to_string();
        match row {
            Row::Install | Row::Update if self.waiting => {
                let name = if row == Row::Install {
                    "store.install"
                } else {
                    "store.update"
                };
                (tr(name), tr("store.preparing"))
            }
            Row::Install => (
                tr("store.install"),
                files::size(plugin.size, env.i18n.tr("store.decimal")),
            ),
            Row::Stop => {
                let job = self.job.clone().unwrap_or_default();
                (tr("store.stop"), format!("{} / {}", job.done, job.total))
            }
            Row::Update => (tr("store.update"), plugin.version.clone()),
            Row::Remove => (tr("store.remove"), String::new()),
        }
    }

    /// The facts below the summary: by whom, under which licence, which
    /// version and how large, which version is here.
    fn facts(env: &Env<'_>, plugin: &Entry) -> Vec<String> {
        let i18n = env.i18n;
        let mut facts = Vec::new();
        let mut first = Vec::new();
        if !plugin.author.is_empty() {
            first.push(i18n.format("store.by", &[("author", Arg::Str(&plugin.author))]));
        }
        if !plugin.licence.is_empty() {
            first.push(i18n.format("store.licence", &[("licence", Arg::Str(&plugin.licence))]));
        }
        if !first.is_empty() {
            facts.push(first.join(" · "));
        }
        let size = files::size(plugin.size, i18n.tr("store.decimal"));
        facts.push(i18n.format(
            "store.version",
            &[
                ("version", Arg::Str(&plugin.version)),
                ("size", Arg::Str(&size)),
            ],
        ));
        if let Some(installed) = plugin.installed(env) {
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
        let title = self.plugin.as_ref().map_or("", |p| p.name.as_str());
        self.header.draw(
            canvas,
            env,
            &style,
            title,
            None,
            self.list.paging(rows.len()),
        );
        let Some(plugin) = &self.plugin else {
            let (icon, title, text) = match self.details {
                Details::Failed => ("cloud", "store.gone", "store.gone.help"),
                _ => ("cloud-download", "store.loading-plugin", ""),
            };
            let area = Rect::new(
                0,
                self.top,
                self.list.area.width,
                self.status.y.saturating_sub(self.top),
            );
            let text = if text.is_empty() {
                ""
            } else {
                env.i18n.tr(text)
            };
            ui::draw_empty(canvas, env, &style, area, icon, env.i18n.tr(title), text);
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
            &plugin.summary,
            style.text,
            Color::BLACK,
            SUMMARY_LINES,
        );
        y += style.gap as f32;
        for fact in PageScreen::facts(env, plugin) {
            paragraph(&mut y, &fact, style.small, style.faint(80), 2);
        }
        let gap = style.gap as u16;
        self.list.area.y = y as u16 + gap;
        self.list.area.height = self.status.y.saturating_sub(self.list.area.y);
        let texts: Vec<(String, String)> = rows
            .iter()
            .map(|&row| self.row_text(env, plugin, row))
            .collect();
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let (name, value) = &texts[index];
                ui::draw_value_row(canvas, env, &style, rect, focused, name, value);
            },
        );
        canvas.fill(self.status, Color::WHITE);
        let note = match &self.job {
            Some(job) if job.plugin == plugin.id => env.i18n.tr("store.downloading").to_string(),
            Some(job) => env
                .i18n
                .format("store.busy", &[("name", Arg::Str(&job.name))]),
            None => String::new(),
        };
        if !note.is_empty() {
            ui::draw_note_line(canvas, env, &style, self.status, &note);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.job = INSTALLER.progress(cx.env().storage);
        if self.details == Details::None {
            self.fetch(cx);
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
        if let Some(reply) = DETAILS.read(event, params) {
            if reply.context.get(params::ID) == Some(self.id.as_str()) {
                self.fetched(&reply, cx);
            }
        } else if let Some(picked) = REMOVE.read(event, params) {
            if picked.option == "remove"
                && let Ok(plugin) = Id::parse(&picked.path)
            {
                let env = *cx.env();
                let mut out = Vec::new();
                INSTALLER.remove(&env, &plugin, &mut out);
                let name = self.plugin.as_ref().map_or("", |p| p.name.as_str());
                out.push(Command::Notify(
                    env.i18n
                        .format("store.removed", &[("name", Arg::Str(name))]),
                ));
                for command in out {
                    cx.command(command);
                }
                self.job = INSTALLER.progress(env.storage);
                self.list.focus = 0;
            }
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            self.job = INSTALLER.progress(cx.env().storage);
            let len = self.rows(cx.env()).len();
            self.list.focus = self.list.focus.min(len.saturating_sub(1));
            self.redraw(cx);
        }
    }
}
