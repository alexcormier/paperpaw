//! Draft of a redesigned library (experiment): a wall of covers with a
//! strip to continue reading and filter pills.
//!
//! Shown in place of today's library when [`crate::DRAFT`] is
//! `files.draft.wall`.
//!
//! The first page starts with the book read last, its cover large beside
//! its title, author and progress, and the next books being read small
//! below them; then the filters (all books, being read, unread, finished,
//! favourites); then covers. Later pages are covers only. A cover says how
//! the book stands: a bar along its bottom while it is read, a check mark
//! once finished, "New" while never opened. Folders and series are
//! stacks of covers; opening one shows its books as the same wall, a
//! series by number with the next to read marked.
//!
//! With buttons alone the focus goes down the page: the book read last,
//! the books beside it, the filters (left and right choose one), the
//! covers; the page buttons go a step down or up (a page of covers at a
//! time among them). The focused book's title and author show at the
//! bottom. Holding confirm (the menu) on a book opens what can be done
//! with it; elsewhere the library's menu. By touch: tap a book to open
//! it, hold it for what can be done with it, tap a filter, the search,
//! or the header for the menu.
//!
//! What is known of each book comes from an index made in the background
//! ([`index`]), never while drawing; a page holds only its own covers,
//! made small to the size they are drawn.

mod index;
mod sheet;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::{self, Focus, Thumbnail, UiStyle, choice::Picked};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, MenuEntry,
    Params, Point, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec,
};

use index::{Build, FINISHED, Index, NEW, NONE, READING, UNRANKED};

/// The wall's menu: library things only.
const MENU: Id = Id::new_static("files.wall.menu");
/// What can be done with a book; parameter `path`.
const SHEET: Id = Id::new_static("files.wall.sheet");
/// The letters to find a title by.
const LETTERS: Id = Id::new_static("files.wall.letters");
/// Finds a book by the letter its title starts with.
const FIND_LETTER: Id = Id::new_static("files.wall.letter");
/// Searches the library, its books shown as the wall.
const SEARCH: Id = Id::new_static("files.wall.search");
const SEARCHED: Event<Typed> = Event::new("files.wall.searched");
/// A letter was chosen (as the `path`).
const LETTER_CHOSEN: Event<Picked> = Event::new("files.wall.letter-chosen");
/// A book's state or favourite changed (its `path`).
const BOOK_CHANGED: Event<Picked> = Event::new("files.wall.book-changed");

/// The filters, in the order of their pills.
const FILTERS: [&str; 5] = ["all", "reading", "unread", "finished", "favourites"];
const FAVOURITES: usize = 4;
/// Pause between steps of the work in the background.
const STEP_DELAY_MS: u64 = 20;

/// Registers what this draft adds (screens, actions, translations).
pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screen(ScreenSpec {
        id: SHEET,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(sheet::BookSheet::new(env, params))),
    })?;
    r.add_screen(ScreenSpec {
        id: LETTERS,
        title: Cow::Borrowed("files.title"),
        factory: Box::new(|env, params| Box::new(sheet::LetterSheet::new(env, params))),
    })?;
    r.add_action(ActionSpec::new(FIND_LETTER, "files.wall.letter").handler(
        |_: &Env<'_>, out: &mut Vec<Command>| {
            out.push(Command::OpenScreen(LETTERS, Params::new()));
        },
    ))?;
    r.add_icon(FIND_LETTER, Cow::Borrowed("letter-case"))?;
    r.add_action(ActionSpec::new(SEARCH, "files.wall.search").handler(
        |env: &Env<'_>, out: &mut Vec<Command>| {
            out.push(ask::text(env.i18n.tr("files.search.prompt"), &SEARCHED).command());
        },
    ))?;
    r.add_icon(SEARCH, Cow::Borrowed("search"))?;
    // Library things first; where the books are and the settings last.
    for (item, order) in [
        (FIND_LETTER, 10),
        (SEARCH, 11),
        (Id::from(crate::SORT), 12),
        (crate::SHELVES_MENU, 13),
        (crate::ROOT.into(), 30),
        (actions::SETTINGS, 40),
    ] {
        r.add_menu_entry(MenuEntry {
            menu: MENU,
            item,
            order,
        })?;
    }
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-wall.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/draft-wall.de.ini")))?;
    Ok(())
}

/// The library screen of this draft.
pub(crate) fn screen(env: &Env<'_>, params: &Params) -> Box<dyn Screen> {
    Box::new(Wall::new(env, params))
}

/// What a wall shows.
#[derive(Debug, Clone, PartialEq)]
enum Scope {
    /// The library folder, with the strip and the filters.
    Library,
    /// A folder below it ([`Index::dirs`]).
    Folder(u16),
    /// A series ([`Index::series`]), by number.
    Series(u16),
    /// The books whose titles start with a letter.
    Letter(u8),
    /// The books a search found (their keys), and the words.
    Search(String, Vec<u32>),
}

/// What the wall is opened on (parameters `path`, or `mode` and `text`).
#[derive(Debug, Clone, PartialEq)]
enum Start {
    Folder(String),
    Search(String),
    Letter(u8),
}

/// The books a search for `words` finds.
fn search(storage: &dyn paperpaw_sdk::Storage, words: String) -> Scope {
    let keys = catalog::search(storage, &words)
        .iter()
        .map(|r| index::key(&r.path))
        .collect();
    Scope::Search(words, keys)
}

/// What a place on the wall holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Book,
    /// A folder ([`Index::dirs`]) as a stack of its books.
    Folder,
    /// A series ([`Index::series`]) as a stack of its books.
    Series,
}

/// A place on the wall: a book, or a stack of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Item {
    kind: Kind,
    /// The folder or series of a stack.
    id: u16,
    /// The book shown ([`Index::lines`]): a stack's on top.
    line: u16,
    /// Books in a stack, and how many of them are finished.
    count: u16,
    finished: u16,
    /// The next book of a series to read.
    next: bool,
}

/// A place on the visible page as drawn: what was read for it.
#[derive(Debug, Default)]
struct Card {
    path: String,
    title: String,
    author: String,
    progress: Option<u8>,
    state: u8,
    /// Its number in a series, as written.
    number: String,
    /// A stack's name.
    name: String,
    thumb: Option<Thumbnail>,
}

/// Where the focus is (with buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    /// A book of the strip (0 the large one).
    Strip(usize),
    Pills,
    Grid,
}

/// Work in the background.
#[derive(Debug)]
enum Work {
    Idle,
    /// The catalog made anew (none yet, or books came or went).
    Walk(catalog::Walk),
    Build(Build),
}

/// Where everything goes, worked out once.
#[derive(Debug, Default)]
struct Geometry {
    /// Below the header.
    top: u16,
    /// Covers end above it (the caption).
    bottom: u16,
    columns: u16,
    /// A cover's size; rows of covers this far apart.
    cover: (u16, u16),
    pitch: u16,
    /// The titles go under the covers (touch screens); else the focused
    /// book's in the caption.
    titles: bool,
    small_line: u16,
    /// The strip: the large cover, its words, the next books read (each a
    /// small cover with its title and progress).
    big: Rect,
    text: Rect,
    recents: Vec<Rect>,
    /// The filters' pills.
    pills: Vec<Rect>,
    /// Where covers start on the first page.
    first_top: u16,
    first_rows: u16,
    rows: u16,
    caption: Option<Rect>,
}

