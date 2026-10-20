//! The library as a calm list with covers and how far each book is
//! read. The library is one list of all books (those in folders
//! included), opening with the book read last as a larger card, so going
//! on reading is one confirm or one tap. Pills under the header filter
//! the books by how far they are read (All, Reading, Unread, Finished);
//! the last pill shows the shelves instead: favourites, authors, the
//! series and the folders (collections). A series lists its books with
//! their covers, numbers and states, the next one to read marked and
//! focused. "Find" in the header picks a book by the first letter of its
//! title or author, without a keyboard; on touch screens it also offers
//! search.
//!
//! On small screens (the X4) the filters sit behind one row above the
//! books, which shows what is shown and opens a choice of it (and of
//! "Find"); larger screens show them as pills.
//!
//! Buttons work as in every list: the focus moves a book at a time, the
//! page buttons turn pages, confirm opens, holding confirm (the menu
//! button) offers what can be done with the focused book. Above the first
//! book (and after the last) the focus reaches the filter's row.
//! Touch: tap the filter's row or a pill, a book, the search button (for
//! "Find"); hold a book for its actions; tap the title for the library's
//! menu.
//!
//! The books come from the library service ([`library::service`]), so a
//! plugin's library shows here as the kernel's does: what is known of
//! each book to filter and sort is its index ([`index`]), kept in a file
//! while a book covers the library; a page's books are read, with the
//! badges plugins put on them, and their words made ready, when it is
//! shown, never while drawing. The kernel makes the catalog anew; the
//! list follows it ([`events::CATALOG_CHANGED`]) and the changes of single
//! books ([`events::BOOK_CHANGED`]).

mod find;
mod grid;
mod index;
mod rows;
mod sheet;
pub(crate) mod shelf;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::library::{self, catalog::State};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    RegisterError, Registrar, Screen, ScreenSpec,
};

use index::{Filter, Index, NEVER, cap, hash};
use rows::{Loaded, Sizes};

/// Finds a book by a letter ([`find`]).
pub(crate) const FIND: Id = Id::new_static("files.list.find");
/// A list of books: a series, a folder, the favourites, a letter, a
/// search ([`shelf`]).
const SHELF: Id = Id::new_static("files.list.shelf");
/// What can be done with a book ([`sheet`]).
const SHEET: Id = Id::new_static("files.list.sheet");
/// What the library shows, chosen from the filter's row.
const SHOW: Id = Id::new_static("files.list.show");
const SHOWN: Event<paperpaw_sdk::ui::choice::Picked> = Event::new("files.list.shown");
/// The choices of [`SHOW`]: the filters' names, then "Find".
const SHOW_OPTIONS: [&str; 7] = [
    "all", "reading", "unread", "finished", "next", "shelves", "find",
];
/// The filters of [`SHOW_OPTIONS`], in their order.
const SHOW_FILTERS: [Filter; 6] = [
    Filter::All,
    Filter::Reading,
    Filter::Unread,
    Filter::Finished,
    Filter::Next,
    Filter::Shelves,
];
/// Where the index is kept while a book covers the library.
const KEPT: &str = ".paperpaw/plugins/files/index.bin";

/// Registers the list's screens and actions.
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screen(ScreenSpec::new(
        SHELF,
        "files.title",
        Box::new(|env, params| Box::new(shelf::ShelfScreen::new(env, params))),
    ))?;
    r.add_screen(ScreenSpec::new(
        SHEET,
        "files.title",
        Box::new(|env, params| Box::new(sheet::BookSheet::new(env, params))),
    ))?;
    r.add_screen(ScreenSpec::new(
        SHOW,
        "files.list.show",
        Box::new(|env, params| Box::new(paperpaw_sdk::ui::choice::Choice::new(env, params))),
    ))?;
    r.add_screen(ScreenSpec::new(
        FIND,
        "files.list.find",
        Box::new(|env, params| Box::new(find::FindScreen::new(env, params))),
    ))?;
    r.add_action(
        paperpaw_sdk::ActionSpec::new(FIND, "files.list.find").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(FIND, Params::new()));
            },
        ),
    )?;
    r.add_icon(FIND, Cow::Borrowed("letter-case"))?;
    Ok(())
}

/// The library screen.
pub(crate) fn screen(env: &Env<'_>, _params: &Params) -> Box<dyn Screen> {
    Box::new(ListScreen::new(env))
}

/// The index of every book, from the library service.
fn make_index(env: &Env<'_>) -> Index {
    library::service(env).index(env, cap(env.device.memory), &archive(env))
}

/// Whether the catalog is still to be made (none yet).
fn collecting(env: &Env<'_>) -> bool {
    matches!(
        library::service(env).catalog_state(env),
        State::Missing | State::Building
    )
}

/// The archive folder, as the setting names it: its books are on the
/// shelves, not in the list.
fn archive(env: &Env<'_>) -> String {
    env.settings
        .read(&crate::ARCHIVE)
        .trim_matches('/')
        .to_string()
}

