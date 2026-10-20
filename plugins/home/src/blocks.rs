//! The home screen's own widgets, which its pages are made of (any
//! plugin's widgets go on them too), and its tools:
//!
//! - status line: the time, Wi-Fi while in a network, and the battery's
//!   charge (above the grid by default, see `pages`);
//! - date: today in words;
//! - clock: the time large, with the date;
//! - continue reading: the book read last, with its cover and progress;
//! - recently read: the books before it, as a row of covers;
//! - tools: an action's icon and name, in a cell of the grid.
//!
//! Every widget has settings of its own (from the editor, *Settings* of
//! the widget): where its text sits, how large it is, and which of its
//! parts it shows. Widgets fill the cells they are given.
//!
//! Widgets draw what is stored (covers once looked up, see `covers`),
//! never opening books themselves, and read it when the home screen
//! updates them, not as they are drawn, so drawing stays quick: covers
//! are made small for their place once it is known ([`Widget::place`])
//! and drawn from memory in every band.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params, screens, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::status::{StatusLine, item_ids};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Drawn, Frame, Size, Widget, WidgetSpec, WidgetTarget, grid, line_height,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, Id, Params, Rect, RegisterError, Registrar, SettingExtra,
    SettingKind, SettingSpec, SettingValue, time,
};

use crate::pages::{CONTINUE, DATE, RECENT, STATUS};
use crate::recent;
use paperpaw_sdk::setting::{Setting, Text};

/// The clock with the date, large.
pub const CLOCK: &str = "home.clock";
/// Whether the clock shows the date below the time.
const CLOCK_DATE: Setting<bool> = Setting::new("home.clock.date");
/// Whether the continue card shows the cover, the progress.
const CARD_COVER: Setting<bool> = Setting::new("home.continue.cover");
const CARD_PROGRESS: Setting<bool> = Setting::new("home.continue.progress");
/// Whether the recent books show their heading and their titles, how many
/// there are at most.
const RECENT_HEADING: Setting<bool> = Setting::new("home.recent.heading");
const RECENT_TITLES: Setting<bool> = Setting::new("home.recent.titles");
const RECENT_COUNT: Setting<i64> = Setting::new("home.recent.count");
/// Whether tools show their names below their icons.
pub(crate) const TOOL_LABELS: Setting<bool> = Setting::new("home.tool-labels");
/// Whether the focus shows at once where there is no touch, or only
/// with the first press (the page seen as it is first).
pub(crate) const FOCUS_AT_ONCE: Setting<bool> = Setting::new("home.focus-at-once");
/// The status items of the status line's left, centre and right (ids
/// separated by commas).
pub(crate) const STATUS_PARTS: [Setting<Text>; 3] = [STATUS_LEFT, STATUS_CENTER, STATUS_RIGHT];
const STATUS_LEFT: Setting<Text> = Setting::new("home.status.left");
const STATUS_CENTER: Setting<Text> = Setting::new("home.status.center");
const STATUS_RIGHT: Setting<Text> = Setting::new("home.status.right");
/// The status line's parts, each a list of status items chosen on their
/// picker: the time on the left; Wi-Fi and the battery on the right.
static STATUS_ITEMS: SettingExtra = SettingExtra::new().editor(screens::STATUS_ITEMS);
static STATUS_SETTINGS: [SettingSpec; 3] = [
    status_part(
        STATUS_LEFT.static_str(),
        "home.status.left",
        "core.status.clock",
    ),
    status_part(STATUS_CENTER.static_str(), "home.status.center", ""),
    status_part(
        STATUS_RIGHT.static_str(),
        "home.status.right",
        "wifi.status, core.status.battery",
    ),
];

const fn status_part(id: &'static str, title: &'static str, default: &'static str) -> SettingSpec {
    SettingSpec::fixed(
        id,
        title,
        SettingKind::List,
        SettingValue::Text(Cow::Borrowed(default)),
        Id::new_static(STATUS),
    )
    .with_extra(&STATUS_ITEMS)
}

