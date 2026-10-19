//! "Now reading": the book being read, its title and author, the page it
//! is at, a bar of how far, and the time left to read it at one's speed.
//!
//! No plugin may read the chapter a reader is at, nor the pages it
//! counted, from what the library keeps; the standard reading events
//! carry the page and the pages. So this widget records them itself
//! ([`Recorder`]): the page of the book read last, and how long a page
//! took (the time between pages turned, pauses left out), kept small in
//! `.quireink/plugins/widgets/reading.ini` when the book is closed.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use quireink_sdk::builtin::{actions, events};
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{
    Arg, Canvas, Command, Env, Id, Rect, RegisterError, Registrar, Storage, config,
};

use super::{
    Book, Lines, current, draw_bar, draw_cover, draw_message, frame_height, look_settings, part,
    percent_text, text_size,
};
use crate::PLUGIN;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.reading";
/// The time between two pages turned that counts as reading, in
/// milliseconds: shorter is leafing through, longer a pause.
const READ_PAGE: core::ops::RangeInclusive<u64> = 2_000..=600_000;
/// Pages the speed is worked out from; beyond them the older count half.
const SPEED_PAGES: u32 = 400;
/// Pages read before there is a speed to go by.
const MIN_PAGES: u32 = 10;

/// Where the book read last is, and how long its pages take.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Position {
    pub path: String,
    /// The page (from 1) and the pages, once the reader counted them.
    pub page: Option<u32>,
    pub pages: Option<u32>,
    /// Seconds read and the pages they took, for the speed.
    pub seconds: u32,
    pub turned: u32,
}

impl Position {
    /// A page turned after `millis` on the last one.
    pub(crate) fn turned(&mut self, millis: u64) {
        if !READ_PAGE.contains(&millis) {
            return;
        }
        self.seconds = self.seconds.saturating_add((millis / 1000) as u32);
        self.turned += 1;
        if self.turned > SPEED_PAGES {
            self.seconds /= 2;
            self.turned /= 2;
        }
    }

    /// Seconds left to read in the book, if its pages are counted and
    /// enough was read for a speed.
    pub(crate) fn left(&self) -> Option<u64> {
        if self.turned < MIN_PAGES {
            return None;
        }
        let pages = self.pages?;
        let left = pages.saturating_sub(self.page?);
        Some(u64::from(left) * u64::from(self.seconds) / u64::from(self.turned))
    }

    fn file() -> String {
        format!("{}/reading.ini", Env::plugin_dir(PLUGIN.as_str()))
    }

    pub(crate) fn load(storage: &dyn Storage) -> Position {
        let Ok(data) = quireink_sdk::read_file(storage, &Self::file(), 4096) else {
            return Position::default();
        };
        let mut position = Position::default();
        let source = core::str::from_utf8(&data).unwrap_or_default();
        for entry in config::parse(source).filter_map(Result::ok) {
            match entry.key {
                "path" => position.path = entry.value.as_str().into_owned(),
                "page" => position.page = entry.value.as_u32(),
                "pages" => position.pages = entry.value.as_u32(),
                "seconds" => position.seconds = entry.value.as_u32().unwrap_or(0),
                "turned" => position.turned = entry.value.as_u32().unwrap_or(0),
                _ => {}
            }
        }
        position
    }

    pub(crate) fn save(&self, storage: &dyn Storage) {
        let mut ini = String::new();
        let mut set = |key: &str, value: &str| ini = config::set(&ini, "reading", key, value);
        set("path", &self.path);
        if let Some(page) = self.page {
            set("page", &page.to_string());
        }
        if let Some(pages) = self.pages {
            set("pages", &pages.to_string());
        }
        set("seconds", &self.seconds.to_string());
        set("turned", &self.turned.to_string());
        let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
        let _ = storage.write(&Self::file(), ini.as_bytes());
    }
}

/// Records the position from the reading events, kept in memory while
/// reading and written when the book is closed or the device sleeps.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    position: Option<Position>,
    /// When the last page was turned (milliseconds since start).
    last: Option<u64>,
    unsaved: bool,
}

