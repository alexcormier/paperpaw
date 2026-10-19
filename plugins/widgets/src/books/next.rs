//! "Next up": a book to read next, as the reader chooses: the next one in
//! the series of the book being read, the newest unread one, or an unread
//! one by chance (another each day); with its cover and title.
//!
//! Which books are new the library does not say: this widget keeps the
//! books it saw (their hashes, `seen.bin`) and the ones that came since
//! (`newest.txt`, a few) in its folder, from the catalog when it changed.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::library;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{Arg, Canvas, Command, Env, Id, Rect, RegisterError, Registrar, Storage};

use super::series::{Member, members};
use super::{
    Book, Generation, Lines, add_settings, catalog_stamp, chosen, cover_size, current, draw_cover,
    draw_message, each_book, frame_height, mix, path_hash, text_size, today,
};
use crate::PLUGIN;
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.next";
/// How the book is chosen.
const PICKS: [&str; 3] = [
    "widgets.next.series",
    "widgets.next.newest",
    "widgets.next.random",
];
/// Unread books chosen by chance looked at, at most, to find one unread.
const CANDIDATES: usize = 6;
/// Books whose hashes are kept as seen, at most.
const MAX_SEEN: usize = 4096;
/// New books remembered, at most.
const MAX_NEWEST: usize = 16;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 8] = [
    widgets::align_spec("widgets.next.align", ID, Align::Left),
    widgets::frame_spec("widgets.next.frame", ID, Frame::None),
    widgets::size_spec("widgets.next.size", ID, Size::Medium, false),
    SettingSpec::fixed_choice(
        "widgets.next.pick",
        "widgets.books.setting.pick",
        &PICKS,
        PICKS[0],
        ID,
    )
    .described("widgets.books.setting.pick.help"),
    widgets::part_spec(
        "widgets.next.caption",
        ID,
        "widgets.books.part.caption",
        true,
    ),
    widgets::part_spec("widgets.next.cover", ID, "widgets.books.part.cover", true),
    widgets::part_spec("widgets.next.author", ID, "widgets.books.part.author", true),
    widgets::part_spec("widgets.next.series", ID, "widgets.books.part.series", true),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    add_settings(r, &ID, "widgets.next", &SETTINGS)?;
    let kept: Kept = Rc::default();
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.next", move |_| {
            Box::new(Next {
                kept: kept.clone(),
                generation: generation.clone(),
                book: None,
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The book chosen, and what it was chosen for (the way, the day, the
/// library's generation, the catalog, the book being read).
type Kept = Rc<RefCell<Option<(Key, Option<String>)>>>;
type Key = (String, i64, u32, u64, String);

/// Whether the book at `path` was not begun.
fn unread(storage: &dyn Storage, path: &str) -> bool {
    match library::book(storage, path) {
        Some(book) => book.progress.unwrap_or(0) == 0,
        None => storage.exists(path),
    }
}

/// The book after `current` in its series (in order) not read yet.
pub(crate) fn next_in_series<'m>(members: &'m [Member], current: &str) -> Option<&'m Member> {
    let at = members.iter().position(|m| m.path == current)?;
    members[at + 1..].iter().find(|m| !m.read)
}

/// The order books are tried in by chance on `day`: by a number that
/// looks random, another each day.
pub(crate) fn chance(path: &str, day: i64) -> u64 {
    mix(path_hash(path) ^ mix(day as u64))
}

/// Keeps the `n` lowest of `(score, path)` in `kept`, lowest first.
pub(crate) fn keep_lowest(kept: &mut Vec<(u64, String)>, n: usize, score: u64, path: &str) {
    if kept.len() == n && kept.last().is_some_and(|(s, _)| *s <= score) {
        return;
    }
    let at = kept.partition_point(|(s, _)| *s <= score);
    kept.insert(at, (score, path.to_string()));
    kept.truncate(n);
}

/// An unread book by chance, another each day; not one opened lately.
fn by_chance(env: &Env<'_>, day: i64) -> Option<String> {
    let opened: Vec<u64> = {
        let mut hashes: Vec<u64> = library::opened_books(env.storage)
            .iter()
            .map(|p| path_hash(p))
            .collect();
        hashes.sort_unstable();
        hashes
    };
    let mut kept = Vec::new();
    each_book(env, &mut |record| {
        if opened.binary_search(&path_hash(&record.path)).is_err() {
            keep_lowest(
                &mut kept,
                CANDIDATES,
                chance(&record.path, day),
                &record.path,
            );
        }
    });
    kept.into_iter()
        .map(|(_, path)| path)
        .find(|path| unread(env.storage, path))
}

/// The next book in the series of `book`, if there is one not read.
fn in_series(env: &Env<'_>, book: &Book) -> Option<String> {
    if book.series.is_empty() {
        return None;
    }
    let mut members = members(env, &book.series);
    for member in &mut members {
        member.read = !unread(env.storage, &member.path);
    }
    next_in_series(&members, &book.path).map(|m| m.path.clone())
}

fn seen_file() -> String {
    format!(
        "{}/seen.bin",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

fn newest_file() -> String {
    format!(
        "{}/newest.txt",
        paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str())
    )
}

/// The books new since they were last looked at, newest first: added to
/// those kept, `seen` the hashes of the books seen before (sorted, `None`
/// the first time: then none counts as new).
pub(crate) fn new_books(
    seen: Option<&[u64]>,
    books: &[(u64, String)],
    newest: Vec<String>,
) -> Vec<String> {
    let Some(seen) = seen else {
        return newest;
    };
    let mut list: Vec<String> = books
        .iter()
        .filter(|(hash, _)| seen.binary_search(hash).is_err())
        .map(|(_, path)| path.clone())
        .collect();
    for path in newest {
        if !list.contains(&path) {
            list.push(path);
        }
    }
    list.truncate(MAX_NEWEST);
    list
}

/// Brings the books seen and the newest up to date with the library;
/// returns the newest, newest first.
fn newest(env: &Env<'_>) -> Vec<String> {
    let storage = env.storage;
    let seen: Option<Vec<u64>> = paperpaw_sdk::read_file(storage, &seen_file(), MAX_SEEN * 8)
        .ok()
        .map(|data| {
            data.as_chunks::<8>()
                .0
                .iter()
                .map(|b| u64::from_le_bytes(*b))
                .collect()
        });
    let kept: Vec<String> = paperpaw_sdk::read_file(storage, &newest_file(), MAX_NEWEST * 512)
        .map(|data| {
            String::from_utf8_lossy(&data)
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    // The books now: their hashes, and the paths of those not seen.
    let mut hashes = Vec::new();
    let mut unseen = Vec::new();
    each_book(env, &mut |record| {
        if hashes.len() >= MAX_SEEN {
            return;
        }
        let hash = path_hash(&record.path);
        hashes.push(hash);
        if let Some(seen) = &seen
            && seen.binary_search(&hash).is_err()
            && unseen.len() < MAX_NEWEST
        {
            unseen.push((hash, record.path));
        }
    });
    hashes.sort_unstable();
    let list = new_books(seen.as_deref(), &unseen, kept.clone());
    // Written only when something changed.
    if seen.as_deref() != Some(hashes.as_slice()) {
        let dir = paperpaw_sdk::Env::plugin_dir(PLUGIN.as_str());
        let _ = storage.create_dir(&dir);
        let bytes: Vec<u8> = hashes.iter().flat_map(|h| h.to_le_bytes()).collect();
        let _ = storage.write(&seen_file(), &bytes);
        if list != kept {
            let _ = storage.write(&newest_file(), list.join("\n").as_bytes());
        }
    }
    list
}

/// The newest book not begun.
fn newest_unread(env: &Env<'_>) -> Option<String> {
    newest(env)
        .into_iter()
        .find(|path| unread(env.storage, path))
}

struct Next {
    kept: Kept,
    generation: Generation,
    book: Option<Book>,
}

impl Next {
    fn choose(&self, env: &Env<'_>) -> Option<String> {
        let pick = chosen(env, WIDGET, "pick").to_string();
        let current = current(env.storage);
        let day = today(env);
        let key: Key = (
            pick.clone(),
            day,
            self.generation.get(),
            catalog_stamp(env.storage),
            current.as_ref().map(|b| b.path.clone()).unwrap_or_default(),
        );
        if let Some((k, path)) = self.kept.borrow().as_ref()
            && *k == key
        {
            return path.clone();
        }
        let first = match pick.as_str() {
            "widgets.next.newest" => newest_unread(env),
            "widgets.next.random" => None,
            _ => current.as_ref().and_then(|book| in_series(env, book)),
        };
        // Else by chance.
        let path = first.or_else(|| by_chance(env, day));
        *self.kept.borrow_mut() = Some((key, path.clone()));
        path
    }
}

impl Widget for Next {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = self.choose(env).and_then(|p| Book::of(env.storage, &p));
        let changed = book != self.book;
        self.book = book;
        changed
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        (UiStyle::new(env).row * 3) as u16 + frame_height(env, WIDGET)
    }

    fn min_height(&self, env: &Env<'_>, _: u16) -> u16 {
        (UiStyle::new(env).row * 2) as u16 + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        match &self.book {
            Some(book) => draw(canvas, env, &look, book),
            None => draw_message(canvas, env, &look, "widgets.next.none"),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::RunAction(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget { area, command }]
    }
}

/// "Discworld · 3".
fn series_text(env: &Env<'_>, book: &Book) -> String {
    match (book.series.is_empty(), book.number.is_empty()) {
        (true, _) => String::new(),
        (false, true) => book.series.clone(),
        (false, false) => env.i18n.format(
            "widgets.next.in-series",
            &[
                ("series", Arg::Str(&book.series)),
                ("number", Arg::Str(&book.number)),
            ],
        ),
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, book: &Book) {
    let style = UiStyle::new(env);
    let id = WIDGET;
    let gap = style.gap as u16;
    let area = look.area;
    let align = widgets::align(env, id);
    let mut lines = Lines::new();
    let caption = widgets::shows(env, id, "caption");
    if caption {
        lines.add(style.small, env.i18n.tr("widgets.next.caption"), look.faint);
    }
    lines.add(text_size(env, id), book.title.as_str(), look.ink);
    if widgets::shows(env, id, "author") {
        lines.add(style.small, book.author.as_str(), look.faint);
    }
    if widgets::shows(env, id, "series") {
        lines.add(style.small, series_text(env, book), look.faint);
    }
    if !widgets::shows(env, id, "cover") {
        lines.draw(canvas, env, area, align);
        return;
    }
    let (w, h) = cover_size(book, area.width * 2 / 5, area.height);
    let text_room = f32::from(area.width.saturating_sub(w + gap));
    if area.width >= area.height && text_room >= style.small * 7.0 {
        // Wide: the cover on the left, the text beside it.
        let at = Rect::new(area.x, area.y + (area.height - h) / 2, w, h);
        draw_cover(canvas, env, look, at, book);
        let shift = (w + gap).min(area.width);
        let rest = Rect::new(area.x + shift, area.y, area.width - shift, area.height);
        lines.draw(canvas, env, rest, align);
        return;
    }
    // High: the cover above, the text below it (what fits in half).
    let half = (area.height / 2).saturating_sub(gap / 2);
    if caption && lines.height(env) > half {
        // The title rather than the caption.
        lines.lines.remove(0);
    }
    lines.fit(env, half);
    let text_h = lines.height(env);
    let room = area.height.saturating_sub(text_h + gap / 2);
    let (with_text, room) = if room >= area.height / 3 {
        (true, room)
    } else {
        (false, area.height)
    };
    let (w, h) = cover_size(book, area.width, room);
    let total = if with_text { h + gap / 2 + text_h } else { h };
    let top = area.y + (area.height - total.min(area.height)) / 2;
    let at = Rect::new(super::place_x(area, w, align), top, w, h);
    draw_cover(canvas, env, look, at, book);
    if with_text {
        let text = Rect::new(area.x, top + h + gap / 2, area.width, text_h);
        lines.draw(canvas, env, text, align);
    }
}
