//! Books: the book being read with its cover and progress, what to read
//! next, a series and the favourites as covers, a highlight of one's own,
//! the library in numbers; and the battery.
//!
//! Each reads what the library service knows ([`library`]: the books
//! opened last, a book's details, the favourites, the catalog of all
//! books) and the annotations ([`annotations`]), never a reader's own
//! files, and shows only covers looked up before: no book is opened to
//! show it. Covers are made small for where they go once a widget is
//! placed ([`Covers`]), and drawn from memory in every band. What takes longer to read (the catalog's books, highlights) is
//! read when the widget shows and kept until it may have changed
//! ([`Generation`]); widgets of books are not read each minute.
//!
//! [`annotations`]: paperpaw_sdk::annotations

mod battery;
mod favourites;
mod highlight;
mod next;
mod numbers;
mod progress;
mod reading;
mod series;
mod shelf;
#[cfg(test)]
pub(crate) mod tests;

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Look, line_height};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, FontHandle, Id, Params, Point, Rect, RegisterError,
    Registrar, SettingSpec, Storage,
};

pub(crate) use crate::{frame_height, today};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/books.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/books.de.ini")))?;
    // What was read of the catalog is read again once books came, went
    // or moved.
    let generation = Generation::default();
    for event in [
        events::LIBRARY_CHANGED.id(),
        events::BOOK_MOVED.id(),
        events::BOOK_REMOVED.id(),
        events::BOOK_FINISHED.id(),
        events::ANNOTATIONS_CHANGED.id(),
    ] {
        let generation = generation.clone();
        r.subscribe(
            event.clone(),
            alloc::boxed::Box::new(move |_, _, _| generation.next()),
        )?;
    }
    progress::register(r)?;
    reading::register(r)?;
    next::register(r, &generation)?;
    series::register(r, &generation)?;
    favourites::register(r)?;
    highlight::register(r, &generation)?;
    numbers::register(r, &generation)?;
    battery::register(r)?;
    Ok(())
}

/// Counts the changes to the library (books came, went, moved, were
/// finished; annotations changed): what widgets keep of it is read again
/// when it moved on.
#[derive(Debug, Clone, Default)]
pub(crate) struct Generation(Rc<Cell<u32>>);

impl Generation {
    fn next(&self) {
        self.0.set(self.0.get().wrapping_add(1));
    }

    pub(crate) fn get(&self) -> u32 {
        self.0.get()
    }
}

// ---- Books ------------------------------------------------------------------

/// A book as the widgets show it, from what the library knows of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Book {
    pub path: String,
    pub title: String,
    pub author: String,
    pub series: String,
    /// Its number in the series, as the book says it (`3`, `2.5`).
    pub number: String,
    /// Percent read, once opened.
    pub progress: Option<u8>,
    /// Its cover, if it was looked up and is there.
    pub cover: Option<ImageRef>,
}

impl Book {
    /// The book at `path`, if it is there.
    pub(crate) fn of(storage: &dyn Storage, path: &str) -> Option<Book> {
        let details = match library::book(storage, path) {
            Some(details) => details,
            None if storage.exists(path) => library::Book::default(),
            None => return None,
        };
        Some(Book {
            title: details.shown_title(path).into_owned(),
            path: path.to_string(),
            author: details.author.clone().unwrap_or_default(),
            series: details.series.clone().unwrap_or_default(),
            number: details.series_index.clone().unwrap_or_default(),
            progress: details.progress,
            cover: details.cover.filter(|c| storage.exists(&c.path)),
        })
    }

    /// Opens it for reading.
    pub(crate) fn open(&self) -> Command {
        open(&self.path)
    }

    /// Read to the end.
    pub(crate) fn finished(&self) -> bool {
        self.progress == Some(100)
    }

    /// Percent read, 0 before it was opened.
    pub(crate) fn percent(&self) -> u8 {
        self.progress.unwrap_or(0)
    }
}

/// Opens the book at `path` for reading.
pub(crate) fn open(path: &str) -> Command {
    Command::OpenSlot(slots::READER, Params::new().with(params::PATH, path))
}

/// The book being read: the one opened last that is still there.
pub(crate) fn current(storage: &dyn Storage) -> Option<Book> {
    library::opened_books(storage)
        .iter()
        .take(MAX_LOOKED_AT)
        .find_map(|path| Book::of(storage, path))
}

