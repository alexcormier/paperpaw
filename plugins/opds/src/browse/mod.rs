//! Browsing a catalog: a feed's entries, each leading to a further feed
//! or a book; *Search* where the catalog can be searched and *More* where
//! its list goes on. A book is downloaded into the library, in the best
//! format PaperPaw reads, and can be read at once.
//!
//! *Back* returns to the feed before, and from the first to the list of
//! catalogs.

mod covers;
mod downloads;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::{HttpReply, Typed};
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::web::host;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, MemoryClass, Params,
    Rect, RefreshMode, Screen, files,
};

use crate::catalogs::{self, Catalog};
use crate::feed::{self, Feed, Search};
use crate::sync::book_name;
use crate::{CHOICE, ID, OPTION_LABELS, PLUGIN, URL, request};

use self::covers::*;
use self::downloads::*;

/// Replies: a feed, a search description, a book, the words to search
/// for, a choice made.
const LOADED: Event<HttpReply> = Event::new("opds.loaded");

const DESCRIBED: Event<HttpReply> = Event::new("opds.described");

const TERMS: Event<Typed> = Event::new("opds.terms");

const CHOSEN: Event<choice::Picked> = Event::new("opds.read");

fn feed_path() -> String {
    format!(
        "{}/browse.xml",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Search,
    Entry(usize),
    More,
}

pub(crate) struct BrowseScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The line at the bottom.
    status: Rect,
    catalog: Catalog,
    /// The feeds shown, the current one last.
    trail: Vec<String>,
    feed: Feed,
    /// The address being loaded, and whether it replaces the current
    /// feed (going back) rather than following it.
    loading: Option<(String, bool)>,
    /// Where searches go, once known.
    template: Option<String>,
    message: Option<String>,
    /// Covers of the feed's entries, where loaded (on devices with the
    /// memory for them).
    covers: Vec<Option<ImageRef>>,
    /// Whether covers show, and the entry whose cover is loading.
    with_covers: bool,
    fetching: Option<usize>,
    /// Entries whose cover failed, not tried again.
    failed: Vec<usize>,
}