/// Opens the library's menu.
fn open_menu(cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenSlot(
        slots::MENU,
        Params::new().with(params::MENU, crate::MENU.as_str()),
    ));
}

/// Offers what can be done with the book at `path`.
fn offer(path: &str, cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenScreen(
        SHEET,
        Params::new().with(params::PATH, path.to_string()),
    ));
}

/// Opens the book at `path` in the reader.
fn read(path: &str, cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenSlot(
        slots::READER,
        Params::new().with(params::PATH, path.to_string()),
    ));
}

/// A shelf of the shelves, with its words made ready.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShelfRow {
    /// "Next in your series" (a filter), else a shelf of `kind`.
    next: bool,
    kind: shelf::Kind,
    /// The series' or folder's name (the folder's path for folders).
    name: String,
    count: usize,
    finished: usize,
    icon: &'static str,
    title: String,
    note: String,
}

impl ShelfRow {
    fn new(next: bool, kind: shelf::Kind, name: &str, count: usize, finished: usize) -> Self {
        ShelfRow {
            next,
            kind,
            name: name.to_string(),
            count,
            finished,
            icon: "",
            title: String::new(),
            note: String::new(),
        }
    }

    /// Writes its icon, title and note.
    fn word(&mut self, env: &Env<'_>) {
        let books = env.i18n.plural("files.books", self.count as i64, &[]);
        let (icon, title, note) = match self.kind {
            _ if self.next => (
                "arrow-bar-to-right",
                env.i18n.tr("files.list.show.next").to_string(),
                books,
            ),
            shelf::Kind::Favourites => ("star", env.i18n.tr("files.favourites").to_string(), books),
            shelf::Kind::Authors => (
                "user",
                env.i18n.tr("files.list.authors").to_string(),
                env.i18n
                    .plural("files.list.authors.count", self.count as i64, &[]),
            ),
            shelf::Kind::Series => {
                let series = env.i18n.tr("files.list.series");
                let note = if self.finished > 0 {
                    let finished =
                        env.i18n
                            .plural("files.list.finished.count", self.finished as i64, &[]);
                    format!("{series} · {books} · {finished}")
                } else {
                    format!("{series} · {books}")
                };
                ("stack-2", self.name.clone(), note)
            }
            _ => (
                "folder",
                paperpaw_sdk::files::name(&self.name).to_string(),
                format!("{} · {books}", env.i18n.tr("files.folder")),
            ),
        };
        self.icon = icon;
        self.title = title;
        self.note = note;
    }
}

/// Where things go on the screen, and the words of the pills: made anew
/// when a setting changes (the view, the size of the interface).
struct Layout {
    style: UiStyle,
    sizes: Sizes,
    header: ui::Header,
    /// The pills and their names (larger screens), and whether the last
    /// shows as an icon.
    pills: Vec<Rect>,
    pill_labels: Vec<String>,
    pill_icon: bool,
    /// The filter's row (small screens).
    filter_row: Option<Rect>,
    /// Where the books (or shelves) go.
    area: Rect,
    /// The covers' grid, where books show as covers ([`crate::VIEW`]).
    grid: Option<grid::Grid>,
}

impl Layout {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let gap = style.gap as u16;
        let pill_labels: Vec<String> = Filter::ALL
            .iter()
            .map(|f| env.i18n.tr(f.label()).to_string())
            .collect();
        let (pills, pill_icon, filter_row, top) = if sizes.compact {
            let row = Rect::new(0, header.area.height, display.width, sizes.filter_row);
            (Vec::new(), false, Some(row), row.y + row.height + gap / 2)
        } else {
            // Room below the pills for the focus's bar.
            let pills_area = Rect::new(0, header.area.height, display.width, sizes.pill + gap);
            let (pills, icon) = rows::pill_places(env, &style, &sizes, pills_area, &pill_labels);
            (pills, icon, None, pills_area.y + pills_area.height)
        };
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let grid = (env.settings.read(&crate::VIEW) == crate::VIEW_COVERS)
            .then(|| grid::Grid::new(env, &style, &sizes, area.height));
        Layout {
            style,
            sizes,
            header,
            pills,
            pill_labels,
            pill_icon,
            filter_row,
            area,
            grid,
        }
    }
}

