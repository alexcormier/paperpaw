//! How the calm list draws a book: a row with a cover, the title on up to
//! two lines, author and series below and how far it is read; the
//! "Continue reading" card; the filter row; a progress bar.
//!
//! Everything drawn here was read before: a book's record of the catalog,
//! its details, its cover made small and the badges plugins put on it
//! ([`Loaded`]) are read when its page is shown, never while drawing.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::library::{self, Badge, catalog::Record};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{Arg, Canvas, Color, Env, FontHandle, Rect};

/// Badges shown on a book's row at most ([`library::service::badges`]).
const BADGES: usize = 2;

/// The sizes of the calm list, worked out once when a screen opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sizes {
    /// Line heights of body text, small text and titles.
    pub(crate) text_line: u16,
    pub(crate) small_line: u16,
    pub(crate) title_line: u16,
    /// A book's row, and its cover's place in it.
    pub(crate) row: u16,
    pub(crate) cover: (u16, u16),
    /// The "Continue reading" card, and its cover's place.
    pub(crate) card: u16,
    pub(crate) card_cover: (u16, u16),
    /// A pill (the card's button): as high as a fingertip on touch screens.
    pub(crate) pill: u16,
    /// How thick a progress bar is.
    pub(crate) bar: u16,
    /// A small screen (the X4's 55 mm wide): fewer things with more room
    /// around them, one line of details under a title, the filters
    /// behind one row ([`Sizes::filter_row`]) instead of chips.
    pub(crate) compact: bool,
    /// The row that shows and changes the filter on small screens.
    pub(crate) filter_row: u16,
}

impl Sizes {
    pub(crate) fn new(env: &Env<'_>, style: &UiStyle) -> Sizes {
        let line = |size: f32| {
            style
                .font(env)
                .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
        };
        let (text_line, small_line, title_line) =
            (line(style.text), line(style.small), line(style.title));
        let gap = style.gap as u16;
        let finger = style.finger as u16;
        let pill = (small_line + gap * 2 / 3).max(finger);
        let bar = env.metrics.tenth_mm(9).max(4) as u16;
        let compact = u32::from(env.device.display.width) < env.metrics.mm(COMPACT_MM);
        if compact {
            // Title on two lines and one line of details beside a cover
            // as high, with a gap and a half above and below.
            let block = 2 * text_line + small_line + gap / 3;
            let row = (block + 2 * gap + gap / 2).max(finger * 2);
            let cover_h = block;
            // A word above the title, the title, the bar: as the rows.
            let card_block = small_line + 2 * title_line + small_line + gap / 2;
            let card = card_block.max(cover_h) + 2 * gap + gap / 2;
            return Sizes {
                text_line,
                small_line,
                title_line,
                row,
                cover: (cover_h * 2 / 3, cover_h),
                card,
                card_cover: (cover_h * 2 / 3, cover_h),
                pill,
                bar,
                compact,
                filter_row: (text_line + gap).max(finger),
            };
        }
        // Title on two lines, the note, how far it is read.
        let row = (2 * text_line + 2 * small_line + gap / 2).max(finger * 2);
        let cover_h = row - gap * 2 / 3;
        // The title on two lines, the author, the bar, the button.
        let card_text = 2 * small_line + 2 * title_line + gap / 2 + pill;
        let card = (card_text + gap).max(row * 3 / 2);
        let card_h = card - gap;
        Sizes {
            text_line,
            small_line,
            title_line,
            row,
            cover: (cover_h * 2 / 3, cover_h),
            card,
            card_cover: (card_h * 2 / 3, card_h),
            pill,
            bar,
            compact,
            filter_row: pill,
        }
    }
}

/// Screens narrower than this many millimetres are small ([`Sizes::compact`]).
const COMPACT_MM: u32 = 75;

