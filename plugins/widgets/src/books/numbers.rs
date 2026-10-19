//! "Library in numbers": the books, those read, being read and not read
//! yet, as large numbers in a row with small names below, and a bar of
//! the share read.
//!
//! The books are counted in the catalog (or the library folder); which
//! are read the details of the books opened lately tell (a hundred at
//! most, as the library remembers them), read again only for the book
//! being read and for books not seen before.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use quireink_sdk::builtin::actions;
use quireink_sdk::library;
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{Canvas, Color, Command, Env, Id, Rect, RegisterError, Registrar};

use super::{
    Generation, add_settings, catalog_stamp, draw_line, each_book, frame_height, path_hash,
};
use quireink_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.library";
/// The numbers that can show, in order: each a part and the key of its
/// name.
const PARTS: [&str; 4] = ["books", "finished", "reading", "unread"];
/// The names below the numbers.
const LABELS: [&str; 4] = [
    "widgets.library.books",
    "widgets.library.finished",
    "widgets.library.reading",
    "widgets.library.unread",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 9] = [
    widgets::align_spec("widgets.library.align", ID, Align::Center),
    widgets::frame_spec("widgets.library.frame", ID, Frame::None),
    widgets::size_spec("widgets.library.size", ID, Size::Fit, true),
    widgets::part_spec(
        "widgets.library.books",
        ID,
        "widgets.books.part.books",
        true,
    ),
    widgets::part_spec(
        "widgets.library.finished",
        ID,
        "widgets.books.part.finished",
        true,
    ),
    widgets::part_spec(
        "widgets.library.reading",
        ID,
        "widgets.books.part.reading",
        true,
    ),
    widgets::part_spec(
        "widgets.library.unread",
        ID,
        "widgets.books.part.unread",
        true,
    ),
    widgets::part_spec("widgets.library.bar", ID, "widgets.books.part.bar", true),
    widgets::part_spec(
        "widgets.library.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar, generation: &Generation) -> Result<(), RegisterError> {
    add_settings(r, &ID, "widgets.library", &SETTINGS)?;
    let kept: Kept = Rc::default();
    let generation = generation.clone();
    r.add_widget(
        WidgetSpec::new(ID, "widgets.library", move |_| {
            Box::new(Library {
                kept: kept.clone(),
                generation: generation.clone(),
                numbers: None,
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The library counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Numbers {
    pub books: usize,
    pub read: usize,
    pub reading: usize,
    pub unread: usize,
}

impl Numbers {
    /// `books` books, of which those opened are as far as `progress`
    /// says (percent, `None` not begun).
    pub(crate) fn count(books: usize, progress: impl Iterator<Item = Option<u8>>) -> Numbers {
        let (mut read, mut reading) = (0, 0);
        for percent in progress {
            match percent.unwrap_or(0) {
                100.. => read += 1,
                0 => {}
                _ => reading += 1,
            }
        }
        // Books opened outside the library still count as books.
        let books = books.max(read + reading);
        Numbers {
            books,
            read,
            reading,
            unread: books - read - reading,
        }
    }

    /// The number of part `name` ([`PARTS`]).
    pub(crate) fn of(&self, name: &str) -> usize {
        match name {
            "books" => self.books,
            "finished" => self.read,
            "reading" => self.reading,
            _ => self.unread,
        }
    }
}

/// What was counted: for which generation and catalog the books, and the
/// progress of the books opened (by their path's hash).
#[derive(Debug, Default)]
struct Counted {
    key: (u32, u64),
    books: usize,
    progress: Vec<(u64, Option<u8>)>,
}

type Kept = Rc<RefCell<Option<Counted>>>;

struct Library {
    kept: Kept,
    generation: Generation,
    numbers: Option<Numbers>,
}

impl Library {
    fn count(&self, env: &Env<'_>) -> Numbers {
        let key = (self.generation.get(), catalog_stamp(env.storage));
        let mut kept = self.kept.borrow_mut();
        if kept.as_ref().is_none_or(|counted| counted.key != key) {
            let mut books = 0;
            each_book(env, &mut |_| books += 1);
            *kept = Some(Counted {
                key,
                books,
                progress: Vec::new(),
            });
        }
        let counted = kept.get_or_insert_with(Counted::default);
        let opened = library::opened_books(env.storage);
        let mut progress = Vec::with_capacity(opened.len());
        for (i, path) in opened.iter().enumerate() {
            let hash = path_hash(path);
            let known = counted.progress.iter().find(|(h, _)| *h == hash);
            // The book being read anew, the others as known.
            let percent = match known {
                Some((_, percent)) if i > 0 => Some(*percent),
                _ => match library::book(env.storage, path) {
                    Some(book) => Some(book.progress),
                    None if env.storage.exists(path) => Some(None),
                    None => None,
                },
            };
            if let Some(percent) = percent {
                progress.push((hash, percent));
            }
        }
        counted.progress = progress;
        Numbers::count(counted.books, counted.progress.iter().map(|(_, p)| *p))
    }
}

impl Widget for Library {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let numbers = Some(self.count(env));
        let changed = numbers != self.numbers;
        self.numbers = numbers;
        changed
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let big = widgets::line_height(env, style.title * 2.0);
        big + widgets::line_height(env, style.small) * 2
            + style.gap as u16
            + frame_height(env, WIDGET)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        if let Some(numbers) = &self.numbers {
            draw(canvas, env, &look, numbers);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget {
            area,
            command: Command::RunAction(actions::LIBRARY),
        }]
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, numbers: &Numbers) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = WIDGET;
    let gap = style.gap as u16;
    let area = super::caption(
        canvas,
        env,
        id,
        look,
        env.i18n.tr("widgets.library.caption"),
    );
    let shown: Vec<(usize, &str)> = PARTS
        .iter()
        .filter(|p| widgets::shows(env, id, p))
        .map(|p| (numbers.of(p), *p))
        .collect();
    let labels: Vec<&str> = shown
        .iter()
        .map(|(_, p)| {
            env.i18n
                .tr(LABELS[PARTS.iter().position(|q| q == p).unwrap_or(0)])
        })
        .collect();
    let small = widgets::line_height(env, style.small);
    let wanted_bar = widgets::shows(env, id, "bar") && numbers.books > 0;
    let bar_h = (style.line as u16 * 3).max(4);
    let widest_label = labels
        .iter()
        .map(|l| font.measure(style.small, l))
        .fold(0.0, f32::max);
    let widest = shown
        .iter()
        .map(|(n, _)| n.to_string())
        .max_by_key(String::len)
        .unwrap_or_default();
    let count = shown.len().max(1) as u16;
    let room_for = |columns: u16, bar: bool| {
        let bar_room = if bar { bar_h + gap } else { 0 };
        (area.height.saturating_sub(bar_room) / count.div_ceil(columns)).saturating_sub(small)
    };
    // The numbers as large as fit: the widest in its column, all rows
    // with their names in the height.
    let size_for = |columns: u16, bar: bool| -> f32 {
        let room = room_for(columns, bar);
        match widgets::size(env, id) {
            Size::Fit => {
                let column = area.width / columns;
                let at_title = font.measure(style.title, &widest).max(1.0);
                let by_width = style.title * f32::from(column) * 0.8 / at_title;
                let line = f32::from(widgets::line_height(env, style.title)).max(1.0);
                let by_height = style.title * f32::from(room) / line;
                by_width.min(by_height).min(style.title * 3.0)
            }
            other
                if f32::from(widgets::line_height(env, other.text(&style) * 1.4))
                    <= f32::from(room) =>
            {
                other.text(&style) * 1.4
            }
            _ => 0.0,
        }
    };
    // In one row, or in two; with the bar or without: whichever makes
    // the numbers largest, their names fitting below them, the bar kept
    // where it costs little.
    let mut best = (0.0, count, false);
    for columns in [count, count.div_ceil(2)] {
        for bar in [wanted_bar, false] {
            // Names cut short count against it, the more the shorter.
            let fits = (f32::from(area.width / columns) / widest_label.max(1.0)).min(1.0);
            let mut score = size_for(columns, bar) * fits;
            if bar != wanted_bar {
                score *= 0.9;
            }
            if score > best.0 {
                best = (score, columns, bar);
            }
        }
    }
    let (_, columns, bar) = best;
    if best.0 < style.small * 0.8 {
        // No room for numbers above names: each beside its name, on a
        // line, the names left out where they do not fit.
        let column = area.width / count;
        let row = super::center_row(env, area, style.small);
        let left = super::place_x(area, column * count, widgets::align(env, id));
        let with_names = shown.iter().zip(&labels).all(|((number, _), label)| {
            font.measure(style.small, &alloc::format!("{number} {label}")) <= f32::from(column)
        });
        for (i, ((number, _), label)) in shown.iter().zip(&labels).enumerate() {
            let cell = Rect::new(left + i as u16 * column, row.y, column, row.height);
            let number = number.to_string();
            let with_name = with_names;
            let text = if with_name {
                alloc::format!("{number} {label}")
            } else {
                number.clone()
            };
            let x = Align::Center.x(cell, font.measure(style.small, &text));
            let at = Rect::new(x as u16, cell.y, cell.width, cell.height);
            draw_line(
                canvas,
                env,
                &font,
                style.small,
                at,
                Align::Left,
                &number,
                look.ink,
            );
            if with_name {
                let shift = font.measure(style.small, &alloc::format!("{number} ")) as u16;
                let name = Rect::new(
                    at.x + shift,
                    at.y,
                    at.width.saturating_sub(shift),
                    at.height,
                );
                draw_line(
                    canvas,
                    env,
                    &font,
                    style.small,
                    name,
                    Align::Left,
                    label,
                    look.faint,
                );
            }
        }
        return;
    }
    let size = size_for(columns, bar).max(style.small * 0.6);
    let room = room_for(columns, bar);
    let bar_room = if bar { bar_h + gap } else { 0 };
    let rows = count.div_ceil(columns);
    let column = area.width / columns;
    let big = widgets::line_height(env, size).min(room);
    let row_h = big + small;
    let total = (row_h * rows + bar_room).min(area.height);
    let top = area.y + (area.height - total) / 2;
    let align = widgets::align(env, id);
    // Columns as wide as their place; the block placed as aligned.
    let used = column * columns;
    let left = super::place_x(area, used, align);
    for (i, ((number, _), label)) in shown.iter().zip(&labels).enumerate() {
        let (col, row) = (i as u16 % columns, i as u16 / columns);
        let y = top + row * row_h;
        let cell = Rect::new(left + col * column, y, column, big);
        let number = number.to_string();
        draw_line(
            canvas,
            env,
            &font,
            size,
            cell,
            Align::Center,
            &number,
            look.ink,
        );
        let under = Rect::new(cell.x, y + big, column, small);
        draw_line(
            canvas,
            env,
            &font,
            style.small,
            under,
            Align::Center,
            label,
            look.faint,
        );
    }
    if bar {
        let inset = column / 6;
        let line = Rect::new(
            left + inset,
            top + row_h * rows + gap,
            used.saturating_sub(2 * inset),
            bar_h,
        );
        draw_share(canvas, env, look, line, numbers);
    }
}

/// A bar of the books: those read dark, those being read shaded, the
/// rest a fine line.
fn draw_share(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, area: Rect, numbers: &Numbers) {
    let style = UiStyle::new(env);
    let fine = (style.line as u16).clamp(1, area.height);
    canvas.fill(
        Rect::new(area.x, area.y + (area.height - fine) / 2, area.width, fine),
        look.ink,
    );
    let share = |n: usize| (u32::from(area.width) * n as u32 / numbers.books.max(1) as u32) as u16;
    let read = share(numbers.read);
    let reading = share(numbers.reading);
    if reading > 0 {
        let at = Rect::new(
            area.x + read,
            area.y,
            reading.min(area.width - read),
            area.height,
        );
        let shade = if look.frame == Frame::Dark {
            Color::gray(140)
        } else {
            Color::gray(150)
        };
        canvas.fill_round_shaded(at, u32::from(area.height / 2), shade);
    }
    if read > 0 {
        let at = Rect::new(
            area.x,
            area.y,
            read.max(area.height).min(area.width),
            area.height,
        );
        canvas.fill_round(at, u32::from(area.height / 2), look.ink);
    }
}
