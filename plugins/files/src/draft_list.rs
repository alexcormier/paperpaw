//! Draft of a redesigned library (experiment): a calm list with covers and
//! progress.
//!
//! Shown in place of today's library when [`crate::DRAFT`] is
//! `files.draft.list`. The library is one list of all books (folders
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
//! Buttons: left and right switch the filter, the page buttons move the
//! focus a book at a time (on touch screens they turn pages), confirm
//! opens, holding confirm (the menu button) offers what can be done with
//! the focused book. Above the first book, the focus reaches "Find".
//! Touch: tap a pill, a book, the search button (for "Find"); hold a book
//! for its actions; tap the title for the library's menu.
//!
//! What is known of each book to filter and sort is kept small
//! ([`index`]); a page's books are read when it is shown, never while
//! drawing.

mod find;
mod index;
mod rows;
mod sheet;
mod shelf;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::library::{self, catalog::Walk};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, MenuEntry, Params,
    Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec,
};

use index::{Filter, Index, NEVER, has_catalog, read_line};
use rows::{Loaded, Sizes};

/// The library's menu in this draft.
const MENU: Id = Id::new_static("files.list.menu");
/// Finds a book by a letter ([`find`]).
const FIND: Id = Id::new_static("files.list.find");
/// Searches with the keyboard; the words come back as [`SEARCHED`].
const SEARCH: Id = Id::new_static("files.list.search");
const SEARCHED: Event<Typed> = Event::new("files.list.searched");
/// The favourite books.
const FAVOURITES: Id = Id::new_static("files.list.favourites");
/// A list of books: a series, a folder, the favourites, a letter, a
/// search ([`shelf`]).
const SHELF: Id = Id::new_static("files.list.shelf");
/// What can be done with a book ([`sheet`]).
const SHEET: Id = Id::new_static("files.list.sheet");
/// Steps of reading books' progress and of making the catalog, apart.
const STEP_MS: u64 = 20;
/// Books whose progress is read in one step.
const STEP_BOOKS: usize = 8;

/// Registers what this draft adds (screens, actions, translations).
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
    r.add_action(
        paperpaw_sdk::ActionSpec::new(SEARCH, "files.list.search").handler(
            |env: &Env<'_>, out: &mut Vec<Command>| {
                let prompt = env.i18n.tr("files.search.prompt");
                out.push(ask::text(prompt, &SEARCHED).command());
            },
        ),
    )?;
    r.add_icon(SEARCH, Cow::Borrowed("search"))?;
    r.on(SEARCHED, |_, typed, out| {
        let text = typed.text.trim();
        if !text.is_empty() {
            out.push(shelf::open(shelf::Kind::Search, text));
        }
    })?;
    r.add_action(
        paperpaw_sdk::ActionSpec::new(FAVOURITES, "files.favourites").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(shelf::open(shelf::Kind::Favourites, ""));
            },
        ),
    )?;
    r.add_icon(FAVOURITES, Cow::Borrowed("star"))?;
    // Only what is about the library; the device's things are in its
    // settings (and night mode on a long press of the menu button).
    for (item, order) in [
        (Id::from(crate::SORT), 10),
        (FIND, 20),
        (SEARCH, 21),
        (FAVOURITES, 30),
        (crate::BY_AUTHOR, 31),
        (actions::SETTINGS, 90),
    ] {
        r.add_menu_entry(MenuEntry {
            menu: MENU,
            item,
            order,
        })?;
    }
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-list.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-list.de.ini")))?;
    Ok(())
}

/// The library screen of this draft.
pub(crate) fn screen(env: &Env<'_>, _params: &Params) -> Box<dyn Screen> {
    Box::new(ListScreen::new(env))
}