/// The library; see the [module documentation](self).
struct ListScreen {
    layout: Layout,
    root: String,
    /// The archive folder, whose books are on the shelves only.
    archive: String,
    index: Index,
    /// No catalog yet: the kernel is making the first.
    collecting: bool,
    filter: Filter,
    by_recent: bool,
    /// The books shown, as indices of the index's items.
    order: Vec<u16>,
    /// How many books each of `order` stands for: more than one for a
    /// series as a stack (covers only); empty where each is one.
    counts: Vec<u16>,
    /// The shelves, while they are shown.
    shelves: Vec<ShelfRow>,
    /// The first book shown is the one read last, as a large card.
    card: bool,
    focus: usize,
    /// The focus shows: with buttons, and on touch screens once a button
    /// moved it.
    shown: bool,
    /// The focus is on the filter's row or the pills.
    filter_focused: bool,
    /// The books of the page shown, read when it was shown.
    loaded: Vec<Loaded>,
    /// The page `loaded` holds.
    loaded_page: Option<usize>,
    /// The words of the filter's row, and of an empty list.
    filter_label: String,
    filter_end: String,
    empty_title: String,
    empty_text: String,
    /// The search button on touch screens.
    find: Option<Rect>,
    /// Paths (hashed) of books moved or removed since the catalog was
    /// made: not shown until it is made anew.
    gone: Vec<u32>,
    /// While covered: the book the focus was on, to find again; the
    /// index is kept in a file.
    kept_focus: Option<u32>,
    covered: bool,
    /// Just opened: the first `on_show` need not load again.
    fresh: bool,
}

impl ListScreen {
    fn new(env: &Env<'_>) -> Self {
        let mut screen = Self {
            layout: Layout::new(env),
            root: library::folder(env).to_string(),
            archive: archive(env),
            index: make_index(env),
            collecting: collecting(env),
            filter: Filter::All,
            by_recent: env.settings.read(&crate::SORT) != crate::SORT_NAME,
            order: Vec::new(),
            counts: Vec::new(),
            shelves: Vec::new(),
            card: false,
            focus: 0,
            shown: !env.device.input.touch,
            filter_focused: false,
            loaded: Vec::new(),
            loaded_page: None,
            filter_label: String::new(),
            filter_end: String::new(),
            empty_title: String::new(),
            empty_text: String::new(),
            find: None,
            gone: Vec::new(),
            kept_focus: None,
            covered: false,
            fresh: true,
        };
        screen.refilter(env);
        screen
    }

    fn len(&self) -> usize {
        if self.filter == Filter::Shelves {
            self.shelves.len()
        } else {
            self.order.len()
        }
    }

    /// The hash of the book the focus is on, if one is.
    fn focused_book(&self) -> Option<u32> {
        let i = *self.order.get(self.focus)?;
        Some(self.index.items.get(usize::from(i))?.hash)
    }

    /// Takes the shown books (or shelves) anew from the index, the focus
    /// on the book whose path hashes to `keep` where it is shown, else at
    /// the place it was (or the first, `keep_place` false).
    fn refilter_keeping(&mut self, env: &Env<'_>, keep: Option<u32>, keep_place: bool) {
        let place = self.focus;
        self.focus = 0;
        self.loaded = Vec::new();
        self.loaded_page = None;
        self.counts = Vec::new();
        if self.filter == Filter::Shelves {
            self.order = Vec::new();
            self.shelves = self.make_shelves(env);
            self.card = false;
        } else {
            self.shelves = Vec::new();
            self.order = self.filter.order(&self.index, self.by_recent);
            if !self.gone.is_empty() {
                let (items, gone) = (&self.index.items, &self.gone);
                self.order
                    .retain(|&i| !gone.contains(&items[usize::from(i)].hash));
            }
            // As covers, the books of a series are one stack.
            if self.layout.grid.is_some() && self.filter == Filter::All {
                (self.order, self.counts) = self.index.stacked(&self.order);
            }
            // The card: the book read last, while it is read (as covers,
            // it is the first cover).
            self.card = self.by_recent
                && self.layout.grid.is_none()
                && matches!(self.filter, Filter::All | Filter::Reading)
                && self.order.first().is_some_and(|&i| {
                    let item = &self.index.items[usize::from(i)];
                    item.recent != NEVER && item.progress != 100
                });
        }
        let found = keep.and_then(|hash| {
            let items = &self.index.items;
            self.order
                .iter()
                .position(|&i| items[usize::from(i)].hash == hash)
        });
        self.focus = match found {
            Some(at) => at,
            None if keep_place => place.min(self.len().saturating_sub(1)),
            None => 0,
        };
        self.load_page(env);
        self.word(env);
    }

    /// Takes the shown books anew, the focus on the first.
    fn refilter(&mut self, env: &Env<'_>) {
        self.refilter_keeping(env, None, false);
    }

    /// Takes the shown books anew, the focus where it was: on its book,
    /// else at its place.
    fn refilter_in_place(&mut self, env: &Env<'_>) {
        let keep = self.focused_book();
        self.refilter_keeping(env, keep, true);
    }