/// Recent books shown as covers at most.
const MAX_COVERS: usize = 8;

/// How many books the recent shelf shows at most.
static RECENT_SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_number(
    "home.recent.count",
    "home.setting.recent-count",
    1,
    MAX_COVERS as i64,
    6,
    Id::new_static(RECENT),
)];

/// The widgets' settings: where each sits, its frame, size and parts.
static SETTINGS: [SettingSpec; 16] = {
    use widgets::{align_spec, frame_spec, part_spec, size_spec};
    const STATUS_ID: Id = Id::new_static(STATUS);
    const DATE_ID: Id = Id::new_static(DATE);
    const CLOCK_ID: Id = Id::new_static(CLOCK);
    const CONTINUE_ID: Id = Id::new_static(CONTINUE);
    const RECENT_ID: Id = Id::new_static(RECENT);
    [
        frame_spec("home.status.frame", STATUS_ID, Frame::None),
        align_spec("home.date.align", DATE_ID, Align::Center),
        frame_spec("home.date.frame", DATE_ID, Frame::None),
        size_spec("home.date.size", DATE_ID, Size::Medium, false),
        align_spec("home.clock.align", CLOCK_ID, Align::Center),
        frame_spec("home.clock.frame", CLOCK_ID, Frame::None),
        size_spec("home.clock.size", CLOCK_ID, Size::Fit, true),
        part_spec("home.clock.date", CLOCK_ID, "home.setting.clock-date", true),
        align_spec("home.continue.align", CONTINUE_ID, Align::Left),
        frame_spec("home.continue.frame", CONTINUE_ID, Frame::Line),
        part_spec(
            "home.continue.cover",
            CONTINUE_ID,
            "home.setting.card-cover",
            true,
        ),
        part_spec(
            "home.continue.progress",
            CONTINUE_ID,
            "home.setting.card-progress",
            true,
        ),
        align_spec("home.recent.align", RECENT_ID, Align::Center),
        frame_spec("home.recent.frame", RECENT_ID, Frame::None),
        part_spec(
            "home.recent.heading",
            RECENT_ID,
            "home.setting.recent-heading",
            true,
        ),
        part_spec(
            "home.recent.titles",
            RECENT_ID,
            "home.setting.recent-titles",
            true,
        ),
    ]
};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    use widgets::categories;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(STATUS),
            "home.widget.status",
            Drawn::new(status_height, draw_status).shows(status_signature),
        )
        .category(categories::DEVICE),
    )?;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(DATE),
            "home.widget.date",
            Drawn::new(date_height, draw_date).shows(date_text),
        )
        .category(categories::TIME),
    )?;
    r.add_widget(
        WidgetSpec::new(Id::new_static(CONTINUE), "home.widget.continue", |_| {
            Box::new(Card::default())
        })
        .category(categories::BOOKS),
    )?;
    r.add_widget(
        WidgetSpec::new(Id::new_static(RECENT), "home.widget.recent", |_| {
            Box::new(Shelf::default())
        })
        .category(categories::BOOKS),
    )?;
    r.add_widget(
        WidgetSpec::drawn(
            Id::new_static(CLOCK),
            "home.widget.clock",
            Drawn::new(clock_height, draw_clock).shows(clock_shows),
        )
        .category(categories::TIME),
    )?;

    // The status line's parts: the time on the left; Wi-Fi and the
    // battery on the right.
    let status = Id::new_static(STATUS);
    r.add_setting_group(widgets::settings_group(&status, "home.widget.status"))?;
    r.add_settings(&STATUS_SETTINGS)?;
    // Each other widget's settings, in its own group.
    for (id, title) in [
        (DATE, "home.widget.date"),
        (CLOCK, "home.widget.clock"),
        (CONTINUE, "home.widget.continue"),
        (RECENT, "home.widget.recent"),
    ] {
        r.add_setting_group(widgets::settings_group(&Id::new_static(id), title))?;
    }
    r.add_settings(&SETTINGS)?;
    r.add_settings(&RECENT_SETTINGS)?;
    Ok(())
}

