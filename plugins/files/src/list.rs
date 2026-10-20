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
//! What is known of each book to filter and sort is kept small
//! ([`index`]); a page's books are read when it is shown, never while
//! drawing.

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
use paperpaw_sdk::library::{self, catalog::Walk};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    RegisterError, Registrar, Screen, ScreenSpec,
};

use index::{Filter, Index, NEVER, has_catalog, read_line};
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
/// Steps of reading books' progress and of making the catalog, apart.
const STEP_MS: u64 = 20;
/// Books whose progress is read in one step.
const STEP_BOOKS: usize = 8;

/// Registers the list's screens and actions.
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screen(ScreenSpec {
        id: SHELF,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(shelf::ShelfScreen::new(env, params))),
    })?;
    r.add_screen(ScreenSpec {
        id: SHEET,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(sheet::BookSheet::new(env, params))),
    })?;
    r.add_screen(ScreenSpec {
        id: SHOW,
        title: Cow::Borrowed("files.list.show"),
        factory: Box::new(|env, params| {
            Box::new(paperpaw_sdk::ui::choice::Choice::new(env, params))
        }),
    })?;
    r.add_screen(ScreenSpec {
        id: FIND,
        title: Cow::Borrowed("files.list.find"),
        factory: Box::new(|env, params| Box::new(find::FindScreen::new(env, params))),
    })?;
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

/// A shelf of the shelves' pill.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShelfRow {
    /// "Next in your series" (a filter), else a shelf of `kind`.
    next: bool,
    kind: shelf::Kind,
    /// The series' or folder's name (the folder's path for folders).
    name: String,
    count: usize,
    finished: usize,
}

/// The library; see the [module documentation](self).
struct ListScreen {
    style: UiStyle,
    sizes: Sizes,
    header: ui::Header,
    root: String,
    index: Index,
    /// The catalog being made anew, and whether it was this time.
    walk: Option<Walk>,
    walked: bool,
    filter: Filter,
    by_recent: bool,
    /// The books shown, as indices of the index's items.
    order: Vec<u16>,
    /// How many books each of `order` stands for: more than one for a
    /// series as a stack (covers only); empty where each is one.
    counts: Vec<u16>,
    /// The covers' grid, where books show as covers ([`crate::VIEW`]).
    grid: Option<grid::Grid>,
    /// The shelves, while their pill is chosen.
    shelves: Vec<ShelfRow>,
    /// The first book shown is the one read last, as a large card.
    card: bool,
    focus: usize,
    /// The focus shows: with buttons, and on touch screens once a button
    /// moved it.
    shown: bool,
    /// The pills, and whether the last shows as an icon (larger screens).
    pills: Vec<Rect>,
    /// The filter's row (small screens).
    filter_row: Option<Rect>,
    /// The focus is on the filter's row or the pills.
    filter_focused: bool,
    pill_icon: bool,
    /// Where the books (or shelves) go.
    area: Rect,
    /// The books of the page shown, read when it was shown.
    loaded: Vec<Loaded>,
    /// The page `loaded` holds.
    loaded_page: Option<usize>,
    /// Just opened: the first `on_show` need not load again.
    fresh: bool,
}