/// A book of a page, read when the page is shown, with its words made
/// ready for where it is drawn ([`prepare_row`], [`prepare_card`],
/// [`super::grid::prepare_cell`]): drawing reads and allocates nothing.
#[derive(Debug, Default)]
pub(crate) struct Loaded {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) series: String,
    pub(crate) number: String,
    /// How far it is read, in percent.
    pub(crate) progress: u8,
    /// It is new: added lately and never opened ([`library::is_new`]).
    pub(crate) new: bool,
    /// Its cover, made small for where it is drawn.
    pub(crate) thumb: Option<Thumbnail>,
    /// Its title broken into the lines drawn.
    pub(crate) lines: Vec<String>,
    /// The line of details under the title (the author, or its number in
    /// a series), shortened to the room it has.
    pub(crate) note: String,
    /// On a small screen: how far the book is read (and its "next" tag)
    /// left out where the details do not fit beside it whole.
    pub(crate) state_hidden: bool,
    /// The series with the book's number ("Sherlock Holmes 5").
    pub(crate) tail: String,
    /// How far it is read, as written ("42 %").
    pub(crate) percent: String,
    /// For a series as a stack of covers: how many books it holds.
    pub(crate) stack: String,
    /// The badges plugins put on it: those of words (joined), and icons,
    /// shown after its details.
    pub(crate) badges: String,
    pub(crate) badge_icons: Vec<&'static str>,
    /// What it says when read out (spoken focus, taps by name): its
    /// title first, then its details and how far it is read.
    pub(crate) spoken: String,
}

impl Loaded {
    /// Reads the book of `record`: its details, its badges and its cover
    /// made small to `cover`, kept as `variant` (each size its own, so
    /// that the rows' small covers are not made anew for the card's).
    pub(crate) fn load(env: &Env<'_>, record: Record, cover: (u16, u16), variant: &str) -> Loaded {
        let storage = env.storage;
        let format = env.device.display.format;
        let book = library::service(env).book(env, &record.path);
        let thumb = book.as_ref().and_then(|book| {
            library::cover_variant(
                storage,
                &record.path,
                book,
                cover.0,
                cover.1,
                format,
                variant,
            )
        });
        // How far it is read as the catalog has it (noted when the book
        // closed), else as its details do.
        let progress = record
            .status
            .progress
            .or_else(|| book.as_ref().and_then(|b| b.progress))
            .unwrap_or(0);
        let new = book
            .as_ref()
            .is_some_and(|b| library::is_new(env, b, record.status.opened));
        let mut badges = String::new();
        let mut badge_icons = Vec::new();
        for badge in library::service::badges(env, &record, BADGES) {
            match badge {
                Badge::Text(text) if badges.is_empty() => badges = text,
                Badge::Text(text) => {
                    badges.push_str(" · ");
                    badges.push_str(&text);
                }
                Badge::Icon(icon) => badge_icons.push(icon),
                _ => {}
            }
        }
        let Record {
            path,
            author,
            title,
            series,
            number,
            ..
        } = record;
        let title = if title.is_empty() {
            library::readable_name(paperpaw_sdk::files::name(&path)).into_owned()
        } else {
            title
        };
        let tail = match (series.is_empty(), number.is_empty()) {
            (true, _) => String::new(),
            (false, true) => series.clone(),
            (false, false) => format!("{series} {number}"),
        };
        Loaded {
            path,
            title,
            author,
            series,
            number,
            progress,
            new,
            thumb,
            tail,
            percent: format!("{progress} %"),
            badges,
            badge_icons,
            ..Loaded::default()
        }
    }

    /// `note` with its badges of words after it.
    fn with_badges(&self, note: String) -> String {
        match (note.is_empty(), self.badges.is_empty()) {
            (_, true) => note,
            (true, false) => self.badges.clone(),
            (false, false) => format!("{note} · {}", self.badges),
        }
    }

    /// How wide its badges' icons are drawn, the gaps before them
    /// included.
    fn icons_width(&self, env: &Env<'_>, style: &UiStyle) -> u16 {
        let gap = style.gap as f32 / 3.0;
        self.badge_icons
            .iter()
            .map(|icon| gap + ui::icon_width(env, icon, style.small))
            .sum::<f32>() as u16
    }

    /// The note in a series' list: the book's number and the author.
    pub(crate) fn series_note(&self, env: &Env<'_>) -> String {
        if self.number.is_empty() {
            return self.author.clone();
        }
        let number = env
            .i18n
            .format("files.series-number", &[("number", Arg::Str(&self.number))]);
        if self.author.is_empty() {
            number
        } else {
            format!("{number} · {}", self.author)
        }
    }
}

