//! Draft of a redesigned library (experiment): shelves of covers by
//! collection, as in a bookshop or Apple Books. The organising axis is
//! what a book is to the reader, not where its file is.
//!
//! The library opens on a stack of named shelves: Continue reading (the
//! book read last first, with its title and how far it is read beside
//! it), Next in your series, Unread, Favourites, Recently finished, a
//! shelf for each collection (folder), then Series, Authors and All
//! books. Empty shelves are left out. Each shelf ends with a tile that
//! opens all of it as a list ([`list`]).
//!
//! With buttons: the page buttons (or up and down) go from shelf to
//! shelf, keeping the column; left and right go along a shelf, which turns
//! a page of covers at its end; confirm opens the book (or the series,
//! the author, the whole shelf); holding confirm (the menu) offers what
//! can be done with the book ([`sheet`]). Above the first shelf, "Find"
//! opens the library's menu: search, titles and authors from A to Z.
//! With touch: tap a cover to open it, hold it for its options, tap a
//! shelf's name to see all of it, swipe a shelf sideways to turn its
//! covers, swipe up and down (or tap the arrows above) for more shelves.
//!
//! What the shelves need of every book (state, series, author) comes
//! from a summary made in the background ([`summary`]); the screen reads
//! it once when it shows and keeps only the first few books of each
//! shelf, and covers made small only for the shelves in view.
//!
//! Shown in place of today's library when [`crate::DRAFT`] is
//! `files.draft.shelves`.

mod art;
mod gather;
mod list;
mod sheet;
mod summary;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Direction, Env, Gesture, GestureKind, Handled, Id, MenuEntry,
    Params, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec, ask, library,
};

use gather::{Gathered, Source, Tile};

/// A shelf seen in full; parameters `mode` and `text` (see
/// [`Source::mode`]).
const LIST: Id = Id::new_static("files.shelves.list");
/// What can be done with a book; parameter `path`.
const SHEET: Id = Id::new_static("files.shelves.sheet");
/// The library's menu of this draft.
const MENU: Id = Id::new_static("files.shelves.menu");
/// Told when a book's state changed (finished, a favourite): the
/// shelves are gathered again.
const CHANGED: Id = Id::new_static("files.shelves.changed");
/// The menu's ways to find a book.
const SEARCH: Id = Id::new_static("files.shelves.search");
const TITLES: Id = Id::new_static("files.shelves.titles");
const AUTHORS: Id = Id::new_static("files.shelves.authors");
const SERIES: Id = Id::new_static("files.shelves.series");
const COLLECTIONS: Id = Id::new_static("files.shelves.collections");
/// The keyboard's reply with the words to search for.
const SEARCH_ENTERED: Event<Typed> = Event::new("files.shelves.search-entered");

/// Pause between steps of making the summary, so input stays quick.
const STEP_DELAY_MS: u64 = 20;
/// Collections shown as shelves of their own, at most.
const MAX_COLLECTIONS: usize = 6;

/// Registers what this draft adds (screens, actions, translations).
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screen(ScreenSpec {
        id: LIST,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(list::ListScreen::new(env, params))),
    })?;
    r.add_screen(ScreenSpec {
        id: SHEET,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(sheet::Sheet::new(env, params))),
    })?;
    for (action, source, title, icon) in [
        (TITLES, Source::All, "files.shelves.titles", "letter-case"),
        (
            AUTHORS,
            Source::AuthorIndex,
            "files.shelves.authors-az",
            "user",
        ),
        (
            SERIES,
            Source::SeriesIndex,
            "files.shelves.series",
            "stack-2",
        ),
        (
            COLLECTIONS,
            Source::FolderIndex,
            "files.shelves.collections",
            "folder",
        ),
    ] {
        r.add_action(
            paperpaw_sdk::ActionSpec::new(action.clone(), title)
                .handler(move |_: &Env<'_>, out: &mut Vec<Command>| out.push(open_list(&source))),
        )?;
        r.add_icon(action, Cow::Borrowed(icon))?;
    }
    r.add_action(
        paperpaw_sdk::ActionSpec::new(SEARCH, "files.shelves.search").handler(
            |env: &Env<'_>, out: &mut Vec<Command>| {
                out.push(ask::text(env.i18n.tr("files.search.prompt"), &SEARCH_ENTERED).command());
            },
        ),
    )?;
    r.add_icon(SEARCH, Cow::Borrowed("search"))?;
    r.on(SEARCH_ENTERED, |_, typed, out| {
        let words = typed.text.trim();
        if !words.is_empty() {
            out.push(open_list(&Source::Search(words.to_string())));
        }
    })?;
    // Only library things, the ways to find a book first; the library
    // folder and the settings last.
    for (item, order) in [
        (SEARCH, 10),
        (TITLES, 20),
        (AUTHORS, 30),
        (SERIES, 40),
        (COLLECTIONS, 50),
        (Id::from(crate::ROOT), 60),
        (actions::SETTINGS, 70),
    ] {
        r.add_menu_entry(MenuEntry {
            menu: MENU,
            item,
            order,
        })?;
    }
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-shelves.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-shelves.de.ini")))?;
    Ok(())
}