impl ListScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let gap = style.gap as u16;
        let (pills, pill_icon, filter_row, top) = if sizes.compact {
            let row = Rect::new(0, header.area.height, display.width, sizes.filter_row);
            (Vec::new(), false, Some(row), row.y + row.height + gap / 2)
        } else {
            let pills_area = Rect::new(0, header.area.height, display.width, sizes.pill + gap / 2);
            let labels: Vec<&str> = Filter::ALL.iter().map(|f| env.i18n.tr(f.label())).collect();
            let (pills, icon) = rows::pill_places(env, &style, &sizes, pills_area, &labels);
            (pills, icon, None, pills_area.y + pills_area.height)
        };
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            root: library::folder(env).to_string(),
            index: Index::build(env.storage),
            walk: None,
            walked: false,
            filter: Filter::All,
            by_recent: env.settings.read(&crate::SORT) != crate::SORT_NAME,
            order: Vec::new(),
            counts: Vec::new(),
            grid: (env.settings.read(&crate::VIEW) == crate::VIEW_COVERS)
                .then(|| grid::Grid::new(env, &style, &sizes, area.height)),
            shelves: Vec::new(),
            card: false,
            focus: 0,
            shown: !env.device.input.touch,
            pills,
            pill_icon,
            filter_row,
            filter_focused: false,
            area,
            loaded: Vec::new(),
            loaded_page: None,
            fresh: true,
            style,
            sizes,
            header,
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

    /// Takes the shown books (or shelves) anew from the index, focus on
    /// the first.
    fn refilter(&mut self, env: &Env<'_>) {
        self.focus = 0;
        self.loaded_page = None;
        self.counts = Vec::new();
        if self.filter == Filter::Shelves {
            self.order = Vec::new();
            self.shelves = self.make_shelves(env);
            self.card = false;
        } else {
            self.shelves = Vec::new();
            self.order = self.index.order(self.filter, self.by_recent);
            // As covers, the books of a series are one stack.
            if self.grid.is_some() && self.filter == Filter::All {
                (self.order, self.counts) = self.index.stacked(&self.order);
            }
            // The card: the book read last, while it is read (as covers,
            // it is the first cover).
            self.card = self.by_recent
                && self.grid.is_none()
                && matches!(self.filter, Filter::All | Filter::Reading)
                && self.order.first().is_some_and(|&i| {
                    let item = &self.index.items[usize::from(i)];
                    item.recent != NEVER && item.progress != 100
                });
        }
        self.load_page(env);
    }

    /// The shelves: favourites and authors, then the series and the
    /// folders, each with how many books it has (and how many are
    /// finished), from one pass over the catalog.
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
        let items = &self.index.items;
        index::scan(env.storage, &mut |offset, line| {
            let finished = items
                .binary_search_by_key(&offset, |i| i.offset)
                .is_ok_and(|at| items[at].progress == 100);
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
                None => rows.push(ShelfRow {
                    next: false,
                    kind,
                    name: name.to_string(),
                    count: 1,
                    finished: usize::from(finished),
                }),
            };
            if !line.series.is_empty() {
                count(&mut series, shelf::Kind::Series, &line.series);
            }
            let inner = line.path.strip_prefix(&prefix).unwrap_or(&line.path);
            if let Some((folder, _)) = inner.split_once('/') {
                count(
                    &mut folders,
                    shelf::Kind::Folder,
                    &format!("{prefix}{folder}"),
                );
            }
        });
        series.sort_by_key(|r| r.name.to_lowercase());
        folders.sort_by_key(|r| r.name.to_lowercase());
        let next = self.index.order(Filter::Next, false).len();
        let mut shelves = Vec::new();
        if next > 0 {
            shelves.push(ShelfRow {
                next: true,
                kind: shelf::Kind::Series,
                name: String::new(),
                count: next,
                finished: 0,
            });
        }
        shelves.extend([
            ShelfRow {
                next: false,
                kind: shelf::Kind::Favourites,
                name: String::new(),
                count: favourites,
                finished: 0,
            },
            ShelfRow {
                next: false,
                kind: shelf::Kind::Authors,
                name: String::new(),
                count: authors.len(),
                finished: 0,
            },
        ]);
        shelves.extend(series);
        shelves.extend(folders);
        shelves
    }

    fn row_height(&self) -> u16 {
        if self.filter == Filter::Shelves {
            self.style.noted_row() as u16
        } else {
            self.sizes.row
        }
    }

    /// How high the books' rows of `page` are drawn: on small screens as
    /// many as fit, sharing what is left over, so a page ends at the
    /// foot of the screen.
    fn row_on(&self, page: usize) -> u16 {
        let row = self.row_height();
        if !self.sizes.compact || self.filter == Filter::Shelves {
            return row;
        }
        let card = if self.card && page == 0 {
            self.sizes.card
        } else {
            0
        };
        let room = self.area.height.saturating_sub(card);
        let rows = (room / row.max(1)).max(1);
        room / rows
    }

    /// The covers' grid, while books show (not the shelves).
    fn covers(&self) -> Option<&grid::Grid> {
        self.grid
            .as_ref()
            .filter(|_| self.filter != Filter::Shelves)
    }

    /// Entries on a page: rows, or covers.
    fn per_page(&self) -> usize {
        match self.covers() {
            Some(g) => g.per_page(),
            None => usize::from(self.area.height / self.row_height().max(1)).max(1),
        }
    }

    /// Entries on the first page: the card and the rows (or covers)
    /// below it.
    fn first_page(&self) -> usize {
        if !self.card {
            return self.per_page();
        }
        let below = self.area.height.saturating_sub(self.sizes.card);
        1 + usize::from(below / self.sizes.row.max(1))
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
        let a = self.area;
        let row = self.row_on(page);
        if let Some(g) = self.covers() {
            return g.place(a, a.y, slot);
        }
        if self.card && page == 0 {
            if slot == 0 {
                return Rect::new(a.x, a.y, a.width, self.sizes.card);
            }
            let y = a.y + self.sizes.card + (slot as u16 - 1) * row;
            return Rect::new(a.x, y, a.width, row);
        }
        Rect::new(a.x, a.y + slot as u16 * row, a.width, row)
    }

    /// Reads the books of the page the focus is on, if they are not read.
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
        let mut loaded = Vec::new();
        for index in self.page_range(page) {
            let item = self.index.items[usize::from(self.order[index])];
            let Some(line) = read_line(env.storage, item.offset) else {
                continue;
            };
            let card = self.card && index == 0;
            let cover = match self.covers() {
                _ if card => self.sizes.card_cover,
                Some(g) => g.cover,
                None => self.sizes.cover,
            };
            let book = Loaded::load(env, line, item.recent != NEVER, cover, card);
            self.index.set_progress(item.hash, book.progress);
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
        if let Some(book) = self.book(index) {
            if self.counts.get(index).is_some_and(|&c| c > 1) {
                // A series as a stack: its books.
                cx.command(shelf::open(shelf::Kind::Series, &book.series));
                return;
            }
            let path = book.path.clone();
            read(&path, cx);
        }
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

    /// Back from a book or an overlay: progress and order may have
    /// changed; read again (the order at once, progress in the
    /// background).
    fn reload(&mut self, env: &Env<'_>) {
        self.index.rerank(env.storage);
        self.index.read_again();
        let filter = self.filter;
        let focus = self.focus;
        self.refilter(env);
        // The book just read comes first: the focus goes there.
        if !self.by_recent || filter == Filter::Shelves {
            self.focus = focus.min(self.len().saturating_sub(1));
            self.load_page(env);
        }
    }

    /// On touch screens, where the header has its search button: left of
    /// the page arrows (as [`ui::Header`] places them).
    fn find_button(&self, env: &Env<'_>) -> Option<Rect> {
        if !env.device.input.touch || self.index.items.is_empty() {
            return None;
        }
        if let Some(row) = self.filter_row {
            // At the end of the filter's row, its icon on the margin.
            let square = row.height;
            let size = self.style.title * ui::ICON_SCALE;
            let icon = ui::icon_width(env, "search", size) as u16;
            let right = (row.x + row.width).saturating_sub(self.style.margin as u16);
            let x = (right + (square - icon) / 2).saturating_sub(square);
            return Some(Rect::new(x, row.y, square, square));
        }
        let a = self.header.area;
        let square = a.height;
        let pages = self.pages();
        let right = if self.len() > 0 && pages > 1 {
            let font = self.style.font(env)?;
            let label = format!("{} / {pages}", self.page_of(self.focus) + 1);
            let label = font.measure(self.style.small, &label) as u16;
            (a.x + a.width).saturating_sub(2 * (square * 4 / 5) + label)
        } else {
            (a.x + a.width).saturating_sub(self.style.margin as u16)
        };
        Some(Rect::new(right.saturating_sub(square), a.y, square, square))
    }

    /// Where the filter is: its row, or the pills' band.
    fn filter_area(&self) -> Rect {
        match self.filter_row {
            Some(row) => row,
            None => {
                let top = self.header.area.y + self.header.area.height;
                Rect::new(0, top, self.area.width, self.area.y.saturating_sub(top))
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

    fn draw_empty(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = &self.style;
        let area = self.area;
        if self.walk.is_some() && self.index.items.is_empty() {
            let found = self.walk.as_ref().map_or(0, Walk::found);
            let message = env.i18n.plural("files.collecting", found as i64, &[]);
            ui::draw_message(canvas, env, style, area, &message);
            return;
        }
        if self.index.items.is_empty() {
            let text = env
                .i18n
                .format("files.empty", &[("dir", Arg::Str(&self.root))]);
            let title = env.i18n.tr("files.empty.title");
            ui::draw_empty(canvas, env, style, area, "books", title, &text);
            return;
        }
        let key = match self.filter {
            Filter::Reading => "files.list.none.reading",
            Filter::Unread => "files.list.none.unread",
            Filter::Next => "files.list.none.next",
            _ => "files.list.none.finished",
        };
        ui::draw_empty(canvas, env, style, area, "books", "", env.i18n.tr(key));
    }

    fn draw_shelf(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        row: &ShelfRow,
        rect: Rect,
        focused: bool,
    ) {
        let style = &self.style;
        let ink = style.focus_ink(focused);
        let room = ui::icon_room(env, style, true);
        let (icon, title, note) = match row.kind {
            _ if row.next => (
                "arrow-bar-to-right",
                env.i18n.tr("files.list.show.next").to_string(),
                env.i18n.plural("files.books", row.count as i64, &[]),
            ),
            shelf::Kind::Favourites => (
                "star",
                env.i18n.tr("files.favourites").to_string(),
                env.i18n.plural("files.books", row.count as i64, &[]),
            ),
            shelf::Kind::Authors => (
                "user",
                env.i18n.tr("files.list.authors").to_string(),
                env.i18n
                    .plural("files.list.authors.count", row.count as i64, &[]),
            ),
            shelf::Kind::Series => {
                let books = env.i18n.plural("files.books", row.count as i64, &[]);
                let note = if row.finished > 0 {
                    let finished =
                        env.i18n
                            .plural("files.list.finished.count", row.finished as i64, &[]);
                    format!(
                        "{} · {books} · {finished}",
                        env.i18n.tr("files.list.series")
                    )
                } else {
                    format!("{} · {books}", env.i18n.tr("files.list.series"))
                };
                ("stack-2", row.name.clone(), note)
            }
            _ => {
                let books = env.i18n.plural("files.books", row.count as i64, &[]);
                (
                    "folder",
                    paperpaw_sdk::files::name(&row.name).to_string(),
                    format!("{} · {books}", env.i18n.tr("files.folder")),
                )
            }
        };
        let band = ui::title_area(style, rect);
        ui::draw_row_icon(canvas, env, style, band, room, Some(icon), ink);
        let text = Rect::new(
            rect.x + room,
            rect.y,
            rect.width.saturating_sub(room),
            rect.height,
        );
        ui::draw_noted_row(canvas, env, style, text, focused, &title, "›", &note);
    }
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        // "Find": a search button on touch screens, in the header or at
        // the end of the filter's row; with buttons in the filter's
        // choice.
        let find = self.find_button(env);
        let paging = (len > 0).then(|| (self.page_of(self.focus), self.pages()));
        let title = env.i18n.tr("files.title");
        self.header.draw(canvas, env, &style, title, None, paging);
        if let Some(row) = self.filter_row {
            if !self.index.items.is_empty() {
                let shown = SHOW_FILTERS
                    .iter()
                    .position(|f| *f == self.filter)
                    .unwrap_or(0);
                let key = format!("files.list.show.{}", SHOW_OPTIONS[shown]);
                let label = env.i18n.tr(&key).to_string();
                let end = if self.filter == Filter::Shelves {
                    String::new()
                } else {
                    env.i18n.plural("files.books", len as i64, &[])
                };
                let focused = self.shown && self.filter_focused;
                rows::draw_filter_row(canvas, env, &style, row, focused, &label, &end, find);
            }
        } else if let Some(button) = find {
            let size = style.title * ui::ICON_SCALE;
            let w = ui::icon_width(env, "search", size);
            let x = f32::from(button.x) + (f32::from(button.width) - w) / 2.0;
            let ink = style.header_colors().1;
            ui::draw_icon(canvas, env, "search", size, x, button, ink);
        }
        let labels: Vec<&str> = Filter::ALL.iter().map(|f| env.i18n.tr(f.label())).collect();
        let active = Filter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        // No pills before there are books to sort.
        if self.filter_row.is_none() && !self.index.items.is_empty() {
            rows::draw_pills(
                canvas,
                env,
                &style,
                &self.pills,
                &labels,
                active,
                self.pill_icon,
            );
        }
        if len == 0 {
            self.draw_empty(canvas, env);
            return;
        }
        let page = self.page_of(self.focus);
        for index in self.page_range(page) {
            let rect = self.place(index);
            if canvas.clip().intersection(&rect).is_none() {
                continue;
            }
            let focused = self.shown && !self.filter_focused && index == self.focus;
            if self.card && index == 0 {
                if let Some(book) = self.book(0) {
                    rows::draw_card(canvas, env, &style, &self.sizes, rect, focused, book);
                }
                continue;
            }
            if let Some(g) = self.covers() {
                if let Some(book) = self.book(index) {
                    let count = self.counts.get(index).copied().unwrap_or(1);
                    let sizes = &self.sizes;
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
                let tail = book.series_label();
                rows::draw_book_row(
                    canvas,
                    env,
                    &style,
                    &self.sizes,
                    rect,
                    focused,
                    book,
                    &book.author,
                    &tail,
                    false,
                );
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !core::mem::take(&mut self.fresh) {
            self.reload(&env);
            cx.request_redraw();
        } else if !has_catalog(env.storage) {
            // No catalog yet: made first, then shown.
            self.walk = Some(Walk::start(env.storage, &self.root));
        }
        cx.wake_at(env.now);
    }

    fn on_hide(&mut self) {
        // The covers go while a book is read: memory for it.
        self.loaded = Vec::new();
        self.loaded_page = None;
    }

    /// Makes the catalog anew or reads books' progress, a step at a time.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(walk) = &mut self.walk {
            if walk.step(&env) {
                // The new catalog: the index anew, the place kept.
                self.walk = None;
                self.walked = true;
                crate::shelves::made(&env);
                // The old index goes first: never two at once.
                self.index = Index::default();
                self.index = Index::build(env.storage);
                let focus = self.focus;
                self.refilter(&env);
                self.focus = focus.min(self.len().saturating_sub(1));
                self.load_page(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            } else if self.index.items.is_empty() {
                // "Collecting the books" counts up.
                cx.request_redraw_area(self.area);
                cx.request_refresh(RefreshMode::Fast);
            }
            cx.wake_at(env.now.add_millis(STEP_MS));
            return;
        }
        if self.index.unknown() > 0 {
            let stacks = self.grid.is_some() && self.filter == Filter::All;
            if self.index.step(env.storage, STEP_BOOKS) && (self.filter != Filter::All || stacks) {
                // Every book's progress known: the filter (or which books
                // are stacked) is right now.
                let focus = self.focus;
                self.refilter(&env);
                self.focus = focus.min(self.len().saturating_sub(1));
                self.load_page(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            cx.wake_at(env.now.add_millis(STEP_MS));
            return;
        }
        if !self.walked && crate::shelves::is_outdated(&env) {
            // Books that came since the catalog was made.
            self.walk = Some(Walk::start(env.storage, &self.root));
            cx.wake_at(env.now.add_millis(STEP_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let touch = cx.env().device.input.touch;
        let side = *a == actions::LEFT || *a == actions::RIGHT;
        if side && self.filter_focused && self.filter_row.is_none() {
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
        if let Some(action) = self.header.tap(cx.env(), &self.style, point, paging) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        if self.header.area.contains(point) {
            // The search button, else the title: the library's menu.
            if self
                .find_button(cx.env())
                .is_some_and(|b| b.contains(point))
            {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                open_menu(cx);
            }
            return Handled::Yes;
        }
        if let Some(row) = self.filter_row
            && row.contains(point)
        {
            if self
                .find_button(cx.env())
                .is_some_and(|b| b.contains(point))
            {
                cx.command(Command::OpenScreen(FIND, Params::new()));
            } else {
                self.choose_shown(cx);
            }
            return Handled::Yes;
        }
        if let Some(n) = self.pills.iter().position(|p| p.contains(point)) {
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
        if *event == events::LIBRARY_CHANGED {
            // Books came or went: the catalog anew.
            self.walk = Some(Walk::start(env.storage, &self.root));
            cx.wake_at(env.now);
        } else if *event == events::SETTING_CHANGED {
            let by_recent = env.settings.read(&crate::SORT) != crate::SORT_NAME;
            let root = library::folder(&env);
            if root != self.root {
                *self = Self::new(&env);
                self.fresh = false;
                self.walk = Some(Walk::start(env.storage, &self.root));
            } else if by_recent != self.by_recent {
                self.by_recent = by_recent;
                self.refilter(&env);
            }
            cx.request_redraw();
            cx.wake_at(env.now);
        }
    }
}