/// Draws a book's cover made small at the top left of `place`, centred
/// in it; a stand-in with the title's first letter where it has none.
pub(crate) fn draw_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    place: Rect,
    book: &Loaded,
    ink: Color,
) {
    match &book.thumb {
        Some(thumb) => {
            let x = place.x + place.width.saturating_sub(thumb.width()) / 2;
            let y = place.y + place.height.saturating_sub(thumb.height()) / 2;
            thumb.draw_round(canvas, x, y, crate::picture_radius(style, place));
        }
        None => crate::draw_placeholder(canvas, env, style, place, &book.title, "", ink),
    }
}

/// Draws a progress bar `width` wide, centred on the line `area`
/// starting at `x`: a light track, the part read dark.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_bar(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    x: u16,
    area: Rect,
    width: u16,
    progress: u8,
    ink: Color,
) {
    let h = sizes.bar;
    let track = Rect::new(x, area.y + area.height.saturating_sub(h) / 2, width, h);
    let radius = u32::from(h / 2);
    if style.grays > 2 && ink == Color::BLACK {
        canvas.fill_round(track, radius, Color::gray(200));
    } else {
        canvas.stroke_round(track, style.line.max(1), radius, ink);
    }
    let read = (u32::from(width) * u32::from(progress.min(100)) / 100) as u16;
    if read > 0 {
        let done = Rect::new(track.x, track.y, read.max(h), h);
        canvas.fill_round(done, radius, ink);
    }
}

/// Draws a small tag (e.g. "New") at `x` on the line `area`: outlined, or
/// filled with `ink`; returns its width.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_tag(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    x: u16,
    area: Rect,
    label: &str,
    filled: bool,
    ink: Color,
    paper: Color,
) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let size = style.small * 0.9;
    let pad = style.gap as u16 / 2;
    let w = font.measure(size, label) as u16 + 2 * pad;
    let h = area.height.saturating_sub(2);
    let tag = Rect::new(x, area.y + (area.height - h) / 2, w, h);
    let radius = u32::from(h / 2);
    let text = if filled {
        canvas.fill_round(tag, radius, ink);
        paper
    } else {
        canvas.stroke_round(tag, style.line.max(1), radius, ink);
        ink
    };
    ui::draw_label(canvas, &font, size, f32::from(x + pad), tag, label, text);
    w
}

/// The width of a tag ([`draw_tag`]) saying `label`.
fn tag_width(env: &Env<'_>, style: &UiStyle, label: &str) -> u16 {
    style.font(env).map_or(0, |font| {
        font.measure(style.small * 0.9, label) as u16 + style.gap as u16
    })
}

/// How wide a bar of how far a book is read is in a row `width` wide.
fn bar_width(env: &Env<'_>, sizes: &Sizes, width: u16) -> u16 {
    if sizes.compact {
        env.metrics.mm(10) as u16
    } else {
        (width / 3).min(env.metrics.mm(20) as u16)
    }
}

/// The width [`draw_state`] takes for `book` with a bar `bar` wide.
fn state_width(env: &Env<'_>, style: &UiStyle, bar: u16, book: &Loaded) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let gap = style.gap as u16;
    match book.progress {
        100 => {
            let icon = ui::icon_width(env, "circle-check", style.small * 1.15);
            let label = font.measure(style.small, env.i18n.tr("files.finished"));
            (icon + label) as u16 + gap / 3
        }
        p if p > 0 => bar + gap / 2 + font.measure(style.small, &book.percent) as u16,
        _ if book.new => tag_width(env, style, env.i18n.tr("files.list.new")),
        _ => 0,
    }
}