/// Books looked at, at most, to find one there.
const MAX_LOOKED_AT: usize = 8;

/// The catalog's file.
fn catalog_file() -> String {
    format!("{}/catalog.txt", library::DIR)
}

/// Whether the library made its catalog (with the books' series and
/// authors), which views of the library make.
pub(crate) fn has_catalog(storage: &dyn Storage) -> bool {
    storage.exists(&catalog_file())
}

/// What tells the catalog changed: its size.
pub(crate) fn catalog_stamp(storage: &dyn Storage) -> u64 {
    storage.size(&catalog_file()).unwrap_or(0)
}

/// Calls `f` with every book of the library: from the catalog where
/// there is one, else from the library folder (each book by its path and
/// file name alone). Nothing is held: the catalog is read in pieces.
pub(crate) fn each_book(env: &Env<'_>, f: &mut dyn FnMut(catalog::Record)) {
    if has_catalog(env.storage) {
        return catalog::each(env.storage, f);
    }
    let folder = library::folder(env);
    library::books_in(env.storage, env.formats, folder, &mut |path| {
        f(catalog::Record::of(path, &library::Book::default()));
    });
}

/// A number of a series as a number to sort by: `2`, `2.5`, `10`
/// (in thousandths; books without one last).
pub(crate) fn series_rank(number: &str) -> u64 {
    let number = number.trim();
    let (whole, part) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let Ok(whole) = whole.parse::<u64>() else {
        return u64::MAX;
    };
    let mut thousandths = 0;
    for (i, digit) in part.bytes().take(3).enumerate() {
        if !digit.is_ascii_digit() {
            break;
        }
        thousandths += u64::from(digit - b'0') * [100, 10, 1][i];
    }
    whole.saturating_mul(1000).saturating_add(thousandths)
}

// ---- Days and chance -----------------------------------------------------------

/// A number that looks random, the same for the same `seed`.
pub(crate) fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The hash of a book's path, as the library names its files by.
pub(crate) fn path_hash(path: &str) -> u64 {
    u64::from_str_radix(&library::key(path), 16).unwrap_or(0)
}

// ---- Settings ------------------------------------------------------------------

/// Registers widget `id`'s settings group, named `title`, with its
/// settings, kept in the program (where its text sits, its frame, how
/// large its text is, the parts it shows and the rest).
pub(crate) fn add_settings(
    r: &mut Registrar,
    id: &Id,
    title: &'static str,
    settings: &'static [SettingSpec],
) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(id, title))?;
    r.add_settings(settings)
}

// The value of widget `id`'s choice `key`.
pub(crate) fn chosen<'e>(env: &Env<'e>, id: &str, key: &str) -> &'e str {
    env.settings.text(&format!("{id}.{key}"))
}

/// Widget `id`'s number `key`, at least 1.
pub(crate) fn count(env: &Env<'_>, id: &str, key: &str) -> usize {
    env.settings.int(&format!("{id}.{key}")).max(1) as usize
}

/// The values of a thickness setting: thin, medium, thick.
pub(crate) const THICKNESSES: [&str; 3] = [
    "widgets.books.thin",
    "widgets.books.medium",
    "widgets.books.thick",
];

/// A ring's or a bar's thickness for something `size` pixels across, as
/// widget `id`'s thickness setting says.
pub(crate) fn thickness(env: &Env<'_>, id: &str, size: u16) -> u32 {
    let share = match chosen(env, id, "thickness") {
        "widgets.books.thin" => 16,
        "widgets.books.thick" => 6,
        _ => 10,
    };
    (u32::from(size) / share)
        .max(UiStyle::new(env).line * 2)
        .max(2)
}

// ---- Drawing -----------------------------------------------------------------

/// The text size widget `id`'s size setting says (for "as large as
/// fits": the large one, where the widget works out nothing better).
pub(crate) fn text_size(env: &Env<'_>, id: &str) -> f32 {
    widgets::size(env, id).text(&UiStyle::new(env))
}

/// `area` less `by` pixels at the top.
pub(crate) fn below(area: Rect, by: u16) -> Rect {
    let by = by.min(area.height);
    Rect::new(area.x, area.y + by, area.width, area.height - by)
}