impl Geometry {
    fn new(env: &Env<'_>, style: &UiStyle, header: u16) -> Self {
        let display = env.device.display;
        let touch = env.device.input.touch;
        let Some(font) = style.font(env) else {
            return Self::default();
        };
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let (width, height) = (display.width, display.height);
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let room = width.saturating_sub(2 * margin);
        let top = header + gap;
        let small_line = line(style.small);

        // Covers about 22 mm wide, at least three a row, their outer
        // edges on the margins.
        let columns = (u32::from(room) / env.metrics.mm(22).max(1)).max(3) as u16;
        let cover_w = room.saturating_sub((columns - 1) * gap) / columns;
        let cover_h = cover_w * 3 / 2;
        let titles = touch;
        let cell = cover_h + if titles { gap / 2 + small_line } else { 0 };
        let pitch = cell + gap;
        let caption_h = if titles {
            0
        } else {
            line(style.text) + small_line + gap / 2
        };
        let bottom = height.saturating_sub(caption_h);
        let caption = (!titles).then(|| Rect::new(0, bottom, width, caption_h));

        // The strip: the large cover, its words beside it, the next books
        // read below the words where the screen is wide, else in a row
        // below it all; as tall as leaves a row of covers below the
        // filters, the cover at most 45 % of the width (26 mm).
        let pill_h = ((style.row * 7 / 10).max(style.finger)) as u16;
        let words = small_line * 3 + line(style.title) * 2 + gap * 3 / 2;
        let widest = (room * 45 / 100).min(env.metrics.mm(26) as u16);
        let left = bottom.saturating_sub(top + pill_h + gap * 2 + cell);
        let least = env.metrics.mm(9) as u16;
        let mut big_h = left.min(widest * 3 / 2).max(words);
        let text_w = room.saturating_sub(big_h * 2 / 3 + gap);
        let beside = big_h.saturating_sub(words + gap);
        // Each of the next books: its cover and some 20 mm of words.
        let item_w = |h: u16| h * 2 / 3 + env.metrics.mm(20) as u16;
        let (recent_y, recent_h, recent_x, recent_w) =
            if beside >= least && text_w >= 2 * item_w(beside) + gap {
                let x = margin + big_h * 2 / 3 + gap;
                (top + big_h - beside, beside, x, text_w)
            } else {
                big_h = left
                    .saturating_sub(least + gap)
                    .min(widest * 3 / 2)
                    .max(words);
                let h = left
                    .saturating_sub(big_h + gap)
                    .clamp(least, env.metrics.mm(12) as u16);
                (top + big_h + gap, h, margin, room)
            };
        let big_w = big_h * 2 / 3;
        let big = Rect::new(margin, top, big_w, big_h);
        let text_x = margin + big_w + gap;
        let text = Rect::new(text_x, top, room.saturating_sub(big_w + gap), words);
        let half = recent_w.saturating_sub(gap) / 2;
        let recents: Vec<Rect> = (0..2)
            .map(|i| Rect::new(recent_x + i * (half + gap), recent_y, half, recent_h))
            .collect();
        let strip_bottom = (top + big_h).max(recent_y + recent_h);

        // The filters, as wide as their words, the row from margin to
        // margin (the search is in the header on touch screens).
        let pills_y = strip_bottom + gap;
        let between = gap / 2;
        let icon = style.text * 1.2;
        let widths: Vec<f32> = FILTERS
            .iter()
            .map(|f| {
                if *f == "favourites" {
                    icon
                } else {
                    font.measure(style.small, env.i18n.tr(&format!("files.wall.filter.{f}")))
                }
            })
            .collect();
        let n = widths.len() as f32;
        let spare = f32::from(room) - f32::from(between) * (n - 1.0) - widths.iter().sum::<f32>();
        let pad = (spare / n).max(0.0);
        let mut x = f32::from(margin);
        let mut pills = Vec::new();
        for (i, w) in widths.iter().enumerate() {
            let right = if i + 1 == widths.len() {
                f32::from(margin + room)
            } else {
                x + w + pad
            };
            pills.push(Rect::new(x as u16, pills_y, (right - x) as u16, pill_h));
            x = right + f32::from(between);
        }

        let first_top = pills_y + pill_h + gap;
        let fit = |from: u16| (bottom.saturating_sub(from) + gap) / pitch.max(1);
        Self {
            top,
            bottom,
            columns,
            cover: (cover_w, cover_h),
            pitch,
            titles,
            small_line,
            big,
            text,
            recents,
            pills,
            first_top,
            first_rows: fit(first_top),
            rows: fit(top).max(1),
            caption,
        }
    }

    /// Places for covers on the first page (with the strip) and on others.
    fn first_count(&self) -> usize {
        usize::from(self.first_rows * self.columns)
    }

    fn count(&self) -> usize {
        usize::from(self.rows * self.columns)
    }
}

/// The strip's parts as drawn ([`Wall::strip_rects`]).
struct StripRects {
    big: Rect,
    text: Rect,
    recents: Vec<Rect>,
    /// The large one with its words.
    card: Rect,
}

/// The library as a wall of covers; see the module documentation.
pub(crate) struct Wall {
    style: UiStyle,
    header: ui::Header,
    geo: Geometry,
    touch: bool,
    root: String,
    /// What the wall was opened on, until the index is there.
    start: Option<Start>,
    work: Work,
    index: Option<Index>,
    scope: Scope,
    filter: usize,
    /// Where to go back to, with the focus there.
    trail: Vec<(Scope, usize)>,
    view: Vec<Item>,
    page: usize,
    zone: Zone,
    /// The focused place of the wall.
    focus: usize,
    /// The focus shows (on touch screens once a button moved it).
    shown: bool,
    /// The books read lately, not finished: the first large.
    strip: Vec<Card>,
    /// The visible page's places, from its first.
    cards: Vec<Card>,
    /// Hidden by a full screen: the covers made small went.
    hidden: bool,
}