/// The height `widget`'s frame takes above and below its content.
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 100, 100);
    100 - widgets::look::look(env, widget, probe).area.height
}

/// `height` pixels placed in the middle of `area`, top to bottom.
fn centred(area: Rect, height: u16) -> u16 {
    area.y + area.height.saturating_sub(height) / 2
}

// ---- Status line --------------------------------------------------------

pub(crate) fn status_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small) + style.gap as u16 / 2 + frame_height(env, STATUS)
}

/// The status line: the status items chosen for its left, centre and
/// right ([`STATUS_PARTS`]), any plugin's, as the reader's lines have them.
pub(crate) fn draw_status(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, STATUS, area);
    let line = StatusLine {
        area: look.area,
        lists: STATUS_PARTS.each_ref().map(|part| env.settings.read(part)),
        size: style.small,
        color: if look.frame == Frame::Dark {
            look.faint
        } else {
            style.faint(60)
        },
        percent: true,
    };
    line.draw(canvas, env, &font, &Params::new());
}

/// What the status line shows now, to draw it again when it changes.
fn status_signature(env: &Env<'_>) -> String {
    let mut signature = String::new();
    for part in &STATUS_PARTS {
        for id in item_ids(env.settings.read(part)) {
            if let Some(value) = env.status_items.value(id, env, &Params::new()) {
                signature.push_str(&format!("{value:?}|"));
            }
        }
        signature.push('/');
    }
    signature
}

// ---- Date ---------------------------------------------------------------

/// The date in words: "Thursday, 15 October".
pub(crate) fn date_text(env: &Env<'_>) -> String {
    time::date_text(env).unwrap_or_default()
}

fn date_height(env: &Env<'_>, _: u16) -> u16 {
    if time::local_time(env).is_none() {
        return 0;
    }
    let style = UiStyle::new(env);
    line_height(env, widgets::size(env, DATE).text(&style)) + frame_height(env, DATE)
}

fn draw_date(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let look = widgets::draw_frame(canvas, env, DATE, area);
    let area = look.area;
    if let Some(font) = style.font(env) {
        // Smaller rather than cut short, down to the body's size.
        let text = date_text(env);
        let width = f32::from(area.width);
        let mut size = widgets::size(env, DATE).text(&style);
        while size > style.text && font.measure(size, &text) > width {
            size = (size * 0.9).max(style.text);
        }
        let date = ui::fit(&font, size, &text, width);
        let x = widgets::align(env, DATE).x(area, font.measure(size, &date));
        ui::draw_label(canvas, &font, size, x, area, &date, look.ink);
    }
}

// ---- Clock ----------------------------------------------------------------

/// What the clock shows: the time and the date.
fn clock_shows(env: &Env<'_>) -> String {
    format!(
        "{}|{}",
        time::clock_text(env).unwrap_or_default(),
        date_text(env)
    )
}

/// The clock's size in `area`: as set, or as large as fits.
fn clock_size(env: &Env<'_>, style: &UiStyle, area: Option<Rect>) -> f32 {
    let size = widgets::size(env, CLOCK);
    if size != Size::Fit {
        return size.text(style) * 1.4;
    }
    let Some(area) = area else {
        return style.title * 2.2;
    };
    let Some(font) = style.font(env) else {
        return style.title * 2.2;
    };
    // The digits fill the height left by the date, and fit the width.
    let date = if env.settings.read(&CLOCK_DATE) {
        f32::from(line_height(env, style.small))
    } else {
        0.0
    };
    let by_height = (f32::from(area.height) - date) * 0.7;
    let sample = time::clock_text(env).unwrap_or_else(|| String::from("00:00"));
    let at_title = font.measure(style.title, &sample).max(1.0);
    let by_width = style.title * f32::from(area.width) * 0.95 / at_title;
    by_height.min(by_width).max(style.title)
}

fn clock_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    let date = if env.settings.read(&CLOCK_DATE) {
        line_height(env, style.small)
    } else {
        0
    };
    line_height(env, clock_size(env, &style, None))
        + date
        + style.gap as u16
        + frame_height(env, CLOCK)
}

