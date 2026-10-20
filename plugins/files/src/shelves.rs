//! The library's authors, series and tags.
//!
//! A catalog of every book under the library folder (`catalog.txt`, one
//! book a line: path, author, title, series, number, tags) is made anew in
//! the background when one of these views (or the library) opens: the
//! first time after PaperPaw starts, after books came, went or moved or
//! the library folder changed, and once it is half an hour old. It is made
//! from the details the library keeps of each book, opening only books
//! not seen before, a few entries at a time. The view shows the catalog
//! there was until the new one is complete.
//!
//! A view lists the authors (or series, or tags) with how many books each
//! has; choosing one lists those books with their covers and states
//! ([`crate::list::shelf`]), a series by its numbers. Only the names are
//! held in memory, never the whole catalog.

use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
    Storage,
};

use crate::ROOT;
use crate::list::shelf::{self as books, Kind};
use paperpaw_sdk::library::catalog::{self, Record, Walk};

/// The view of authors (`mode` = `authors`), series (`series`) or tags
/// (`tags`).
pub(crate) const MODE_AUTHORS: &str = "authors";
pub(crate) const MODE_SERIES: &str = "series";
pub(crate) const MODE_TAGS: &str = "tags";

/// What books are listed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shelf {
    Authors,
    Series,
    Tags,
}

impl Shelf {
    fn of(mode: Option<&str>) -> Shelf {
        match mode {
            Some(MODE_SERIES) => Shelf::Series,
            Some(MODE_TAGS) => Shelf::Tags,
            _ => Shelf::Authors,
        }
    }

    /// The list of books one of its names opens.
    fn books(self) -> Kind {
        match self {
            Shelf::Authors => Kind::Author,
            Shelf::Series => Kind::Series,
            Shelf::Tags => Kind::Tag,
        }
    }
}
/// Names held at most.
const MAX_NAMES: usize = 2000;
/// Pause between steps of making the catalog, so the screen stays
/// responsive.
const STEP_DELAY_MS: u64 = 30;
/// How long a catalog made is taken as it is, in seconds, unless books
/// came, went or moved meanwhile: changes made outside PaperPaw (a phone's
/// own file manager, a computer) are found after that.
const CATALOG_AGE: u64 = 30 * 60;

/// When the catalog was made last, in seconds since PaperPaw started,
/// plus one; 0 where it is to be made (anew) when a view shows. Shared
/// by the plugin's handlers and its screens (on the heap: a plugin file
/// has no writable data of its own, ADR 0024).
#[derive(Debug, Clone, Default)]
pub(crate) struct CatalogMade(Rc<Cell<u32>>);

impl CatalogMade {
    /// The catalog is to be made anew: books came, went or moved, or the
    /// library folder changed.
    pub(crate) fn outdated(&self) {
        self.0.set(0);
    }

    /// The catalog was just made (by any view of the library).
    pub(crate) fn made(&self, env: &Env<'_>) {
        self.0.set(seconds(env).saturating_add(1));
    }

    /// Whether the catalog is to be made anew before a view shows it.
    pub(crate) fn is_outdated(&self, env: &Env<'_>) -> bool {
        match self.0.get() {
            0 => true,
            made => u64::from(seconds(env).saturating_sub(made - 1)) >= CATALOG_AGE,
        }
    }
}

fn seconds(env: &Env<'_>) -> u32 {
    u32::try_from(env.now.millis() / 1000).unwrap_or(u32::MAX - 1)
}

/// What a book is listed under in a view: its author, its series, or
/// each of its tags.
fn keys(record: &Record, shelf: Shelf) -> Vec<&str> {
    match shelf {
        Shelf::Authors => alloc::vec![record.author.as_str()],
        Shelf::Series => alloc::vec![record.series.as_str()],
        Shelf::Tags => record.tags().collect(),
    }
}

/// The names of the authors, series or tags and how many books each
/// has, alphabetically.
fn names(storage: &dyn Storage, shelf: Shelf) -> Vec<(String, usize)> {
    let mut names: Vec<(String, usize)> = Vec::new();
    catalog::each(storage, &mut |line| {
        for key in keys(&line, shelf) {
            // Books without series are no series; those without author
            // are listed under "Unknown author".
            if shelf == Shelf::Series && key.is_empty() {
                continue;
            }
            if let Some((_, count)) = names.iter_mut().find(|(n, _)| n == key) {
                *count += 1;
            } else if names.len() < MAX_NAMES {
                names.push((key.to_string(), 1));
            }
        }
    });
    names.sort_by_key(|(n, _)| n.to_lowercase());
    names
}

pub(crate) struct ShelfScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    shelf: Shelf,
    names: Vec<(String, usize)>,
    /// The catalog being made anew, until it is complete.
    walk: Option<Walk>,
    walked: bool,
    catalog: CatalogMade,
}