/// A line of text `size` pixels large at the top of `area`, placed as
/// `align` says and shortened to fit; returns its height.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    size: f32,
    area: Rect,
    align: Align,
    text: &str,
    color: Color,
) -> u16 {
    let height = line_height(env, size).min(area.height);
    if text.is_empty() || height == 0 {
        return 0;
    }
    let text = ui::fit(font, size, text, f32::from(area.width));
    let x = align.x(area, font.measure(size, &text));
    let row = Rect::new(area.x, area.y, area.width, height);
    ui::draw_label(canvas, font, size, x, row, &text, color);
    height
}

/// Lines of text, each with its size and colour, drawn one below the
/// other together in the middle of `area` (from the top where `top`),
/// as many as fit, a little room between them.
pub(crate) struct Lines<'t> {
    pub lines: Vec<(f32, Cow<'t, str>, Color)>,
}

impl<'t> Lines<'t> {
    pub(crate) fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Adds a line, if it says anything.
    pub(crate) fn add(&mut self, size: f32, text: impl Into<Cow<'t, str>>, color: Color) {
        let text = text.into();
        if !text.is_empty() {
            self.lines.push((size, text, color));
        }
    }

    /// The height they take, the room between them included.
    pub(crate) fn height(&self, env: &Env<'_>) -> u16 {
        let gap = Self::gap(env);
        let lines: u16 = self.lines.iter().map(|l| line_height(env, l.0)).sum();
        lines + gap * self.lines.len().saturating_sub(1) as u16
    }

    fn gap(env: &Env<'_>) -> u16 {
        UiStyle::new(env).gap as u16 / 4
    }

    /// Leaves out lines from the end until they fit `height`; the first
    /// stays.
    pub(crate) fn fit(&mut self, env: &Env<'_>, height: u16) {
        while self.lines.len() > 1 && self.height(env) > height {
            self.lines.pop();
        }
    }

    /// Draws them in `area` as `align` says (lines larger than the body
    /// text that are too wide in the body size); returns where they went.
    pub(crate) fn draw(
        &mut self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        area: Rect,
        align: Align,
    ) -> Rect {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return area;
        };
        // A long title in the body size rather than shortened much.
        for (size, text, _) in &mut self.lines {
            if *size > style.text && font.measure(*size, text) > f32::from(area.width) {
                *size = style.text;
            }
        }
        // A title still too long goes on in a second line, where there is
        // room for one more.
        let width = f32::from(area.width);
        let room = self.height(env)
            + line_height(env, self.lines.first().map_or(0.0, |l| l.0))
            + Self::gap(env)
            <= area.height;
        if let Some((size, text, color)) = self.lines.first().cloned()
            && room
            && font.measure(size, &text) > width
        {
            let ranges = font.wrap(size, &text, width);
            if let (Some(first), Some(second)) = (ranges.first(), ranges.get(1)) {
                let head = String::from(text[first.clone()].trim_end());
                let rest = String::from(text[second.start..].trim());
                self.lines[0].1 = Cow::Owned(head);
                self.lines.insert(1, (size, Cow::Owned(rest), color));
            }
        }
        self.fit(env, area.height);
        let total = self.height(env).min(area.height);
        let top = area.y + (area.height - total) / 2;
        let mut y = top;
        for (size, text, color) in &self.lines {
            let row = Rect::new(area.x, y, area.width, area.y + area.height - y);
            y += draw_line(canvas, env, &font, *size, row, align, text, *color) + Self::gap(env);
        }
        Rect::new(area.x, top, area.width, total)
    }
}

/// The size of a cover fitted into `width` × `height`: as its picture
/// is, or 2:3 for a book without one.
pub(crate) fn cover_size(book: &Book, width: u16, height: u16) -> (u16, u16) {
    match &book.cover {
        Some(cover) => ui::fit_image(cover, width, height),
        None => {
            let w = width.min(height * 2 / 3);
            (w, (w * 3 / 2).min(height))
        }
    }
}

/// Covers made small for the places a widget draws them at, when it is
/// placed ([`Widget::place`](paperpaw_sdk::widgets::Widget::place)) or
/// its books change: each picture read once, then drawn from memory in
/// every band of every frame.
#[derive(Debug, Default)]
pub(crate) struct Covers(Vec<(Rect, Thumbnail)>);