fn draw_clock(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, CLOCK, area);
    let area = look.area;
    let size = clock_size(env, &style, Some(area));
    let big = line_height(env, size);
    let with_date = env.settings.read(&CLOCK_DATE);
    let small = if with_date {
        line_height(env, style.small)
    } else {
        0
    };
    let top = centred(area, big + small);
    let clock = time::clock_text(env).unwrap_or_else(|| String::from("--:--"));
    let align = widgets::align(env, CLOCK);
    ui::draw_label(
        canvas,
        &font,
        size,
        align.x(area, font.measure(size, &clock)),
        Rect::new(area.x, top, area.width, big),
        &clock,
        look.ink,
    );
    if !with_date {
        return;
    }
    let date = date_text(env);
    ui::draw_label(
        canvas,
        &font,
        style.small,
        align.x(area, font.measure(style.small, &date)),
        Rect::new(area.x, top + big, area.width, small),
        &date,
        look.faint,
    );
}

// ---- Continue reading -----------------------------------------------------

fn card_height(env: &Env<'_>, _: u16) -> u16 {
    (UiStyle::new(env).row as f32 * 3.0) as u16
}

/// A book a widget shows, and its cover if it has one (looked up already).
#[derive(Debug, Clone, PartialEq)]
struct Book {
    book: recent::Recent,
    cover: Option<ImageRef>,
}

impl Book {
    fn of(env: &Env<'_>, book: recent::Recent) -> Self {
        Self {
            cover: book.cover.clone().filter(|i| env.storage.exists(&i.path)),
            book,
        }
    }

    fn open(&self) -> Command {
        let params = Params::new().with(params::PATH, self.book.path.clone());
        Command::OpenSlot(slots::READER, params)
    }
}

/// Where `image` goes, fitted into `place`: `x` and `y` say where in it,
/// from 0 (left, top) to 1 (right, bottom).
fn fitted(image: &ImageRef, place: Rect, x: f32, y: f32) -> Rect {
    let (w, h) = ui::fit_image(image, place.width, place.height);
    let left = f32::from(place.width - w) * x;
    let top = f32::from(place.height - h) * y;
    Rect::new(place.x + left as u16, place.y + top as u16, w, h)
}

/// `image` made small to `at`, as the panel shows it: read once, then
/// drawn from memory.
fn small_cover(env: &Env<'_>, image: &ImageRef, at: Rect) -> Option<Thumbnail> {
    let format = env.device.display.format;
    Thumbnail::new(env.storage, image, at.width, at.height, format)
}

/// Draws `image` at `at`: from `small` where it was made for that size,
/// otherwise from its file (where there was no memory to make it small).
fn draw_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    image: &ImageRef,
    small: Option<&Thumbnail>,
    at: Rect,
) {
    match small {
        Some(small) if (small.width(), small.height()) == (at.width, at.height) => {
            small.draw(canvas, at.x, at.y);
        }
        _ => ui::draw_image(canvas, env.storage, image, at),
    }
}

/// The book read last; the library before any was read.
#[derive(Debug, Default)]
struct Card {
    book: Option<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    /// The book's cover, made small for that place.
    cover: Option<Thumbnail>,
}

impl Card {
    /// The cover `book` shows in `area` and where, if it shows one: on
    /// the left, in the middle from top to bottom.
    fn cover_place<'b>(
        env: &Env<'_>,
        area: Rect,
        book: Option<&'b Book>,
    ) -> Option<(&'b ImageRef, Rect)> {
        let image = book?.cover.as_ref()?;
        if !env.settings.read(&CARD_COVER) {
            return None;
        }
        let r = widgets::look::look(env, CONTINUE, area).area;
        let place = Rect::new(r.x, r.y, r.width / 3, r.height);
        Some((image, fitted(image, place, 0.0, 0.5)))
    }

    fn make_cover(&mut self, env: &Env<'_>) {
        self.cover = self.area.and_then(|area| {
            let (image, at) = Self::cover_place(env, area, self.book.as_ref())?;
            small_cover(env, image, at)
        });
    }
}