/// Draws how far a book is read on the line `area` from `x`: a bar and
/// the percentage while it is read, a check when finished, "New" for a
/// new book; nothing for a book opened but not begun. Returns where what
/// it drew ends.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_state(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    x: u16,
    area: Rect,
    bar: u16,
    book: &Loaded,
    focused: bool,
) -> u16 {
    let Some(font) = style.font(env) else {
        return x;
    };
    let ink = style.focus_ink(focused);
    let faint = style.focus_faint(focused, 80);
    let gap = style.gap as u16;
    match book.progress {
        100 => {
            let size = style.small * 1.15;
            let w = ui::draw_icon(canvas, env, "circle-check", size, f32::from(x), area, ink);
            let at = f32::from(x) + w + f32::from(gap / 3);
            let label = env.i18n.tr("files.finished");
            let w = ui::draw_label(canvas, &font, style.small, at, area, label, faint);
            (at + w) as u16
        }
        p if p > 0 => {
            draw_bar(canvas, style, sizes, x, area, bar, p, ink);
            let at = f32::from(x + bar + gap / 2);
            let w = ui::draw_label(canvas, &font, style.small, at, area, &book.percent, ink);
            (at + w) as u16
        }
        _ if book.new => {
            let paper = style.focus_paper(focused);
            let label = env.i18n.tr("files.list.new");
            x + draw_tag(canvas, env, style, x, area, label, false, ink, paper)
        }
        _ => x,
    }
}

/// `text` broken into at most `lines` lines of `width` pixels, the last
/// one shortened with "…" where there is more.
pub(crate) fn wrap_lines(
    env: &Env<'_>,
    style: &UiStyle,
    size: f32,
    text: &str,
    width: f32,
    lines: usize,
) -> Vec<String> {
    let Some(font) = style.font(env) else {
        return Vec::new();
    };
    let ranges = font.wrap(size, text, width);
    let more = ranges.len() > lines;
    ranges
        .iter()
        .take(lines)
        .enumerate()
        .map(|(n, range)| {
            if more && n + 1 == lines {
                ui::fit(&font, size, text[range.start..].trim_end(), width)
            } else {
                text[range.clone()].trim_end().to_string()
            }
        })
        .collect()
}

/// `words` that say something, joined as a part of the screen is read
/// out.
pub(crate) fn spoken(words: &[&str]) -> String {
    let mut out = String::new();
    for word in words.iter().filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(word);
    }
    out
}

/// How far `book` is read, in words: the percentage, "Finished", "New",
/// or nothing.
pub(crate) fn state_words<'a>(env: &Env<'a>, book: &'a Loaded) -> &'a str {
    match book.progress {
        100 => env.i18n.tr("files.finished"),
        p if p > 0 => &book.percent,
        _ if book.new => env.i18n.tr("files.list.new"),
        _ => "",
    }
}

/// Where the text of a row (or card) with a cover `cover` wide starts,
/// and how wide it is in a row `width` wide.
fn text_room(style: &UiStyle, cover: u16, width: u16) -> (u16, u16) {
    let margin = style.margin as u16;
    let x = margin + cover + margin;
    (x, width.saturating_sub(x + margin))
}

/// Makes `book` ready to be drawn as a row `width` wide
/// ([`draw_book_row`]), with `note` (the author, or its number in a
/// series) under its title; `next` marks the book to read next in a
/// series.
pub(crate) fn prepare_row(
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    width: u16,
    book: &mut Loaded,
    note: String,
    next: bool,
) {
    let (x, room) = text_room(style, sizes.cover.0, width);
    book.lines = wrap_lines(env, style, style.text, &book.title, f32::from(room), 2);
    let note = book.with_badges(note);
    let next_word = if next {
        env.i18n.tr("files.list.next")
    } else {
        ""
    };
    book.spoken = spoken(&[
        &book.title,
        &note,
        next_word,
        state_words(env, book),
        &book.tail,
    ]);
    book.note = note;
    if !sizes.compact {
        return;
    }
    // The details share their line with how far the book is read.
    let gap = style.gap as u16;
    let right = x + room;
    let mut end = right;
    let state = state_width(env, style, bar_width(env, sizes, room), book);
    if state > 0 {
        end = right.saturating_sub(state);
    }
    if next {
        let w = tag_width(env, style, env.i18n.tr("files.list.next"));
        end = end.saturating_sub(w + if state > 0 { gap / 2 } else { 0 });
    }
    let room = end.saturating_sub(x + if end < right { gap } else { 0 });
    let room = room.saturating_sub(book.icons_width(env, style));
    let Some(font) = style.font(env) else {
        return;
    };
    // Too long beside it, but whole on the line alone: the smaller thing,
    // how far it is read, gives way.
    let whole = (right - x).saturating_sub(book.icons_width(env, style));
    let width = font.measure(style.small, &book.note);
    if end < right && width > f32::from(room) + 1.0 && width <= f32::from(whole) + 1.0 {
        book.state_hidden = true;
        return;
    }
    book.note = ui::fit(&font, style.small, &book.note, f32::from(room));
}

