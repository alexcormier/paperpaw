//! The plugins of the library for this device, under the headings of
//! their groups, each with what it does and whether it is here; *Search*
//! above them, *Fonts* first with the looks. Choosing one shows its page.
//! The list kept from the last visit shows at once, while a fresh one is
//! fetched.
//!
//! However long the list, the screen keeps only the runs of plugins of one
//! group and the plugins of the page shown, read from the index on the
//! storage when the page changes (never while drawing).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::Progress;
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::ui::sections::{Item, Section};
use paperpaw_sdk::ui::{self, ListChange, Sections, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, HttpRequest, Id, Params,
    Rect, RefreshMode, Screen, ask, files,
};

use crate::catalog::{self, Entry, GROUPS, Index};
use crate::{CATALOG, DOWNLOADED, FONTS, INSTALLER, PAGE_SCREEN, SEARCH_SCREEN};

/// The answer with the list.
const LOADED: Event<HttpReply> = Event::new("store.loaded");
/// What is searched for.
const SEARCHED: Event<Typed> = Event::new("store.searched");
/// Parameter of a plugin's page: its place in the index.
pub(crate) const ENTRY: &str = "entry";

/// The group *Fonts* goes with.
fn fonts_group() -> usize {
    catalog::group("look")
}

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trouble {
    None,
    Loading,
    Offline,
    Failed,
}

/// What an entry of the list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Search,
    Fonts,
    /// Plugin `n` of the index.
    Plugin(usize),
}

/// A section of plugins of one group: the first of them in the index,
/// how many, and whether *Fonts* comes before them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Part {
    group: usize,
    first: usize,
    count: usize,
    fonts: bool,
}