/// The library screen of this draft.
pub(crate) fn screen(env: &Env<'_>, params: &Params) -> Box<dyn Screen> {
    Box::new(Shelves::new(env, params))
}

/// Opens `source` seen in full.
fn open_list(source: &Source) -> Command {
    let (mode, name) = source.mode();
    Command::OpenScreen(
        LIST,
        Params::new()
            .with(params::MODE, mode)
            .with(params::TEXT, name.to_string()),
    )
}

/// Opens the library's menu.
fn open_menu() -> Command {
    Command::OpenSlot(slots::MENU, Params::new().with(params::MENU, MENU.as_str()))
}

/// Opens what can be done with the book at `path`.
fn open_sheet(path: &str) -> Command {
    Command::OpenScreen(SHEET, Params::new().with(params::PATH, path.to_string()))
}

/// Opens the book at `path` to read.
fn open_book(path: &str) -> Command {
    Command::OpenSlot(
        slots::READER,
        Params::new().with(params::PATH, path.to_string()),
    )
}

/// What a tile opens: a book to read, or a shelf seen in full.
fn open_tile(tile: &Tile, from: &Source) -> Command {
    match tile {
        Tile::Book(item) => open_book(&item.path),
        Tile::Group(group) => {
            let name = group.name.clone();
            open_list(&match from {
                Source::AuthorIndex => Source::Author(name),
                Source::FolderIndex => Source::Folder(name),
                _ => Source::Series(name),
            })
        }
    }
}

/// The library folder, without slashes around it.
fn root(env: &Env<'_>) -> String {
    library::folder(env).to_string()
}

/// The name of a shelf of `source`.
fn shelf_title(env: &Env<'_>, source: &Source) -> String {
    let key = match source {
        Source::Continue => "files.shelves.continue",
        Source::Next => "files.shelves.next",
        Source::Unread => "files.shelves.unread",
        Source::Favourites => "files.favourites",
        Source::Finished => "files.shelves.finished",
        Source::SeriesIndex => "files.shelves.series",
        Source::AuthorIndex => "files.shelves.authors",
        Source::FolderIndex => "files.shelves.collections",
        Source::All => "files.shelves.all",
        Source::Folder(name) | Source::Series(name) | Source::Author(name) => {
            return name.clone();
        }
        Source::Search(words) => {
            return env
                .i18n
                .format("files.search.title", &[("text", Arg::Str(words))]);
        }
    };
    env.i18n.tr(key).to_string()
}

/// Covers made small, as large as they are drawn, for what is in view
/// only: the book's path, the size asked, the cover (none where the book
/// has none).
#[derive(Debug, Default)]
pub(crate) struct Covers {
    kept: Vec<(String, u16, u16, Option<Thumbnail>)>,
}

impl Covers {
    /// Keeps the covers of `wanted` (path, width, height) and makes the
    /// missing ones; the others go first, so memory never holds both.
    pub(crate) fn show(&mut self, env: &Env<'_>, wanted: &[(String, u16, u16)]) {
        self.kept.retain(|(p, w, h, _)| {
            wanted
                .iter()
                .any(|(wp, ww, wh)| wp == p && ww == w && wh == h)
        });
        for (path, w, h) in wanted {
            if self.get(path, *w, *h).is_some()
                || self
                    .kept
                    .iter()
                    .any(|(p, kw, kh, _)| p == path && kw == w && kh == h)
            {
                continue;
            }
            let format = env.device.display.format;
            let thumb = library::book(env.storage, path)
                .and_then(|book| library::cover(env.storage, path, &book, *w, *h, format));
            self.kept.push((path.clone(), *w, *h, thumb));
        }
    }

    pub(crate) fn get(&self, path: &str, w: u16, h: u16) -> Option<&Thumbnail> {
        self.kept
            .iter()
            .find(|(p, kw, kh, _)| p == path && *kw == w && *kh == h)
            .and_then(|(_, _, _, t)| t.as_ref())
    }

    pub(crate) fn clear(&mut self) {
        self.kept = Vec::new();
    }
}

/// A shelf on the first screen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shelf {
    source: Source,
    title: String,
    gathered: Gathered,
}

impl Shelf {
    /// Its tiles and the one that opens all of it.
    fn len(&self) -> usize {
        self.gathered.tiles.len() + 1
    }

