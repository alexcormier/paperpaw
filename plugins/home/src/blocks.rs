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
use paperpaw_sdk::status::{StatusLine, StatusValue, item_ids};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{
    self, Align, Drawn, Frame, Size, Widget, WidgetSpec, WidgetTarget, grid, line_height,
};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, FontHandle, Id, Params, Rect, RegisterError, Registrar,
    SettingExtra, SettingKind, SettingSpec, SettingValue, time,
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
static SETTINGS: [SettingSpec; 21] = {
    use widgets::{Padding, align_spec, frame_spec, padding_spec, part_spec, size_spec};
    const STATUS_ID: Id = Id::new_static(STATUS);
    const DATE_ID: Id = Id::new_static(DATE);
    const CLOCK_ID: Id = Id::new_static(CLOCK);
    const CONTINUE_ID: Id = Id::new_static(CONTINUE);
    const RECENT_ID: Id = Id::new_static(RECENT);
    [
        frame_spec("home.status.frame", STATUS_ID, Frame::None),
        padding_spec("home.status.padding", STATUS_ID, Padding::Auto),
        align_spec("home.date.align", DATE_ID, Align::Center),
        frame_spec("home.date.frame", DATE_ID, Frame::None),
        padding_spec("home.date.padding", DATE_ID, Padding::Auto),
        size_spec("home.date.size", DATE_ID, Size::Medium, false),
        align_spec("home.clock.align", CLOCK_ID, Align::Center),
        frame_spec("home.clock.frame", CLOCK_ID, Frame::None),
        padding_spec("home.clock.padding", CLOCK_ID, Padding::Auto),
        size_spec("home.clock.size", CLOCK_ID, Size::Fit, true),
        part_spec("home.clock.date", CLOCK_ID, "home.setting.clock-date", true),
        align_spec("home.continue.align", CONTINUE_ID, Align::Left),
        frame_spec("home.continue.frame", CONTINUE_ID, Frame::Line),
        padding_spec("home.continue.padding", CONTINUE_ID, Padding::Auto),
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
        padding_spec("home.recent.padding", RECENT_ID, Padding::Auto),
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
            Drawn::new(status_widget_height, draw_status_widget).shows(status_signature),
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

/// The height of the status line above the grid: without a frame, flush
/// with the screen's margins (its spacing setting is for the widget).
pub(crate) fn status_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    let frame = if widgets::frame(env, STATUS) == Frame::None {
        0
    } else {
        frame_height(env, STATUS)
    };
    line_height(env, style.small) + style.gap as u16 / 2 + frame
}

/// The status line above the grid ([`status_height`]): the status items
/// chosen for its left, centre and right ([`STATUS_PARTS`]), any
/// plugin's, as the reader's lines have them.
pub(crate) fn draw_status(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, STATUS, area);
    let inner = if look.frame == Frame::None {
        area
    } else {
        look.area
    };
    draw_status_line(canvas, env, &look, inner);
}

/// The status line as a widget in the grid: inside its spacing.
fn status_widget_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small) + style.gap as u16 / 2 + frame_height(env, STATUS)
}

fn draw_status_widget(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, STATUS, area);
    draw_status_line(canvas, env, &look, look.area);
}

/// The status items in `area`, as many as fit whole: where all do not,
/// the battery's percentage is left out first, then the centre's items,
/// then the left's (the percentage back where that makes room), so that
/// nothing is cut or drawn outside.
fn draw_status_line(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &widgets::Look, area: Rect) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let size = style.small;
    let mut lists = STATUS_PARTS.each_ref().map(|part| env.settings.read(part));
    let mut percent = true;
    let room = f32::from(area.width);
    'fit: for drop in [None, Some(1), Some(0)] {
        if let Some(part) = drop {
            lists[part] = "";
        }
        for with in [true, false] {
            percent = with;
            if status_width(env, &font, size, &lists, percent) <= room {
                break 'fit;
            }
        }
    }
    let line = StatusLine {
        area,
        lists,
        size,
        color: if look.frame == Frame::Dark {
            look.faint
        } else {
            style.faint(60)
        },
        percent,
    };
    line.draw(canvas, env, &font, &Params::new());
}