impl BrowseScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
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
        let catalog = params
            .get(ID)
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| catalogs::load(env).get(i).cloned())
            .unwrap_or_default();
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            status,
            trail: Vec::new(),
            feed: Feed::default(),
            loading: None,
            template: None,
            message: None,
            covers: Vec::new(),
            with_covers: env.device.memory == MemoryClass::Large
                && env.settings.read(&crate::COVERS),
            fetching: None,
            failed: Vec::new(),
            catalog,
            style,
            header,
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.feed.search.is_some() {
            rows.push(Row::Search);
        }
        rows.extend((0..self.feed.entries.len()).map(Row::Entry));
        if self.feed.next.is_some() {
            rows.push(Row::More);
        }
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn say(&mut self, message: String, cx: &mut Cx<'_, '_>) {
        self.message = Some(message);
        self.redraw(cx);
    }

    /// Loads feed `url`; `back` when returning to it.
    fn load(&mut self, url: &str, back: bool, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !env.caps.network {
            let message = env.i18n.tr("opds.no-network").to_string();
            return self.say(message, cx);
        }
        self.loading = Some((url.to_string(), back));
        let message = env
            .i18n
            .format("opds.loading", &[("host", Arg::Str(host(url)))]);
        self.say(message, cx);
        let params = Params::new().with(URL, url.to_string());
        cx.command(Command::Http(crate::list_request(
            &self.catalog,
            url,
            feed_path(),
            &LOADED,
            params,
        )));
    }

    /// Tells why an answer did not come, if it did not; `true` if it
    /// came.
    fn answered(&mut self, reply: &HttpReply, url: &str, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        let status = reply.status;
        let host = Arg::Str(host(url));
        let message = match status {
            200..=299 => return true,
            0 => env.i18n.format("opds.failed-host", &[("host", host)]),
            401 | 403 => paperpaw_sdk::ui::tr_input(&env, "opds.unauthorized").to_string(),
            status => env.i18n.format(
                "opds.error",
                &[("host", host), ("status", Arg::Int(i64::from(status)))],
            ),
        };
        self.say(message, cx);
        false
    }

    fn loaded(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let requested = reply.context.get(URL).unwrap_or_default().to_string();
        let Some((url, back)) = self.loading.clone().filter(|(url, _)| *url == requested) else {
            return;
        };
        self.loading = None;
        if !self.answered(reply, &url, cx) {
            let _ = env.storage.remove(&feed_path());
            return;
        }
        let base = reply.final_url.as_deref().unwrap_or(&url).to_string();
        let feed = crate::read_list(&self.catalog, env.storage, &feed_path(), &base);
        let _ = env.storage.remove(&feed_path());
        if feed.entries.is_empty() && feed.title.is_empty() {
            let message = env.i18n.tr("opds.not-a-catalog").to_string();
            return self.say(message, cx);
        }
        if back {
            self.trail.pop();
        } else {
            self.trail.push(url);
        }
        if feed.search != self.feed.search {
            self.template = None;
        }
        self.feed = feed;
        self.covers = alloc::vec![None; self.feed.entries.len()];
        self.fetching = None;
        self.failed.clear();
        self.list.focus = 0;
        self.message = None;
        self.redraw(cx);
        self.next_cover(cx);
    }

    /// Asks for the words to search for, once the address is known.
    fn search(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let template = match (&self.template, &self.feed.search) {
            (Some(template), _) => template.clone(),
            (None, Some(Search::Template(template))) => template.clone(),
            (None, Some(Search::Description(url))) => {
                let url = url.clone();
                cx.command(Command::Http(request(
                    &self.catalog,
                    &url,
                    feed_path(),
                    &DESCRIBED,
                    Params::new().with(URL, url.clone()),
                )));
                let message = env
                    .i18n
                    .format("opds.loading", &[("host", Arg::Str(host(&url)))]);
                return self.say(message, cx);
            }
            (None, None) => return,
        };
        self.template = Some(template);
        cx.command(ask::text(env.i18n.tr("opds.search"), &TERMS).command());
    }

    fn described(&mut self, reply: &HttpReply, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let url = reply.context.get(URL).unwrap_or_default().to_string();
        if !self.answered(reply, &url, cx) {
            let _ = env.storage.remove(&feed_path());
            return;
        }
        let base = reply.final_url.as_deref().unwrap_or(&url).to_string();
        let template = feed::search_template(env.storage, &feed_path(), &base);
        let _ = env.storage.remove(&feed_path());
        match template {
            Some(template) => {
                self.template = Some(template);
                self.message = None;
                self.search(cx);
            }
            None => {
                let message = env.i18n.tr("opds.no-search").to_string();
                self.say(message, cx);
            }
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        match self.rows().get(index).copied() {
            Some(Row::Search) => self.search(cx),
            Some(Row::Entry(i)) => self.open(i, cx),
            Some(Row::More) => {
                if let Some(next) = self.feed.next.clone() {
                    self.load(&next, false, cx);
                }
            }
            None => {}
        }
    }

    fn back(&mut self, cx: &mut Cx<'_, '_>) {
        if self.trail.len() > 1 {
            let previous = self.trail[self.trail.len() - 2].clone();
            self.load(&previous, true, cx);
        } else {
            cx.command(Command::Back);
        }
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.rows().len();
        match self.list.move_focus(delta, len) {
            ListChange::None => {}
            ListChange::Page => {
                self.redraw(cx);
                self.next_cover(cx);
            }
            ListChange::Focus => {
                cx.request_redraw_area(self.list.area);
                cx.request_refresh(RefreshMode::Fast);
            }
        }
    }
}

