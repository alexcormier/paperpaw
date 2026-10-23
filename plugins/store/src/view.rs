//! A list of plugins: those of a group, those installed (those with an
//! update first), the newest, or those a search found. A group's list can be
//! ordered by name, newest first or most recently updated first; long
//! lists can jump to a page. The plugins to show are picked through the
//! index on the storage once, as the list opens or its order changes, and
//! kept there; the screen holds only those of the page shown.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::install::{Origins, Progress};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen, ask,
};

use crate::catalog::{self, Entry, Index, Order};
use crate::list::{group_name, open_page};
use crate::room::Room;
use crate::{DOWNLOADED, INSTALLER, queue, rows};

/// Parameters: which list, its order.
const VIEW: &str = "view";
pub(crate) const ORDER: &str = "order";
/// The newest plugins shown at most.
const NEWEST: usize = 30;

/// The answer to *Sort by*.
const SORTED: Event<Picked> = Event::new("store.sorted");
/// The page to go to.
const JUMPED: Event<Typed> = Event::new("store.jumped");

/// Which plugins a list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum View {
    Group(usize),
    Installed,
    New,
    Search(String),
}

impl View {
    pub(crate) fn params(&self) -> Params {
        let view = match self {
            View::Group(group) => alloc::format!("group:{group}"),
            View::Installed => "installed".to_string(),
            View::New => "new".to_string(),
            View::Search(_) => "search".to_string(),
        };
        let params = Params::new().with(VIEW, view);
        match self {
            View::Search(text) => params.with(params::TEXT, text.clone()),
            _ => params,
        }
    }

    fn of(params: &Params) -> View {
        let view = params.get(VIEW).unwrap_or_default();
        match view {
            "installed" => View::Installed,
            "new" => View::New,
            "search" => View::Search(params.get(params::TEXT).unwrap_or_default().to_string()),
            _ => View::Group(
                view.strip_prefix("group:")
                    .and_then(|g| g.parse().ok())
                    .filter(|&g| g < catalog::GROUPS.len())
                    .unwrap_or(0),
            ),
        }
    }
}

/// A row above the plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Top {
    /// The fonts browser, with the looks.
    Fonts,
    /// The order, to choose another.
    Sort,
    /// Updates all plugins of the list that have an update.
    UpdateAll,
}

pub(crate) struct ViewScreen {
    style: UiStyle,
    header: ui::Header,
    view: View,
    order: Order,
    index: Option<Index>,
    top: Vec<Top>,
    /// How many plugins the list has.
    hits: usize,
    list: ListView,
    status: Rect,
    /// The plugins of the page shown: their place in the index.
    page: Vec<(usize, Entry)>,
    job: Option<Progress>,
    /// The plugins waiting in the update queue (read with the job, never
    /// while drawing).
    queued: Vec<String>,
    /// The plugins of the installed list with an update.
    updates: Vec<String>,
    /// Which versions the store installed (read with the list, never
    /// while drawing).
    origins: Origins,
    /// What the installed list says at its foot of the room left (read
    /// with the list, never while drawing).
    room: Option<String>,
}