/// How wide the status items of `lists` are side by side at `size`,
/// with the narrowest gaps the status line leaves between them (as
/// [`StatusLine`] measures them).
fn status_width(
    env: &Env<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    size: f32,
    lists: &[&str; 3],
    percent: bool,
) -> f32 {
    let mut width = 0.0;
    let mut count = 0usize;
    for list in lists {
        for id in item_ids(list) {
            let Some(value) = env.status_items.value(id, env, &Params::new()) else {
                continue;
            };
            count += 1;
            width += match value {
                StatusValue::Icon(name) => ui::icon_width(env, &name, size),
                StatusValue::Text(text) => font.measure(size, &text),
                StatusValue::Level { percent: p, .. } if percent => {
                    size * 1.6 + font.measure(size, &format!("{p} %"))
                }
                StatusValue::Level { .. } => size * 1.3,
                StatusValue::Bar { .. } => size * 4.0,
            };
        }
    }
    width + size * 0.4 * count.saturating_sub(1) as f32
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

/// Today as long as fits `width` pixels whole at a size from `max` down
/// to `min`: in full ("Thursday, 15 October"), else short ("Thu 15
/// Oct"); `None` where not even the short one does.
fn fitting_date(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    max: f32,
    min: f32,
    width: f32,
) -> Option<(f32, String)> {
    [time::date_text(env), time::short_date_text(env)]
        .into_iter()
        .flatten()
        .find_map(|text| {
            widgets::largest_size(max, min, |size| font.measure(size, &text) <= width)
                .map(|size| (size, text))
        })
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
    let Some(font) = style.font(env) else {
        return;
    };
    // As large as set, smaller down to the body's size rather than cut
    // short, then in short words; where not even those fit a line, in
    // lines ("Wed" above "14 Oct"), down to the small size.
    let width = f32::from(area.width);
    let max = widgets::size(env, DATE).text(&style);
    let align = widgets::align(env, DATE);
    if let Some((size, text)) = fitting_date(env, &font, max, style.text.min(max), width) {
        widgets::draw_line(canvas, env, area, align, size, size, &text, look.ink);
    } else if let Some(short) = time::short_date_text(env) {
        // The weekday above the day and the month.
        let short = match short.split_once(' ') {
            Some((weekday, rest)) => format!("{}\n{rest}", weekday.trim_end_matches(',')),
            None => short,
        };
        let lines =
            widgets::Wrapped::fitting(env, &short, max, style.small, area.width, area.height);
        lines.draw(canvas, env, area, align, look.ink);
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

/// The clock's size for `clock` in `width` pixels with `height` pixels
/// for its digits: as set, or as large as fits; smaller where the width
/// needs it, down to the body's size.
fn clock_size(
    env: &Env<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    clock: &str,
    width: f32,
    height: f32,
) -> f32 {
    let want = match widgets::size(env, CLOCK) {
        // The digits about half the height (their line about 0.7).
        Size::Fit => (height * 0.7).max(style.text),
        size => size.text(style) * 1.4,
    };
    widgets::largest_size(want, style.text, |size| font.measure(size, clock) <= width)
        .unwrap_or(style.text)
}

fn clock_height(env: &Env<'_>, _: u16) -> u16 {
    let style = UiStyle::new(env);
    let date = if env.settings.read(&CLOCK_DATE) {
        line_height(env, style.small)
    } else {
        0
    };
    let size = match widgets::size(env, CLOCK) {
        Size::Fit => style.title * 2.2,
        size => size.text(&style) * 1.4,
    };
    line_height(env, size) + date + style.gap as u16 + frame_height(env, CLOCK)
}

/// The top and bottom of the ink of `text` at `size` from its baseline
/// (the top negative); the line's where it draws nothing.
fn ink_of(font: &FontHandle<'_>, size: f32, text: &str) -> (i32, i32) {
    font.ink(size, text).unwrap_or_else(|| {
        let metrics = font.line_metrics(size);
        (-(metrics.ascent as i32), -(metrics.descent as i32))
    })
}

fn draw_clock(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, CLOCK, area);
    let area = look.area;
    let width = f32::from(area.width);
    let clock = time::clock_text(env).unwrap_or_else(|| String::from("--:--"));
    let gap = (style.gap / 2) as i32;
    // The date below where it fits whole (in short words where the full
    // ones do not), small; the time in the rest.
    let mut date = if env.settings.read(&CLOCK_DATE) {
        fitting_date(env, &font, style.small, style.small, width).map(|(size, text)| {
            let (top, bottom) = ink_of(&font, size, &text);
            (size, text, top, bottom)
        })
    } else {
        None
    };
    let date_height = |date: &Option<(f32, String, i32, i32)>| {
        date.as_ref()
            .map_or(0, |(_, _, top, bottom)| bottom - top + gap)
    };
    let digits = |date: &Option<(f32, String, i32, i32)>| {
        let room = (i32::from(area.height) - date_height(date)).max(0) as f32;
        let size = clock_size(env, &style, &font, &clock, width, room);
        let (top, bottom) = ink_of(&font, size, &clock);
        (size, top, bottom)
    };
    let (mut size, mut top, mut bottom) = digits(&date);
    // Both do not fit: the time alone.
    if date.is_some() && bottom - top + date_height(&date) > i32::from(area.height) {
        date = None;
        (size, top, bottom) = digits(&date);
    }
    // Together in the middle by their ink.
    let total = bottom - top + date_height(&date);
    let y = i32::from(area.y) + (i32::from(area.height) - total) / 2;
    let align = widgets::align(env, CLOCK);
    // Shortened only where not even the body's size fits (a sliver).
    let shown = ui::fit(&font, size, &clock, width);
    let x = align.x(area, font.measure(size, &shown));
    let high = (bottom - top) as u16;
    let at = Rect::new(x.max(0.0) as u16, y.max(0) as u16, area.width, high);
    canvas.describe(paperpaw_sdk::semantics::Role::Text, &clock, at, false, None);
    font.draw(canvas, size, x, y - top, &shown, look.ink);
    if let Some((small, text, date_top, date_bottom)) = &date {
        let y = y + (bottom - top) + gap;
        let x = align.x(area, font.measure(*small, text));
        font.draw(canvas, *small, x, y - date_top, text, look.faint);
        let high = (date_bottom - date_top) as u16;
        let at = Rect::new(x.max(0.0) as u16, y.max(0) as u16, area.width, high);
        canvas.describe(paperpaw_sdk::semantics::Role::Text, text, at, false, None);
    }
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
    /// Where its parts go there, worked out with the place.
    parts: CardParts,
    /// The book's cover, made small for that place.
    cover: Option<Thumbnail>,
}

/// Where the parts of the continue card go: the cover, and beside it
/// the column of text with the caption, the title and the progress,
/// each where it fits.
#[derive(Debug, Default, Clone, PartialEq)]
struct CardParts {
    /// Where the cover goes, if it shows.
    cover: Option<Rect>,
    /// The column of text; none where the cover alone fits.
    text: Option<Rect>,
    /// "Continue reading" above the title.
    caption: bool,
    /// The title, in one line or two.
    title: widgets::Wrapped,
    /// The percentage and the bar below.
    progress: bool,
}

impl CardParts {
    /// The parts of `book`'s card in `area`: the cover on the left where
    /// the text keeps room enough beside it (else the text alone, or the
    /// cover alone where not even the text has room); the title as large
    /// as fits in up to two lines, then the progress and the caption
    /// where there is room for them too (the caption is left out first).
    fn of(env: &Env<'_>, area: Rect, book: &Book) -> Self {
        let style = UiStyle::new(env);
        let r = widgets::look::look(env, CONTINUE, area).area;
        let gap = style.gap as u16;
        // A few words of a title at the body's size.
        let least = (style.text * 4.0) as u16;
        let mut parts = Self::default();
        let image = book
            .cover
            .as_ref()
            .filter(|_| env.settings.read(&CARD_COVER));
        let mut text = r;
        if let Some(image) = image {
            let place = Rect::new(r.x, r.y, r.width / 3, r.height);
            let at = fitted(image, place, 0.0, 0.5);
            let beside = r.width.saturating_sub(at.width + gap);
            if beside >= least {
                parts.cover = Some(at);
                text = Rect::new(r.x + at.width + gap, r.y, beside, r.height);
            } else if r.width < least {
                // No room for text: the cover alone, in the middle.
                parts.cover = Some(fitted(image, r, 0.5, 0.5));
                return parts;
            }
        }
        let small = line_height(env, style.small);
        let half = gap / 2;
        let line = line_height(env, style.text);
        let mut room = text.height;
        parts.progress = env.settings.read(&CARD_PROGRESS) && room >= line + half + small;
        if parts.progress {
            room -= half + small;
        }
        let caption = env.i18n.tr("home.continue");
        let caption_fits = style
            .font(env)
            .is_some_and(|font| font.measure(style.small, caption) <= f32::from(text.width));
        parts.caption = caption_fits && room >= line + half + small;
        if parts.caption {
            room -= half + small;
        }
        // Two lines at most; in a flat card, down to the small size.
        let two = room.min(2 * line_height(env, style.title));
        let least = if room >= line {
            style.text
        } else {
            style.small
        };
        let title = &book.book.title;
        parts.title = widgets::Wrapped::fitting(env, title, style.title, least, text.width, two);
        parts.text = Some(text);
        parts
    }

    /// The height its column of text takes.
    fn height(&self, env: &Env<'_>) -> u16 {
        let style = UiStyle::new(env);
        let small = line_height(env, style.small) + style.gap as u16 / 2;
        self.title.height(env)
            + if self.caption { small } else { 0 }
            + if self.progress { small } else { 0 }
    }
}

impl Card {
    fn make_parts(&mut self, env: &Env<'_>) {
        self.parts = match (self.area, &self.book) {
            (Some(area), Some(book)) => CardParts::of(env, area, book),
            _ => CardParts::default(),
        };
        self.cover = match (self.parts.cover, &self.book) {
            (
                Some(at),
                Some(Book {
                    cover: Some(image), ..
                }),
            ) => small_cover(env, image, at),
            _ => None,
        };
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
            self.make_parts(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_parts(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        card_height(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let book = self.book.as_ref();
        if self.area == Some(area) {
            draw_card(canvas, env, area, book, &self.parts, self.cover.as_ref());
        } else {
            // Drawn where it was not placed (e.g. while dragged).
            let parts = book
                .map(|b| CardParts::of(env, area, b))
                .unwrap_or_default();
            draw_card(canvas, env, area, book, &parts, None);
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

fn draw_card(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    shown: Option<&Book>,
    parts: &CardParts,
    small: Option<&Thumbnail>,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let look = widgets::draw_frame(canvas, env, CONTINUE, area);
    let Some(Book { book, cover }) = shown else {
        // Nothing read yet: where the books are (a tap opens the library).
        ui::draw_empty(
            canvas,
            env,
            &style,
            look.area,
            "books",
            env.i18n.tr("home.start.title"),
            env.i18n.tr("home.start.text"),
        );
        return;
    };
    // The cover on the left, where there is one.
    if let (Some(at), Some(image)) = (parts.cover, cover) {
        draw_cover(canvas, env, image, small, at);
    }
    let Some(r) = parts.text else {
        return;
    };
    // The lines of text, together in the middle.
    let label_h = line_height(env, style.small);
    let gap = style.gap as u16 / 2;
    let mut y = centred(r, parts.height(env));
    let align = widgets::align(env, CONTINUE);
    let row = |y: u16, h: u16| Rect::new(r.x, y, r.width, h);
    if parts.caption {
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
    }
    let title_h = parts.title.height(env);
    parts
        .title
        .draw(canvas, env, row(y, title_h), align, look.ink);
    y += title_h + gap;
    if !parts.progress {
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
    let w = (f32::from(r.width) - percent_w).max(1.0) as u32;
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
    /// Where its parts go there.
    parts: ShelfParts,
    /// The books' covers, made small for their places.
    covers: Vec<Option<Thumbnail>>,
    /// The lines of the titles of books without a cover, in their frames.
    names: Vec<Vec<String>>,
}

/// Where the parts of the shelf go: the heading, and each book's cover
/// with its title below; together in the middle of the place.
#[derive(Debug, Default, Clone, PartialEq)]
struct ShelfParts {
    /// The heading's row, where it shows.
    heading: Option<Rect>,
    /// Each book's place for its cover, and the row of its title where
    /// titles show.
    places: Vec<(Rect, Option<Rect>)>,
}

impl ShelfParts {
    /// The parts of a shelf of `books` in `area` (inside its spacing):
    /// covers 3:4 as large as the columns and the height allow; where
    /// they would be smaller than two lines of text, the heading is left
    /// out, then the titles.
    fn of(env: &Env<'_>, area: Rect, books: &[Book]) -> Self {
        if books.is_empty() {
            return Self::default();
        }
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return Self::default();
        };
        let (fit, _) = shelf_columns(env, area.width);
        let columns = books.len().clamp(1, fit) as u16;
        let gap = style.gap as u16;
        let label = line_height(env, style.small);
        let heading_fits =
            font.measure(style.small, env.i18n.tr("home.recent")) <= f32::from(area.width);
        let (mut heading, mut title) = shelf_labels(env);
        if !heading_fits {
            heading = 0;
        }
        let least = 2 * label;
        if area.height.saturating_sub(heading + title) < least {
            heading = 0;
        }
        if area.height.saturating_sub(heading + title) < least {
            title = 0;
        }
        let room = area.height.saturating_sub(heading + title);
        // Covers 3:4, as wide as the columns or as high as the room allows.
        let column = area.width / columns;
        let w = column.saturating_sub(gap).min(room * 3 / 4);
        let h = w * 4 / 3;
        let total = heading + h + title;
        let top = area.y + area.height.saturating_sub(total) / 2;
        let left = area.x + area.width.saturating_sub(column * columns) / 2;
        let heading = (heading > 0).then(|| Rect::new(area.x, top, area.width, label));
        let top = top + heading.map_or(0, |_| label + gap / 2);
        let places = (0..columns)
            .map(|i| {
                let start = left + i * column;
                let cover = Rect::new(start + (column - w) / 2, top, w, h);
                let row = Rect::new(start + gap / 2, top + h, column.saturating_sub(gap), title);
                (cover, (title > 0).then_some(row))
            })
            .collect();
        Self { heading, places }
    }
}

impl Shelf {
    fn make_covers(&mut self, env: &Env<'_>) {
        let Some(area) = self.area else {
            self.parts = ShelfParts::default();
            self.covers.clear();
            self.names.clear();
            return;
        };
        let inner = widgets::look::look(env, RECENT, area).area;
        self.parts = ShelfParts::of(env, inner, &self.books);
        let shown = self.parts.places.iter().zip(&self.books);
        self.covers = shown
            .clone()
            .map(|((place, _), book)| {
                let image = book.cover.as_ref()?;
                small_cover(env, image, fitted(image, *place, 0.5, 1.0))
            })
            .collect();
        self.names = shown
            .map(|((place, _), book)| match book.cover {
                Some(_) => Vec::new(),
                None => placeholder_lines(env, *place, &book.book.title),
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
        if self.area == Some(area) {
            draw_shelf(canvas, env, area, self);
        } else {
            // Drawn where it was not placed (e.g. while dragged).
            let mut moved = Shelf {
                books: self.books.clone(),
                area: Some(area),
                ..Shelf::default()
            };
            let inner = widgets::look::look(env, RECENT, area).area;
            moved.parts = ShelfParts::of(env, inner, &moved.books);
            draw_shelf(canvas, env, area, &moved);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let parts = if self.area == Some(area) {
            self.parts.clone()
        } else {
            let inner = widgets::look::look(env, RECENT, area).area;
            ShelfParts::of(env, inner, &self.books)
        };
        parts
            .places
            .iter()
            .zip(&self.books)
            .map(|((cover, title), book)| WidgetTarget {
                area: title.map_or(*cover, |t| cover.union(&t)),
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

fn draw_shelf(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, shelf: &Shelf) {
    let books = &shelf.books;
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    // Nothing read before the current book: nothing to show.
    if books.is_empty() {
        return;
    }
    let look = widgets::draw_frame(canvas, env, RECENT, area);
    let align = widgets::align(env, RECENT);
    if let Some(row) = shelf.parts.heading {
        let text = env.i18n.tr("home.recent");
        let x = align.x(row, font.measure(style.small, text));
        ui::draw_label(canvas, &font, style.small, x, row, text, look.faint);
    }
    for (i, ((cover, row), Book { book, cover: image })) in
        shelf.parts.places.iter().zip(books).enumerate()
    {
        match image {
            // In the middle of its place, at the bottom.
            Some(image) => {
                let at = fitted(image, *cover, 0.5, 1.0);
                let small = shelf.covers.get(i).and_then(Option::as_ref);
                draw_cover(canvas, env, image, small, at);
            }
            None => {
                let lines = shelf.names.get(i).map_or(&[][..], Vec::as_slice);
                draw_placeholder(canvas, env, &style, *cover, lines, look.ink);
            }
        }
        if let Some(row) = row {
            let title = ui::fit(&font, style.small, &book.title, f32::from(row.width));
            let x = align.x(*row, font.measure(style.small, &title));
            ui::draw_label(canvas, &font, style.small, x, *row, &title, look.ink);
        }
    }
}

/// The lines of `title` in the frame of a book without a cover at
/// `area`: broken between words, as many as fit; where a word is wider
/// than a line, or more is left than fits, the last line shown ends in
/// "…" and nothing follows it. None where the frame is too small.
fn placeholder_lines(env: &Env<'_>, area: Rect, title: &str) -> Vec<String> {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return Vec::new();
    };
    let pad = style.gap as f32 / 2.0;
    let width = f32::from(area.width) - 2.0 * pad;
    let line = font.line_metrics(style.small).line_height;
    let fits = ((f32::from(area.height) - 2.0 * pad) / line) as usize;
    if width < style.small * 2.0 || fits == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = title.split_whitespace().collect();
    let mut lines: Vec<String> = Vec::new();
    for (n, word) in words.iter().enumerate() {
        let joined = lines.last().map(|last| format!("{last} {word}"));
        match joined {
            Some(joined) if font.measure(style.small, &joined) <= width => {
                if let Some(last) = lines.last_mut() {
                    *last = joined;
                }
            }
            _ if lines.len() == fits || font.measure(style.small, word) > width => {
                // No line left, or a word too wide: the rest shortened
                // on the last line there is, and no more.
                let rest = words[n..].join(" ");
                let full = lines.len() == fits;
                match lines.last_mut() {
                    Some(last) if full => {
                        *last = ui::fit(&font, style.small, &format!("{last} {rest}"), width);
                    }
                    _ => lines.push(ui::fit(&font, style.small, &rest, width)),
                }
                break;
            }
            _ => lines.push(String::from(*word)),
        }
    }
    lines
}

/// A book without a cover: a frame with its title (`lines`).
fn draw_placeholder(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    lines: &[String],
    ink: Color,
) {
    canvas.stroke(area, style.line.max(2), ink);
    let Some(font) = style.font(env) else {
        return;
    };
    let pad = style.gap as f32 / 2.0;
    let line = font.line_metrics(style.small).line_height;
    for (n, text) in lines.iter().enumerate() {
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