    /// Writes the words of the filter's row and of an empty list, and
    /// where the search button is, so drawing writes none.
    fn word(&mut self, env: &Env<'_>) {
        let shown = SHOW_FILTERS
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let mut key = String::from("files.list.show.");
        key.push_str(SHOW_OPTIONS[shown]);
        self.filter_label = env.i18n.tr(&key).to_string();
        let total = self.index.total;
        self.filter_end = if self.filter == Filter::Shelves {
            String::new()
        } else if self.filter == Filter::All && total > self.index.items.len() {
            // Some left out: said, not hidden.
            env.i18n.format(
                "files.list.left-out",
                &[
                    ("shown", Arg::Int(self.index.items.len() as i64)),
                    ("count", Arg::Int(total as i64)),
                ],
            )
        } else {
            env.i18n.plural("files.books", self.len() as i64, &[])
        };
        (self.empty_title, self.empty_text) = if self.collecting && self.index.items.is_empty() {
            (
                String::new(),
                env.i18n.tr("files.list.collecting").to_string(),
            )
        } else if self.index.items.is_empty() && self.index.archived > 0 {
            (
                String::new(),
                env.i18n.tr("files.list.none.all").to_string(),
            )
        } else if self.index.items.is_empty() {
            (
                env.i18n.tr("files.empty.title").to_string(),
                env.i18n
                    .format("files.empty", &[("dir", Arg::Str(&self.root))]),
            )
        } else {
            let key = match self.filter {
                Filter::Reading => "files.list.none.reading",
                Filter::Unread => "files.list.none.unread",
                Filter::Next => "files.list.none.next",
                _ => "files.list.none.finished",
            };
            (String::new(), env.i18n.tr(key).to_string())
        };
        self.find = self.find_button(env);
    }

    /// The shelves: the next books of the series begun, favourites and
    /// authors, then the series and the folders, each with how many books
    /// it has (and how many are finished), from one pass over the catalog.
    fn make_shelves(&self, env: &Env<'_>) -> Vec<ShelfRow> {
        let favourites = library::favourites(env.storage).len();
        let mut authors: Vec<String> = Vec::new();
        let mut series: Vec<ShelfRow> = Vec::new();
        let mut folders: Vec<ShelfRow> = Vec::new();
        let prefix = if self.root.is_empty() {
            String::new()
        } else {
            format!("{}/", self.root)
        };
        library::catalog::each(env.storage, &mut |line| {
            let finished = line.reading() == library::catalog::Reading::Finished;
            if !authors.contains(&line.author) {
                authors.push(line.author.clone());
            }
            let count = |rows: &mut Vec<ShelfRow>, kind, name: &str| match rows
                .iter_mut()
                .find(|r| r.name == name)
            {
                Some(row) => {
                    row.count += 1;
                    row.finished += usize::from(finished);
                }
                None => rows.push(ShelfRow::new(false, kind, name, 1, usize::from(finished))),
            };
            if !line.series.is_empty() {
                count(&mut series, shelf::Kind::Series, &line.series);
            }
            let inner = line.path.strip_prefix(&prefix).unwrap_or(&line.path);
            if let Some((folder, _)) = inner.split_once('/') {
                let folder = format!("{prefix}{folder}");
                count(&mut folders, shelf::Kind::Folder, &folder);
            }
        });
        series.sort_by_key(|r| r.name.to_lowercase());
        folders.sort_by_key(|r| r.name.to_lowercase());
        let next = Filter::Next.order(&self.index, false).len();
        let mut shelves = Vec::new();
        if next > 0 {
            shelves.push(ShelfRow::new(true, shelf::Kind::Series, "", next, 0));
        }
        shelves.push(ShelfRow::new(
            false,
            shelf::Kind::Favourites,
            "",
            favourites,
            0,
        ));
        shelves.push(ShelfRow::new(
            false,
            shelf::Kind::Authors,
            "",
            authors.len(),
            0,
        ));
        shelves.extend(series);
        shelves.extend(folders);
        for row in &mut shelves {
            row.word(env);
        }
        shelves
    }

    fn row_height(&self) -> u16 {
        if self.filter == Filter::Shelves {
            self.layout.style.noted_row() as u16
        } else {
            self.layout.sizes.row
        }
    }

    /// How high the books' rows of `page` are drawn: on small screens as
    /// many as fit, sharing what is left over, so a page ends at the
    /// foot of the screen.
    fn row_on(&self, page: usize) -> u16 {
        let row = self.row_height();
        if !self.layout.sizes.compact || self.filter == Filter::Shelves {
            return row;
        }
        let card = if self.card && page == 0 {
            self.layout.sizes.card
        } else {
            0
        };
        let room = self.layout.area.height.saturating_sub(card);
        let rows = (room / row.max(1)).max(1);
        room / rows
    }

    /// The covers' grid, while books show (not the shelves).
    fn covers(&self) -> Option<&grid::Grid> {
        self.layout
            .grid
            .as_ref()
            .filter(|_| self.filter != Filter::Shelves)
    }

    /// Entries on a page: rows, or covers.
    fn per_page(&self) -> usize {
        match self.covers() {
            Some(g) => g.per_page(),
            None => usize::from(self.layout.area.height / self.row_height().max(1)).max(1),
        }
    }

    /// Entries on the first page: the card and the rows below it.
    fn first_page(&self) -> usize {
        if !self.card {
            return self.per_page();
        }
        let below = self
            .layout
            .area
            .height
            .saturating_sub(self.layout.sizes.card);
        1 + usize::from(below / self.layout.sizes.row.max(1))
    }

