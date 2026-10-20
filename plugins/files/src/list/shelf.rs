//! A list of books with their covers and states: a series (by number, the
//! next book to read marked and focused), a folder, the favourites, the
//! books under a letter, or what a search found.
//!
//! The books are found in one pass over the catalog when the list opens;
//! only where their lines start is kept (and for a series how far each is
//! read). A page's books are read when it is shown.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::library;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use super::index::{self, hash, letter, read_line, sort_key, surname};
use super::rows::{self, Loaded, Sizes};

/// The parameter naming the series, folder, letter or search words.
const NAME: &str = "name";
/// Books of a series whose progress is read when it opens, at most.
const MAX_SERIES: usize = 200;

/// What a list of books holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Series,
    Folder,
    Favourites,
    /// The authors (the list of them; not a list of books).
    Authors,
    /// The books of an author, of a tag.
    Author,
    Tag,
    Letter,
    Search,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Series => "series",
            Kind::Folder => "folder",
            Kind::Favourites => "favourites",
            Kind::Authors => "authors",
            Kind::Author => "author",
            Kind::Tag => "tag",
            Kind::Letter => "letter",
            Kind::Search => "search",
        }
    }

    fn of(mode: &str) -> Kind {
        [
            Kind::Series,
            Kind::Folder,
            Kind::Favourites,
            Kind::Author,
            Kind::Tag,
            Kind::Letter,
            Kind::Search,
        ]
        .into_iter()
        .find(|k| k.as_str() == mode)
        .unwrap_or(Kind::Favourites)
    }
}

/// The command that opens the list of `kind` named `name`.
pub(crate) fn open(kind: Kind, name: &str) -> Command {
    Command::OpenScreen(
        super::SHELF,
        Params::new()
            .with(params::MODE, kind.as_str())
            .with(NAME, name.to_string()),
    )
}

/// A book of the list.
#[derive(Debug, Clone, Copy)]
struct Entry {
    offset: u32,
    key: [u8; 4],
    /// Its number in a series.
    number: f32,
    /// How far it is read (series only, else as its page read it).
    progress: u8,
}

pub(crate) struct ShelfScreen {
    style: UiStyle,
    sizes: Sizes,
    header: ui::Header,
    kind: Kind,
    name: String,
    entries: Vec<Entry>,
    /// The series' next book to read.
    next: Option<usize>,
    /// Hashes of the books opened last (others are new).
    opened: Vec<u32>,
    /// The line below the header: how many books, how many finished.
    summary: Rect,
    list: ListView,
    loaded: Vec<Loaded>,
    loaded_page: Option<usize>,
    fresh: bool,
}

impl ShelfScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let kind = Kind::of(params.get(params::MODE).unwrap_or_default());
        let name = params.get(NAME).unwrap_or_default().to_string();
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let summary = Rect::new(
            0,
            header.area.height,
            display.width,
            sizes.small_line + style.gap as u16,
        );
        let top = summary.y + summary.height;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let opened = library::opened_books(env.storage)
            .iter()
            .map(|p| hash(p))
            .collect();
        let mut screen = Self {
            list: ListView::new(area, u32::from(sizes.row)).focus_on_keys(env),
            entries: find(env, kind, &name),
            next: None,
            opened,
            summary,
            loaded: Vec::new(),
            loaded_page: None,
            fresh: true,
            kind,
            name,
            style,
            sizes,
            header,
        };
        screen.mark_next();
        screen.list.focus = screen.next.unwrap_or(0);
        screen.load_page(env);
        screen
    }

    /// The series' next book: the first not begun, after the finished
    /// and the one being read.
    fn mark_next(&mut self) {
        self.next = (self.kind == Kind::Series)
            .then(|| self.entries.iter().position(|e| e.progress == 0))
            .flatten();
    }

    fn load_page(&mut self, env: &Env<'_>) {
        let page = self.list.page();
        if self.loaded_page == Some(page) {
            return;
        }
        self.loaded = Vec::new();
        let rows = self.list.rows();
        let mut loaded = Vec::new();
        for entry in self.entries.iter_mut().skip(page * rows).take(rows) {
            let Some(line) = read_line(env.storage, entry.offset) else {
                continue;
            };
            let opened = self.opened.contains(&hash(&line.path));
            let book = Loaded::load(env, line, opened, self.sizes.cover, false);
            entry.progress = book.progress;
            loaded.push(book);
        }
        self.loaded = loaded;
        self.loaded_page = Some(page);
    }

    fn book(&self, index: usize) -> Option<&Loaded> {
        let rows = self.list.rows();
        (self.loaded_page == Some(index / rows))
            .then(|| self.loaded.get(index % rows))
            .flatten()
    }

    fn title(&self, env: &Env<'_>) -> String {
        match self.kind {
            Kind::Favourites => env.i18n.tr("files.favourites").to_string(),
            Kind::Search => env
                .i18n
                .format("files.search.title", &[("text", Arg::Str(&self.name))]),
            Kind::Letter => env
                .i18n
                .format("files.list.letter", &[("letter", Arg::Str(&self.name))]),
            Kind::Folder => paperpaw_sdk::files::name(&self.name).to_string(),
            Kind::Author if self.name.is_empty() => env.i18n.tr("files.unknown-author").to_string(),
            _ => self.name.clone(),
        }
    }

    fn open_book(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(book) = self.book(index) {
            let path = book.path.clone();
            super::read(&path, cx);
        }
    }
}

