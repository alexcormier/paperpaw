//! "Highlight": one of one's own highlights, from the books read lately
//! or from all, another each day (or each time it shows): the text in
//! quotes, the book's title below. A tap opens the book at its place.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use quireink_sdk::annotations::{self, Kind};
use quireink_sdk::builtin::events::{self, Goto};
use quireink_sdk::document::Locator;
use quireink_sdk::event::Event;
use quireink_sdk::library;
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, Storage};

use super::{
    Generation, add_settings, chosen, draw_line, draw_message, each_book, frame_height, mix, open,
    path_hash, today,
};
use quireink_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.highlight";
/// Opens a book at a place: the book, then the place in it.
const OPEN_AT: Event<Goto> = Event::new("widgets.open-at");
/// Where highlights come from: the books read lately, or all.
const SOURCES: [&str; 2] = ["widgets.highlight.recent", "widgets.highlight.all"];
/// When another shows: each day, or each time the widget shows.
const CHANGES: [&str; 2] = ["widgets.highlight.daily", "widgets.highlight.shown"];
/// Books read lately looked at.
const RECENT_BOOKS: usize = 12;
/// Books with annotations looked at, at most.
const MAX_BOOKS: usize = 200;
/// The largest file of a book's annotations read: it is read whole.
const MAX_FILE: u64 = 128 * 1024;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 10] = [
    widgets::align_spec("widgets.highlight.align", ID, Align::Center),
    widgets::frame_spec("widgets.highlight.frame", ID, Frame::None),
    widgets::size_spec("widgets.highlight.size", ID, Size::Fit, true),
    SettingSpec::fixed_choice(
        "widgets.highlight.source",
        "widgets.books.setting.highlight-source",
        &SOURCES,
        SOURCES[0],
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.highlight.change",
        "widgets.books.setting.highlight-change",
        &CHANGES,
        CHANGES[0],
        ID,
    ),
    widgets::part_spec(
        "widgets.highlight.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
    widgets::part_spec(
        "widgets.highlight.marks",
        ID,
        "widgets.books.part.marks",
        true,
    ),
    widgets::part_spec(
        "widgets.highlight.title",
        ID,
        "widgets.books.part.title",
        true,
    ),
    widgets::part_spec(
        "widgets.highlight.author",
        ID,
        "widgets.books.part.author",
        false,
    ),
    widgets::part_spec(
        "widgets.highlight.note",
        ID,
        "widgets.books.part.note",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    r.on(OPEN_AT, |_, goto, out| {
        out.push(open(&goto.path));
        out.push(events::GOTO.emit(goto.clone()));
    })?;
    add_settings(r, &ID, "widgets.highlight", &SETTINGS)?;
    let kept: Kept = Rc::default();
    let shows = Rc::new(Cell::new(0u64));
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.highlight", move |_| {
            Box::new(HighlightWidget {
                kept: kept.clone(),
                shows: shows.clone(),
                generation: generation.clone(),
                quote: None,
            })
        })
        .category(widgets::categories::WORDS),
    )
}

/// A highlight as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quote {
    pub path: String,
    pub title: String,
    pub author: String,
    pub text: String,
    pub note: String,
    pub start: Locator,
    pub end: Locator,
}

/// The highlight chosen and what for (the source, the seed, the
/// library's generation).
type Kept = Rc<RefCell<Option<((String, u64, u32), Option<Quote>)>>>;

/// Whether the book at `path` has annotations not too large to read.
fn annotated(storage: &dyn Storage, path: &str) -> bool {
    storage
        .size(&annotations::file(path))
        .is_ok_and(|size| size > 0 && size <= MAX_FILE)
}

