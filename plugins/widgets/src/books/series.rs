//! "Series": the books of a series as covers in a row, in their order,
//! each with its number, those read marked and the one being read
//! underlined. The series is the book being read's, or one the reader
//! names.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::shelf::{self, Place, Shelved};
use super::{
    Book, Generation, add_settings, catalog_stamp, chosen, count, current, draw_message, each_book,
    frame_height, has_catalog, series_rank,
};

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.series";
/// The series shown; empty for the book being read's.
const NAME: Setting<Text> = Setting::new("widgets.series.name");
/// Which of a long series' books show: from the first, or those round
/// the book being read.
const STARTS: [&str; 2] = ["widgets.series.around", "widgets.series.first"];
/// Books of a series looked at, at most.
const MAX_MEMBERS: usize = 60;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 11] = [
    widgets::align_spec("widgets.series.align", ID, Align::Center),
    widgets::frame_spec("widgets.series.frame", ID, Frame::None),
    widgets::size_spec("widgets.series.size", ID, Size::Medium, false),
    SettingSpec::fixed_text(
        "widgets.series.name",
        "widgets.books.setting.series-name",
        "",
        ID,
    )
    .described("widgets.books.setting.series-name.help"),
    SettingSpec::fixed_number(
        "widgets.series.count",
        "widgets.books.setting.count",
        2,
        12,
        6,
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.series.start",
        "widgets.books.setting.series-start",
        &STARTS,
        STARTS[0],
        ID,
    ),
    widgets::part_spec(
        "widgets.series.caption",
        ID,
        "widgets.books.part.caption",
        true,
    ),
    widgets::part_spec(
        "widgets.series.numbers",
        ID,
        "widgets.books.part.numbers",
        true,
    ),
    widgets::part_spec(
        "widgets.series.titles",
        ID,
        "widgets.books.part.titles",
        false,
    ),
    widgets::part_spec("widgets.series.read", ID, "widgets.books.part.read", true),
    widgets::part_spec(
        "widgets.series.current",
        ID,
        "widgets.books.part.current",
        true,
    ),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    add_settings(r, &ID, "widgets.series", &SETTINGS)?;
    let kept: Kept = Rc::default();
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.series", move |_| {
            Box::new(Series {
                kept: kept.clone(),
                generation: generation.clone(),
                shown: Shown::default(),
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// A book of a series as the catalog has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub path: String,
    pub title: String,
    pub number: String,
    /// Read to the end (looked up once the series is read).
    pub read: bool,
}

/// The books of the series `name` (any case), in their order: by number,
/// then title.
pub(crate) fn members(env: &Env<'_>, name: &str) -> Vec<Member> {
    let name = name.trim().to_lowercase();
    let mut found = Vec::new();
    if name.is_empty() {
        return found;
    }
    each_book(env, &mut |record| {
        if found.len() < MAX_MEMBERS && record.series.trim().to_lowercase() == name {
            found.push(Member {
                path: record.path,
                title: record.title,
                number: record.number,
                read: false,
            });
        }
    });
    order(&mut found);
    found
}

/// Puts a series' books in order: by number, then title.
pub(crate) fn order(members: &mut [Member]) {
    members.sort_by(|a, b| {
        series_rank(&a.number)
            .cmp(&series_rank(&b.number))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
}

/// Which `count` of `len` books show: from the first, or with the one at
/// `at` among the first shown (one before it, where there is one).
pub(crate) fn window(
    len: usize,
    count: usize,
    at: Option<usize>,
    around: bool,
) -> core::ops::Range<usize> {
    let count = count.min(len);
    let start = match at {
        Some(at) if around => at.saturating_sub(1).min(len - count),
        _ => 0,
    };
    start..start + count
}

/// What was read of the catalog: the series and its books, and what that
/// was read for (the series, the library's generation, the catalog).
type Kept = Rc<RefCell<Option<((String, u32, u64), Vec<Member>)>>>;

#[derive(Debug, Default, PartialEq)]
struct Shown {
    name: String,
    /// The book being read.
    current: String,
    books: Vec<Shelved>,
    /// Books of the series in all, and read.
    total: usize,
    read: usize,
    /// Why there is nothing: the message's key.
    empty: Option<&'static str>,
}

struct Series {
    kept: Kept,
    generation: Generation,
    shown: Shown,
}

impl Series {
    fn read(&self, env: &Env<'_>) -> Shown {
        let current = current(env.storage);
        let named = env.settings.read(&NAME).trim().to_string();
        let name = if named.is_empty() {
            current
                .as_ref()
                .map(|b| b.series.clone())
                .unwrap_or_default()
        } else {
            named
        };
        if name.is_empty() {
            return Shown {
                empty: Some("widgets.series.none"),
                ..Shown::default()
            };
        }
        if !has_catalog(env.storage) {
            return Shown {
                name,
                empty: Some("widgets.series.no-catalog"),
                ..Shown::default()
            };
        }
        let key = (
            name.to_lowercase(),
            self.generation.get(),
            catalog_stamp(env.storage),
        );
        let mut kept = self.kept.borrow_mut();
        if kept.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut members = members(env, &name);
            for member in &mut members {
                member.read = paperpaw_sdk::library::book(env.storage, &member.path)
                    .is_some_and(|b| b.progress == Some(100));
            }
            *kept = Some((key, members));
        }
        let members = kept.as_ref().map(|(_, m)| m.as_slice()).unwrap_or_default();
        if members.is_empty() {
            return Shown {
                name,
                empty: Some("widgets.series.none"),
                ..Shown::default()
            };
        }
        let id = WIDGET;
        let current_path = current.as_ref().map(|b| b.path.as_str());
        let at = members
            .iter()
            .position(|m| Some(m.path.as_str()) == current_path);
        let around = chosen(env, id, "start") != STARTS[1];
        let shown = window(members.len(), count(env, id, "count"), at, around);
        let numbers = widgets::shows(env, id, "numbers");
        let titles = widgets::shows(env, id, "titles");
        let mut books = Vec::new();
        for member in &members[shown] {
            let Some(book) = Book::of(env.storage, &member.path) else {
                continue;
            };
            let label = match (numbers && !member.number.is_empty(), titles) {
                (true, true) => format!("{} · {}", member.number, book.title),
                (true, false) => member.number.clone(),
                (false, true) => book.title.clone(),
                (false, false) => String::new(),
            };
            books.push(Shelved {
                read: book.finished() && widgets::shows(env, id, "read"),
                current: Some(member.path.as_str()) == current_path
                    && widgets::shows(env, id, "current"),
                label,
                book,
            });
        }
        let read = members.iter().filter(|m| m.read).count();
        Shown {
            name,
            current: current_path.unwrap_or_default().to_string(),
            total: members.len(),
            read,
            books,
            empty: None,
        }
    }

    fn labelled(env: &Env<'_>) -> bool {
        let id = WIDGET;
        widgets::shows(env, id, "numbers") || widgets::shows(env, id, "titles")
    }

    /// Where the books go in `area`, and which of them: where fewer fit
    /// than were read, those round the book being read (or the first).
    fn places(&self, env: &Env<'_>, area: Rect) -> (Vec<Place>, &[Shelved]) {
        let label = if Self::labelled(env) {
            shelf::label_height(env)
        } else {
            0
        };
        let align = widgets::align(env, WIDGET);
        let books = &self.shown.books;
        let places = shelf::places(env, area, books.len(), false, label, align);
        let at = books.iter().position(|b| b.book.path == self.shown.current);
        let around = chosen(env, WIDGET, "start") != STARTS[1];
        let shown = window(books.len(), places.len(), at, around);
        (places, &books[shown])
    }

    /// The area below the caption.
    fn content(&self, env: &Env<'_>, look: &Look) -> Rect {
        if widgets::shows(env, WIDGET, "caption")
            && look.area.height >= 3 * widgets::caption_height(env)
        {
            super::below(look.area, widgets::caption_height(env))
        } else {
            look.area
        }
    }
}

impl Widget for Series {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = self.read(env);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let count = count(env, WIDGET, "count").clamp(1, 12) as u16;
        let style = UiStyle::new(env);
        let cover = (width / count).saturating_sub(style.gap as u16) * 3 / 2;
        cover + shelf::label_height(env) + widgets::caption_height(env) + frame_height(env, WIDGET)
    }

    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        let full = self.height(env, width);
        full / 2
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        if let Some(key) = self.shown.empty {
            draw_message(canvas, env, &look, key);
            return;
        }
        let title = if self.shown.read > 0 {
            env.i18n.format(
                "widgets.series.caption-read",
                &[
                    ("series", Arg::Str(&self.shown.name)),
                    ("read", Arg::Int(self.shown.read as i64)),
                    ("count", Arg::Int(self.shown.total as i64)),
                ],
            )
        } else {
            env.i18n.format(
                "widgets.series.caption",
                &[
                    ("series", Arg::Str(&self.shown.name)),
                    ("count", Arg::Int(self.shown.total as i64)),
                ],
            )
        };
        let rest = self.content(env, &look);
        let (places, books) = self.places(env, rest);
        shelf::draw_caption(canvas, env, WIDGET, &look, rest, &places, &title);
        shelf::draw(canvas, env, &look, &places, books);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::look::look(env, WIDGET, area);
        let rest = self.content(env, &look);
        let (places, books) = self.places(env, rest);
        places
            .into_iter()
            .zip(books)
            .map(|(place, shelved)| WidgetTarget {
                area: place.cover.union(&place.label),
                command: shelved.book.open(),
            })
            .collect()
    }
}