    /// How many it has, for its name and its last tile.
    fn count(&self, env: &Env<'_>) -> String {
        let total = self.gathered.total as i64;
        if self.source.groups() {
            let more = if self.gathered.more { "+" } else { "" };
            return alloc::format!("{total}{more}");
        }
        env.i18n.plural("files.books", total, &[])
    }
}

/// Where everything of the stack of shelves goes, worked out when the
/// screen opens.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// Below the header.
    area: Rect,
    per_page: usize,
    shelf_height: u16,
    label_height: u16,
    /// Room for a cover; covers are drawn at its bottom left.
    cover: (u16, u16),
    /// From one cover's left edge to the next: the first on the left
    /// margin, the last on the right one.
    step: u16,
    columns: usize,
    caption_height: u16,
    /// How far a tile's focus reaches past its cover on each side.
    pad: u16,
}

impl Layout {
    fn new(env: &Env<'_>, style: &UiStyle, area: Rect) -> Self {
        let line = |size: f32| {
            style
                .font(env)
                .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
        };
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let room = area.width.saturating_sub(2 * margin);
        let columns = (u32::from(room) / env.metrics.mm(21).max(1)).max(3) as u16;
        let pad = crate::cell_side(style, room / columns);
        let least_space = 2 * pad + gap / 2;
        let widest = room.saturating_sub((columns - 1) * least_space) / columns;
        let tallest = widest * 3 / 2;
        let label_height = line(style.text) + gap / 2;
        // Two lines of the title below a cover, and its author.
        let caption_height = line(style.small) * 3;
        // The name, room for the focus above and below, the caption.
        let fixed = label_height + gap / 2 + gap / 4 + caption_height + gap;
        // As many shelves as fit with covers near their largest.
        let per_page = (area.height / (fixed + tallest * 3 / 4).max(1)).max(1);
        let shelf_height = area.height / per_page;
        let height = tallest.min(shelf_height.saturating_sub(fixed));
        // Shelves one below the other, what is left over below them.
        let shelf_height = shelf_height.min(fixed + height);
        let width = widest.min(height * 2 / 3);
        let step = room.saturating_sub(width) / (columns - 1).max(1);
        Self {
            area,
            per_page: usize::from(per_page),
            shelf_height,
            label_height,
            cover: (width, height),
            step,
            columns: usize::from(columns),
            caption_height,
            pad,
        }
    }

    /// The top of the shelf in place `slot` of the page.
    fn top(&self, slot: usize) -> u16 {
        self.area.y + slot as u16 * self.shelf_height
    }

    fn shelf_rect(&self, slot: usize) -> Rect {
        Rect::new(
            self.area.x,
            self.top(slot),
            self.area.width,
            self.shelf_height,
        )
    }

    fn label_rect(&self, slot: usize) -> Rect {
        Rect::new(
            self.area.x,
            self.top(slot),
            self.area.width,
            self.label_height,
        )
    }

    /// The room for the cover in `column` of the shelf in `slot`.
    fn cover_rect(&self, style: &UiStyle, slot: usize, column: usize) -> Rect {
        let x = style.margin as u16 + column as u16 * self.step;
        let y = self.top(slot) + self.label_height + style.gap as u16 / 2;
        Rect::new(x, y, self.cover.0, self.cover.1)
    }

    fn caption_rect(&self, style: &UiStyle, slot: usize, column: usize) -> Rect {
        let cover = self.cover_rect(style, slot, column);
        Rect::new(
            cover.x,
            cover.y + cover.height + style.gap as u16 / 4,
            cover.width,
            self.caption_height,
        )
    }

    /// Where the focus of a tile spanning `span` columns from `column`
    /// is drawn around: its cover and caption, and a little more.
    fn cell_rect(&self, style: &UiStyle, slot: usize, column: usize, span: usize) -> Rect {
        let first = self.cover_rect(style, slot, column);
        let last = self.cover_rect(style, slot, column + span - 1);
        let caption = self.caption_rect(style, slot, column);
        let gap = style.gap as u16;
        let x = first.x.saturating_sub(self.pad);
        let y = first.y.saturating_sub(gap / 2);
        Rect::new(
            x,
            y,
            last.x + last.width + self.pad - x,
            caption.y + caption.height + gap / 2 - y,
        )
    }
}

/// A tile in view: which of its shelf's, in which column, how many
/// columns wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed {
    tile: usize,
    column: usize,
    span: usize,
}