impl ViewScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area().height + style.gap as u16;
        let view = View::of(params);
        // The installed list says at its foot how much room is left.
        let line = if view == View::Installed {
            (style.small * 1.3) as u16 + style.gap as u16 * 2
        } else {
            0
        };
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(0, top, display.width, status.y.saturating_sub(top));
        let order = Order::of(params.get(ORDER).unwrap_or_default());
        let mut screen = Self {
            list: ListView::new(area, rows::height(&style)).focus_on_keys(env),
            status,
            style,
            header,
            view,
            order,
            index: Index::kept(env.storage),
            top: Vec::new(),
            hits: 0,
            page: Vec::new(),
            job: INSTALLER.progress(env.storage),
            updates: Vec::new(),
            origins: Origins::default(),
            queued: queue::waiting(env),
            room: None,
        };
        screen.pick(env);
        screen
    }

    /// Picks the plugins of the list anew.
    fn pick(&mut self, env: &Env<'_>) {
        let Some(index) = &self.index else {
            return;
        };
        let storage = env.storage;
        let mut updates = Vec::new();
        self.origins = Origins::read(storage);
        let origins = &self.origins;
        self.hits = match &self.view {
            View::Group(group) => {
                let group = *group;
                index.pick(
                    storage,
                    |_, e| (e.group == group).then_some(0),
                    self.order,
                    usize::MAX,
                )
            }
            View::Installed => {
                self.room = Room::of(env, None).line(env);
                index.pick(
                    storage,
                    |_, e| {
                        let newer = e.newer_in(env, origins);
                        if newer {
                            updates.push(e.id.clone());
                        }
                        e.installed(env).map(|_| u32::from(newer))
                    },
                    Order::Rank,
                    usize::MAX,
                )
            }
            View::New => index.pick(storage, |_, _| Some(0), Order::Added, NEWEST),
            View::Search(text) => index.search(storage, text),
        };
        self.top.clear();
        match self.view {
            View::Group(group) => {
                let fonts = env.actions.contains(crate::FONTS.as_str());
                if fonts && catalog::GROUPS[group] == "look" {
                    self.top.push(Top::Fonts);
                }
                if self.hits > 0 {
                    self.top.push(Top::Sort);
                }
            }
            View::Installed if !updates.is_empty() => self.top.push(Top::UpdateAll),
            _ => {}
        }
        self.updates = updates;
        self.read_page(env);
    }

    /// Rows of the list: the one above the plugins, then the plugins.
    fn len(&self) -> usize {
        self.top.len() + self.hits
    }

    /// The plugin of row `row`, its number among those of the list.
    fn hit_of(&self, row: usize) -> Option<usize> {
        row.checked_sub(self.top.len())
    }

    /// The plugins of the page shown, read from the index.
    fn read_page(&mut self, env: &Env<'_>) {
        self.page.clear();
        let Some(index) = &self.index else {
            return;
        };
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        for row in first..(first + rows).min(self.len()) {
            if let Some(hit) = self.hit_of(row)
                && let Some(n) = catalog::hit(env.storage, hit)
                && let Some(plugin) = index.entry(env.storage, n)
            {
                self.page.push((n, plugin));
            }
        }
    }

    /// The plugin of row `row`, if it is on the page shown.
    fn plugin(&self, row: usize) -> Option<&(usize, Entry)> {
        let first = self.list.page() * self.list.rows();
        let skip = if self.list.page() == 0 {
            self.top.len()
        } else {
            0
        };
        row.checked_sub(first + skip)
            .and_then(|at| self.page.get(at))
    }

    fn activate(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(&top) = self.top.get(row) {
            match top {
                Top::Fonts => cx.command(Command::RunAction(crate::FONTS)),
                Top::Sort => {
                    let options: Vec<&str> = Order::ALL.iter().map(|o| o.key()).collect();
                    cx.command(
                        paperpaw_sdk::ask::choice(env.i18n.tr("store.sort"), &SORTED)
                            .options("store.order.", &options)
                            .command(),
                    );
                }
                Top::UpdateAll => {
                    let mut out = Vec::new();
                    queue::add(&env, &self.updates, &mut out);
                    for command in out {
                        cx.command(command);
                    }
                    self.job = INSTALLER.progress(env.storage);
                    self.queued = queue::waiting(&env);
                    self.redraw(cx);
                }
            }
            return;
        }
        if let Some((n, plugin)) = self.plugin(row) {
            cx.command(open_page(*n, plugin));
        }
    }

    /// Asks which page to go to.
    fn ask_page(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let pages = self.list.pages(self.len()) as i64;
        let title = env
            .i18n
            .format("store.go-to-page", &[("pages", Arg::Int(pages))]);
        cx.command(ask::text(title, &JUMPED).number().command());
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.read_page(&env);
        }
        self.list.request(change, cx);
    }

    fn title(&self, env: &Env<'_>) -> String {
        match &self.view {
            View::Group(group) => group_name(env, *group),
            View::Installed => env.i18n.tr("store.installed-list").to_string(),
            View::New => env.i18n.tr("store.new").to_string(),
            View::Search(text) => env
                .i18n
                .format("store.search.title", &[("query", Arg::Str(text))]),
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }
}