pub(crate) type Shared = Rc<RefCell<Recorder>>;

impl Recorder {
    /// The position, read from its file the first time.
    pub(crate) fn position(&mut self, storage: &dyn Storage) -> &mut Position {
        self.position.get_or_insert_with(|| Position::load(storage))
    }

    fn reading(&mut self, env: &Env<'_>, reading: &events::Reading, turned: bool) {
        let now = env.now.millis();
        let last = self.last.replace(now);
        let position = self.position(env.storage);
        if position.path != reading.path {
            position.path = reading.path.clone();
        } else if turned && let Some(last) = last {
            position.turned(now.saturating_sub(last));
        }
        position.page = reading.page;
        position.pages = reading.pages;
        self.unsaved = true;
    }

    fn save(&mut self, storage: &dyn Storage) {
        if let (true, Some(position)) = (self.unsaved, &self.position) {
            position.save(storage);
            self.unsaved = false;
        }
    }
}

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    let recorder: Shared = Rc::default();
    for (event, turned) in [(events::BOOK_OPENED, false), (events::PAGE_TURNED, true)] {
        let recorder = recorder.clone();
        r.on(event, move |env, reading, _| {
            recorder.borrow_mut().reading(env, reading, turned);
        })?;
    }
    let saving = recorder.clone();
    r.on(events::BOOK_CLOSED, move |env, reading, _| {
        let mut recorder = saving.borrow_mut();
        recorder.reading(env, reading, false);
        recorder.last = None;
        recorder.save(env.storage);
    })?;
    let saving = recorder.clone();
    r.on(events::SUSPEND, move |env, _, _| {
        saving.borrow_mut().save(env.storage);
    })?;

    look_settings(
        r,
        &ID,
        "widgets.reading",
        Align::Left,
        Frame::None,
        Size::Medium,
        false,
    )?;
    part(r, &ID, "caption", true)?;
    part(r, &ID, "cover", true)?;
    part(r, &ID, "author", true)?;
    part(r, &ID, "pages", true)?;
    part(r, &ID, "bar", true)?;
    part(r, &ID, "left", true)?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.reading", move |_| {
            Box::new(Reading {
                recorder: recorder.clone(),
                shown: None,
            })
        })
        .category(widgets::categories::BOOKS),
    )
}

/// The book being read and where in it, read when updated.
struct Reading {
    recorder: Shared,
    shown: Option<(Book, Position)>,
}

impl Widget for Reading {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = current(env.storage).map(|book| {
            let mut recorder = self.recorder.borrow_mut();
            let position = recorder.position(env.storage);
            let position = if position.path == book.path {
                position.clone()
            } else {
                Position::default()
            };
            (book, position)
        });
        let changed = shown != self.shown;
        self.shown = shown;
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
        match &self.shown {
            Some((book, position)) => draw(canvas, env, &look, book, position),
            None => draw_message(canvas, env, &look, "widgets.books.nothing-read"),
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .shown
            .as_ref()
            .map_or(Command::RunAction(actions::LIBRARY), |(book, _)| {
                book.open()
            });
        alloc::vec![WidgetTarget { area, command }]
    }
}

/// "Page 34 of 210 · 16 %", or the percentage alone while the pages are
/// not counted.
pub(crate) fn where_text(env: &Env<'_>, book: &Book, position: &Position) -> String {
    let percent = percent_text(env, book.percent());
    match (position.page, position.pages) {
        (Some(page), Some(pages)) if pages > 0 => {
            let page = env.i18n.format(
                "widgets.reading.page",
                &[
                    ("page", Arg::Int(i64::from(page))),
                    ("pages", Arg::Int(i64::from(pages))),
                ],
            );
            format!("{page} · {percent}")
        }
        _ => env.i18n.format(
            "widgets.reading.percent",
            &[("percent", Arg::Str(&percent))],
        ),
    }
}