impl Widget for Card {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = recent::load(env.storage, 1)
            .into_iter()
            .next()
            .map(|book| Book::of(env, book));
        let changed = book != self.book;
        self.book = book;
        if changed {
            self.make_cover(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_cover(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        card_height(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        draw_card(canvas, env, area, self.book.as_ref(), self.cover.as_ref());
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::RunAction(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget { area, command }]
    }
}

fn draw_card(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    shown: Option<&Book>,
    small: Option<&Thumbnail>,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, CONTINUE, area);
    let r = look.area;
    let Some(Book { book, .. }) = shown else {
        // Nothing read yet: where the books are (a tap opens the library).
        ui::draw_empty(
            canvas,
            env,
            &style,
            r,
            "books",
            env.i18n.tr("home.start.title"),
            env.i18n.tr("home.start.text"),
        );
        return;
    };
    // The cover on the left, where there is one.
    let r = match Card::cover_place(env, area, shown) {
        Some((image, at)) => {
            draw_cover(canvas, env, image, small, at);
            let shift = at.width + style.gap as u16;
            Rect::new(r.x + shift, r.y, r.width.saturating_sub(shift), r.height)
        }
        None => r,
    };
    let inner = f32::from(r.width);
    let progress = env.settings.read(&CARD_PROGRESS);
    // The lines of text, together in the middle.
    let label_h = line_height(env, style.small);
    // Long titles in the body size rather than cut short.
    let size = if font.measure(style.title, &book.title) <= inner {
        style.title
    } else {
        style.text
    };
    let title_h = line_height(env, size);
    let gap = style.gap as u16 / 2;
    let total = label_h + gap + title_h + if progress { gap + label_h } else { 0 };
    let mut y = centred(r, total);
    let align = widgets::align(env, CONTINUE);
    let row = |y: u16, h: u16| Rect::new(r.x, y, r.width, h);
    let label = env.i18n.tr("home.continue");
    ui::draw_label(
        canvas,
        &font,
        style.small,
        align.x(r, font.measure(style.small, label)),
        row(y, label_h),
        label,
        look.faint,
    );
    y += label_h + gap;
    let title = ui::fit(&font, size, &book.title, inner);
    ui::draw_label(
        canvas,
        &font,
        size,
        align.x(r, font.measure(size, &title)),
        row(y, title_h),
        &title,
        look.ink,
    );
    y += title_h + gap;
    if !progress {
        return;
    }
    let percent = env.i18n.format(
        "home.percent",
        &[("percent", Arg::Int((book.progress + 0.5) as i64))],
    );
    let x = f32::from(r.x);
    ui::draw_label(
        canvas,
        &font,
        style.small,
        x,
        row(y, label_h),
        &percent,
        look.faint,
    );
    // The progress bar, beside it.
    let bar_h = (style.line * 4).max(3);
    let percent_w = font.measure(style.small, &percent) + style.gap as f32;
    let bx = (x + percent_w) as i32;
    let by = i32::from(y) + i32::from(label_h) / 2 - bar_h as i32 / 2;
    let w = (inner - percent_w).max(1.0) as u32;
    canvas.stroke_rect(bx, by, w, bar_h, style.line.max(1), look.ink);
    let filled = (w as f32 * (book.progress / 100.0).clamp(0.0, 1.0)) as u32;
    canvas.fill_rect(bx, by, filled, bar_h, look.ink);
}

// ---- Recently read ----------------------------------------------------------

/// The recent books after the current one, as many as set.
#[derive(Debug, Default)]
struct Shelf {
    books: Vec<Book>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    /// The books' covers, made small for their places.
    covers: Vec<Option<Thumbnail>>,
}

impl Shelf {
    fn make_covers(&mut self, env: &Env<'_>) {
        let Some(area) = self.area else {
            return self.covers.clear();
        };
        let area = widgets::look::look(env, RECENT, area).area;
        self.covers = shelf_places(env, area, &self.books)
            .into_iter()
            .map(|(place, _, book)| {
                let image = book.cover.as_ref()?;
                small_cover(env, image, fitted(image, place, 0.5, 1.0))
            })
            .collect();
    }
}

impl Widget for Shelf {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let count = shelf_count(env);
        let books: Vec<Book> = recent::load(env.storage, count + 1)
            .into_iter()
            .skip(1)
            .take(count)
            .map(|book| Book::of(env, book))
            .collect();
        let changed = books != self.books;
        self.books = books;
        if changed {
            self.make_covers(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        shelf_height(env, width, &self.books)
    }

    /// Covers a third as high, where there is less room.
    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        let (heading, title) = shelf_labels(env);
        let full = shelf_height(env, width, &self.books);
        let covers = full.saturating_sub(heading + title);
        if full == 0 {
            0
        } else {
            heading + title + covers / 3
        }
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        draw_shelf(canvas, env, area, &self.books, &self.covers);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let area = widgets::look::look(env, RECENT, area).area;
        shelf_places(env, area, &self.books)
            .into_iter()
            .map(|(cover, title, book)| WidgetTarget {
                area: cover.union(&title),
                command: book.open(),
            })
            .collect()
    }
}

/// How many books the row shows at most: as many as set, but no more
/// than fit across the screen (looking each up takes time).
fn shelf_count(env: &Env<'_>) -> usize {
    shelf_columns(env, env.device.display.width).0
}

/// The covers a row holds at `width`, and how wide each one's place is.
fn shelf_columns(env: &Env<'_>, width: u16) -> (usize, u16) {
    let count = env.settings.read(&RECENT_COUNT).clamp(1, MAX_COVERS as i64) as u32;
    let columns = (u32::from(width) / env.metrics.mm(18).max(1)).clamp(1, count);
    (columns as usize, width / columns as u16)
}

/// Heights of the heading and of a title below a cover (0 where not
/// shown).
fn shelf_labels(env: &Env<'_>) -> (u16, u16) {
    let style = UiStyle::new(env);
    let label = line_height(env, style.small);
    let heading = if env.settings.read(&RECENT_HEADING) {
        label + style.gap as u16 / 2
    } else {
        0
    };
    let title = if env.settings.read(&RECENT_TITLES) {
        label
    } else {
        0
    };
    (heading, title)
}

fn shelf_height(env: &Env<'_>, width: u16, books: &[Book]) -> u16 {
    if books.is_empty() {
        return 0;
    }
    let style = UiStyle::new(env);
    let (_, cell) = shelf_columns(env, width);
    let (heading, title) = shelf_labels(env);
    let cover = (cell.saturating_sub(style.gap as u16)) * 4 / 3;
    heading + cover + title + frame_height(env, RECENT)
}

/// Where each cover goes, and its title below (as wide as its column),
/// with its book: as large as the area allows, the row in the middle.
fn shelf_places<'b>(env: &Env<'_>, area: Rect, books: &'b [Book]) -> Vec<(Rect, Rect, &'b Book)> {
    let style = UiStyle::new(env);
    // Few books: larger covers, as many columns as books.
    let (fit, _) = shelf_columns(env, area.width);
    let columns = books.len().clamp(1, fit);
    let shown = columns;
    let (heading, title) = shelf_labels(env);
    let gap = style.gap as u16;
    let room = area.height.saturating_sub(heading + title);
    // Covers 3:4, as wide as the columns or as high as the room allows.
    let column = area.width / columns as u16;
    let w = column.saturating_sub(gap).min(room * 3 / 4);
    let h = w * 4 / 3;
    let row_w = column * shown as u16;
    let left = area.x + (area.width.saturating_sub(row_w)) / 2;
    let top = area.y + heading + room.saturating_sub(h) / 2;
    books
        .iter()
        .take(columns)
        .enumerate()
        .map(|(i, book)| {
            let start = left + i as u16 * column;
            let cover = Rect::new(start + (column - w) / 2, top, w, h);
            let label = Rect::new(start + gap / 2, top + h, column.saturating_sub(gap), title);
            (cover, label, book)
        })
        .collect()
}

fn draw_shelf(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    books: &[Book],
    small: &[Option<Thumbnail>],
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    // Nothing read before the current book: nothing to show.
    if books.is_empty() {
        return;
    }
    let look = widgets::draw_frame(canvas, env, RECENT, area);
    let area = look.area;
    let (heading, title_h) = shelf_labels(env);
    let align = widgets::align(env, RECENT);
    if heading > 0 {
        let text = env.i18n.tr("home.recent");
        ui::draw_label(
            canvas,
            &font,
            style.small,
            align.x(area, font.measure(style.small, text)),
            Rect::new(area.x, area.y, area.width, line_height(env, style.small)),
            text,
            look.faint,
        );
    }
    let places = shelf_places(env, area, books);
    for (i, (cover, row, Book { book, cover: image })) in places.into_iter().enumerate() {
        match image {
            // In the middle of its place, at the bottom.
            Some(image) => {
                let at = fitted(image, cover, 0.5, 1.0);
                let small = small.get(i).and_then(Option::as_ref);
                draw_cover(canvas, env, image, small, at);
            }
            None => draw_placeholder(canvas, env, &style, cover, &book.title, look.ink),
        }
        if title_h == 0 {
            continue;
        }
        let title = ui::fit(&font, style.small, &book.title, f32::from(row.width));
        let x = align.x(row, font.measure(style.small, &title));
        ui::draw_label(canvas, &font, style.small, x, row, &title, look.ink);
    }
}

/// A book without a cover: a frame with its title.
fn draw_placeholder(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    title: &str,
    ink: Color,
) {
    canvas.stroke(area, style.line.max(2), ink);
    let Some(font) = style.font(env) else {
        return;
    };
    let pad = style.gap as f32 / 2.0;
    let width = f32::from(area.width) - 2.0 * pad;
    let line = font.line_metrics(style.small).line_height;
    let fits = ((f32::from(area.height) - 2.0 * pad) / line).max(1.0) as usize;
    // Broken between words only; a word longer than a line shortened.
    let mut lines: Vec<String> = Vec::new();
    for word in title.split_whitespace() {
        let joined = lines.last().map(|last| format!("{last} {word}"));
        match joined {
            Some(joined) if font.measure(style.small, &joined) <= width => {
                if let Some(last) = lines.last_mut() {
                    *last = joined;
                }
            }
            _ => lines.push(ui::fit(&font, style.small, word, width)),
        }
    }
    for (n, text) in lines.iter().take(fits).enumerate() {
        let y = area.y + pad as u16 + (n as f32 * line) as u16;
        let row = Rect::new(area.x, y, area.width, line as u16);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(area.x) + pad,
            row,
            text,
            ink,
        );
    }
}

// ---- Tools ----------------------------------------------------------------

/// A tool in its cells: its icon, and its name below (where shown),
/// together in the middle.
pub(crate) fn draw_tool(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, action: &Id) {
    let labels = env.settings.read(&TOOL_LABELS);
    paperpaw_sdk::widgets::grid::draw_tool(canvas, env, area, action, labels);
}

/// How many of the recent books the widgets in `items` show: the current
/// book, and the ones after it on the shelf.
pub(crate) fn books_shown(env: &Env<'_>, items: &[grid::Item]) -> usize {
    items
        .iter()
        .map(|item| match &item.what {
            // Each shelf as many as its placement shows.
            grid::What::Widget(id) if widgets::base(id) == RECENT => {
                env.settings.with_placement(Some(id), || shelf_count(env)) + 1
            }
            grid::What::Widget(id) if widgets::base(id) == CONTINUE => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// The first `books` recent books whose covers were not looked up yet.
pub(crate) fn missing_covers(env: &Env<'_>, books: usize) -> Vec<String> {
    recent::load(env.storage, books)
        .into_iter()
        .filter(|b| !b.indexed)
        .map(|b| b.path)
        .collect()
}