/// Opens the library's menu.
fn open_menu(cx: &mut Cx<'_, '_>) {
    cx.command(Command::OpenSlot(
        slots::MENU,
        Params::new().with(params::MENU, MENU.as_str()),
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
    /// The shelves, while their pill is chosen.
    shelves: Vec<ShelfRow>,
    /// The first book shown is the one read last, as a large card.
    card: bool,
    focus: usize,
    /// The focus shows: with buttons, and on touch screens once a button
    /// moved it.
    shown: bool,
    /// The pills, and whether the last shows as an icon.
    pills: Vec<Rect>,
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
        let pills_area = Rect::new(0, header.area.height, display.width, sizes.pill + gap / 2);
        let labels: Vec<&str> = Filter::ALL.iter().map(|f| env.i18n.tr(f.label())).collect();
        let (pills, pill_icon) = rows::pill_places(env, &style, &sizes, pills_area, &labels);
        let top = pills_area.y + pills_area.height;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            root: library::folder(env).to_string(),
            index: Index::build(env.storage),
            walk: None,
            walked: false,
            filter: Filter::All,
            by_recent: env.settings.read(&crate::SORT) != crate::SORT_NAME,
            order: Vec::new(),
            shelves: Vec::new(),
            card: false,
            focus: 0,
            shown: !env.device.input.touch,
            pills,
            pill_icon,
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
        if self.filter == Filter::Shelves {
            self.order = Vec::new();
            self.shelves = self.make_shelves(env);
            self.card = false;
        } else {
            self.shelves = Vec::new();
            self.order = self.index.order(self.filter, self.by_recent);
            // The card: the book read last, while it is read.
            self.card = self.by_recent
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
        let mut shelves = alloc::vec![
            ShelfRow {
                kind: shelf::Kind::Favourites,
                name: String::new(),
                count: favourites,
                finished: 0,
            },
            ShelfRow {
                kind: shelf::Kind::Authors,
                name: String::new(),
                count: authors.len(),
                finished: 0,
            },
        ];
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

    /// Rows on a page, and on the first page below the card.
    fn per_page(&self) -> usize {
        usize::from(self.area.height / self.row_height().max(1)).max(1)
    }

    /// Entries on the first page: the card and the rows below it.
    fn first_page(&self) -> usize {
        if self.card {
            let below = self.area.height.saturating_sub(self.sizes.card);
            1 + usize::from(below / self.sizes.row.max(1))
        } else {
            self.per_page()
        }
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
        if self.card && page == 0 {
            if slot == 0 {
                return Rect::new(a.x, a.y, a.width, self.sizes.card);
            }
            let y = a.y + self.sizes.card + (slot as u16 - 1) * self.sizes.row;
            return Rect::new(a.x, y, a.width, self.sizes.row);
        }
        let row = self.row_height();
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
            let cover = if card {
                self.sizes.card_cover
            } else {
                self.sizes.cover
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
        self.header.note_focused = false;
        let env = *cx.env();
        self.refilter(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Moves the focus by `delta` entries. Past the first (or the last)
    /// it reaches "Find" in the header, as a tap on it does.
    fn step(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.len();
        let before = self.focus;
        if self.header.note_focused {
            self.header.note_focused = false;
            self.focus = if delta > 0 { 0 } else { len.saturating_sub(1) };
        } else if !self.shown {
            // The first press shows where the focus is.
            self.shown = true;
        } else {
            let next = self.focus as isize + delta;
            if next < 0 || next >= len as isize {
                self.header.note_focused = true;
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
            cx.request_redraw_area(self.header.area);
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
        self.header.note_focused = false;
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
            if row.kind == shelf::Kind::Authors {
                cx.command(Command::RunAction(crate::BY_AUTHOR));
            } else {
                cx.command(shelf::open(row.kind, &row.name));
            }
            return;
        }
        if let Some(book) = self.book(index) {
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
        // "Find": a note the focus reaches with buttons, a search button
        // on touch screens.
        let find = self.find_button(env);
        let note = format!("{} ›", env.i18n.tr("files.list.find"));
        let note = find.is_none().then_some(note.as_str());
        let paging = (len > 0).then(|| (self.page_of(self.focus), self.pages()));
        let title = env.i18n.tr("files.title");
        self.header.draw(canvas, env, &style, title, note, paging);
        if let Some(button) = find {
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
        if !self.index.items.is_empty() {
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
            let focused = self.shown && !self.header.note_focused && index == self.focus;
            if self.card && index == 0 {
                if let Some(book) = self.book(0) {
                    rows::draw_card(canvas, env, &style, &self.sizes, rect, focused, book);
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

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let steps = if *action == actions::LEFT {
            -1
        } else if *action == actions::RIGHT {
            1
        } else {
            return Hint::Default;
        };
        Hint::Text(env.i18n.tr(self.filter.step(steps).label()).to_string())
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
            if self.index.step(env.storage, STEP_BOOKS) && !matches!(self.filter, Filter::All) {
                // Every book's progress known: the filter is right now.
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
        if !self.walked {
            // Books that came since the catalog was made.
            self.walk = Some(Walk::start(env.storage, &self.root));
            cx.wake_at(env.now.add_millis(STEP_MS));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let touch = cx.env().device.input.touch;
        if *a == actions::LEFT || *a == actions::RIGHT {
            let steps = if *a == actions::LEFT { -1 } else { 1 };
            self.set_filter(self.filter.step(steps), cx);
        } else if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
            let steps = if *a == actions::NEXT_PAGE { 1 } else { -1 };
            // On touch screens (also swipes) a page; with buttons alone
            // a book, as the other buttons switch the filter.
            if touch {
                self.turn(steps, cx);
            } else {
                self.step(steps, cx);
            }
        } else if [actions::UP, actions::PREVIOUS].contains(a) {
            self.step(-1, cx);
        } else if [actions::DOWN, actions::NEXT].contains(a) {
            self.step(1, cx);
        } else if *a == actions::CONFIRM && self.header.note_focused {
            cx.command(Command::OpenScreen(FIND, Params::new()));
        } else if *a == actions::CONFIRM {
            self.activate(self.focus, cx);
        } else if *a == actions::MENU {
            if touch || self.header.note_focused || !self.offer(self.focus, cx) {
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

    fn on_event(&mut self, event: &Id, _params: &Params, cx: &mut Cx<'_, '_>) {
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