/// Draws a book's note in `area` from `x`, its badges' icons after it.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_note(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    font: &FontHandle<'_>,
    x: u16,
    area: Rect,
    book: &Loaded,
    ink: Color,
) {
    ui::draw_label(
        canvas,
        font,
        style.small,
        f32::from(x),
        area,
        &book.note,
        ink,
    );
    if book.badge_icons.is_empty() {
        return;
    }
    let mut at = f32::from(x) + font.measure(style.small, &book.note);
    for icon in &book.badge_icons {
        at += style.gap as f32 / 3.0;
        at += ui::draw_icon(canvas, env, icon, style.small, at, area, ink);
    }
}

/// Makes `book` ready to be drawn as the card in a row `width` wide
/// ([`draw_card`]).
pub(crate) fn prepare_card(
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    width: u16,
    book: &mut Loaded,
) {
    let (_, room) = text_room(style, sizes.card_cover.0, width);
    book.lines = wrap_lines(env, style, style.title, &book.title, f32::from(room), 2);
    let eyebrow = env.i18n.tr("files.list.continue-reading");
    book.spoken = spoken(&[&book.title, eyebrow, &book.author, &book.percent]);
}

/// Draws a book's row in `rect` as [`prepare_row`] made it ready: its
/// cover at the margin, then its title on up to two lines, its details
/// below and how far it is read (with its series on larger screens);
/// `next` marks the book to read next in a series.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_book_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    rect: Rect,
    focused: bool,
    book: &Loaded,
    next: bool,
) {
    canvas.describe(Role::Row, &book.spoken, rect, focused, None);
    if sizes.compact {
        draw_compact_row(canvas, env, style, sizes, rect, focused, book, next);
        return;
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let ink = style.focus_ink(focused);
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let (cw, ch) = sizes.cover;
    let place = Rect::new(rect.x + margin, rect.y + (rect.height - ch) / 2, cw, ch);
    draw_cover(canvas, env, style, place, book, ink);

    let (x, width) = text_room(style, cw, rect.width);
    let x = rect.x + x;
    let note = &book.note;
    let tail = &book.tail;
    let state = book.progress > 0 || book.new || next || !tail.is_empty();
    let block = book.lines.len() as u16 * sizes.text_line
        + if note.is_empty() { 0 } else { sizes.small_line }
        + if state { sizes.small_line + gap / 3 } else { 0 };
    let mut y = rect.y + rect.height.saturating_sub(block) / 2;
    for text in &book.lines {
        let area = Rect::new(x, y, width, sizes.text_line);
        ui::draw_label(canvas, &font, style.text, f32::from(x), area, text, ink);
        y += sizes.text_line;
    }
    if !note.is_empty() || !book.badge_icons.is_empty() {
        let area = Rect::new(x, y, width, sizes.small_line);
        let faint = style.focus_faint(focused, 80);
        draw_note(canvas, env, style, &font, x, area, book, faint);
        y += sizes.small_line;
    }
    if state {
        y += gap / 3;
        let area = Rect::new(x, y, width, sizes.small_line);
        let mut at = x;
        if next {
            let paper = style.focus_paper(focused);
            let label = env.i18n.tr("files.list.next");
            at += draw_tag(canvas, env, style, at, area, label, true, ink, paper) + gap / 2;
        }
        let bar = bar_width(env, sizes, width);
        let end = draw_state(canvas, env, style, sizes, at, area, bar, book, focused);
        if !tail.is_empty() {
            let x = if end > at { end + gap * 2 / 3 } else { at };
            let faint = style.focus_faint(focused, 80);
            ui::draw_label(canvas, &font, style.small, f32::from(x), area, tail, faint);
        }
    }
}

/// A book's row on a small screen: its cover at the margin, the title on
/// up to two lines, and one line below: its details at the start, how
/// far it is read at the end, on the right margin; `next` marks the book
/// to read next in a series.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_compact_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    rect: Rect,
    focused: bool,
    book: &Loaded,
    next: bool,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let ink = style.focus_ink(focused);
    let faint = style.focus_faint(focused, 80);
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let (cw, ch) = sizes.cover;
    let place = Rect::new(rect.x + margin, rect.y + (rect.height - ch) / 2, cw, ch);
    draw_cover(canvas, env, style, place, book, ink);
    let (x, width) = text_room(style, cw, rect.width);
    let x = rect.x + x;
    let right = x + width;
    // The title and the details below it, together centred on the cover.
    let block = book.lines.len() as u16 * sizes.text_line + gap / 3 + sizes.small_line;
    let mut y = rect.y + rect.height.saturating_sub(block) / 2;
    for text in &book.lines {
        let area = Rect::new(x, y, width, sizes.text_line);
        ui::draw_label(canvas, &font, style.text, f32::from(x), area, text, ink);
        y += sizes.text_line;
    }
    let line = Rect::new(x, y + gap / 3, width, sizes.small_line);
    let bar = bar_width(env, sizes, width);
    let mut end = right;
    let state = if book.state_hidden {
        0
    } else {
        state_width(env, style, bar, book)
    };
    if state > 0 {
        end = right.saturating_sub(state);
        draw_state(canvas, env, style, sizes, end, line, bar, book, focused);
    }
    if next && !book.state_hidden {
        let label = env.i18n.tr("files.list.next");
        let w = tag_width(env, style, label);
        let at = end.saturating_sub(w + if state > 0 { gap / 2 } else { 0 });
        let paper = style.focus_paper(focused);
        draw_tag(canvas, env, style, at, line, label, true, ink, paper);
    }
    if !book.note.is_empty() || !book.badge_icons.is_empty() {
        draw_note(canvas, env, style, &font, x, line, book, faint);
    }
}