impl Screen for BrowseScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = self.rows();
        let title = if self.feed.title.is_empty() {
            &self.catalog.name
        } else {
            &self.feed.title
        };
        self.header.draw(
            canvas,
            env,
            &style,
            title,
            None,
            self.list.paging(rows.len()),
        );
        let entries = &self.feed.entries;
        let covers = &self.covers;
        // A column for covers, as wide as a book's picture of the row's height.
        let column = if self.with_covers && entries.iter().any(|e| e.thumbnail.is_some()) {
            (style.noted_row() * 2 / 3) as u16 + style.gap as u16
        } else {
            0
        };
        let readable = |ext: &str| env.formats.supports(&format!("book.{ext}"));
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let (title, value, note) = match rows[index] {
                    Row::Search => (
                        env.i18n.tr("opds.search").to_string(),
                        String::from("›"),
                        String::new(),
                    ),
                    Row::More => (
                        env.i18n.tr("opds.more").to_string(),
                        String::from("›"),
                        String::new(),
                    ),
                    Row::Entry(i) => {
                        let entry = &entries[i];
                        let book = entry.readable_book(readable);
                        let value = match book {
                            Some((_, ext)) => ext.to_uppercase(),
                            None if entry.feed.is_some() => String::from("›"),
                            None => String::new(),
                        };
                        // Which edition, where a catalog offers several.
                        let mut note = entry.note().to_string();
                        if let Some((book, _)) = book.filter(|(b, _)| !b.title.is_empty()) {
                            if !note.is_empty() {
                                note.push_str(" · ");
                            }
                            note.push_str(&book.title);
                        }
                        (entry.title.clone(), value, note)
                    }
                };
                let rect = match rows[index] {
                    Row::Entry(i) if column > 0 => {
                        if let Some(cover) = &covers[i] {
                            let pad = style.gap as u16 / 2;
                            let (w, h) = ui::fit_image(
                                cover,
                                column - style.gap as u16,
                                rect.height.saturating_sub(2 * pad),
                            );
                            let area = Rect::new(
                                rect.x + style.margin as u16,
                                rect.y + (rect.height - h) / 2,
                                w,
                                h,
                            );
                            ui::draw_image(canvas, env.storage, cover, area);
                        }
                        Rect::new(
                            rect.x + column,
                            rect.y,
                            rect.width.saturating_sub(column),
                            rect.height,
                        )
                    }
                    _ => rect,
                };
                ui::draw_noted_row(canvas, env, &style, rect, focused, &title, &value, &note);
            },
        );
        canvas.fill(self.status, Color::WHITE);
        let text = match &self.message {
            Some(message) => message.clone(),
            None if rows.is_empty() && self.loading.is_none() => {
                env.i18n.tr("opds.empty").to_string()
            }
            None => String::new(),
        };
        ui::draw_note_line(canvas, env, &style, self.status, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.trail.is_empty() && self.loading.is_none() {
            let url = self.catalog.url.clone();
            self.load(&url, false, cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if let Some(delta) = ui::list_step(a, self.list.rows() as isize) {
            self.move_focus(delta, cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus, cx);
        } else if *a == actions::BACK {
            self.back(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.rows().len();
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
        if let Some(reply) = LOADED.read(event, params) {
            self.loaded(&reply, cx);
        } else if let Some(reply) = COVER.read(event, params) {
            self.cover_loaded(&reply, cx);
        } else if let Some(reply) = DESCRIBED.read(event, params) {
            self.described(&reply, cx);
        } else if let Some(reply) = DOWNLOADED.read(event, params) {
            self.downloaded(&reply, cx);
        } else if let Some(typed) = TERMS.read(event, params) {
            let terms = typed.text.trim();
            if let (false, Some(template)) = (terms.is_empty(), &self.template) {
                let url = feed::search_url(template, terms);
                self.load(&url, false, cx);
            }
        } else if let Some(picked) = CHOSEN.read(event, params)
            && picked.option == "read"
        {
            let path = picked.path;
            cx.command(Command::OpenSlot(
                slots::READER,
                Params::new().with(params::PATH, path.to_string()),
            ));
        }
    }
}