    fn page_of(&self, index: usize) -> usize {
        let first = self.first_page();
        if index < first {
            0
        } else {
            1 + (index - first) / self.per_page()
        }
    }

    fn page_start(&self, page: usize) -> usize {
        match page {
            0 => 0,
            p => self.first_page() + (p - 1) * self.per_page(),
        }
    }

    fn pages(&self) -> usize {
        self.page_of(self.len().saturating_sub(1)) + 1
    }

    /// The entries of `page`.
    fn page_range(&self, page: usize) -> core::ops::Range<usize> {
        let start = self.page_start(page);
        start..self.page_start(page + 1).min(self.len())
    }

    /// Where entry `index` is drawn (on its page).
    fn place(&self, index: usize) -> Rect {
        let page = self.page_of(index);
        let slot = index - self.page_start(page);
        let a = self.layout.area;
        if let Some(g) = self.covers() {
            return g.place(a, a.y, slot);
        }
        let row = self.row_on(page);
        if self.card && page == 0 {
            if slot == 0 {
                return Rect::new(a.x, a.y, a.width, self.layout.sizes.card);
            }
            let y = a.y + self.layout.sizes.card + (slot as u16 - 1) * row;
            return Rect::new(a.x, y, a.width, row);
        }
        Rect::new(a.x, a.y + slot as u16 * row, a.width, row)
    }

    /// Reads the books of the page the focus is on, if they are not read,
    /// and makes their words ready.
    fn load_page(&mut self, env: &Env<'_>) {
        if self.filter == Filter::Shelves {
            self.loaded = Vec::new();
            return;
        }
        let page = self.page_of(self.focus);
        if self.loaded_page == Some(page) {
            return;
        }
        // The page before goes first: never two pages of covers at once.
        self.loaded = Vec::new();
        let (style, sizes) = (&self.layout.style, &self.layout.sizes);
        let width = self.layout.area.width;
        let mut loaded = Vec::new();
        let service = library::service(env);
        for index in self.page_range(page) {
            let item = usize::from(self.order[index]);
            let Some(record) = service.record(env, &self.index, item) else {
                continue;
            };
            let card = self.card && index == 0;
            // Each size of cover kept apart: the card's, the grid's, the
            // rows'.
            let (cover, variant) = match self.covers() {
                _ if card => (sizes.card_cover, "card"),
                Some(g) => (g.cover, "grid"),
                None => (sizes.cover, ""),
            };
            let mut book = Loaded::load(env, record, cover, variant);
            match self.covers() {
                _ if card => rows::prepare_card(env, style, sizes, width, &mut book),
                Some(g) => {
                    let count = self.counts.get(index).copied().unwrap_or(1);
                    grid::prepare_cell(env, style, sizes, g, &mut book, count);
                }
                None => {
                    let note = book.author.clone();
                    rows::prepare_row(env, style, sizes, width, &mut book, note, false);
                }
            }
            loaded.push(book);
        }
        self.loaded = loaded;
        self.loaded_page = Some(page);
    }

    /// The book of entry `index`, if its page is read.
    fn book(&self, index: usize) -> Option<&Loaded> {
        let page = self.page_of(index);
        if self.loaded_page != Some(page) {
            return None;
        }
        self.loaded.get(index - self.page_start(page))
    }