impl Wall {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let geo = Geometry::new(env, &style, header.area.height);
        let root = library::folder(env).to_string();
        let text = params.get(params::TEXT).unwrap_or_default().trim();
        let start = match params.get(params::MODE) {
            Some("search") if !text.is_empty() => Some(Start::Search(text.to_string())),
            Some("letter") => text
                .bytes()
                .next()
                .map(|c| Start::Letter(c.to_ascii_uppercase())),
            _ => params
                .get(params::PATH)
                .map(|p| p.trim_matches('/').to_string())
                .filter(|p| *p != root)
                .map(Start::Folder),
        };
        let work = if env.storage.size(&index::catalog_path()).is_ok() {
            Work::Build(Build::start(env.storage))
        } else {
            Work::Walk(catalog::Walk::start(env.storage, &root))
        };
        let touch = env.device.input.touch;
        let mut wall = Self {
            style,
            header,
            geo,
            touch,
            root,
            start,
            work,
            index: None,
            scope: Scope::Library,
            filter: 0,
            trail: Vec::new(),
            view: Vec::new(),
            page: 0,
            zone: Zone::Grid,
            focus: 0,
            shown: !touch,
            strip: Vec::new(),
            cards: Vec::new(),
            hidden: false,
        };
        wall.load_strip(env);
        wall.zone = if wall.strip.is_empty() {
            Zone::Pills
        } else {
            Zone::Strip(0)
        };
        wall
    }

    /// Whether the first page has the strip and the filters.
    fn has_top(&self) -> bool {
        self.scope == Scope::Library
    }

    fn pages(&self) -> usize {
        let len = self.view.len();
        if !self.has_top() {
            return len.div_ceil(self.geo.count().max(1)).max(1);
        }
        let first = self.geo.first_count();
        1 + len.saturating_sub(first).div_ceil(self.geo.count().max(1))
    }

    /// The places of page `page`.
    fn range(&self, page: usize) -> core::ops::Range<usize> {
        let (first, count) = (self.geo.first_count(), self.geo.count());
        let (start, n) = match (self.has_top(), page) {
            (true, 0) => (0, first),
            (true, p) => (first + (p - 1) * count, count),
            (false, p) => (p * count, count),
        };
        let end = (start + n).min(self.view.len());
        start.min(end)..end
    }

    fn page_of(&self, place: usize) -> usize {
        let (first, count) = (self.geo.first_count(), self.geo.count().max(1));
        if !self.has_top() {
            place / count
        } else if place < first {
            0
        } else {
            1 + (place - first) / count
        }
    }

    /// Where place `place` (of the visible page) has its cover.
    fn cover_rect(&self, place: usize) -> Rect {
        let geo = &self.geo;
        let start = self.range(self.page).start;
        let slot = place.saturating_sub(start) as u16;
        let top = if self.has_top() && self.page == 0 {
            geo.first_top
        } else {
            geo.top
        };
        let (w, h) = geo.cover;
        let gap = self.style.gap as u16;
        Rect::new(
            self.style.margin as u16 + (slot % geo.columns) * (w + gap),
            top + (slot / geo.columns) * geo.pitch,
            w,
            h,
        )
    }

    /// How far the focus reaches out around a cover.
    fn ring(&self) -> u16 {
        (self.style.gap as u16 / 3).max(4)
    }

    /// The part of the screen the focus at `zone` (and `focus`) covers.
    fn zone_area(&self, zone: Zone, focus: usize) -> Option<Rect> {
        let ring = self.ring() + 1;
        let geo = &self.geo;
        match zone {
            Zone::Strip(i) => {
                let rects = self.strip_rects();
                let at = if i == 0 {
                    Some(rects.card)
                } else {
                    rects.recents.get(i - 1).copied()
                };
                at.map(|r| r.grow(ring + self.style.line as u16 * 2 + 3))
            }
            Zone::Pills => geo.pills.first().map(|r| {
                Rect::new(
                    0,
                    r.y.saturating_sub(ring * 2),
                    r.x * 2 + 2000,
                    r.height + ring * 4,
                )
            }),
            Zone::Grid => {
                let cover = self.cover_rect(focus);
                let below = if geo.titles {
                    geo.small_line + self.style.gap as u16
                } else {
                    0
                };
                Some(Rect::new(cover.x, cover.y, cover.width, cover.height + below).grow(ring))
            }
        }
    }

    /// The books read lately and not finished, the last first, with their
    /// covers made small: the strip.
    fn load_strip(&mut self, env: &Env<'_>) {
        let storage = env.storage;
        let mut strip = Vec::new();
        for path in library::opened_books(storage) {
            if strip.len() > self.geo.recents.len() {
                break;
            }
            let Some(book) = library::book(storage, &path) else {
                continue;
            };
            if book.progress == Some(100) {
                continue;
            }
            let place = if strip.is_empty() {
                self.geo.big
            } else {
                let at = self.geo.recents[strip.len() - 1];
                Rect::new(at.x, at.y, at.height * 2 / 3, at.height)
            };
            let format = if strip.is_empty() {
                env.device.display.format
            } else {
                wall_format(env)
            };
            let thumb = library::cover(storage, &path, &book, place.width, place.height, format);
            strip.push(Card {
                title: book.shown_title(&path).into_owned(),
                author: book.author.clone().unwrap_or_default(),
                progress: book.progress,
                state: if book.progress.unwrap_or(0) > 0 {
                    READING
                } else {
                    NEW
                },
                thumb,
                path,
                ..Card::default()
            });
        }
        self.strip = strip;
    }

    /// The places of the visible page, read from the catalog and the
    /// books' details, their covers made small.
    fn load_page(&mut self, env: &Env<'_>) {
        let Some(index) = &self.index else {
            self.cards = Vec::new();
            return;
        };
        let range = self.range(self.page);
        // The old covers go first: never two pages' at once.
        self.cards = Vec::new();
        let (w, h) = self.geo.cover;
        let lift = stack_lift(w);
        let cards = self.view[range]
            .iter()
            .map(|item| {
                let size = match item.kind {
                    Kind::Book => (w, h),
                    _ => (w - 2 * lift, h - 2 * lift),
                };
                card(env, index, item, size)
            })
            .collect();
        self.cards = cards;
    }

    /// The wall of the current scope and filter, anew from the index; the
    /// focus kept where it can be.
    fn rebuild(&mut self, env: &Env<'_>) {
        let Some(index) = &mut self.index else {
            return;
        };
        let by_name = env.settings.read(&crate::SORT) == crate::SORT_NAME;
        let root = index.dir(&self.root);
        self.view = view(index, &self.scope, self.filter, root, by_name);
        if self.scope == Scope::Library && self.filter == 0 {
            // The books of the strip are not shown twice among all books
            // (a stack keeps them; a filter lists all it finds).
            let strip: Vec<u32> = self.strip.iter().map(|c| index::key(&c.path)).collect();
            let lines = &index.lines;
            self.view.retain(|item| {
                item.kind != Kind::Book || !strip.contains(&lines[usize::from(item.line)].key)
            });
        }
        self.focus = self.focus.min(self.view.len().saturating_sub(1));
        if self.zone == Zone::Grid {
            self.page = self.page_of(self.focus);
        } else {
            self.page = 0;
        }
        self.page = self.page.min(self.pages() - 1);
        self.load_page(env);
    }

    /// Shows `scope`, from its first page (a series from the book to read
    /// next); `remember` keeps the way back.
    fn enter(&mut self, scope: Scope, remember: bool, cx: &mut Cx<'_, '_>) {
        if remember {
            self.trail.push((self.scope.clone(), self.focus));
        }
        self.scope = scope;
        self.focus = 0;
        self.zone = Zone::Grid;
        self.page = 0;
        let env = *cx.env();
        self.rebuild(&env);
        if let Some(next) = self.view.iter().position(|i| i.next) {
            self.focus = next;
            self.page = self.page_of(next);
            self.load_page(&env);
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Goes back to where the wall was before; `false` at the library.
    fn back(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        if let Some((scope, focus)) = self.trail.pop() {
            self.scope = scope;
            self.focus = focus;
            self.zone = Zone::Grid;
            self.rebuild(&env);
            if self.scope == Scope::Library && self.page == 0 && self.focus == 0 {
                self.zone = self.home_zone();
            }
        } else if self.filter != 0 {
            // A filter is undone before the library is left.
            self.set_filter(0, &env);
            self.zone = Zone::Pills;
        } else {
            return false;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        true
    }

    /// Where the focus starts on the library's first page.
    fn home_zone(&self) -> Zone {
        if self.strip.is_empty() {
            Zone::Pills
        } else {
            Zone::Strip(0)
        }
    }

    fn set_filter(&mut self, filter: usize, env: &Env<'_>) {
        self.filter = filter;
        self.focus = 0;
        self.page = 0;
        self.rebuild(env);
    }

    /// Opens place `place`: a book in the reader, a stack as a wall.
    fn open(&mut self, place: usize, cx: &mut Cx<'_, '_>) {
        let Some(item) = self.view.get(place).copied() else {
            return;
        };
        self.focus = place;
        match item.kind {
            Kind::Book => {
                if let Some(card) = self.card_at(place) {
                    let path = card.path.clone();
                    self.read(path, cx);
                }
            }
            Kind::Folder => self.enter(Scope::Folder(item.id), true, cx),
            Kind::Series => self.enter(Scope::Series(item.id), true, cx),
        }
    }

    fn read(&mut self, path: String, cx: &mut Cx<'_, '_>) {
        if path.is_empty() {
            return;
        }
        self.drop_covers();
        cx.command(Command::OpenSlot(
            slots::READER,
            Params::new().with(params::PATH, path),
        ));
    }

    fn card_at(&self, place: usize) -> Option<&Card> {
        let start = self.range(self.page).start;
        self.cards.get(place.checked_sub(start)?)
    }

    /// The covers made small go, for the memory of what opens above.
    fn drop_covers(&mut self) {
        for card in self.strip.iter_mut().chain(self.cards.iter_mut()) {
            card.thumb = None;
        }
        self.hidden = true;
    }

    /// Offers what can be done with the book at `path`.
    fn offer(&self, path: &str, cx: &mut Cx<'_, '_>) {
        if !path.is_empty() {
            cx.command(Command::OpenScreen(
                SHEET,
                Params::new().with(params::PATH, path.to_string()),
            ));
        }
    }

    fn open_menu(cx: &mut Cx<'_, '_>) {
        cx.command(Command::OpenSlot(
            slots::MENU,
            Params::new().with(params::MENU, MENU.as_str()),
        ));
    }

    /// The path of the book the focus is on, if it is on one.
    fn focused_book(&self) -> Option<&str> {
        match self.zone {
            Zone::Strip(i) => self.strip.get(i).map(|c| c.path.as_str()),
            Zone::Pills => None,
            Zone::Grid => {
                let item = self.view.get(self.focus)?;
                (item.kind == Kind::Book)
                    .then(|| self.card_at(self.focus).map(|c| c.path.as_str()))
                    .flatten()
            }
        }
    }

    /// Moves the focus to `zone` and `focus`, turning the page if need be,
    /// and asks for what changed to be drawn.
    fn go(&mut self, zone: Zone, focus: usize, cx: &mut Cx<'_, '_>) {
        let before = self.zone_area(self.zone, self.focus);
        let page = match zone {
            Zone::Grid => self.page_of(focus),
            _ => 0,
        };
        let shown = self.shown;
        self.shown = true;
        self.zone = zone;
        self.focus = focus;
        if page != self.page {
            self.page = page;
            let env = *cx.env();
            self.load_page(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        if !shown {
            cx.request_redraw();
        }
        for area in [before, self.zone_area(zone, focus), self.geo.caption]
            .into_iter()
            .flatten()
        {
            cx.request_redraw_area(area);
        }
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Turns to page `page` (touch screens, and page buttons among the
    /// covers): the focus to its first place.
    fn turn(&mut self, page: usize, cx: &mut Cx<'_, '_>) {
        if page >= self.pages() || page == self.page {
            return;
        }
        let start = self.range(page).start;
        self.page = page;
        self.focus = start;
        self.zone = if page == 0 && self.has_top() && self.touch {
            self.home_zone()
        } else {
            Zone::Grid
        };
        let env = *cx.env();
        self.load_page(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Right, or on: along the strip, through the filters, along the
    /// covers.
    fn forward(&mut self, cx: &mut Cx<'_, '_>) {
        match self.zone {
            Zone::Strip(i) if i + 1 < self.strip.len() => self.go(Zone::Strip(i + 1), 0, cx),
            Zone::Strip(_) => self.go(Zone::Pills, 0, cx),
            Zone::Pills => {
                if self.filter + 1 < FILTERS.len() {
                    let env = *cx.env();
                    self.set_filter(self.filter + 1, &env);
                    self.zone = Zone::Pills;
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Partial);
                }
            }
            Zone::Grid if self.focus + 1 < self.view.len() => {
                self.go(Zone::Grid, self.focus + 1, cx);
            }
            Zone::Grid => {}
        }
    }

    /// Left, or back: the other way.
    fn backward(&mut self, cx: &mut Cx<'_, '_>) {
        match self.zone {
            Zone::Strip(i) if i > 0 => self.go(Zone::Strip(i - 1), 0, cx),
            Zone::Strip(_) => {}
            Zone::Pills => {
                if self.filter > 0 {
                    let env = *cx.env();
                    self.set_filter(self.filter - 1, &env);
                    self.zone = Zone::Pills;
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Partial);
                }
            }
            Zone::Grid if self.focus > 0 => self.go(Zone::Grid, self.focus - 1, cx),
            Zone::Grid if self.has_top() => self.go(Zone::Pills, 0, cx),
            Zone::Grid => {}
        }
    }

    /// A page button on (down): from the strip to the filters, to the
    /// covers, then a page of covers.
    fn down(&mut self, cx: &mut Cx<'_, '_>) {
        match self.zone {
            Zone::Strip(_) => self.go(Zone::Pills, 0, cx),
            Zone::Pills if !self.view.is_empty() => self.go(Zone::Grid, 0, cx),
            Zone::Pills => {}
            Zone::Grid => {
                let page = self.page_of(self.focus) + 1;
                if page < self.pages() {
                    let start = self.range(page).start;
                    self.go(Zone::Grid, start, cx);
                }
            }
        }
    }

    /// A page button back (up): the other way.
    fn up(&mut self, cx: &mut Cx<'_, '_>) {
        match self.zone {
            Zone::Strip(_) => {}
            Zone::Pills if !self.strip.is_empty() => self.go(Zone::Strip(0), 0, cx),
            Zone::Pills => {}
            Zone::Grid => {
                let page = self.page_of(self.focus);
                if page > 0 {
                    let start = self.range(page - 1).start;
                    self.go(Zone::Grid, start, cx);
                } else if self.has_top() {
                    self.go(Zone::Pills, 0, cx);
                }
            }
        }
    }

    /// Where the search sits in the header on touch screens, and the
    /// spaces that keep its room as the header's note: right of the
    /// title, left of the page arrows (as [`ui::Header`] lays them out).
    fn search_spot(&self, env: &Env<'_>) -> Option<(Rect, String)> {
        if !self.touch {
            return None;
        }
        let style = &self.style;
        let font = style.font(env)?;
        let area = self.header.area;
        let pages = self.pages();
        let pager = if pages > 1 {
            let label = format!("{} / {}", self.page + 1, pages);
            area.height * 4 / 5 * 2 + font.measure(style.small, &label) as u16
        } else {
            0
        };
        let icon = ui::icon_width(env, "search", style.title).max(1.0);
        let space = font.measure(style.small, " ").max(1.0);
        let spaces = " ".repeat((icon / space) as usize + 2);
        let width = font.measure(style.small, &spaces) as u16;
        let right = (area.x + area.width).saturating_sub(pager + style.margin as u16);
        let spot = Rect::new(right.saturating_sub(width), area.y, width, area.height);
        Some((spot, spaces))
    }

    /// The title of the header.
    fn title(&self, env: &Env<'_>) -> String {
        let index = self.index.as_ref();
        match &self.scope {
            Scope::Library if self.filter == 0 => env.i18n.tr("files.title").to_string(),
            Scope::Library => env
                .i18n
                .tr(&format!("files.wall.filter.{}.title", FILTERS[self.filter]))
                .to_string(),
            Scope::Folder(dir) => index
                .and_then(|i| i.dirs.get(usize::from(*dir)))
                .map(|d| library_name(d).to_string())
                .unwrap_or_default(),
            Scope::Series(series) => index
                .and_then(|i| i.series.get(usize::from(*series)))
                .cloned()
                .unwrap_or_default(),
            Scope::Letter(letter) => {
                let mut text = [0u8; 4];
                let letter = (*letter as char).encode_utf8(&mut text);
                env.i18n
                    .format("files.wall.letter.title", &[("letter", Arg::Str(letter))])
            }
            Scope::Search(words, _) => env
                .i18n
                .format("files.search.title", &[("text", Arg::Str(words))]),
        }
    }

    /// Where the strip's parts are: the words follow the large cover's
    /// picture (which may be narrower than its place).
    fn strip_rects(&self) -> StripRects {
        let geo = &self.geo;
        let shift = match self.strip.first() {
            Some(card) => geo.big.width - picture_rect(geo.big, card, true).width,
            None => 0,
        };
        let big = Rect::new(geo.big.x, geo.big.y, geo.big.width - shift, geo.big.height);
        let text = Rect::new(
            geo.text.x - shift,
            geo.text.y,
            geo.text.width + shift,
            geo.text.height,
        );
        let beside = geo.recents.first().is_some_and(|r| r.x > geo.big.x);
        let recents: Vec<Rect> = geo
            .recents
            .iter()
            .enumerate()
            .map(|(i, r)| {
                if beside {
                    let half = shift / 2;
                    Rect::new(r.x - shift + i as u16 * half, r.y, r.width + half, r.height)
                } else {
                    *r
                }
            })
            .collect();
        // Focused, the large one is framed with its words; beside the next
        // books, the cover alone.
        let card = if beside {
            big
        } else {
            big.union(&Rect::new(text.x, text.y, text.width, big.height))
        };
        StripRects {
            big,
            text,
            recents,
            card,
        }
    }

    fn draw_strip(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = &self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let focused = |i: usize| self.shown && self.zone == Zone::Strip(i);
        let Some(first) = self.strip.first() else {
            return;
        };
        let rects = self.strip_rects();
        let frame = |canvas: &mut Canvas<'_>, at: Rect| {
            let ring = (style.line * 2).max(3);
            let area = at.grow(self.ring() + ring as u16);
            canvas.stroke_round(area, ring, style.corner, Color::BLACK);
        };
        // The large one: framed with its words when focused (a frame,
        // not a bar: the words stay black on paper).
        if focused(0) {
            frame(canvas, rects.card);
        }
        let (ink, faint) = (Color::BLACK, style.faint(80));
        canvas.describe(
            paperpaw_sdk::semantics::Role::Button,
            &first.title,
            rects.card,
            focused(0),
            None,
        );
        // Its progress is in the words: no bar on it.
        draw_picture(
            canvas,
            env,
            style,
            picture_rect(rects.big, first, true),
            first,
        );
        let t = rects.text;
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let mut y = t.y;
        let row = |y: u16, size: f32| Rect::new(t.x, y, t.width, line(size));
        let label = env.i18n.tr("files.wall.continue");
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(t.x),
            row(y, style.small),
            label,
            faint,
        );
        y += line(style.small) + style.gap as u16 / 2;
        let wrapped = font.wrap(style.title, &first.title, f32::from(t.width));
        let more = wrapped.len() > 2;
        for (n, range) in wrapped.into_iter().take(2).enumerate() {
            let text = if more && n == 1 {
                ui::fit(
                    &font,
                    style.title,
                    &first.title[range.start..],
                    f32::from(t.width),
                )
            } else {
                first.title[range].to_string()
            };
            let at = row(y, style.title);
            ui::draw_label(canvas, &font, style.title, f32::from(t.x), at, &text, ink);
            y += line(style.title);
        }
        let at = row(y, style.small);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(t.x),
            at,
            &first.author,
            faint,
        );
        y += line(style.small) + style.gap as u16 / 2;
        draw_progress_line(canvas, env, style, row(y, style.small), first.progress, ink);

        // The next books: a small cover, the title and how far read.
        for (i, (card, at)) in self.strip.iter().skip(1).zip(&rects.recents).enumerate() {
            if focused(i + 1) {
                frame(canvas, *at);
            }
            canvas.describe(
                paperpaw_sdk::semantics::Role::Button,
                &card.title,
                *at,
                focused(i + 1),
                None,
            );
            let cover = Rect::new(at.x, at.y, at.height * 2 / 3, at.height);
            let shown = picture_rect(cover, card, false);
            draw_picture(canvas, env, style, shown, card);
            draw_marks(canvas, env, style, shown, card, false);
            let x = shown.x + shown.width + style.gap as u16 / 2;
            let width = (at.x + at.width).saturating_sub(x);
            let mut y = at.y;
            for range in font
                .wrap(style.text, &card.title, f32::from(width))
                .into_iter()
                .take(2)
            {
                let row = Rect::new(x, y, width, line(style.text));
                let text = &card.title[range];
                ui::draw_label(canvas, &font, style.text, f32::from(x), row, text, ink);
                y += line(style.text);
            }
            let row = Rect::new(x, y, width, line(style.small));
            let state = Card {
                progress: card.progress,
                state: card.state,
                ..Card::default()
            };
            let note = book_note(env, &state);
            ui::draw_label(canvas, &font, style.small, f32::from(x), row, &note, faint);
        }
    }

    fn draw_pills(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = &self.style;
        for (i, at) in self.geo.pills.iter().enumerate() {
            let on = i == self.filter;
            let focused = on && self.shown && self.zone == Zone::Pills;
            let icon = match i {
                FAVOURITES => Some(("star", "files.wall.filter.favourites")),
                i if i >= FILTERS.len() => Some(("search", "files.wall.search")),
                _ => None,
            };
            match icon {
                Some((icon, name)) => {
                    let name = env.i18n.tr(name);
                    let ink = ui::draw_icon_button(canvas, style, *at, name, on, focused);
                    let size = style.text * 1.1;
                    let w = ui::icon_width(env, icon, size);
                    let x = f32::from(at.x) + (f32::from(at.width) - w) / 2.0;
                    ui::draw_icon(canvas, env, icon, size, x, *at, ink);
                }
                None => {
                    let key = format!("files.wall.filter.{}", FILTERS[i]);
                    let label = env.i18n.tr(&key);
                    ui::draw_button(canvas, env, style, *at, label, on, focused);
                }
            }
        }
    }

    /// The focused book's title and author (and state) at the bottom, for
    /// buttons (covers show no titles then).
    fn draw_caption(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let style = &self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        if !self.shown {
            return;
        }
        let (title, note) = match self.zone {
            Zone::Strip(0) => return,
            Zone::Strip(i) => match self.strip.get(i) {
                Some(card) => (card.title.clone(), book_note(env, card)),
                None => return,
            },
            Zone::Pills => {
                let lines = self.index.as_ref().map_or(&[][..], |i| &i.lines[..]);
                let count = lines
                    .iter()
                    .filter(|line| matches(line, self.filter))
                    .count() as i64;
                (
                    env.i18n
                        .tr(&format!("files.wall.filter.{}.title", FILTERS[self.filter]))
                        .to_string(),
                    env.i18n.plural("files.books", count, &[]),
                )
            }
            Zone::Grid => {
                let (Some(item), Some(card)) =
                    (self.view.get(self.focus), self.card_at(self.focus))
                else {
                    return;
                };
                match item.kind {
                    Kind::Book => (card.title.clone(), book_note(env, card)),
                    _ => (card.name.clone(), stack_note(env, item)),
                }
            }
        };
        let margin = style.margin as u16;
        let width = area.width.saturating_sub(2 * margin);
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let x = f32::from(margin);
        let top = area.y + style.gap as u16 / 2;
        let row = Rect::new(margin, top, width, line(style.text));
        ui::draw_label(canvas, &font, style.text, x, row, &title, Color::BLACK);
        let row = Rect::new(margin, top + line(style.text), width, line(style.small));
        ui::draw_label(canvas, &font, style.small, x, row, &note, style.faint(80));
    }
}

/// The name of a folder: its last part.
fn library_name(dir: &str) -> &str {
    dir.rsplit('/').next().unwrap_or(dir)
}

/// What a stack's covers are lifted by, for the covers behind.
fn stack_lift(width: u16) -> u16 {
    (width / 18).max(3)
}

/// How the wall's covers are made small: as the panel shows them, but in
/// black and white (dithered, an eighth of a byte a pixel) on small
/// devices whose panels have a few grays (the X4 Pro): at 4 bits a pixel,
/// nine covers took some 105 KB there, and the wall's pages peaked at
/// 255 KB of 256. The large cover of the strip keeps its grays.
fn wall_format(env: &Env<'_>) -> paperpaw_sdk::PixelFormat {
    let display = env.device.display.format;
    let small = env.device.memory == paperpaw_sdk::MemoryClass::Small;
    if small && display.gray_levels().is_some_and(|g| g > 2) && !display.is_color() {
        paperpaw_sdk::PixelFormat::Mono1
    } else {
        display
    }
}

/// Reads place `item` of the wall: its book's line of the catalog, its
/// details and its cover made small to `size`.
fn card(env: &Env<'_>, index: &Index, item: &Item, size: (u16, u16)) -> Card {
    let storage = env.storage;
    let Some(line) = index.lines.get(usize::from(item.line)) else {
        return Card::default();
    };
    let Some(record) = index::record_at(storage, line.offset) else {
        return Card::default();
    };
    let book = library::book(storage, &record.path);
    let format = wall_format(env);
    let thumb = book
        .as_ref()
        .and_then(|b| library::cover(storage, &record.path, b, size.0, size.1, format));
    let name = match item.kind {
        Kind::Book => String::new(),
        Kind::Folder => index
            .dirs
            .get(usize::from(item.id))
            .map(|d| library_name(d).to_string())
            .unwrap_or_default(),
        Kind::Series => index
            .series
            .get(usize::from(item.id))
            .cloned()
            .unwrap_or_default(),
    };
    Card {
        progress: book.as_ref().and_then(|b| b.progress),
        state: line.reading(),
        title: record.title,
        author: record.author,
        number: record.number,
        path: record.path,
        name,
        thumb,
    }
}

/// The wall of `scope`: in the library folder (or a folder) its folders
/// and series as stacks among its books, by when they were read and then
/// by title (`by_name`: by title alone); a filter, a series, a letter or a
/// search as books.
fn view(index: &mut Index, scope: &Scope, filter: usize, root: u16, by_name: bool) -> Vec<Item> {
    let book = |line: usize| Item {
        kind: Kind::Book,
        id: NONE,
        line: line as u16,
        count: 1,
        finished: 0,
        next: false,
    };
    let lines = &index.lines;
    let mut items: Vec<Item> = Vec::new();
    match scope {
        Scope::Library if filter > 0 => {
            items = (0..lines.len())
                .filter(|&i| matches(&lines[i], filter))
                .map(book)
                .collect();
            match FILTERS[filter] {
                "unread" => items.sort_by_key(|i| lines[usize::from(i.line)].sort),
                "favourites" => {
                    items.sort_by_key(|i| index.favourite_rank(&lines[usize::from(i.line)]));
                }
                _ => items.sort_by_key(|i| {
                    let line = &lines[usize::from(i.line)];
                    (line.rank, line.sort)
                }),
            }
            return items;
        }
        Scope::Series(series) => {
            items = (0..lines.len())
                .filter(|&i| lines[i].series == *series)
                .map(book)
                .collect();
            items.sort_by_key(|i| {
                let line = &lines[usize::from(i.line)];
                (line.number, line.sort)
            });
            if let Some(next) = items
                .iter_mut()
                .find(|i| lines[usize::from(i.line)].reading() == NEW)
            {
                next.next = true;
            }
            return items;
        }
        Scope::Letter(letter) => {
            items = (0..lines.len())
                .filter(|&i| index::initial(lines[i].sort) == *letter)
                .map(book)
                .collect();
            items.sort_by_key(|i| lines[usize::from(i.line)].sort);
            return items;
        }
        Scope::Search(_, keys) => {
            items = (0..lines.len())
                .filter(|&i| keys.contains(&lines[i].key))
                .map(book)
                .collect();
            items.sort_by_key(|i| lines[usize::from(i.line)].sort);
            return items;
        }
        Scope::Library | Scope::Folder(_) => {}
    }

    // A folder: its own books, series as stacks, folders below it as
    // stacks of all their books.
    let dir = match scope {
        Scope::Folder(dir) => *dir,
        _ => root,
    };
    let here = index
        .dirs
        .get(usize::from(dir))
        .cloned()
        .unwrap_or_default();
    let prefix = format!("{here}/");
    let mut children: Vec<String> = Vec::new();
    let mut child_of: Vec<Option<u16>> = Vec::new();
    for name in &index.dirs {
        let child = name
            .strip_prefix(&prefix)
            .map(|rest| format!("{prefix}{}", rest.split('/').next().unwrap_or(rest)));
        child_of.push(child.map(|c| match children.iter().position(|n| *n == c) {
            Some(i) => i as u16,
            None => {
                children.push(c);
                (children.len() - 1) as u16
            }
        }));
    }
    let child_ids: Vec<u16> = children.iter().map(|c| index.dir(c)).collect();
    let lines = &index.lines;
    for (i, line) in lines.iter().enumerate() {
        let (kind, id) = if line.dir == dir {
            if line.series == NONE {
                items.push(book(i));
                continue;
            }
            (Kind::Series, line.series)
        } else if let Some(Some(child)) = child_of.get(usize::from(line.dir)) {
            (Kind::Folder, child_ids[usize::from(*child)])
        } else {
            continue;
        };
        let finished = u16::from(line.reading() == FINISHED);
        match items.iter_mut().find(|s| s.kind == kind && s.id == id) {
            Some(stack) => {
                stack.count += 1;
                stack.finished += finished;
                // On top: the one read last, else the first of a series,
                // else the first by title.
                let top = &lines[usize::from(stack.line)];
                let better = (line.rank, line.number, line.sort) < (top.rank, top.number, top.sort);
                if better {
                    stack.line = i as u16;
                }
            }
            None => items.push(Item {
                kind,
                id,
                line: i as u16,
                count: 1,
                finished,
                next: false,
            }),
        }
    }
    // A series of one book is that book.
    for item in &mut items {
        if item.kind == Kind::Series && item.count == 1 {
            *item = book(usize::from(item.line));
        }
    }
    let name_sort = |item: &Item| match item.kind {
        Kind::Book => lines[usize::from(item.line)].sort,
        Kind::Folder => index::sort_key(library_name(
            index
                .dirs
                .get(usize::from(item.id))
                .map_or("", String::as_str),
        )),
        Kind::Series => index::sort_key(
            index
                .series
                .get(usize::from(item.id))
                .map_or("", String::as_str),
        ),
    };
    items.sort_by_cached_key(|item| {
        let rank = if by_name {
            UNRANKED
        } else {
            lines[usize::from(item.line)].rank
        };
        (rank, name_sort(item))
    });
    items
}

/// Whether the book of `line` passes filter `filter` ([`FILTERS`]).
fn matches(line: &index::Line, filter: usize) -> bool {
    match FILTERS[filter] {
        "reading" => line.reading() == READING,
        "unread" => line.reading() == NEW,
        "finished" => line.reading() == FINISHED,
        "favourites" => line.favourite(),
        _ => true,
    }
}

/// "Arthur Conan Doyle · 42 %": a book's author and how far it is read.
fn book_note(env: &Env<'_>, card: &Card) -> String {
    let state = match (card.state, card.progress) {
        (FINISHED, _) | (_, Some(100)) => env.i18n.tr("files.finished").to_string(),
        (_, Some(p)) if p > 0 => format!("{p} %"),
        _ => env.i18n.tr("files.wall.new").to_string(),
    };
    if card.author.is_empty() {
        state
    } else {
        format!("{} · {state}", card.author)
    }
}

/// "Series · 4 books, 2 finished".
fn stack_note(env: &Env<'_>, item: &Item) -> String {
    let kind = env.i18n.tr(if item.kind == Kind::Series {
        "files.wall.series"
    } else {
        "files.folder"
    });
    let books = env.i18n.plural("files.books", i64::from(item.count), &[]);
    if item.finished == 0 {
        format!("{kind} · {books}")
    } else {
        let done = env.i18n.format(
            "files.wall.finished-count",
            &[("count", Arg::Int(i64::from(item.finished)))],
        );
        format!("{kind} · {books}, {done}")
    }
}

/// Draws the focus around `area` as the style draws it, at its full size
/// (a cover drawn over it after keeps a frame of it around).
fn draw_focus_box(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect) {
    let radius = style.corner.min(u32::from(area.width.min(area.height)) / 8);
    let ring = (style.line * 2).max(3);
    match style.focus {
        Focus::Fill => canvas.fill_round(area, radius, Color::BLACK),
        Focus::Soft if style.grays > 2 => {
            let gray = if style.grays >= 16 { 215 } else { 170 };
            canvas.fill_round(area, radius, Color::gray(gray));
        }
        _ => canvas.stroke_round(area, ring, radius, Color::BLACK),
    }
}

/// Where a book's picture goes in its place `at`: as large as its cover
/// made small (a picture may be smaller than its place), on the left, at
/// the bottom (or the top, `top`); all of `at` for a stand-in.
fn picture_rect(at: Rect, card: &Card, top: bool) -> Rect {
    match &card.thumb {
        Some(thumb) => {
            let (w, h) = (thumb.width().min(at.width), thumb.height().min(at.height));
            let y = if top { at.y } else { at.y + at.height - h };
            Rect::new(at.x, y, w, h)
        }
        None => at,
    }
}

/// A book's cover in `at` ([`picture_rect`]), or a stand-in with its
/// title and author.
fn draw_picture(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, at: Rect, card: &Card) {
    match &card.thumb {
        Some(thumb) => {
            let radius = crate::picture_radius(style, at);
            canvas.small_pictures(|canvas| thumb.draw_round(canvas, at.x, at.y, radius));
        }
        None => draw_placeholder(canvas, env, style, at, &card.title, &card.author),
    }
}

/// Where a stack's covers go in its place `at`: the top one (a little
/// lower and narrower than the place, for the two behind it) and all of
/// them together.
fn stack_rects(at: Rect, card: &Card) -> (Rect, Rect) {
    let lift = stack_lift(at.width);
    let place = Rect::new(
        at.x,
        at.y + 2 * lift,
        at.width - 2 * lift,
        at.height - 2 * lift,
    );
    let front = picture_rect(place, card, false);
    let all = Rect::new(
        front.x,
        front.y - 2 * lift,
        front.width + 2 * lift,
        front.height + 2 * lift,
    );
    (front, all)
}

/// A book without a cover picture: a frame with its title, large, and its
/// author at the bottom.
fn draw_placeholder(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    at: Rect,
    title: &str,
    author: &str,
) {
    let radius = crate::cover_radius(style, at);
    canvas.fill_round(at, radius, Color::WHITE);
    canvas.stroke_round(at, ui::button_border(style), radius, Color::BLACK);
    let Some(font) = style.font(env) else {
        return;
    };
    let pad = (at.width / 10).max(4);
    let width = f32::from(at.width.saturating_sub(2 * pad));
    let size = if at.width > style.row as u16 * 2 {
        style.title
    } else {
        style.small
    };
    let line = font.line_metrics(size).line_height as u16;
    let mut y = at.y + pad * 2;
    for range in font.wrap(size, title, width).into_iter().take(4) {
        let text = &title[range];
        let w = font.measure(size, text);
        let x = f32::from(at.x) + (f32::from(at.width) - w) / 2.0;
        let row = Rect::new(at.x + pad, y, at.width.saturating_sub(2 * pad), line);
        ui::draw_label(
            canvas,
            &font,
            size,
            x.max(f32::from(row.x)),
            row,
            text,
            Color::BLACK,
        );
        y += line;
    }
    // A short rule, then the author below it (the bottom is kept for
    // the marks).
    let rule = at.width / 4;
    let rule_y = y + pad / 2;
    let small = font.line_metrics(style.small).line_height as u16;
    let room = at.y + at.height.saturating_sub(pad + small * 2);
    if author.is_empty() || rule_y + pad > room {
        return;
    }
    canvas.fill(
        Rect::new(
            at.x + (at.width - rule) / 2,
            rule_y,
            rule,
            style.line.max(1) as u16,
        ),
        Color::BLACK,
    );
    let text = ui::fit(&font, style.small, author, width);
    let w = font.measure(style.small, &text);
    let x = f32::from(at.x) + (f32::from(at.width) - w) / 2.0;
    let row = Rect::new(
        at.x + pad,
        rule_y + pad / 2,
        at.width.saturating_sub(2 * pad),
        small,
    );
    ui::draw_label(canvas, &font, style.small, x, row, &text, style.faint(80));
}

/// The marks on a cover: a bar along its bottom while it is read, a check
/// mark once finished, "New" (or "Next") while never opened.
fn draw_marks(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    at: Rect,
    card: &Card,
    next: bool,
) {
    let pad = (at.width / 16).max(3);
    let reading = matches!(card.progress, Some(p) if p > 0 && p < 100);
    if card.state == FINISHED || card.progress == Some(100) {
        // A black disc with a white check mark, ringed white.
        let r = u32::from((at.width / 9).max(8));
        let cx = i32::from(at.x + at.width) - i32::from(pad) - r as i32;
        let cy = i32::from(at.y + at.height) - i32::from(pad) - r as i32;
        let centre = Point::new(cx, cy);
        canvas.fill_circle(centre, r + 2, Color::WHITE);
        canvas.fill_circle(centre, r, Color::BLACK);
        let s = r as i32;
        let width = (r / 4).max(2);
        canvas.draw_line(
            Point::new(cx - s / 2, cy),
            Point::new(cx - s / 8, cy + s * 3 / 8),
            width,
            Color::WHITE,
        );
        canvas.draw_line(
            Point::new(cx - s / 8, cy + s * 3 / 8),
            Point::new(cx + s / 2, cy - s * 3 / 8),
            width,
            Color::WHITE,
        );
    } else if reading {
        // A bar along the bottom, on white, framed: read at one bit.
        let p = u32::from(card.progress.unwrap_or(0));
        let height = (at.height / 28).max(5);
        let bar = Rect::new(
            at.x + pad,
            at.y + at.height - pad - height,
            at.width.saturating_sub(2 * pad),
            height,
        );
        canvas.fill_round(bar.grow(2), u32::from(height), Color::WHITE);
        canvas.stroke_round(bar, 1, u32::from(height / 2), Color::BLACK);
        let filled = (u32::from(bar.width) * p / 100) as u16;
        canvas.fill_round(
            Rect::new(bar.x, bar.y, filled.max(height), bar.height),
            u32::from(height / 2),
            Color::BLACK,
        );
    } else if card.state == NEW || next {
        let key = if next {
            "files.wall.next"
        } else {
            "files.wall.new"
        };
        draw_tag(canvas, env, style, at, env.i18n.tr(key), true, false);
    }
}

/// A book's number in its series on a cover's top left, black on white.
fn draw_number_tag(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, at: Rect, text: &str) {
    draw_tag(canvas, env, style, at, text, false, true);
}

/// A small label on a cover's bottom left (or top left): white on black
/// (`dark`), or black on white.
fn draw_tag(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    at: Rect,
    text: &str,
    dark: bool,
    top: bool,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let pad = (at.width / 16).max(3);
    let size = style.small * 0.85;
    let w = font.measure(size, text) as u16 + pad * 2;
    let h = font.line_metrics(size).line_height as u16;
    let y = if top {
        at.y + pad
    } else {
        at.y + at.height - pad - h
    };
    let tag = Rect::new(at.x + pad, y, w.min(at.width), h);
    let (paper, ink) = if dark {
        (Color::BLACK, Color::WHITE)
    } else {
        (Color::WHITE, Color::BLACK)
    };
    canvas.fill_round(tag.grow(1), u32::from(h / 2 + 1), Color::WHITE);
    canvas.fill_round(tag, u32::from(h / 2), paper);
    if !dark {
        canvas.stroke_round(tag, 1, u32::from(h / 2), Color::BLACK);
    }
    ui::draw_label(canvas, &font, size, f32::from(tag.x + pad), tag, text, ink);
}

/// A bar of how far a book is read and the percent beside it (or
/// "Finished", or "New"), in `row`.
fn draw_progress_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    row: Rect,
    progress: Option<u8>,
    ink: Color,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let label = match progress {
        Some(100) => env.i18n.tr("files.finished").to_string(),
        Some(p) if p > 0 => format!("{p} %"),
        _ => env.i18n.tr("files.wall.not-started").to_string(),
    };
    let w = font.measure(style.small, &label);
    let x = f32::from(row.x + row.width) - w;
    ui::draw_label(canvas, &font, style.small, x, row, &label, ink);
    let p = u32::from(progress.unwrap_or(0).min(100));
    let height = (style.gap as u16 / 2).max(6);
    let width = (x as u16).saturating_sub(row.x + style.gap as u16);
    let bar = Rect::new(row.x, row.y + (row.height - height) / 2, width, height);
    let radius = u32::from(height / 2);
    canvas.stroke_round(bar, 1, radius, ink);
    let filled = (u32::from(bar.width) * p / 100) as u16;
    if filled > 0 {
        let fill = Rect::new(bar.x, bar.y, filled.max(height), bar.height);
        canvas.fill_round(fill, radius, ink);
    }
}

/// A stack of books: two covers' edges behind the top one, its name and
/// how many books it holds.
#[allow(clippy::too_many_arguments)]
fn draw_stack(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    at: Rect,
    item: &Item,
    card: &Card,
    plate: bool,
) {
    let lift = stack_lift(at.width);
    let (front, _) = stack_rects(at, card);
    let radius = crate::cover_radius(style, front);
    let border = style.line.max(2);
    for step in [2, 1] {
        let behind = Rect::new(
            front.x + step * lift,
            front.y - step * lift,
            front.width,
            front.height,
        );
        canvas.fill_round(behind, radius, Color::WHITE);
        canvas.stroke_round(behind, border, radius, Color::BLACK);
    }
    draw_picture(canvas, env, style, front, card);
    if card.thumb.is_some() {
        canvas.stroke_round(front, 1, radius, Color::BLACK);
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let pad = (front.width / 16).max(3);
    let count = env.i18n.plural("files.books", i64::from(item.count), &[]);
    if !plate {
        draw_tag(canvas, env, style, front, &count, true, false);
        return;
    }
    // The name on a plate across the bottom of the top cover, a little
    // smaller than notes so that it leaves the cover seen.
    let size = style.small * 0.9;
    let small = font.line_metrics(size).line_height as u16;
    let inner = front.width.saturating_sub(4 * pad);
    let lines: Vec<String> = font
        .wrap(size, &card.name, f32::from(inner))
        .into_iter()
        .take(2)
        .map(|r| card.name[r].to_string())
        .collect();
    let plate_h = small * (lines.len() as u16 + 1) + pad * 2;
    let plate = Rect::new(
        front.x + pad,
        front.y + front.height - pad - plate_h,
        front.width - 2 * pad,
        plate_h,
    );
    let r = crate::cover_radius(style, plate);
    canvas.fill_round(plate, r, Color::WHITE);
    canvas.stroke_round(plate, border, r, Color::BLACK);
    let mut y = plate.y + pad;
    for text in &lines {
        let row = Rect::new(plate.x + pad, y, inner, small);
        let text = ui::fit(&font, size, text, f32::from(inner));
        ui::draw_label(
            canvas,
            &font,
            size,
            f32::from(row.x),
            row,
            &text,
            Color::BLACK,
        );
        y += small;
    }
    let row = Rect::new(plate.x + pad, y, inner, small);
    ui::draw_label(
        canvas,
        &font,
        size,
        f32::from(row.x),
        row,
        &count,
        style.faint(80),
    );
}

impl Screen for Wall {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let pages = self.pages();
        let title = self.title(env);
        let paging = Some((self.page, pages));
        // On touch screens a search beside the title: room kept for it as
        // the header's note, the icon drawn there.
        let search = self.search_spot(env);
        let room = search.as_ref().map(|(_, spaces)| spaces.as_str());
        self.header.draw(canvas, env, &style, &title, room, paging);
        if let Some((spot, _)) = &search {
            let size = style.title;
            let w = ui::icon_width(env, "search", size);
            let x = f32::from(spot.x) + (f32::from(spot.width) - w) / 2.0;
            let ink = style.header_colors().1;
            ui::draw_icon(canvas, env, "search", size, x, *spot, ink);
        }
        let geo = &self.geo;
        let display = env.device.display;
        let below = Rect::new(
            0,
            geo.top,
            display.width,
            display.height.saturating_sub(geo.top),
        );
        let top = self.has_top() && self.page == 0;
        let Some(index) = &self.index else {
            if top {
                self.draw_strip(canvas, env);
            }
            let found = match &self.work {
                Work::Walk(walk) => walk.found(),
                Work::Build(build) => build.found(),
                Work::Idle => 0,
            };
            let message = env.i18n.plural("files.collecting", found as i64, &[]);
            let from = if top && !self.strip.is_empty() {
                geo.first_top
            } else {
                geo.top
            };
            let area = Rect::new(0, from, display.width, geo.bottom.saturating_sub(from));
            ui::draw_message(canvas, env, &style, area, &message);
            return;
        };
        if index.lines.is_empty() && self.scope == Scope::Library {
            let text = env
                .i18n
                .format("files.empty", &[("dir", Arg::Str(&self.root))]);
            let title = env.i18n.tr("files.empty.title");
            ui::draw_empty(canvas, env, &style, below, "books", title, &text);
            return;
        }
        if top {
            self.draw_strip(canvas, env);
            self.draw_pills(canvas, env);
        }
        if self.view.is_empty() {
            let from = if top { geo.first_top } else { geo.top };
            let area = Rect::new(0, from, display.width, geo.bottom.saturating_sub(from));
            let key = match &self.scope {
                Scope::Library => format!("files.wall.none.{}", FILTERS[self.filter]),
                _ => String::from("files.search.none"),
            };
            let text = ui::tr_input(env, &key);
            ui::draw_message(canvas, env, &style, area, text);
        }
        let range = self.range(self.page);
        let ring = self.ring();
        let series = matches!(self.scope, Scope::Series(_));
        let font = style.font(env);
        for (place, card) in range.zip(&self.cards) {
            let item = &self.view[place];
            let at = self.cover_rect(place);
            let focused = self.shown && self.zone == Zone::Grid && place == self.focus;
            let label = if item.kind == Kind::Book {
                &card.title
            } else {
                &card.name
            };
            canvas.describe(
                paperpaw_sdk::semantics::Role::Button,
                label,
                at,
                focused,
                None,
            );
            let shown = match item.kind {
                Kind::Book => picture_rect(at, card, false),
                _ => stack_rects(at, card).1,
            };
            if focused {
                draw_focus_box(canvas, &style, shown.grow(ring));
            }
            match item.kind {
                Kind::Book => {
                    draw_picture(canvas, env, &style, shown, card);
                    draw_marks(canvas, env, &style, shown, card, item.next);
                    if series && !card.number.is_empty() {
                        // Its number, top left: a series reads in order.
                        let text = env
                            .i18n
                            .format("files.series-number", &[("number", Arg::Str(&card.number))]);
                        draw_number_tag(canvas, env, &style, shown, &text);
                    }
                }
                _ => draw_stack(canvas, env, &style, at, item, card, !geo.titles),
            }
            if geo.titles
                && let Some(font) = &font
            {
                let ink = style.focus_ink(false);
                let row = Rect::new(
                    at.x,
                    at.y + at.height + style.gap as u16 / 2,
                    at.width,
                    geo.small_line,
                );
                ui::draw_label(canvas, font, style.small, f32::from(at.x), row, label, ink);
            }
        }
        if let Some(caption) = geo.caption {
            self.draw_caption(canvas, env, caption);
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if core::mem::take(&mut self.hidden) {
            // Back from a book: what was read last and how far changed.
            if let Some(index) = &mut self.index {
                index.rerank(env.storage);
                for path in library::opened_books(env.storage).iter().take(1) {
                    index.refresh(env.storage, path);
                }
            }
            self.load_strip(&env);
            self.rebuild(&env);
            cx.request_redraw();
        }
        if !matches!(self.work, Work::Idle) {
            cx.wake_at(env.now);
        }
    }

    fn on_hide(&mut self) {
        self.drop_covers();
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match &mut self.work {
            Work::Idle => return,
            Work::Walk(walk) => {
                if walk.step(&env) {
                    self.work = Work::Build(Build::start(env.storage));
                }
                if self.index.is_none() {
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                }
            }
            Work::Build(build) => {
                if build.step(&env) {
                    let Work::Build(build) = core::mem::replace(&mut self.work, Work::Idle) else {
                        return;
                    };
                    let first = self.index.is_none();
                    self.index = Some(build.index);
                    if let (Some(start), Some(index)) = (self.start.take(), &mut self.index) {
                        self.trail.push((Scope::Library, 0));
                        self.scope = match start {
                            Start::Folder(dir) => Scope::Folder(index.dir(&dir)),
                            Start::Search(words) => search(env.storage, words),
                            Start::Letter(letter) => Scope::Letter(letter),
                        };
                        self.zone = Zone::Grid;
                    }
                    if first && self.has_top() {
                        self.zone = self.home_zone();
                    }
                    self.rebuild(&env);
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Partial);
                    return;
                }
            }
        }
        cx.wake_at(env.now.add_millis(STEP_DELAY_MS));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let touch = self.touch;
        if *a == actions::NEXT_PAGE || *a == actions::PREV_PAGE {
            let next = *a == actions::NEXT_PAGE;
            if touch {
                let page = if next {
                    self.page + 1
                } else {
                    self.page.saturating_sub(1)
                };
                self.turn(page, cx);
            } else if next {
                self.down(cx);
            } else {
                self.up(cx);
            }
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            self.forward(cx);
        } else if [actions::LEFT, actions::PREVIOUS].contains(a) {
            self.backward(cx);
        } else if *a == actions::DOWN {
            match self.zone {
                Zone::Grid => {
                    let to = self.focus + usize::from(self.geo.columns);
                    if to < self.view.len() {
                        self.go(Zone::Grid, to, cx);
                    }
                }
                _ => self.down(cx),
            }
        } else if *a == actions::UP {
            let columns = usize::from(self.geo.columns);
            match self.zone {
                Zone::Grid if self.focus >= columns => {
                    self.go(Zone::Grid, self.focus - columns, cx)
                }
                _ => self.up(cx),
            }
        } else if *a == actions::CONFIRM {
            match self.zone {
                Zone::Strip(i) => {
                    if let Some(card) = self.strip.get(i) {
                        let path = card.path.clone();
                        self.read(path, cx);
                    }
                }
                Zone::Pills if !self.view.is_empty() => self.go(Zone::Grid, 0, cx),
                Zone::Pills => {}
                Zone::Grid => self.open(self.focus, cx),
            }
        } else if *a == actions::MENU {
            match self.focused_book().map(String::from) {
                Some(path) => self.offer(&path, cx),
                None => Self::open_menu(cx),
            }
        } else if *a == actions::BACK {
            return if self.back(cx) {
                Handled::Yes
            } else {
                Handled::No
            };
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let point = gesture.start;
        let top = self.has_top() && self.page == 0 && self.index.is_some();
        // What a finger is on: a book of the strip, a filter, a place.
        let strip = if top || (self.has_top() && self.page == 0) {
            let rects = self.strip_rects();
            match rects.recents.iter().position(|r| r.contains(point)) {
                Some(i) => Some(i + 1),
                None => rects.card.contains(point).then_some(0),
            }
        } else {
            None
        }
        .filter(|i| *i < self.strip.len());
        let place = (0..self.cards.len())
            .map(|slot| self.range(self.page).start + slot)
            .find(|place| {
                let at = self.cover_rect(*place);
                let below = if self.geo.titles {
                    self.geo.small_line + self.style.gap as u16
                } else {
                    0
                };
                Rect::new(at.x, at.y, at.width, at.height + below).contains(point)
            });
        if gesture.kind == GestureKind::LongPress {
            let path = match (strip, place) {
                (Some(i), _) => self.strip.get(i).map(|c| c.path.clone()),
                (_, Some(place)) if self.view[place].kind == Kind::Book => {
                    self.card_at(place).map(|c| c.path.clone())
                }
                _ => None,
            };
            return match path {
                Some(path) => {
                    self.offer(&path, cx);
                    Handled::Yes
                }
                None => Handled::No,
            };
        }
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = Some((self.page, self.pages()));
        if let Some(action) = self.header.tap(cx.env(), &self.style, point, paging) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let search = self.search_spot(cx.env());
        if let Some((spot, _)) = search
            && spot.grow(self.style.gap as u16).contains(point)
        {
            cx.command(Command::RunAction(SEARCH));
            return Handled::Yes;
        }
        if self.header.area.contains(point) {
            Self::open_menu(cx);
            return Handled::Yes;
        }
        if let Some(i) = strip {
            let path = self.strip[i].path.clone();
            self.read(path, cx);
            return Handled::Yes;
        }
        if top && let Some(pill) = self.geo.pills.iter().position(|r| r.contains(point)) {
            if pill >= FILTERS.len() {
                cx.command(Command::RunAction(SEARCH));
            } else if pill != self.filter {
                let env = *cx.env();
                self.set_filter(pill, &env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            return Handled::Yes;
        }
        match place {
            Some(place) => {
                self.open(place, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(changed) = BOOK_CHANGED.read(event, params) {
            if let Some(index) = &mut self.index {
                index.rerank(env.storage);
                index.refresh(env.storage, &changed.path);
            }
            self.load_strip(&env);
            self.rebuild(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        if let Some(chosen) = LETTER_CHOSEN.read(event, params) {
            let letter = chosen.path.bytes().next().unwrap_or(b'#');
            self.enter(Scope::Letter(letter), true, cx);
            return;
        }
        if let Some(typed) = SEARCHED.read(event, params) {
            let words = typed.text.trim().to_string();
            if !words.is_empty() {
                self.enter(search(env.storage, words), true, cx);
            }
            return;
        }
        if *event == events::LIBRARY_CHANGED {
            // Books came or went: the catalog anew, then the index.
            self.work = Work::Walk(catalog::Walk::start(env.storage, &self.root));
            cx.wake_at(env.now);
            return;
        }
        if *event == events::SETTING_CHANGED {
            // The UI size, the order or the library folder may have
            // changed: everything anew.
            *self = Self::new(&env, &Params::new());
            cx.request_redraw();
            cx.wake_at(env.now);
        }
    }
}