pub(crate) struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    /// Where the list goes, and the line at the bottom.
    area: Rect,
    status: Rect,
    /// The list read; `None` until one was.
    index: Option<Index>,
    /// The sections after *Search*.
    parts: Vec<Part>,
    list: Sections,
    /// The plugins of the page shown.
    page: Vec<(usize, Entry)>,
    /// The download going on.
    job: Option<Progress>,
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
            index: env.catalog_device.and_then(|_| Index::kept(env.storage)),
            parts: Vec::new(),
            list: Sections::new(area, 0, 1, Vec::new()),
            page: Vec::new(),
            job: INSTALLER.progress(env.storage),
            trouble: Trouble::None,
            keys: !env.device.input.touch,
            asked: false,
            style,
            header,
        };
        screen.arrange(env);
        screen
    }

    /// The sections anew from the index, the focus kept where it was.
    fn arrange(&mut self, env: &Env<'_>) {
        let focus = self.list.focus;
        self.parts.clear();
        let Some(index) = &self.index else {
            self.list = Sections::new(self.area, 0, 1, Vec::new());
            self.page.clear();
            return;
        };
        let mut fonts = env.actions.contains(FONTS.as_str());
        let mut first = 0;
        for &(group, count) in index.runs() {
            if fonts && group > fonts_group() {
                self.parts.push(Part {
                    group: fonts_group(),
                    first,
                    count: 0,
                    fonts: true,
                });
                fonts = false;
            }
            let with_fonts = fonts && group == fonts_group();
            fonts &= !with_fonts;
            self.parts.push(Part {
                group,
                first,
                count,
                fonts: with_fonts,
            });
            first += count;
        }
        if fonts {
            self.parts.push(Part {
                group: fonts_group(),
                first,
                count: 0,
                fonts: true,
            });
        }
        let sections = core::iter::once(Section {
            heading: false,
            count: 1,
        })
        .chain(self.parts.iter().map(|part| Section {
            heading: true,
            count: part.count + usize::from(part.fonts),
        }))
        .collect();
        let heading = ui::heading_height(env, &self.style);
        let row = self.style.noted_row() as u16;
        self.list = Sections::new(self.area, heading, row, sections);
        self.list.focus_on(focus);
        self.read_page(env);
    }

    /// What entry `entry` of the list is.
    fn row(&self, entry: usize) -> Option<Row> {
        let (section, at) = self.list.section_of(entry)?;
        let Some(part) = section.checked_sub(1).and_then(|n| self.parts.get(n)) else {
            return Some(Row::Search);
        };
        match (part.fonts, at) {
            (true, 0) => Some(Row::Fonts),
            (true, at) => Some(Row::Plugin(part.first + at - 1)),
            (false, at) => Some(Row::Plugin(part.first + at)),
        }
    }

    /// The plugins of the page shown, read from the index.
    fn read_page(&mut self, env: &Env<'_>) {
        self.page.clear();
        let Some(index) = &self.index else {
            return;
        };
        for entry in self.list.page_entries() {
            if let Some(Row::Plugin(n)) = self.row(entry)
                && let Some(plugin) = index.entry(env.storage, n)
            {
                self.page.push((n, plugin));
            }
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Fetches the list anew.
    fn fetch(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(device) = env.catalog_device else {
            return;
        };
        if !env.caps.network {
            self.trouble = Trouble::Offline;
            return self.redraw(cx);
        }
        let list = env.settings.read(&CATALOG);
        let url = catalog::list_url(list.trim(), device, env.i18n.language());
        let mut request = HttpRequest::new("GET", &url, LOADED);
        request.save_to = Some(catalog::list_part());
        cx.command(Command::Http(request));
        if self.index.is_none() {
            self.trouble = Trouble::Loading;
            self.redraw(cx);
        }
    }

    fn loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let fetched = catalog::list_part();
        let index = catalog::Device::of(&env)
            .filter(|_| (200..300).contains(&reply.status))
            .and_then(|device| catalog::take(env.storage, &fetched, &device));
        match index {
            Some(index) => {
                self.index = Some(index);
                self.trouble = Trouble::None;
                self.arrange(&env);
            }
            None => {
                let _ = env.storage.remove(&fetched);
                // The list kept still serves; no answer at all is no
                // connection.
                if self.index.is_none() {
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

    fn activate(&mut self, entry: usize, cx: &mut Cx<'_, '_>) {
        match self.row(entry) {
            Some(Row::Search) => {
                let env = *cx.env();
                cx.command(ask::text(env.i18n.tr("store.search"), &SEARCHED).command());
            }
            Some(Row::Fonts) => cx.command(Command::RunAction(FONTS)),
            Some(Row::Plugin(n)) => {
                if let Some((_, plugin)) = self.page.iter().find(|(m, _)| *m == n) {
                    cx.command(open_page(n, plugin));
                }
            }
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
        self.list.is_empty() && matches!(self.trouble, Trouble::Offline | Trouble::Failed)
    }

    fn paging(&self) -> Option<(usize, usize)> {
        Some(self.list.paging())
    }

    /// Asks for what `change` needs, the page read first where it turned.
    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.read_page(&env);
        }
        self.list.request(change, cx);
    }
}

/// The command that opens the page of plugin `n` of the index.
pub(crate) fn open_page(n: usize, plugin: &Entry) -> Command {
    Command::OpenScreen(
        PAGE_SCREEN,
        Params::new()
            .with(params::ID, plugin.id.clone())
            .with(ENTRY, n.to_string()),
    )
}

/// What the row of `plugin` says at its end: how far its download is,
/// whether it waits for the reader or a restart, whether it is here, or
/// its size.
pub(crate) fn state(env: &Env<'_>, job: Option<&Progress>, plugin: &Entry) -> String {
    if let Some(job) = job.filter(|j| j.plugin == plugin.id) {
        if job.done >= job.total {
            return env.i18n.tr("store.state.waiting").to_string();
        }
        return format!("{} / {}", job.done, job.total);
    }
    let restart = env
        .plugins
        .get(&plugin.id)
        .is_some_and(paperpaw_sdk::PluginInfo::restart_needed);
    let key = if restart {
        "store.state.restart"
    } else if plugin.newer(env) {
        "store.state.update"
    } else if plugin.installed(env).is_some() {
        "store.state.installed"
    } else {
        return files::size(plugin.size, env.i18n.tr("store.decimal"));
    };
    env.i18n.tr(key).to_string()
}

/// Draws the row of `plugin`: its name, its state, what it does.
pub(crate) fn draw_plugin(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    plugin: &Entry,
    job: Option<&Progress>,
) {
    let state = state(env, job, plugin);
    ui::draw_noted_row(
        canvas,
        env,
        style,
        rect,
        focused,
        &plugin.name,
        &state,
        &plugin.summary,
    );
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("store.title");
        self.header
            .draw(canvas, env, &style, title, None, self.paging());
        if self.list.is_empty() {
            let (icon, title, text) = match self.trouble {
                _ if env.catalog_device.is_none() => {
                    ("cloud", "store.unsupported", "store.unsupported.help")
                }
                Trouble::Offline => ("wifi-off", "store.offline", "store.offline.help"),
                Trouble::Failed => ("cloud", "store.unreachable", "store.unreachable.help"),
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
        let job = self.job.as_ref();
        self.list
            .draw(canvas, &style, self.keys, |canvas, item, rect, focused| {
                let entry = match item {
                    Item::Heading(section) => {
                        let group = section
                            .checked_sub(1)
                            .and_then(|n| self.parts.get(n))
                            .map_or(0, |part| part.group);
                        let key = format!("store.group.{}", GROUPS[group]);
                        return ui::draw_heading(canvas, env, &style, rect, env.i18n.tr(&key));
                    }
                    Item::Entry(entry) => entry,
                };
                let (name, value, note) = match self.row(entry) {
                    Some(Row::Search) => ("store.search", "", "store.search.summary"),
                    Some(Row::Fonts) => ("store.fonts", "›", "store.fonts.summary"),
                    Some(Row::Plugin(n)) => {
                        if let Some((_, plugin)) = self.page.iter().find(|(m, _)| *m == n) {
                            draw_plugin(canvas, env, &style, rect, focused, plugin, job);
                        }
                        return;
                    }
                    None => return,
                };
                let (name, note) = (env.i18n.tr(name), env.i18n.tr(note));
                ui::draw_noted_row(canvas, env, &style, rect, focused, name, value, note);
            });
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
        self.job = INSTALLER.progress(env.storage);
        self.read_page(&env);
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
            self.list.step(false)
        } else if forward.contains(a) {
            self.list.step(true)
        } else if *a == actions::NEXT_PAGE {
            self.list.turn(true)
        } else if *a == actions::PREV_PAGE {
            self.list.turn(false)
        } else if *a == actions::CONFIRM {
            let focus = if self.list.is_empty() {
                usize::MAX
            } else {
                self.list.focus
            };
            self.activate(focus, cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.changed(change, cx);
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
        if self.list.is_empty() {
            if self.area.contains(gesture.start) {
                self.activate(usize::MAX, cx);
                return Handled::Yes;
            }
            return Handled::No;
        }
        let Some(entry) = self.list.hit(gesture.start) else {
            return Handled::No;
        };
        self.list.focus = entry;
        self.activate(entry, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = LOADED.read(event, params) {
            self.loaded(&reply, cx);
        } else if let Some(typed) = SEARCHED.read(event, params) {
            let text = typed.text.trim();
            if !text.is_empty() {
                cx.command(Command::OpenScreen(
                    SEARCH_SCREEN,
                    Params::new().with(params::TEXT, text.to_string()),
                ));
            }
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            // Progress, or a plugin installed or removed.
            self.job = INSTALLER.progress(cx.env().storage);
            self.redraw(cx);
        }
    }
}