    fn set_filter(&mut self, filter: Filter, cx: &mut Cx<'_, '_>) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        let env = *cx.env();
        self.refilter(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Moves the focus by `delta` entries. Past the first (or the last)
    /// it reaches the filter's row (or the pills), as in other lists the
    /// header's note.
    fn step(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.len();
        let before = self.focus;
        if !self.index.has_books() {
            // Nothing to move to: no books, no filter shown.
            return;
        }
        if self.filter_focused {
            self.filter_focused = false;
            self.focus = if delta > 0 { 0 } else { len.saturating_sub(1) };
        } else if !self.shown {
            // The first press shows where the focus is.
            self.shown = true;
        } else {
            let next = self.focus as isize + delta;
            if next < 0 || next >= len as isize {
                self.filter_focused = true;
            } else {
                self.focus = next as usize;
            }
        }
        self.moved(before, cx);
    }

    /// Draws what a moved focus changed: the two entries on one page, else
    /// the new page.
    fn moved(&mut self, before: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if self.page_of(before) == self.page_of(self.focus) {
            cx.request_redraw_area(self.filter_area());
            if before < self.len() {
                cx.request_redraw_area(self.place(before));
            }
            if self.focus < self.len() {
                cx.request_redraw_area(self.place(self.focus));
            }
            cx.request_refresh(RefreshMode::Fast);
        } else {
            self.load_page(&env);
            self.find = self.find_button(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    /// Turns to the next (`1`) or the previous (`-1`) page.
    fn turn(&mut self, pages: isize, cx: &mut Cx<'_, '_>) {
        let page = self.page_of(self.focus) as isize + pages;
        if page < 0 || page >= self.pages() as isize {
            return;
        }
        self.focus = self.page_start(page as usize);
        self.filter_focused = false;
        let env = *cx.env();
        self.load_page(&env);
        self.find = self.find_button(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Opens entry `index`: a book in the reader, a shelf's books.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if self.filter == Filter::Shelves {
            let Some(row) = self.shelves.get(index) else {
                return;
            };
            if row.next {
                self.set_filter(Filter::Next, cx);
            } else if row.kind == shelf::Kind::Authors {
                cx.command(Command::RunAction(crate::BY_AUTHOR));
            } else {
                cx.command(shelf::open(row.kind, &row.name));
            }
            return;
        }
        let Some(book) = self.book(index) else {
            return;
        };
        if self.counts.get(index).is_some_and(|&c| c > 1) {
            // A series as a stack: its books.
            cx.command(shelf::open(shelf::Kind::Series, &book.series));
            return;
        }
        let path = book.path.clone();
        let env = *cx.env();
        if env.storage.exists(&path) {
            read(&path, cx);
            return;
        }
        // Gone without a word (deleted on a computer, say): left out, and
        // the catalog made anew.
        self.gone.push(hash(&path));
        cx.command(library::request_catalog());
        self.refilter_in_place(&env);
        cx.command(Command::Notify(env.i18n.tr("files.list.gone").to_string()));
        cx.request_redraw();
    }

    /// Offers what can be done with book `index`; `false` for a shelf.
    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        match self.book(index) {
            Some(book) if self.filter != Filter::Shelves => {
                let path = book.path.clone();
                offer(&path, cx);
                true
            }
            _ => false,
        }
    }

    /// On touch screens, where the search button is: at the end of the
    /// filter's row, or in the header left of the page arrows (as
    /// [`ui::Header`] places them).
    fn find_button(&self, env: &Env<'_>) -> Option<Rect> {
        if !env.device.input.touch || !self.index.has_books() {
            return None;
        }
        let style = &self.layout.style;
        if let Some(row) = self.layout.filter_row {
            // At the end of the filter's row, its icon on the margin.
            let square = row.height;
            let size = style.title * ui::ICON_SCALE;
            let icon = ui::icon_width(env, "search", size) as u16;
            let right = (row.x + row.width).saturating_sub(style.margin as u16);
            let x = (right + (square - icon) / 2).saturating_sub(square);
            return Some(Rect::new(x, row.y, square, square));
        }
        let a = self.layout.header.area;
        let square = a.height;
        let pages = self.pages();
        let right = if self.len() > 0 && pages > 1 {
            let font = style.font(env)?;
            let label = format!("{} / {pages}", self.page_of(self.focus) + 1);
            let label = font.measure(style.small, &label) as u16;
            (a.x + a.width).saturating_sub(2 * (square * 4 / 5) + label)
        } else {
            (a.x + a.width).saturating_sub(style.margin as u16)
        };
        Some(Rect::new(right.saturating_sub(square), a.y, square, square))
    }

    /// Where the filter is: its row, or the pills' band.
    fn filter_area(&self) -> Rect {
        match self.layout.filter_row {
            Some(row) => row,
            None => {
                let top = self.layout.header.area.y + self.layout.header.area.height;
                let a = self.layout.area;
                Rect::new(0, top, a.width, a.y.saturating_sub(top))
            }
        }
    }

    /// Offers what to show: the filters and "Find".
    fn choose_shown(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let title = env.i18n.tr("files.list.show");
        cx.command(paperpaw_sdk::ui::choice::open(
            SHOW,
            &SHOWN,
            title,
            "",
            "files.list.show.",
            &SHOW_OPTIONS,
        ));
    }

    /// The catalog was made anew: the index from it, the place kept.
    fn rebuild(&mut self, env: &Env<'_>) {
        self.collecting = collecting(env);
        let keep = self.focused_book();
        // The old index goes first: never two at once.
        self.index = Index::default();
        self.index = make_index(env);
        self.gone.clear();
        self.refilter_keeping(env, keep, true);
    }

    fn draw_shelf(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        row: &ShelfRow,
        rect: Rect,
        focused: bool,
    ) {
        let style = &self.layout.style;
        // A row, whose words are those drawn in it.
        canvas.describe(Role::Row, "", rect, focused, None);
        let ink = style.focus_ink(focused);
        let room = ui::icon_room(env, style, true);
        let band = ui::title_area(style, rect);
        ui::draw_row_icon(canvas, env, style, band, room, Some(row.icon), ink);
        let text = Rect::new(
            rect.x + room,
            rect.y,
            rect.width.saturating_sub(room),
            rect.height,
        );
        ui::draw_noted_row(
            canvas, env, style, text, focused, &row.title, "›", &row.note,
        );
    }
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.covered {
            // Below an overlay left open before it showed again: nothing
            // to draw until it shows ([`Screen::on_show`]).
            return;
        }
        let layout = &self.layout;
        let style = layout.style;
        let len = self.len();
        let paging = (len > 0).then(|| (self.page_of(self.focus), self.pages()));
        let title = env.i18n.tr("files.title");
        layout.header.draw(canvas, env, &style, title, None, paging);
        let has_books = self.index.has_books();
        let focused_filter = self.shown && self.filter_focused;
        if let Some(row) = layout.filter_row {
            if has_books {
                let (label, end) = (&self.filter_label, &self.filter_end);
                rows::draw_filter_row(
                    canvas,
                    env,
                    &style,
                    row,
                    focused_filter,
                    label,
                    end,
                    self.find,
                );
            }
        } else {
            if let Some(button) = self.find {
                let size = style.title * ui::ICON_SCALE;
                let w = ui::icon_width(env, "search", size);
                let x = f32::from(button.x) + (f32::from(button.width) - w) / 2.0;
                let ink = style.header_colors().1;
                ui::draw_icon(canvas, env, "search", size, x, button, ink);
            }
            // No pills before there are books to sort. Next in your series
            // is on the shelves: their pill is the active one then.
            if has_books {
                let shown = if self.filter == Filter::Next {
                    Filter::Shelves
                } else {
                    self.filter
                };
                let active = Filter::ALL.iter().position(|f| *f == shown);
                rows::draw_pills(
                    canvas,
                    env,
                    &style,
                    &layout.pills,
                    &layout.pill_labels,
                    active,
                    focused_filter,
                    layout.pill_icon,
                );
            }
        }
        if len == 0 {
            let area = layout.area;
            if self.collecting && !has_books {
                ui::draw_message(canvas, env, &style, area, &self.empty_text);
            } else {
                let (title, text) = (&self.empty_title, &self.empty_text);
                ui::draw_empty(canvas, env, &style, area, "books", title, text);
            }
            return;
        }
        let sizes = &layout.sizes;
        let page = self.page_of(self.focus);
        for index in self.page_range(page) {
            let rect = self.place(index);
            if canvas.clip().intersection(&rect).is_none() {
                continue;
            }
            let focused = self.shown && !self.filter_focused && index == self.focus;
            if self.card && index == 0 {
                if let Some(book) = self.book(0) {
                    rows::draw_card(canvas, env, &style, sizes, rect, focused, book);
                }
                continue;
            }
            if let Some(g) = self.covers() {
                if let Some(book) = self.book(index) {
                    let count = self.counts.get(index).copied().unwrap_or(1);
                    grid::draw_cell(canvas, env, &style, sizes, g, rect, focused, book, count);
                }
                continue;
            }
            if focused {
                ui::draw_focus(canvas, &style, rect);
            } else {
                ui::draw_separator(canvas, &style, rect, 0);
            }
            if self.filter == Filter::Shelves {
                self.draw_shelf(canvas, env, &self.shelves[index], rect, focused);
            } else if let Some(book) = self.book(index) {
                rows::draw_book_row(canvas, env, &style, sizes, rect, focused, book, false);
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if core::mem::take(&mut self.fresh) {
            if library::service(&env).catalog_state(&env) == State::Missing {
                // No catalog yet: the kernel makes it, then it shows.
                cx.command(library::request_catalog());
            }
        } else if core::mem::take(&mut self.covered) {
            // Back from a book or a screen over the whole library: the
            // index as it was kept (else made anew), then what changed
            // meanwhile: the books read last, the status of those read.
            self.archive = archive(&env);
            self.index =
                Index::load(env.storage, KEPT, &self.archive).unwrap_or_else(|| make_index(&env));
            self.index.refresh(env.storage);
            self.collecting = collecting(&env);
            let keep = self.kept_focus.take();
            self.refilter_keeping(&env, keep, true);
            cx.request_redraw();
        }
        // The index kept once it is settled.
        cx.wake_at(env.now);
    }

    fn on_hide(&mut self) {
        // Covered by a book (or another full screen): the covers and the
        // index go, memory for the book; the index is kept in a file.
        self.loaded = Vec::new();
        self.loaded_page = None;
        if self.covered {
            return;
        }
        self.covered = true;
        self.kept_focus = self.focused_book();
        self.order = Vec::new();
        self.counts = Vec::new();
        self.shelves = Vec::new();
        self.index = Index::default();
    }

    /// Keeps the index in a file, to be read again instead of made anew
    /// when the library shows after a book.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !self.covered && self.index.dirty {
            let _ = env.storage.create_dir(".paperpaw/plugins/files");
            self.index.dirty = !self.index.save(env.storage, KEPT, &self.archive);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let touch = cx.env().device.input.touch;
        let side = *a == actions::LEFT || *a == actions::RIGHT;
        if side && self.filter_focused && self.layout.filter_row.is_none() {
            // On the pills, left and right go from pill to pill.
            let steps = if *a == actions::LEFT { -1 } else { 1 };
            self.set_filter(self.filter.step(steps), cx);
        } else if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
            self.turn(if *a == actions::NEXT_PAGE { 1 } else { -1 }, cx);
        } else if *a == actions::UP || *a == actions::DOWN {
            // A row: of covers in the grid.
            let row = self.covers().map_or(1, |g| isize::from(g.columns as i16));
            self.step(if *a == actions::UP { -row } else { row }, cx);
        } else if [actions::PREVIOUS, actions::LEFT].contains(a) {
            self.step(-1, cx);
        } else if [actions::NEXT, actions::RIGHT].contains(a) {
            self.step(1, cx);
        } else if *a == actions::CONFIRM && self.filter_focused {
            self.choose_shown(cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.focus, cx);
        } else if *a == actions::MENU {
            if touch || self.filter_focused || !self.offer(self.focus, cx) {
                open_menu(cx);
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let point = gesture.start;
        let hit = self
            .page_range(self.page_of(self.focus))
            .find(|&i| self.place(i).contains(point));
        if gesture.kind == GestureKind::LongPress {
            if let Some(index) = hit {
                self.focus = index;
                return if self.offer(index, cx) {
                    Handled::Yes
                } else {
                    Handled::No
                };
            }
            return Handled::No;
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = Some((self.page_of(self.focus), self.pages()));
        let style = self.layout.style;
        if let Some(action) = self.layout.header.tap(cx.env(), &style, point, paging) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        if self.layout.header.area.contains(point) {
            // The search button, else the title: the library's menu.
            if self.find.is_some_and(|b| b.contains(point)) {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                open_menu(cx);
            }
            return Handled::Yes;
        }
        if let Some(row) = self.layout.filter_row
            && row.contains(point)
        {
            if self.find.is_some_and(|b| b.contains(point)) {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                self.choose_shown(cx);
            }
            return Handled::Yes;
        }
        if let Some(n) = self.layout.pills.iter().position(|p| p.contains(point)) {
            self.set_filter(Filter::ALL[n], cx);
            return Handled::Yes;
        }
        match hit {
            Some(index) => {
                self.focus = index;
                self.activate(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = SHOWN.read(event, params) {
            match SHOW_OPTIONS.iter().position(|o| *o == picked.option) {
                Some(n) if n < SHOW_FILTERS.len() => {
                    self.filter_focused = false;
                    self.set_filter(SHOW_FILTERS[n], cx);
                    cx.request_redraw();
                }
                Some(_) => cx.command(Command::OpenScreen(FIND, Params::new())),
                None => {}
            }
            return;
        }
        let env = *cx.env();
        if *event == events::CATALOG_CHANGED {
            // Made anew (books came, went or moved): the index from it,
            // or, while covered, when it shows (the one kept is of the old
            // catalog).
            if self.covered {
                self.gone.clear();
                self.collecting = false;
            } else {
                self.rebuild(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                cx.wake_at(env.now);
            }
            return;
        }
        if events::BOOK_CHANGED.read(event, params).is_some() {
            // Finished, a favourite, read further (on its sheet, say): how
            // the books are known anew from the status logged, its page
            // read again. While covered, done when it shows.
            if !self.covered {
                self.index.refresh(env.storage);
                self.loaded_page = None;
                self.refilter_in_place(&env);
                cx.request_redraw();
                cx.wake_at(env.now);
            }
            return;
        }
        let gone = if let Some(moved) = events::BOOK_MOVED.read(event, params) {
            Some(moved.from)
        } else {
            events::BOOK_REMOVED
                .read(event, params)
                .map(|removed| removed.path)
        };
        if let Some(path) = gone {
            // Moved (to the archive, say) or removed: not shown where it
            // was until the kernel made the catalog anew.
            self.gone.push(hash(&path));
            if !self.covered {
                self.refilter_in_place(&env);
                cx.request_redraw();
            }
        } else if *event == events::SETTING_CHANGED {
            let root = library::folder(&env);
            if root != self.root {
                // Another library folder: the kernel makes its catalog;
                // while covered, the index is made when it shows.
                if self.covered {
                    self.root = root.to_string();
                    self.gone.clear();
                    self.kept_focus = None;
                } else {
                    *self = Self::new(&env);
                    self.fresh = false;
                }
            } else if !self.covered {
                // The view, the order, the size of the interface: laid out
                // anew, the focus on its book; another archive folder: the
                // index anew.
                self.by_recent = env.settings.read(&crate::SORT) != crate::SORT_NAME;
                self.layout = Layout::new(&env);
                if archive(&env) != self.archive {
                    self.archive = archive(&env);
                    self.index = Index::default();
                    self.index = make_index(&env);
                }
                self.refilter_in_place(&env);
            } else {
                // Laid out anew when it shows.
                self.by_recent = env.settings.read(&crate::SORT) != crate::SORT_NAME;
                self.layout = Layout::new(&env);
            }
            cx.request_redraw();
            if !self.covered {
                cx.wake_at(env.now);
            }
        }
    }
}