/// The library as a stack of shelves.
pub(crate) struct Shelves {
    style: UiStyle,
    header: ui::Header,
    layout: Layout,
    root: String,
    shelves: Vec<Shelf>,
    /// The first tile in view of each shelf.
    first: Vec<usize>,
    /// The shelf and the tile with the focus.
    focus: (usize, usize),
    /// Whether the focus shows: on touch screens only once a button
    /// moved it.
    shown: bool,
    page: usize,
    covers: Covers,
    /// The summary being made anew.
    walk: Option<summary::Walk>,
    /// Gathered from a summary (else the first one is being made).
    ready: bool,
    fresh: bool,
}

impl Shelves {
    fn new(env: &Env<'_>, _: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, false);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut screen = Self {
            layout: Layout::new(env, &style, area),
            style,
            header,
            root: root(env),
            shelves: Vec::new(),
            first: Vec::new(),
            focus: (0, 0),
            shown: !env.device.input.touch,
            page: 0,
            covers: Covers::default(),
            walk: None,
            ready: false,
            fresh: true,
        };
        screen.gather(env);
        screen
    }

    /// Gathers the shelves from the summary, keeping the place, and makes
    /// the covers in view.
    fn gather(&mut self, env: &Env<'_>) {
        self.ready = summary::exists(env.storage);
        let collections = gather::collections(env, &self.root, MAX_COLLECTIONS);
        let mut sources = alloc::vec![
            Source::Continue,
            Source::Next,
            Source::Unread,
            Source::Favourites,
            Source::Finished,
        ];
        sources.extend(collections.into_iter().map(Source::Folder));
        sources.extend([Source::SeriesIndex, Source::AuthorIndex, Source::All]);
        // Two pages of covers a shelf; the rest when it is seen in full.
        let cap = self.layout.columns * 2;
        let gathered = gather::gather(env, &self.root, &sources, cap);
        let shelves: Vec<Shelf> = sources
            .into_iter()
            .zip(gathered)
            .filter(|(_, g)| !g.tiles.is_empty())
            .map(|(source, gathered)| Shelf {
                title: shelf_title(env, &source),
                source,
                gathered,
            })
            .collect();
        if shelves != self.shelves {
            let focused = self.shelves.get(self.focus.0).map(|s| s.source.clone());
            self.shelves = shelves;
            // The same shelf keeps the focus, if it is still there.
            let at = focused
                .and_then(|s| self.shelves.iter().position(|shelf| shelf.source == s))
                .unwrap_or(0);
            self.first = alloc::vec![0; self.shelves.len()];
            self.focus = (at, 0);
            self.page = at / self.layout.per_page;
        }
        self.show_covers(env);
    }

    fn pages(&self) -> usize {
        self.shelves.len().div_ceil(self.layout.per_page).max(1)
    }

    /// Whether tile `tile` of shelf `shelf` is the large one: the book
    /// read last, with its title beside it.
    fn hero(&self, shelf: usize, tile: usize) -> bool {
        tile == 0
            && self.layout.columns >= 3
            && self
                .shelves
                .get(shelf)
                .is_some_and(|s| s.source == Source::Continue)
    }

    /// The tiles of `shelf` in view, from its first.
    fn view(&self, shelf: usize) -> Vec<Placed> {
        let Some(s) = self.shelves.get(shelf) else {
            return Vec::new();
        };
        let mut placed = Vec::new();
        let mut column = 0;
        let mut tile = self.first.get(shelf).copied().unwrap_or(0);
        while tile < s.len() {
            let span = if self.hero(shelf, tile) { 2 } else { 1 };
            if column + span > self.layout.columns {
                break;
            }
            placed.push(Placed { tile, column, span });
            column += span;
            tile += 1;
        }
        placed
    }

    /// The covers of the shelves in view, as large as they are drawn.
    fn show_covers(&mut self, env: &Env<'_>) {
        let mut wanted = Vec::new();
        let (w, h) = self.layout.cover;
        for shelf in self.page_shelves() {
            let s = &self.shelves[shelf];
            for placed in self.view(shelf) {
                if let Some(tile) = s.gathered.tiles.get(placed.tile) {
                    let place = art::cover_place(&self.style, tile, Rect::new(0, 0, w, h));
                    wanted.push((tile.item().path.clone(), place.width, place.height));
                }
            }
        }
        self.covers.show(env, &wanted);
    }

    /// The shelves on the page shown.
    fn page_shelves(&self) -> core::ops::Range<usize> {
        let first = self.page * self.layout.per_page;
        first..(first + self.layout.per_page).min(self.shelves.len())
    }

    fn redraw_tile(&self, cx: &mut Cx<'_, '_>, shelf: usize, tile: usize) {
        let slot = shelf.wrapping_sub(self.page * self.layout.per_page);
        if slot >= self.layout.per_page {
            return;
        }
        if let Some(p) = self.view(shelf).into_iter().find(|p| p.tile == tile) {
            let cell = self
                .layout
                .cell_rect(&self.style, slot, p.column, p.span)
                .grow(self.style.line as u16);
            cx.request_redraw_area(cell);
        }
    }

    /// Moves the focus to `to` (shelf, tile), turning the shelf or the
    /// page if it is out of view.
    fn focus_on(&mut self, to: (usize, usize), cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let was = self.focus;
        let page = to.0 / self.layout.per_page;
        self.focus = to;
        let scrolled = self.scroll_to(to);
        if page != self.page || scrolled {
            let turned = page != self.page;
            self.page = page;
            self.show_covers(&env);
            if turned {
                cx.request_redraw();
            } else {
                // The shelf turned: it, and the tile the focus left.
                let slot = to.0 - page * self.layout.per_page;
                cx.request_redraw_area(self.layout.shelf_rect(slot));
                self.redraw_tile(cx, was.0, was.1);
            }
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        self.redraw_tile(cx, was.0, was.1);
        self.redraw_tile(cx, to.0, to.1);
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Turns shelf `to.0` so that tile `to.1` is in view: a page of
    /// covers on, or back so that it is the last; whether it turned.
    fn scroll_to(&mut self, to: (usize, usize)) -> bool {
        let (shelf, tile) = to;
        if self.view(shelf).iter().any(|p| p.tile == tile) {
            return false;
        }
        let first = self.first[shelf];
        if tile > first {
            // On: it becomes the first in view.
            self.first[shelf] = tile;
        } else {
            // Back: the first from which it is the last in view.
            let mut start = tile;
            while start > 0 {
                self.first[shelf] = start - 1;
                if !self.view(shelf).iter().any(|p| p.tile == tile) {
                    break;
                }
                start -= 1;
            }
            self.first[shelf] = start;
        }
        true
    }

    /// One shelf up or down, keeping the column.
    fn step_shelf(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        if self.shelves.is_empty() {
            return;
        }
        if !self.shown {
            // Touch: the page buttons and swipes turn whole pages.
            return self.turn_page(delta, cx);
        }
        if self.header.note_focused {
            if delta > 0 {
                self.header.note_focused = false;
                cx.request_redraw_area(self.header.area);
                self.focus_on((0, self.first[0]), cx);
                self.redraw_tile(cx, 0, self.focus.1);
            }
            return;
        }
        let (shelf, tile) = self.focus;
        if delta < 0 && shelf == 0 {
            // Above the first shelf: the menu.
            self.header.note_focused = true;
            cx.request_redraw_area(self.header.area);
            self.redraw_tile(cx, shelf, tile);
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        let to = shelf as isize + delta;
        if to < 0 || to as usize >= self.shelves.len() {
            return;
        }
        let to = to as usize;
        let column = self
            .view(shelf)
            .iter()
            .find(|p| p.tile == tile)
            .map_or(0, |p| p.column);
        let view = self.view(to);
        let target = view
            .iter()
            .rev()
            .find(|p| p.column <= column)
            .or(view.first())
            .map_or(0, |p| p.tile);
        self.focus_on((to, target), cx);
    }

    /// A page of shelves on or back.
    fn turn_page(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let page = (self.page as isize + delta.signum()).clamp(0, self.pages() as isize - 1);
        if page as usize == self.page {
            return;
        }
        self.page = page as usize;
        self.focus = (self.page * self.layout.per_page, 0);
        let env = *cx.env();
        self.show_covers(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Along the shelf with the focus.
    fn step_tile(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        if self.shelves.is_empty() || self.header.note_focused {
            return;
        }
        if !self.shown {
            self.shown = true;
            let (shelf, tile) = self.focus;
            self.redraw_tile(cx, shelf, tile);
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        let (shelf, tile) = self.focus;
        let to = tile as isize + delta;
        if to < 0 || to as usize >= self.shelves[shelf].len() {
            return;
        }
        self.focus_on((shelf, to as usize), cx);
    }

    /// Turns shelf `shelf` a page of covers on or back (a swipe).
    fn turn_shelf(&mut self, shelf: usize, delta: isize, cx: &mut Cx<'_, '_>) {
        let view = self.view(shelf);
        let len = self.shelves[shelf].len();
        let to = if delta > 0 {
            view.last().map_or(0, |p| p.tile + 1)
        } else {
            self.first[shelf].saturating_sub(1)
        };
        if to >= len || (delta < 0 && self.first[shelf] == 0) {
            return;
        }
        let tile = if self.focus.0 == shelf {
            to
        } else {
            self.focus.1
        };
        if self.focus.0 == shelf {
            self.focus_on((shelf, tile), cx);
        } else {
            let before = self.first[shelf];
            self.scroll_to((shelf, to));
            if self.first[shelf] != before {
                let env = *cx.env();
                self.show_covers(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
        }
    }

    /// Opens tile `tile` of shelf `shelf`.
    fn open(&mut self, shelf: usize, tile: usize, cx: &mut Cx<'_, '_>) {
        let Some(s) = self.shelves.get(shelf) else {
            return;
        };
        let command = match s.gathered.tiles.get(tile) {
            Some(t) => open_tile(t, &s.source),
            None => open_list(&s.source),
        };
        if matches!(s.gathered.tiles.get(tile), Some(Tile::Book(_))) {
            // The covers go while the book is read: memory for it.
            self.covers.clear();
        }
        cx.command(command);
    }

    /// The shelf and tile at `point`, if any; the shelf alone with `None`
    /// for its name.
    fn hit(&self, point: paperpaw_sdk::Point) -> Option<(usize, Option<usize>)> {
        for (slot, shelf) in self.page_shelves().enumerate() {
            if !self.layout.shelf_rect(slot).contains(point) {
                continue;
            }
            if self.layout.label_rect(slot).contains(point) {
                return Some((shelf, None));
            }
            let tile = self.view(shelf).into_iter().find(|p| {
                self.layout
                    .cell_rect(&self.style, slot, p.column, p.span)
                    .contains(point)
            });
            return Some((shelf, tile.map(|p| p.tile)));
        }
        None
    }

    fn draw_shelf(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, slot: usize, shelf: usize) {
        let style = &self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let s = &self.shelves[shelf];
        let margin = style.margin as u16;
        // The name on the margin, how many on the right.
        let label = self.layout.label_rect(slot);
        let count = alloc::format!("{} ›", s.count(env));
        let count_width = font.measure(style.small, &count);
        let right = f32::from(label.width - margin);
        let gray = style.faint(80);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            right - count_width,
            label,
            &count,
            gray,
        );
        let room = Rect::new(
            margin,
            label.y,
            (right - count_width) as u16 - margin - style.gap as u16,
            label.height,
        );
        let x = f32::from(margin);
        ui::draw_label(canvas, &font, style.text, x, room, &s.title, Color::BLACK);
        for placed in self.view(shelf) {
            let cell = self
                .layout
                .cell_rect(style, slot, placed.column, placed.span);
            if cell.intersection(&canvas.clip()).is_none() {
                continue;
            }
            let focused =
                self.shown && !self.header.note_focused && self.focus == (shelf, placed.tile);
            if focused {
                ui::draw_focus(canvas, &crate::grid_style(style), cell);
            }
            let place = self.layout.cover_rect(style, slot, placed.column);
            let caption = self.layout.caption_rect(style, slot, placed.column);
            let ink = style.focus_ink(focused);
            match s.gathered.tiles.get(placed.tile) {
                Some(tile) => {
                    let inner = art::cover_place(style, tile, place);
                    let thumb = self
                        .covers
                        .get(&tile.item().path, inner.width, inner.height);
                    // "New" says nothing on shelves of books never opened.
                    let marks = match s.source {
                        Source::Unread | Source::Next => art::Marks::Quiet,
                        // The book read last has its bar beside it.
                        _ if placed.span > 1 => art::Marks::None,
                        _ => art::Marks::All,
                    };
                    art::draw_tile(canvas, env, style, place, tile, thumb, focused, marks);
                    if placed.span > 1 {
                        let height = thumb.map_or(place.height, Thumbnail::height);
                        let drawn = Rect::new(
                            place.x,
                            (place.y + place.height).saturating_sub(height),
                            place.width,
                            height,
                        );
                        self.draw_hero_text(canvas, env, slot, tile, drawn, focused);
                    } else {
                        let note = match tile {
                            Tile::Book(item) => item.author.clone(),
                            Tile::Group(group) => {
                                env.i18n.plural("files.books", group.count as i64, &[])
                            }
                        };
                        let gray = style.focus_faint(focused, 80);
                        draw_caption(canvas, &font, style, caption, tile.name(), &note, ink, gray);
                    }
                }
                None => art::draw_see_all(canvas, env, style, place, &s.count(env), focused),
            }
        }
    }

    /// Beside the book read last: its title, author and how far it is
    /// read, as large as it reads well.
    fn draw_hero_text(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        slot: usize,
        tile: &Tile,
        drawn: Rect,
        focused: bool,
    ) {
        let style = &self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let item = tile.item();
        let cover = self.layout.cover_rect(style, slot, 0);
        // From the top of the cover as drawn (covers stand on the shelf).
        let top = drawn.y.max(cover.y);
        let end = self.layout.cover_rect(style, slot, 1);
        let gap = style.gap as u16;
        let x = cover.x + cover.width + gap;
        let width = (end.x + end.width).saturating_sub(x);
        let ink = style.focus_ink(focused);
        // As large as text, smaller where a word would be split.
        let splits = |size: f32| {
            font.wrap(size, &item.title, f32::from(width))
                .iter()
                .any(|r| {
                    let (before, after) = item.title.split_at(r.end);
                    !before.ends_with(' ') && !after.is_empty() && !after.starts_with(' ')
                })
        };
        let size = [1.0, 0.9, 0.8]
            .iter()
            .map(|f| style.text * f)
            .find(|&size| !splits(size))
            .unwrap_or(style.small);
        let line = font.line_metrics(size).line_height as u16;
        let small = font.line_metrics(style.small).line_height as u16;
        let border = style.line.max(1) as u16;
        let bar = (env.metrics.tenth_mm(9) as u16).max(3) + 2 * border;
        // At the bottom how far it is read: level with the cover's foot,
        // or with the captions' where the title needs the room.
        let author = !item.author.is_empty();
        let needed = font.wrap(size, &item.title, f32::from(width)).len().min(3) as u16;
        let above = line * needed + if author { small } else { 0 } + small + gap / 4 + bar;
        let caption = self.layout.caption_rect(style, slot, 0);
        let bottom = if top + above <= cover.y + cover.height {
            cover.y + cover.height
        } else {
            caption.y + caption.height
        };
        let bar_area = Rect::new(x, bottom.saturating_sub(bar), width, bar);
        let percent_row = Rect::new(x, bar_area.y.saturating_sub(small + gap / 4), width, small);
        // Above it the title, as many lines as leave room for the author.
        let room = percent_row
            .y
            .saturating_sub(top + if author { small } else { 0 });
        let lines = usize::from(room / line.max(1)).max(1);
        let mut y = top;
        draw_lines(
            canvas,
            &font,
            size,
            x,
            &mut y,
            width,
            line,
            &item.title,
            lines,
            ink,
        );
        if author {
            let row = Rect::new(x, y, width, small);
            let gray = style.focus_faint(focused, 80);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(x),
                row,
                &item.author,
                gray,
            );
        }
        let percent = item.progress.unwrap_or(0);
        let text = alloc::format!("{percent} %");
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(x),
            percent_row,
            &text,
            ink,
        );
        art::progress_in(
            canvas,
            bar_area,
            border,
            percent,
            focused && style.dark_focus(),
        );
    }
}

/// Draws `text` wrapped to `width` in at most `lines` lines from `y` on
/// (the last shortened with "…" where there is more), moving `y` below
/// them.
#[allow(clippy::too_many_arguments)]
fn draw_lines(
    canvas: &mut Canvas<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    size: f32,
    x: u16,
    y: &mut u16,
    width: u16,
    line: u16,
    text: &str,
    lines: usize,
    ink: Color,
) {
    let wrapped = font.wrap(size, text, f32::from(width));
    let count = wrapped.len().min(lines);
    for (n, range) in wrapped.iter().take(count).enumerate() {
        let shown = if n + 1 == count && wrapped.len() > count {
            ui::fit(font, size, text[range.start..].trim_end(), f32::from(width))
        } else {
            text[range.clone()].to_string()
        };
        let row = Rect::new(x, *y, width, line);
        ui::draw_label(canvas, font, size, f32::from(x), row, &shown, ink);
        *y += line;
    }
}

/// A cover's caption: its title in two lines at most, then `note` (the
/// author) in gray.
#[allow(clippy::too_many_arguments)]
fn draw_caption(
    canvas: &mut Canvas<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    style: &UiStyle,
    area: Rect,
    text: &str,
    note: &str,
    ink: Color,
    gray: Color,
) {
    let line = area.height / 3;
    let mut y = area.y;
    draw_lines(
        canvas,
        font,
        style.small,
        area.x,
        &mut y,
        area.width,
        line,
        text,
        2,
        ink,
    );
    let row = Rect::new(area.x, y, area.width, line);
    ui::draw_label(
        canvas,
        font,
        style.small,
        f32::from(area.x),
        row,
        note,
        gray,
    );
}

impl Screen for Shelves {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let note = env.i18n.tr("files.shelves.find");
        self.header.draw(
            canvas,
            env,
            &style,
            env.i18n.tr("files.title"),
            Some(note),
            Some((self.page, self.pages())),
        );
        let area = self.layout.area;
        if self.shelves.is_empty() {
            match &self.walk {
                Some(walk) if !self.ready => {
                    let found = walk.found() as i64;
                    let message = env.i18n.plural("files.collecting", found, &[]);
                    ui::draw_message(canvas, env, &style, area, &message);
                }
                _ => {
                    let text = env
                        .i18n
                        .format("files.empty", &[("dir", Arg::Str(&self.root))]);
                    let title = env.i18n.tr("files.empty.title");
                    ui::draw_empty(canvas, env, &style, area, "books", title, &text);
                }
            }
            return;
        }
        for (slot, shelf) in self.page_shelves().enumerate() {
            let rect = self.layout.shelf_rect(slot);
            if rect.intersection(&canvas.clip()).is_some() {
                self.draw_shelf(canvas, env, slot, shelf);
            }
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !core::mem::take(&mut self.fresh) {
            // Back from a book: its progress and the shelves changed.
            self.gather(&env);
            cx.request_redraw();
        }
        if self.walk.is_none() {
            self.walk = Some(summary::Walk::start(env.storage, &self.root));
        }
        cx.wake_at(env.now);
    }

    /// Covered by a book or another screen: the covers go, and the
    /// summary is made anew when the shelves show again, not meanwhile.
    fn on_hide(&mut self) {
        self.covers.clear();
        self.walk = None;
    }

    /// A step of making the summary; the shelves anew once it is made.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(walk) = &mut self.walk else {
            return;
        };
        if !walk.step(&env) {
            if !self.ready && walk.found() % 12 == 0 {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            cx.wake_at(env.now.add_millis(STEP_DELAY_MS));
            return;
        }
        self.walk = None;
        let before = self.shelves.clone();
        let was_ready = self.ready;
        self.gather(&env);
        if self.shelves != before || !was_ready {
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [actions::NEXT_PAGE, actions::DOWN].contains(a) {
            self.step_shelf(1, cx);
        } else if [actions::PREV_PAGE, actions::UP].contains(a) {
            self.step_shelf(-1, cx);
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            self.step_tile(1, cx);
        } else if [actions::LEFT, actions::PREVIOUS].contains(a) {
            self.step_tile(-1, cx);
        } else if *a == actions::CONFIRM {
            if self.header.note_focused || self.shelves.is_empty() {
                cx.command(open_menu());
            } else {
                let (shelf, tile) = self.focus;
                self.open(shelf, tile, cx);
            }
        } else if *a == actions::MENU {
            let (shelf, tile) = self.focus;
            let book = self
                .shelves
                .get(shelf)
                .and_then(|s| s.gathered.tiles.get(tile))
                .filter(|_| self.shown && !self.header.note_focused);
            match book {
                Some(Tile::Book(item)) => cx.command(open_sheet(&item.path)),
                _ => cx.command(open_menu()),
            }
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM && !self.header.note_focused {
            return paperpaw_sdk::Hint::Text(env.i18n.tr("files.option.open").to_string());
        }
        paperpaw_sdk::Hint::Default
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let page = Some((self.page, self.pages()));
        match gesture.kind {
            GestureKind::Tap => {
                if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, page) {
                    cx.command(Command::RunAction(action));
                    return Handled::Yes;
                }
                if self.header.area.contains(gesture.start) {
                    cx.command(open_menu());
                    return Handled::Yes;
                }
                match self.hit(gesture.start) {
                    Some((shelf, None)) => {
                        let source = self.shelves[shelf].source.clone();
                        cx.command(open_list(&source));
                    }
                    Some((shelf, Some(tile))) => {
                        self.focus = (shelf, tile);
                        self.open(shelf, tile, cx);
                    }
                    None => return Handled::No,
                }
                Handled::Yes
            }
            GestureKind::LongPress => match self.hit(gesture.start) {
                Some((shelf, Some(tile))) => {
                    let item = match self.shelves[shelf].gathered.tiles.get(tile) {
                        Some(Tile::Book(item)) => item.path.clone(),
                        _ => return Handled::No,
                    };
                    self.focus = (shelf, tile);
                    cx.command(open_sheet(&item));
                    Handled::Yes
                }
                _ => Handled::No,
            },
            GestureKind::Swipe(direction @ (Direction::Left | Direction::Right)) => {
                let Some((shelf, _)) = self.hit(gesture.start) else {
                    return Handled::No;
                };
                let delta = if direction == Direction::Left { 1 } else { -1 };
                self.turn_shelf(shelf, delta, cx);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        let library_changed = *event == paperpaw_sdk::builtin::events::LIBRARY_CHANGED;
        if *event == CHANGED || library_changed {
            let env = *cx.env();
            self.gather(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            if library_changed && self.walk.is_none() {
                self.walk = Some(summary::Walk::start(env.storage, &self.root));
                cx.wake_at(env.now);
            }
        } else if *event == paperpaw_sdk::builtin::events::SETTING_CHANGED {
            // The UI size or the library folder may have changed.
            let env = *cx.env();
            *self = Self::new(&env, &Params::new());
            self.fresh = false;
            self.walk = Some(summary::Walk::start(env.storage, &self.root));
            cx.request_redraw();
            cx.wake_at(env.now);
        }
    }
}