/// The "Continue reading" card on a small screen: the book read last as
/// a row with a word above its title, the title larger, and a bar of how
/// far it is read; its cover as large as the rows', so its text starts
/// where theirs does.
fn draw_compact_card(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    rect: Rect,
    focused: bool,
    book: &Loaded,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    if focused {
        ui::draw_focus(canvas, style, rect);
    }
    let ink = style.focus_ink(focused);
    let faint = style.focus_faint(focused, 80);
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let (cw, ch) = sizes.card_cover;
    let place = Rect::new(rect.x + margin, rect.y + (rect.height - ch) / 2, cw, ch);
    draw_cover(canvas, env, style, place, book, ink);
    let (x, width) = text_room(style, cw, rect.width);
    let x = rect.x + x;
    let right = x + width;
    let block = 2 * sizes.small_line + book.lines.len() as u16 * sizes.title_line + gap / 2;
    let mut y = rect.y + rect.height.saturating_sub(block) / 2;
    let eyebrow = env.i18n.tr("files.list.continue-reading");
    let area = Rect::new(x, y, width, sizes.small_line);
    ui::draw_label(
        canvas,
        &font,
        style.small,
        f32::from(x),
        area,
        eyebrow,
        faint,
    );
    y += sizes.small_line;
    for text in &book.lines {
        let area = Rect::new(x, y, width, sizes.title_line);
        ui::draw_label(canvas, &font, style.title, f32::from(x), area, text, ink);
        y += sizes.title_line;
    }
    y += gap / 2;
    let area = Rect::new(x, y, width, sizes.small_line);
    let label = font.measure(style.small, &book.percent) as u16;
    let bar = width.saturating_sub(label + gap / 2);
    draw_bar(canvas, style, sizes, x, area, bar, book.progress, ink);
    let at = f32::from(right - label);
    ui::draw_label(canvas, &font, style.small, at, area, &book.percent, ink);
    if !focused {
        ui::draw_separator(canvas, style, rect, 0);
    }
}