/// The books of the list `kind` named `name`, in its order: a series by
/// number, the others by title.
fn find(env: &Env<'_>, kind: Kind, name: &str) -> Vec<Entry> {
    let storage = env.storage;
    let favourites: Vec<u32> = if kind == Kind::Favourites {
        library::favourites(storage)
            .iter()
            .map(|p| hash(p))
            .collect()
    } else {
        Vec::new()
    };
    let folder = alloc::format!("{name}/");
    let words: Vec<String> = name
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let wanted = name.bytes().next().unwrap_or(b'#');
    let mut entries = Vec::new();
    index::scan(storage, &mut |offset, line| {
        let found = match kind {
            Kind::Series => line.series == name,
            Kind::Folder => line.path.starts_with(&folder),
            Kind::Favourites => favourites.contains(&hash(&line.path)),
            Kind::Author => line.author == name,
            Kind::Tag => line.tags.split('\u{1f}').any(|t| t == name),
            Kind::Letter => {
                let title = sort_key(&line.title)[0];
                let title = if title.is_ascii_lowercase() {
                    title.to_ascii_uppercase()
                } else {
                    b'#'
                };
                title == wanted || letter(surname(&line.author)) == wanted
            }
            Kind::Search => {
                let text = alloc::format!(
                    "{} {} {} {}",
                    line.title,
                    line.author,
                    line.series,
                    paperpaw_sdk::files::name(&line.path)
                )
                .to_lowercase();
                !words.is_empty() && words.iter().all(|w| text.contains(w.as_str()))
            }
            Kind::Authors => false,
        };
        if found {
            entries.push(Entry {
                offset,
                key: sort_key(&line.title),
                number: line.number.parse().unwrap_or(f32::MAX),
                progress: 0,
            });
        }
    });
    if kind == Kind::Series {
        entries.sort_by(|a, b| a.number.total_cmp(&b.number).then(a.key.cmp(&b.key)));
        for entry in entries.iter_mut().take(MAX_SERIES) {
            entry.progress = read_line(storage, entry.offset)
                .and_then(|line| library::book(storage, &line.path))
                .and_then(|book| book.progress)
                .unwrap_or(0);
        }
    } else {
        entries.sort_by_key(|e| (e.key, e.offset));
    }
    entries
}

impl Screen for ShelfScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.entries.len();
        let title = self.title(env);
        self.header
            .draw(canvas, env, &style, &title, None, self.list.paging(len));
        let Some(font) = style.font(env) else {
            return;
        };
        // How many books, and for a series how many are finished.
        let books = env.i18n.plural("files.books", len as i64, &[]);
        let finished = self.entries.iter().filter(|e| e.progress == 100).count();
        let summary = if self.kind == Kind::Series && finished > 0 {
            let done = env
                .i18n
                .plural("files.list.finished.count", finished as i64, &[]);
            alloc::format!("{books} · {done}")
        } else {
            books
        };
        let x = style.margin as f32;
        let faint = style.faint(80);
        ui::draw_label(canvas, &font, style.small, x, self.summary, &summary, faint);
        if len == 0 {
            let text = match self.kind {
                Kind::Favourites => ui::tr_input(env, "files.no-favourites"),
                _ => env.i18n.tr("files.search.none"),
            };
            ui::draw_empty(canvas, env, &style, self.list.area, "books", "", text);
            return;
        }
        let series = self.kind == Kind::Series;
        let next = self.next;
        let sizes = self.sizes;
        let rows = self.list.rows();
        let (loaded, loaded_page) = (&self.loaded, self.loaded_page);
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                if loaded_page != Some(index / rows) {
                    return;
                }
                let Some(book) = loaded.get(index % rows) else {
                    return;
                };
                let (note, tail) = if series {
                    (book.series_note(env), String::new())
                } else {
                    (book.author.clone(), book.series_label())
                };
                let is_next = next == Some(index);
                rows::draw_book_row(
                    canvas, env, &style, &sizes, rect, focused, book, &note, &tail, is_next,
                );
            });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if core::mem::take(&mut self.fresh) {
            return;
        }
        // Back from a book or its actions: states and the next may have
        // changed.
        let env = *cx.env();
        let focus = self.list.focus;
        self.entries = find(&env, self.kind, &self.name);
        self.mark_next();
        self.list.focus = focus.min(self.entries.len().saturating_sub(1));
        self.loaded_page = None;
        self.load_page(&env);
        cx.request_redraw();
    }

    fn on_hide(&mut self) {
        self.loaded = Vec::new();
        self.loaded_page = None;
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let len = self.entries.len();
        if let Some(change) = self.list.navigate(a, len) {
            if change == ListChange::Page {
                let env = *cx.env();
                self.load_page(&env);
            }
            self.list.request(change, cx);
        } else if *a == actions::CONFIRM {
            self.open_book(self.list.focus, cx);
        } else if *a == actions::MENU {
            if let Some(book) = self.book(self.list.focus) {
                let path = book.path.clone();
                super::offer(&path, cx);
            }
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.entries.len();
        let hit = self.list.hit(gesture.start, len);
        match gesture.kind {
            GestureKind::LongPress => {
                let Some(book) = hit.and_then(|i| self.book(i)) else {
                    return Handled::No;
                };
                let path = book.path.clone();
                super::offer(&path, cx);
                Handled::Yes
            }
            GestureKind::Tap => {
                let paging = self.list.paging(len);
                if let Some(action) = self
                    .header
                    .tap(cx.env(), &self.style, gesture.start, paging)
                {
                    cx.command(Command::RunAction(action));
                    return Handled::Yes;
                }
                match hit {
                    Some(index) => {
                        self.list.focus = index;
                        self.open_book(index, cx);
                        Handled::Yes
                    }
                    None => Handled::No,
                }
            }
            _ => Handled::No,
        }
    }
}