/// "About 2 h 5 min left", "About 40 min left".
pub(crate) fn left_text(env: &Env<'_>, seconds: u64) -> String {
    let minutes = seconds.div_ceil(60).max(1);
    let (hours, minutes) = (minutes / 60, minutes % 60);
    if hours == 0 {
        env.i18n.format(
            "widgets.reading.left-minutes",
            &[("minutes", Arg::Int(minutes as i64))],
        )
    } else {
        env.i18n.format(
            "widgets.reading.left-hours",
            &[
                ("hours", Arg::Int(hours as i64)),
                ("minutes", Arg::Int(minutes as i64)),
            ],
        )
    }
}

/// The parts of "Now reading" below the cover's side, in the order they
/// give way where there is too little room: the time left first, the
/// title never.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Left,
    Caption,
    Author,
    Bar,
    Pages,
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, book: &Book, position: &Position) {
    let style = UiStyle::new(env);
    let gap = style.gap as u16;
    let id = WIDGET;
    let area = look.area;
    // The cover on the left, where shown and there is room for the text.
    let (w, h) = super::cover_size(book, area.width / 3, area.height);
    let text_room = area.width.saturating_sub(w + gap);
    let rest = if widgets::shows(env, id, "cover") && f32::from(text_room) >= style.small * 7.0 {
        let at = Rect::new(area.x, area.y + (area.height - h) / 2, w, h);
        draw_cover(canvas, env, look, at, book);
        Rect::new(
            area.x + area.width - text_room,
            area.y,
            text_room,
            area.height,
        )
    } else {
        area
    };
    let left = position.left().map(|s| left_text(env, s));
    let mut parts: Vec<Part> = [
        (Part::Caption, "caption"),
        (Part::Author, "author"),
        (Part::Bar, "bar"),
        (Part::Pages, "pages"),
        (Part::Left, "left"),
    ]
    .into_iter()
    .filter(|(part, name)| widgets::shows(env, id, name) && (*part != Part::Left || left.is_some()))
    .map(|(part, _)| part)
    .collect();
    let small = widgets::line_height(env, style.small);
    let line_gap = gap / 4;
    let bar_h = (style.line as u16 * 3).max(4);
    let title_size = text_size(env, id);
    let height = |parts: &[Part]| -> u16 {
        let mut total = widgets::line_height(env, title_size);
        for part in parts {
            total += match part {
                Part::Bar => bar_h + gap,
                _ => small + line_gap,
            };
        }
        total
    };
    for drop in [
        Part::Left,
        Part::Caption,
        Part::Bar,
        Part::Author,
        Part::Pages,
    ] {
        if height(&parts) <= rest.height {
            break;
        }
        parts.retain(|p| *p != drop);
    }
    let has = |part: Part| parts.contains(&part);
    let total = height(&parts).min(rest.height);
    let mut y = rest.y + (rest.height - total) / 2;
    let align = widgets::align(env, id);
    let mut top = Lines::new();
    if has(Part::Caption) {
        top.add(
            style.small,
            env.i18n.tr("widgets.reading.caption"),
            look.faint,
        );
    }
    top.add(title_size, book.title.as_str(), look.ink);
    if has(Part::Author) {
        top.add(style.small, book.author.as_str(), look.faint);
    }
    let h = top.height(env);
    top.draw(canvas, env, Rect::new(rest.x, y, rest.width, h), align);
    y += h;
    if has(Part::Bar) {
        let bar = Rect::new(rest.x, y + gap / 2, rest.width, bar_h);
        draw_bar(canvas, env, look, bar, book.percent());
        y += bar_h + gap;
    }
    let mut bottom = Lines::new();
    if has(Part::Pages) {
        bottom.add(style.small, where_text(env, book, position), look.ink);
    }
    if let (true, Some(left)) = (has(Part::Left), left) {
        bottom.add(style.small, left, look.faint);
    }
    if !bottom.lines.is_empty() {
        let h = bottom.height(env);
        bottom.draw(
            canvas,
            env,
            Rect::new(rest.x, y + line_gap, rest.width, h),
            align,
        );
    }
}