impl ShelfScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params, catalog: CatalogMade) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let shelf = Shelf::of(params.get(params::MODE));
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            names: names(env.storage, shelf),
            walk: None,
            walked: false,
            catalog,
            shelf,
            style,
            header,
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens the books of name `index`.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some((name, _)) = self.names.get(index) {
            cx.command(books::open(self.shelf.books(), name));
        }
    }

    fn title(&self) -> &'static str {
        match self.shelf {
            Shelf::Authors => "files.by-author",
            Shelf::Series => "files.by-series",
            Shelf::Tags => "files.by-tag",
        }
    }
}

impl Screen for ShelfScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.names.len();
        let title = env.i18n.tr(self.title());
        self.header
            .draw(canvas, env, &style, title, None, self.list.paging(len));
        if len == 0 {
            let area = self.list.area();
            // An empty view says what would be in it.
            let (icon, text) = match &self.walk {
                Some(walk) => {
                    let message = env
                        .i18n
                        .plural("files.collecting", walk.found() as i64, &[]);
                    ui::draw_message(canvas, env, &style, area, &message);
                    return;
                }
                None if self.shelf == Shelf::Series => ("books", "files.no-series"),
                None if self.shelf == Shelf::Tags => ("tag", "files.no-tags"),
                None => ("books", "files.no-books"),
            };
            let text = ui::tr_input(env, text);
            ui::draw_empty(canvas, env, &style, area, icon, "", text);
            return;
        }
        let names = &self.names;
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                let (name, count) = &names[index];
                let name = if name.is_empty() {
                    env.i18n.tr("files.unknown-author")
                } else {
                    name.as_str()
                };
                let note = env.i18n.plural("files.books", *count as i64, &[]);
                ui::draw_noted_row(canvas, env, &style, rect, focused, name, "›", &note);
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.walk.is_none() && !self.walked && self.catalog.is_outdated(&env) {
            self.walk = Some(Walk::start(env.storage, env.settings.read(&ROOT)));
        }
        // Also where it was on its way when another screen came on top:
        // the timer asked for went with it.
        if self.walk.is_some() {
            cx.wake_at(env.now);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(walk) = &mut self.walk else {
            return;
        };
        if !walk.step(&env) {
            if self.names.is_empty() {
                // "Collecting …" counts up.
                self.redraw(cx);
            }
            cx.wake_at(env.now.add_millis(STEP_DELAY_MS));
            return;
        }
        // The new catalog: the names again, keeping the place.
        self.walk = None;
        self.walked = true;
        self.catalog.made(&env);
        self.names = names(env.storage, self.shelf);
        self.list.set_len(self.names.len());
        self.redraw(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus(delta, self.names.len());
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.names.len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.list.set_focus(index);
                self.activate(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::library;

    #[test]
    fn catalog_lines_and_views() {
        let storage = MemoryStorage::new();
        let books = [
            (
                "b/2.epub",
                "Ann",
                "Second",
                "Tales",
                "2",
                &["Fantasy", "Kids"][..],
            ),
            ("b/1.epub", "Ann", "First", "Tales", "1", &["Fantasy"]),
            ("b/x.epub", "bob", "Alone", "", "", &[]),
            ("b/y.txt", "", "Nobody's", "", "", &[]),
        ];
        let records: Vec<Record> = books
            .iter()
            .map(|(path, author, title, series, number, tags)| {
                let some = |s: &str| (!s.is_empty()).then(|| String::from(s));
                let book = library::Book {
                    author: some(author),
                    title: some(title),
                    series: some(series),
                    series_index: some(number),
                    tags: tags.iter().map(|t| String::from(*t)).collect(),
                    ..library::Book::default()
                };
                Record::of(path, &book)
            })
            .collect();
        catalog::write(&storage, &records);
        let authors = names(&storage, Shelf::Authors);
        assert_eq!(
            authors,
            [
                (String::new(), 1),
                (String::from("Ann"), 2),
                (String::from("bob"), 1)
            ]
        );
        assert_eq!(names(&storage, Shelf::Series), [(String::from("Tales"), 2)]);
        assert_eq!(
            names(&storage, Shelf::Tags),
            [(String::from("Fantasy"), 2), (String::from("Kids"), 1)]
        );
        // Searching: every word, anywhere, in any case.
        let found: Vec<String> = catalog::search(&storage, "ann TALES")
            .into_iter()
            .map(|l| l.title)
            .collect();
        assert_eq!(found, ["First", "Second"]);
        assert_eq!(catalog::search(&storage, "kids")[0].title, "Second");
        assert!(catalog::search(&storage, "nothing").is_empty());
    }
}