/// The books highlights are chosen from: those read lately with
/// annotations, or every book of the library with annotations.
fn books(env: &Env<'_>, all: bool) -> Vec<String> {
    let mut books: Vec<String> = library::opened_books(env.storage)
        .into_iter()
        .take(RECENT_BOOKS)
        .filter(|path| annotated(env.storage, path))
        .collect();
    if !all {
        return books;
    }
    // The books with annotations: their files are named by the hashes
    // of the books' paths.
    let file = annotations::file("");
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut hashes = Vec::new();
    let _ = env.storage.list(dir, &mut |entry| {
        let hash = entry
            .name
            .strip_suffix(".txt")
            .and_then(|h| u64::from_str_radix(h, 16).ok());
        if let Some(hash) = hash
            && entry.size <= MAX_FILE
            && hashes.len() < MAX_BOOKS * 4
        {
            hashes.push(hash);
        }
    });
    hashes.sort_unstable();
    each_book(env, &mut |record| {
        if books.len() < MAX_BOOKS
            && hashes.binary_search(&path_hash(&record.path)).is_ok()
            && !books.contains(&record.path)
        {
            books.push(record.path);
        }
    });
    books
}

/// The highlights of a book with text, as (start, end, text, note).
pub(crate) type Found = (Locator, Locator, String, String);

/// Picks by `seed` one of `books`, and one of its highlights; books
/// without any are passed over. `highlights` reads a book's.
pub(crate) fn pick(
    books: &[String],
    seed: u64,
    highlights: &mut dyn FnMut(&str) -> Vec<Found>,
) -> Option<(String, Found)> {
    let n = books.len();
    if n == 0 {
        return None;
    }
    let start = (mix(seed) % n as u64) as usize;
    for i in 0..n {
        let path = &books[(start + i) % n];
        let mut found = highlights(path);
        if found.is_empty() {
            continue;
        }
        let at = (mix(seed ^ 0x5851_f42d_4c95_7f2d) % found.len() as u64) as usize;
        return Some((path.clone(), found.swap_remove(at)));
    }
    None
}

/// The highlights of the book at `path` with text.
fn highlights_of(storage: &dyn Storage, path: &str) -> Vec<Found> {
    annotations::load(storage, path)
        .into_iter()
        .filter(|a| a.mark.kind == Kind::Highlight && !a.text.trim().is_empty())
        .map(|a| (a.mark.start, a.mark.end, a.text, a.note))
        .collect()
}

struct HighlightWidget {
    kept: Kept,
    /// Times it was shown, for one each time.
    shows: Rc<Cell<u64>>,
    generation: Generation,
    quote: Option<Quote>,
}

impl HighlightWidget {
    fn choose(&self, env: &Env<'_>) -> Option<Quote> {
        let id = WIDGET;
        let source = chosen(env, id, "source").to_string();
        let seed = if chosen(env, id, "change") == CHANGES[1] {
            self.shows.set(self.shows.get().wrapping_add(1));
            self.shows.get() ^ env.now.millis()
        } else {
            today(env) as u64
        };
        let key = (source.clone(), seed, self.generation.get());
        if let Some((k, quote)) = self.kept.borrow().as_ref()
            && *k == key
        {
            return quote.clone();
        }
        let books = books(env, source == SOURCES[1]);
        let quote = pick(&books, seed, &mut |path| highlights_of(env.storage, path)).map(
            |(path, (start, end, text, note))| {
                let book = library::book(env.storage, &path).unwrap_or_default();
                Quote {
                    title: book.title_or(&path).to_string(),
                    author: book.author.clone().unwrap_or_default(),
                    path,
                    text,
                    note,
                    start,
                    end,
                }
            },
        );
        *self.kept.borrow_mut() = Some((key, quote.clone()));
        quote
    }
}

impl Widget for HighlightWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let quote = self.choose(env);
        let changed = quote != self.quote;
        self.quote = quote;
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
        match &self.quote {
            Some(quote) => draw(canvas, env, &look, quote),
            None => draw_message(canvas, env, &look, "widgets.highlight.none"),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let Some(quote) = &self.quote else {
            return Vec::new();
        };
        let goto = Goto::new(quote.path.clone(), quote.start).marking(quote.end);
        alloc::vec![WidgetTarget {
            area,
            command: OPEN_AT.emit(goto),
        }]
    }
}