impl Covers {
    /// The covers of the books in `places`, each made small for the
    /// place it goes ([`cover_size`] large), as the panel shows it.
    pub(crate) fn make<'b>(
        env: &Env<'_>,
        places: impl IntoIterator<Item = (&'b Book, Rect)>,
    ) -> Self {
        let format = env.device.display.format;
        let made = places
            .into_iter()
            .filter_map(|(book, at)| {
                let cover = book.cover.as_ref()?;
                let small = Thumbnail::new(env.storage, cover, at.width, at.height, format)?;
                Some((at, small))
            })
            .collect();
        Self(made)
    }

    /// The cover made for `at`, if one was.
    fn at(&self, at: Rect) -> Option<&Thumbnail> {
        self.0
            .iter()
            .find(|(place, small)| {
                *place == at && (small.width(), small.height()) == (at.width, at.height)
            })
            .map(|(_, small)| small)
    }
}

/// Draws `book`'s cover into `at` (as large as [`cover_size`] says): as
/// made small in `covers`, else from its picture (where there was no
/// memory to make it small); a book without one as a frame with its
/// title.
pub(crate) fn draw_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    at: Rect,
    book: &Book,
    covers: &Covers,
) {
    if at.width < 2 || at.height < 2 {
        return;
    }
    match (&book.cover, covers.at(at)) {
        (Some(_), Some(small)) => small.draw(canvas, at.x, at.y),
        (Some(cover), None) => canvas.small_pictures(|canvas| {
            ui::draw_image(canvas, env.storage, cover, at);
        }),
        (None, _) => draw_placeholder(canvas, env, look, at, &book.title),
    }
}

/// A book without a cover: a frame with its title inside.
fn draw_placeholder(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, at: Rect, title: &str) {
    let style = UiStyle::new(env);
    canvas.stroke(at, style.line.max(2), look.ink);
    let pad = (style.gap as u16 / 2).min(at.width / 6);
    let inner = Rect::new(
        at.x + pad,
        at.y + pad,
        at.width.saturating_sub(2 * pad),
        at.height.saturating_sub(2 * pad),
    );
    if inner.width < 8 || inner.height < 8 {
        return;
    }
    let size = style.small.min(f32::from(inner.width) / 3.0);
    let Some(font) = style.font(env) else {
        return;
    };
    let line = line_height(env, size).max(1);
    let fits = usize::from(inner.height / line).max(1);
    let mut lines: Vec<String> = Vec::new();
    let width = f32::from(inner.width);
    for word in title.split_whitespace() {
        let joined = lines.last().map(|last| format!("{last} {word}"));
        match joined {
            Some(joined) if font.measure(size, &joined) <= width => {
                if let Some(last) = lines.last_mut() {
                    *last = joined;
                }
            }
            _ => lines.push(ui::fit(&font, size, word, width)),
        }
    }
    for (n, text) in lines.iter().take(fits).enumerate() {
        let row = Rect::new(inner.x, inner.y + n as u16 * line, inner.width, line);
        ui::draw_label(canvas, &font, size, f32::from(inner.x), row, text, look.ink);
    }
}

/// "16 %".
pub(crate) fn percent_text(env: &Env<'_>, percent: u8) -> String {
    env.i18n.format(
        "widgets.books.percent",
        &[("percent", Arg::Int(i64::from(percent)))],
    )
}

/// Draws a progress ring of `radius` around `center`, `thickness` wide:
/// a fine line all round and over it the part done, `percent`, from
/// twelve o'clock with round ends.
pub(crate) fn draw_ring(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    center: Point,
    radius: u32,
    thickness: u32,
    percent: u8,
) {
    let style = UiStyle::new(env);
    let thickness = thickness.min(radius);
    if thickness == 0 {
        // No room for a ring.
        return;
    }
    let fine = style.line.clamp(1, thickness);
    // The fine line in the middle of the ring's width.
    let track = radius - (thickness - fine) / 2;
    canvas.stroke_circle(center, track, fine, look.ink);
    let percent = u32::from(percent.min(100));
    if percent == 0 {
        return;
    }
    let sweep = percent * 360 / 100;
    canvas.fill_ring(center, radius, thickness, 0, sweep.max(1), look.ink);
    if sweep < 360 && thickness >= 4 {
        // Round ends.
        let middle = (radius - thickness / 2) as i32;
        let end = (thickness / 2).max(1);
        for degrees in [0, sweep as i32] {
            let at = paperpaw_sdk::round::point_at(center, middle, degrees);
            canvas.fill_circle(at, end, look.ink);
        }
    }
}