impl Screen for ViewScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = self.title(env);
        let paging = self.list.paging(self.len());
        self.header.draw(canvas, env, &style, &title, None, paging);
        if self.len() == 0 {
            let (icon, title, text) = match self.view {
                View::Search(_) => ("search", "store.search.none", "store.search.none.help"),
                _ => ("puzzle", "store.empty", ""),
            };
            ui::draw_empty(
                canvas,
                env,
                &style,
                self.list.area(),
                icon,
                env.i18n.tr(title),
                if text.is_empty() {
                    ""
                } else {
                    env.i18n.tr(text)
                },
            );
            return;
        }
        if let Some(room) = self.room.as_deref().filter(|_| self.status.height > 0) {
            ui::draw_note_line(canvas, env, &style, self.status, room);
        }
        let job = self.job.as_ref();
        let queued = self.queued.as_slice();
        let jump = self.list.pages(self.len()) > 1;
        let room = ui::icon_room(env, &style, true);
        let top = &self.top;
        let order = self.order;
        let len = self.len();
        self.list
            .draw(canvas, &style, len, |canvas, row, rect, focused| {
                if let Some(&top) = top.get(row) {
                    let ink = style.focus_ink(focused);
                    let (icon, name, value) = match top {
                        Top::Fonts => ("typography", env.i18n.tr("store.fonts"), "›".to_string()),
                        Top::Sort => {
                            let key = alloc::format!("store.order.{}", order.key());
                            (
                                "sort-ascending",
                                env.i18n.tr("store.sort"),
                                env.i18n.tr(&key).to_string(),
                            )
                        }
                        Top::UpdateAll => (
                            "circle-arrow-up",
                            env.i18n.tr("store.update-all"),
                            String::new(),
                        ),
                    };
                    // Below the order of a long list: how to go to a page.
                    if top == Top::Sort && jump {
                        let band = ui::title_area(&style, rect);
                        let rest =
                            ui::draw_row_icon(canvas, env, &style, band, room, Some(icon), ink);
                        ui::draw_value_row(canvas, env, &style, rest, focused, name, &value);
                        let help = ui::tr_input(env, "store.jump.help");
                        rows::draw_note(canvas, env, &style, rect, rest.x, focused, help);
                        return;
                    }
                    let rest = ui::draw_row_icon(canvas, env, &style, rect, room, Some(icon), ink);
                    ui::draw_value_row(canvas, env, &style, rest, focused, name, &value);
                    return;
                }
                if let Some((_, plugin)) = self.plugin(row) {
                    let origins = &self.origins;
                    rows::draw(
                        canvas, env, &style, rect, focused, plugin, job, queued, origins,
                    );
                }
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.job = INSTALLER.progress(env.storage);
        self.queued = queue::waiting(&env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        if let Some(change) = self.list.navigate(action, len) {
            self.changed(change, cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *action == actions::MENU && self.list.pages(len) > 1 {
            // A long press of Select: go to a page.
            self.ask_page(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let paging = self.list.paging(len);
        let env = *cx.env();
        let point = gesture.start;
        // Go to a page: a tap on the page number, or a long press on it or
        // on the arrows beside it (a larger target for a finger).
        let jump = match gesture.kind {
            GestureKind::Tap => self.header.on_page_number(&env, &self.style, point, paging),
            GestureKind::LongPress => self
                .header
                .tap(&env, &self.style, point, paging)
                .is_some_and(|action| action == actions::NEXT_PAGE || action == actions::PREV_PAGE),
            _ => return Handled::No,
        };
        if jump && self.list.pages(len) > 1 {
            self.ask_page(cx);
            return Handled::Yes;
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(row);
        self.activate(row, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(picked) = SORTED.read(event, params) {
            self.order = Order::of(&picked.option);
            self.list.set_focus(0);
            self.pick(&env);
            self.redraw(cx);
        } else if let Some(typed) = JUMPED.read(event, params) {
            let len = self.len();
            if let Ok(page) = typed.text.trim().parse::<usize>() {
                let page = page.clamp(1, self.list.pages(len)) - 1;
                self.list.set_focus(page * self.list.rows());
                self.read_page(&env);
                self.redraw(cx);
            }
        } else if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            self.job = INSTALLER.progress(env.storage);
            self.queued = queue::waiting(&env);
            // What is installed and has updates changed.
            if *event == events::PLUGINS_CHANGED && self.view == View::Installed {
                self.pick(&env);
                self.list.set_len(self.len());
            }
            self.redraw(cx);
        }
    }
}