/// The largest text size from `largest` down to the small one at which
/// `text` takes no more than `height` pixels at `width`; `None` if it
/// does not fit even small.
fn fitting_size(env: &Env<'_>, text: &str, width: u16, height: u16, largest: f32) -> Option<f32> {
    let style = UiStyle::new(env);
    let font = style.font(env)?;
    let fits = |size: f32| {
        let lines = font.wrap(size, text, f32::from(width)).len() as u16;
        lines * widgets::line_height(env, size) <= height
    };
    let mut size = largest;
    while size > style.small {
        if fits(size) {
            return Some(size);
        }
        size *= 0.92;
    }
    fits(style.small).then_some(style.small)
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, quote: &Quote) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = WIDGET;
    let align = widgets::align(env, id);
    let gap = style.gap as u16;
    let mut area = super::caption(
        canvas,
        env,
        id,
        look,
        env.i18n.tr("widgets.highlight.caption"),
    );
    // Below: the book (and the note), small.
    let source = {
        let title = widgets::shows(env, id, "title");
        let author = widgets::shows(env, id, "author") && !quote.author.is_empty();
        match (title, author) {
            (true, true) => format!("{} · {}", quote.title, quote.author),
            (true, false) => quote.title.clone(),
            (false, true) => quote.author.clone(),
            (false, false) => String::new(),
        }
    };
    let small = widgets::line_height(env, style.small);
    let mut note = widgets::shows(env, id, "note") && !quote.note.is_empty();
    let mut with_source = !source.is_empty();
    let mut marks = widgets::shows(env, id, "marks");
    let mark_size = style.title * 2.0;
    let wanted = widgets::size(env, id);
    // Where the text does not fit, the mark goes first, then the note,
    // then the book; what is left is shortened.
    let (size, text, mark_h, below) = loop {
        let below = u16::from(with_source) * small + u16::from(note) * small;
        let below = if below > 0 { below + gap / 2 } else { 0 };
        // The mark's ink is in the upper part of its line: half a line
        // is enough room for it.
        let mark_h = if marks {
            widgets::line_height(env, mark_size) / 2
        } else {
            0
        };
        let text = if marks {
            quote.text.clone()
        } else {
            env.i18n.format(
                "widgets.highlight.quoted",
                &[("text", Arg::Str(&quote.text))],
            )
        };
        let room = area.height.saturating_sub(below + mark_h);
        let size = match wanted {
            Size::Fit => fitting_size(env, &text, area.width, room, style.title * 1.6),
            other => {
                let size = other.text(&style);
                let lines = font.wrap(size, &text, f32::from(area.width)).len() as u16;
                (lines * widgets::line_height(env, size) <= room).then_some(size)
            }
        };
        match size {
            Some(size) => break (size, text, mark_h, below),
            None if marks => marks = false,
            None if note => note = false,
            None if with_source => with_source = false,
            None => {
                let size = match wanted {
                    Size::Fit => style.small,
                    other => other.text(&style),
                };
                break (size, text, mark_h, below);
            }
        }
    };
    let room = area.height.saturating_sub(below + mark_h);
    let lines = font.wrap(size, &text, f32::from(area.width)).len() as u16;
    let text_h = (lines * widgets::line_height(env, size)).min(room);
    let total = (mark_h + text_h + below).min(area.height);
    area = Rect::new(
        area.x,
        area.y + (area.height - total) / 2,
        area.width,
        total,
    );
    if marks {
        let mark = "\u{201c}";
        let w = font.measure(mark_size, mark);
        let x = align.x(area, w);
        let metrics = font.line_metrics(mark_size);
        let baseline = i32::from(area.y) + metrics.ascent as i32 - (mark_h / 4) as i32;
        font.draw(canvas, mark_size, x, baseline, mark, look.faint);
    }
    let text_area = Rect::new(area.x, area.y + mark_h, area.width, text_h);
    widgets::draw_wrapped(canvas, env, text_area, align, size, &text, look.ink);
    let mut y = area.y + mark_h + text_h + gap / 2;
    if note {
        let row = Rect::new(area.x, y, area.width, small);
        y += draw_line(
            canvas,
            env,
            &font,
            style.small,
            row,
            align,
            &quote.note,
            look.ink,
        );
    }
    if with_source {
        let row = Rect::new(area.x, y, area.width, small);
        draw_line(
            canvas,
            env,
            &font,
            style.small,
            row,
            align,
            &source,
            look.faint,
        );
    }
}