/// Draws the percentage inside a ring of `radius` around `center`: the
/// number large, the sign small beside it.
pub(crate) fn draw_ring_percent(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    center: Point,
    inner: u32,
    percent: u8,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let number = percent.to_string();
    let room = inner as f32 * 1.5;
    let least = style.small * 0.6;
    let mut size = (inner as f32 * 0.8).max(least);
    let sign = |size: f32| (size * 0.45).max(least * 0.8);
    let width = |size: f32| font.measure(size, &number) + font.measure(sign(size), "%");
    if width(size) > room {
        size = (size * room / width(size)).max(least);
    }
    let total = width(size);
    let x = center.x as f32 - total / 2.0;
    let metrics = font.line_metrics(size);
    // The digits' middle at the centre.
    let baseline = center.y + (metrics.ascent * 0.36) as i32;
    font.draw(canvas, size, x, baseline, &number, look.ink);
    let x = x + font.measure(size, &number);
    font.draw(canvas, sign(size), x, baseline, "%", look.faint);
}

/// Draws the percentage alone, as large as fits `area` and placed as
/// `align` says: where a ring would be too small to read.
pub(crate) fn draw_percent_alone(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    area: Rect,
    align: Align,
    percent: u8,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let text = percent_text(env, percent);
    let line = f32::from(line_height(env, 100.0)).max(1.0) / 100.0;
    let by_height = f32::from(area.height) / line;
    let by_width = 90.0 * f32::from(area.width) / font.measure(100.0, &text).max(1.0);
    let size = by_height.min(by_width).min(style.title * 2.0);
    draw_line(
        canvas,
        env,
        &font,
        size,
        center_row(env, area, size),
        align,
        &text,
        look.ink,
    );
}

/// A row for a line of text `size` large in the middle of `area`.
pub(crate) fn center_row(env: &Env<'_>, area: Rect, size: f32) -> Rect {
    let h = line_height(env, size).min(area.height);
    Rect::new(area.x, area.y + (area.height - h) / 2, area.width, h)
}

/// Draws a bar `percent` full in `area`: a fine line along it, the part
/// done thick, with round ends.
pub(crate) fn draw_bar(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    area: Rect,
    percent: u8,
) {
    let style = UiStyle::new(env);
    let h = area.height.max(2);
    let fine = (style.line as u16).clamp(1, h);
    let middle = area.y + (h - fine) / 2;
    canvas.fill(Rect::new(area.x, middle, area.width, fine), look.ink);
    let done = (u32::from(area.width) * u32::from(percent.min(100)) / 100) as u16;
    if done > 0 {
        let done = done.max(h);
        canvas.fill_round(
            Rect::new(area.x, area.y, done.min(area.width), h),
            u32::from(h / 2),
            look.ink,
        );
    }
}

/// Draws the message `key` in the middle of `look`'s area (e.g. "Nothing
/// read yet").
pub(crate) fn draw_message(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, key: &str) {
    widgets::draw_message(canvas, env, look, env.i18n.tr(key));
}

/// The caption of widget `id` (key `caption`), drawn at the top of its
/// area if it shows one; returns the area left below it.
pub(crate) fn caption(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    id: &str,
    look: &Look,
    text: &str,
) -> Rect {
    let rest = below_caption(env, id, look);
    if rest != look.area {
        widgets::draw_caption(canvas, env, id, look, text);
    }
    rest
}

/// The area [`caption`] leaves below it, without drawing it.
pub(crate) fn below_caption(env: &Env<'_>, id: &str, look: &Look) -> Rect {
    if !widgets::shows(env, id, "caption") || look.area.height < 3 * caption_room(env) {
        return look.area;
    }
    below(look.area, caption_room(env))
}

/// The height of a caption, if widget `id` shows one.
pub(crate) fn caption_room(env: &Env<'_>) -> u16 {
    widgets::caption_height(env)
}

/// Where a box `width` wide sits in `area` as `align` says.
pub(crate) fn place_x(area: Rect, width: u16, align: Align) -> u16 {
    align.x(area, f32::from(width)) as u16
}
