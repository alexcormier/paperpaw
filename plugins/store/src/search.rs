//! The plugins whose name or summary holds what was searched for. The
//! search goes through the index on the storage once, as the screen
//! opens, and keeps its hits there; the screen holds only those of the
//! page shown.

use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::install::Progress;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use crate::catalog::{self, Entry, Index};
use crate::list::{draw_plugin, open_page};
use crate::{DOWNLOADED, INSTALLER};

pub(crate) struct SearchScreen {
    style: UiStyle,
    header: ui::Header,
    query: alloc::string::String,
    index: Option<Index>,
    /// How many plugins were found.
    hits: usize,
    list: ListView,
    /// The plugins of the page shown: their place in the index.
    page: Vec<(usize, Entry)>,
    job: Option<Progress>,
}

impl SearchScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let query = params.get(params::TEXT).unwrap_or_default().to_string();
        let index = Index::kept(env.storage);
        let hits = index
            .as_ref()
            .map_or(0, |index| index.search(env.storage, &query));
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            query,
            index,
            hits,
            page: Vec::new(),
            job: INSTALLER.progress(env.storage),
        };
        screen.read_page(env);
        screen
    }

    /// The plugins of the page shown, read from the index.
    fn read_page(&mut self, env: &Env<'_>) {
        self.page.clear();
        let Some(index) = &self.index else {
            return;
        };
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        for hit in first..(first + rows).min(self.hits) {
            if let Some(n) = catalog::hit(env.storage, hit)
                && let Some(plugin) = index.entry(env.storage, n)
            {
                self.page.push((n, plugin));
            }
        }
    }

    fn activate(&mut self, hit: usize, cx: &mut Cx<'_, '_>) {
        let first = self.list.page() * self.list.rows();
        if let Some((n, plugin)) = hit.checked_sub(first).and_then(|at| self.page.get(at)) {
            cx.command(open_page(*n, plugin));
        }
    }

    fn changed(&mut self, change: ListChange, cx: &mut Cx<'_, '_>) {
        if change == ListChange::Page {
            let env = *cx.env();
            self.read_page(&env);
        }
        self.list.request(change, cx);
    }
}

impl Screen for SearchScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env
            .i18n
            .format("store.search.title", &[("query", Arg::Str(&self.query))]);
        let paging = self.list.paging(self.hits);
        self.header.draw(canvas, env, &style, &title, None, paging);
        if self.hits == 0 {
            ui::draw_empty(
                canvas,
                env,
                &style,
                self.list.area,
                "search",
                env.i18n.tr("store.search.none"),
                env.i18n.tr("store.search.none.help"),
            );
            return;
        }
        let first = self.list.page() * self.list.rows();
        let (page, job) = (&self.page, self.job.as_ref());
        self.list
            .draw(canvas, &style, self.hits, |canvas, hit, rect, focused| {
                if let Some((_, plugin)) = hit.checked_sub(first).and_then(|at| page.get(at)) {
                    draw_plugin(canvas, env, &style, rect, focused, plugin, job);
                }
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        self.job = INSTALLER.progress(cx.env().storage);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(change) = self.list.navigate(action, self.hits) {
            self.changed(change, cx);
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
        let paging = self.list.paging(self.hits);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(hit) = self.list.hit(gesture.start, self.hits) else {
            return Handled::No;
        };
        self.list.focus = hit;
        self.activate(hit, cx);
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if DOWNLOADED.read(event, params).is_some() || *event == events::PLUGINS_CHANGED {
            self.job = INSTALLER.progress(cx.env().storage);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }
}