/// Draws the "Continue reading" card in `rect` as [`prepare_card`] made
/// it ready: the book read last with a large cover, its title, author
/// and progress, and a button to go on.
pub(crate) fn draw_card(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    sizes: &Sizes,
    rect: Rect,
    focused: bool,
    book: &Loaded,
) {
    canvas.describe(Role::Row, &book.spoken, rect, focused, None);
    if sizes.compact {
        draw_compact_card(canvas, env, style, sizes, rect, focused, book);
        return;
    }
    let Some(font) = style.font(env) else {
        return;
    };
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    // A light card where the panel has many grays, else an outline; the
    // focus as every row's.
    let back = ui::focus_rect(style, rect);
    if focused {
        ui::draw_focus(canvas, style, rect);
    } else if style.grays >= 16 {
        canvas.fill_round(back, style.radius(u32::from(back.height)), Color::gray(232));
    } else {
        let radius = style.radius(u32::from(back.height));
        canvas.stroke_round(back, style.line.max(1), radius, Color::BLACK);
    }
    let ink = style.focus_ink(focused);
    let paper = if focused {
        style.focus_paper(true)
    } else {
        Color::WHITE
    };
    let faint = style.focus_faint(focused, 80);
    let (cw, ch) = sizes.card_cover;
    let place = Rect::new(rect.x + margin, rect.y + (rect.height - ch) / 2, cw, ch);
    draw_cover(canvas, env, style, place, book, ink);

    let (x, width) = text_room(style, cw, rect.width);
    let x = rect.x + x;
    let block =
        2 * sizes.small_line + book.lines.len() as u16 * sizes.title_line + gap / 2 + sizes.pill;
    let mut y = rect.y + rect.height.saturating_sub(block) / 2;
    for text in &book.lines {
        let area = Rect::new(x, y, width, sizes.title_line);
        ui::draw_label(canvas, &font, style.title, f32::from(x), area, text, ink);
        y += sizes.title_line;
    }
    let area = Rect::new(x, y, width, sizes.small_line);
    ui::draw_label(
        canvas,
        &font,
        style.small,
        f32::from(x),
        area,
        &book.author,
        faint,
    );
    y += sizes.small_line;
    let area = Rect::new(x, y, width, sizes.small_line);
    let bar = width * 3 / 5;
    draw_bar(canvas, style, sizes, x, area, bar, book.progress, ink);
    let at = f32::from(x + bar + gap / 2);
    ui::draw_label(canvas, &font, style.small, at, area, &book.percent, ink);
    y += sizes.small_line + gap / 2;
    // The button: dark, or light on the dark focus.
    let label = env.i18n.tr("files.list.continue-reading");
    let pad = gap;
    let w = font.measure(style.small, label) as u16 + 2 * pad;
    let button = Rect::new(x, y, w, sizes.pill);
    canvas.fill_round(button, u32::from(sizes.pill / 2), ink);
    let at = f32::from(x + pad);
    ui::draw_label(canvas, &font, style.small, at, button, label, paper);
}

/// Draws the filter's row of a small screen in `rect`: what is shown
/// (`label`) with a mark that it opens a choice, and at the end `end`
/// (how many books) or, on touch screens, the search button
/// (`search`).
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_filter_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    rect: Rect,
    focused: bool,
    label: &str,
    end: &str,
    search: Option<Rect>,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    if focused {
        ui::draw_focus(canvas, style, rect);
    }
    let ink = style.focus_ink(focused);
    let faint = style.focus_faint(focused, 80);
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let x = rect.x + margin;
    canvas.describe(Role::Button, label, rect, focused, None);
    let w = ui::draw_label(canvas, &font, style.text, f32::from(x), rect, label, ink);
    let icon_x = f32::from(x) + w + f32::from(gap / 3);
    ui::draw_icon(canvas, env, "chevron-down", style.small, icon_x, rect, ink);
    let right = (rect.x + rect.width).saturating_sub(margin);
    match search {
        Some(button) => {
            let size = style.title * ui::ICON_SCALE;
            let w = ui::icon_width(env, "search", size);
            let x = f32::from(button.x) + (f32::from(button.width) - w) / 2.0;
            ui::draw_icon(canvas, env, "search", size, x, button, ink);
        }
        None => {
            let w = font.measure(style.small, end);
            let at = f32::from(right) - w;
            ui::draw_label(canvas, &font, style.small, at, rect, end, faint);
        }
    }
    if !focused {
        ui::draw_separator(canvas, style, rect, 0);
    }
}
