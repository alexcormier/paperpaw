//! "Book progress": the book being read, its cover large and beside it a
//! ring round the share read, the percentage inside; or the ring round a
//! small cover, the ring alone, or a bar below the title.

use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{Canvas, Command, Env, Id, Point, Rect, RegisterError, Registrar};

use super::{
    Book, Lines, THICKNESSES, add_settings, caption, chosen, cover_size, current, draw_bar,
    draw_cover, draw_message, draw_percent_alone, draw_ring, draw_ring_percent, frame_height,
    percent_text, place_x, text_size, thickness,
};
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.progress";
/// How it is drawn: the cover with the ring beside it, the ring round a
/// small cover, the ring alone, the cover with a bar.
const STYLES: [&str; 4] = [
    "widgets.progress.beside",
    "widgets.progress.round",
    "widgets.progress.ring",
    "widgets.progress.bar",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 9] = [
    widgets::align_spec("widgets.progress.align", ID, Align::Center),
    widgets::frame_spec("widgets.progress.frame", ID, Frame::None),
    widgets::size_spec("widgets.progress.size", ID, Size::Medium, false),
    SettingSpec::fixed_choice(
        "widgets.progress.style",
        "widgets.books.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    ),
    SettingSpec::fixed_choice(
        "widgets.progress.thickness",
        "widgets.books.setting.thickness",
        &THICKNESSES,
        THICKNESSES[1],
        ID,
    ),
    widgets::part_spec(
        "widgets.progress.cover",
        ID,
        "widgets.books.part.cover",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.title",
        ID,
        "widgets.books.part.title",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.author",
        ID,
        "widgets.books.part.author",
        true,
    ),
    widgets::part_spec(
        "widgets.progress.caption",
        ID,
        "widgets.books.part.caption",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    add_settings(r, &ID, "widgets.progress", &SETTINGS)?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.progress", |_| Box::new(Progress::default()))
            .category(widgets::categories::BOOKS),
    )
}

/// The book being read, read when updated.
#[derive(Debug, Default)]
struct Progress {
    book: Option<Book>,
}

impl Widget for Progress {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let book = current(env.storage);
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
        draw(canvas, env, area, self.book.as_ref());
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let command = self
            .book
            .as_ref()
            .map_or(Command::RunAction(actions::LIBRARY), Book::open);
        alloc::vec![WidgetTarget { area, command }]
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, book: Option<&Book>) {
    let look = widgets::draw_frame(canvas, env, WIDGET, area);
    let Some(book) = book else {
        draw_message(canvas, env, &look, "widgets.books.nothing-read");
        return;
    };
    let caption_text = env.i18n.tr("widgets.progress.caption");
    let area = caption(canvas, env, WIDGET, &look, caption_text);
    let style = UiStyle::new(env);
    let gap = style.gap as u16;
    let cover = widgets::shows(env, WIDGET, "cover");
    let mut lines = texts(env, &look, book);
    let style_value = chosen(env, WIDGET, "style");
    // Too small for a ring: the percentage alone.
    if style_value != STYLES[3] && f32::from(area.width.min(area.height)) < style.small * 3.0 {
        let align = widgets::align(env, WIDGET);
        draw_percent_alone(canvas, env, &look, area, align, book.percent());
        return;
    }
    match style_value {
        "widgets.progress.round" => draw_round(canvas, env, &look, area, book),
        "widgets.progress.bar" => {
            // The cover where the text beside it has room.
            let (cw, _) = cover_size(book, area.width * 45 / 100, area.height);
            let room = f32::from(area.width.saturating_sub(cw + gap));
            let cover = cover && room >= style.small * 7.0;
            let rest = draw_side_cover(canvas, env, &look, area, book, cover, gap);
            draw_bar_and_text(canvas, env, &look, rest, book);
        }
        "widgets.progress.ring" => {
            let layout = ring_layout(env, area, &mut lines);
            draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, 0);
        }
        _ => {
            let (cw, ch) = cover_size(book, area.width * 2 / 5, area.height);
            // The cover where the ring beside it has room, else the
            // ring alone.
            let rest_w = area.width.saturating_sub(cw + gap);
            if !cover || rest_w < area.height * 6 / 10 {
                let layout = ring_layout(env, area, &mut lines);
                draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, 0);
                return;
            }
            let rest = Rect::new(area.x + cw + gap, area.y, rest_w, area.height);
            let layout = ring_layout(env, rest, &mut lines);
            // Without text, the cover and the ring together, as aligned.
            let shift = if layout.text.is_none() {
                let group = cw + gap + layout.ring.width;
                let x = place_x(area, group, widgets::align(env, WIDGET));
                let at = Rect::new(x, area.y + (area.height - ch) / 2, cw, ch);
                draw_cover(canvas, env, &look, at, book);
                i32::from(layout.ring.x) - i32::from(x + cw + gap)
            } else {
                let at = Rect::new(area.x, area.y + (area.height - ch) / 2, cw, ch);
                draw_cover(canvas, env, &look, at, book);
                0
            };
            draw_ring_layout(canvas, env, &look, &layout, &mut lines, book, shift);
        }
    }
}

/// The title and the author, as far as shown.
fn texts<'b>(env: &Env<'_>, look: &Look, book: &'b Book) -> Lines<'b> {
    let style = UiStyle::new(env);
    let mut lines = Lines::new();
    if widgets::shows(env, WIDGET, "title") {
        lines.add(text_size(env, WIDGET), book.title.as_str(), look.ink);
    }
    if widgets::shows(env, WIDGET, "author") {
        lines.add(style.small, book.author.as_str(), look.faint);
    }
    lines
}

/// The cover on the left of `area`, if shown; returns the room right of
/// it.
fn draw_side_cover(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    area: Rect,
    book: &Book,
    shown: bool,
    gap: u16,
) -> Rect {
    if !shown {
        return area;
    }
    let (w, h) = cover_size(book, area.width * 45 / 100, area.height);
    let at = Rect::new(area.x, area.y + (area.height - h) / 2, w, h);
    draw_cover(canvas, env, look, at, book);
    let shift = (w + gap).min(area.width);
    Rect::new(area.x + shift, area.y, area.width - shift, area.height)
}

/// Where a ring and its text go.
struct RingLayout {
    ring: Rect,
    /// Where the text goes, if it has room; beside the ring (`true`) or
    /// below it.
    text: Option<(Rect, bool)>,
}

/// The ring in `area`: with the title and the author beside it where
/// `area` is wide, else below it; the text left out where it has no room.
fn ring_layout(env: &Env<'_>, area: Rect, lines: &mut Lines<'_>) -> RingLayout {
    let style = UiStyle::new(env);
    let gap = style.gap as u16;
    let align = widgets::align(env, WIDGET);
    let wide = area.width >= area.height * 3 / 2;
    if wide {
        // Beside the ring, room for a title of some length.
        let d = area.height.min(area.width * 2 / 5);
        let room = area.width.saturating_sub(d + gap * 3 / 2);
        if f32::from(room) >= style.small * 6.0 && !lines.lines.is_empty() {
            let ring = Rect::new(area.x, area.y + (area.height - d) / 2, d, d);
            let text = Rect::new(area.x + area.width - room, area.y, room, area.height);
            return RingLayout {
                ring,
                text: Some((text, true)),
            };
        }
    }
    lines.fit(env, area.height / 2);
    let text_h = lines.height(env);
    let below = text_h + gap / 2;
    let d = area.width.min(area.height.saturating_sub(below));
    if d >= area.height / 2 && !lines.lines.is_empty() {
        let top = area.y + (area.height - (d + below).min(area.height)) / 2;
        let ring = Rect::new(place_x(area, d, align), top, d, d);
        let text = Rect::new(area.x, top + d + gap / 2, area.width, text_h);
        return RingLayout {
            ring,
            text: Some((text, false)),
        };
    }
    let d = area.width.min(area.height);
    let ring = Rect::new(
        place_x(area, d, align),
        area.y + (area.height - d) / 2,
        d,
        d,
    );
    RingLayout { ring, text: None }
}

/// Draws the ring of `layout`, moved `shift` pixels to the left, with the
/// percentage inside, and the text where it goes.
fn draw_ring_layout(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    layout: &RingLayout,
    lines: &mut Lines<'_>,
    book: &Book,
    shift: i32,
) {
    let mut ring = layout.ring;
    ring.x = (i32::from(ring.x) - shift).max(0) as u16;
    draw_ring_in(canvas, env, look, ring, book.percent(), true);
    let Some((text, beside)) = layout.text else {
        return;
    };
    let align = widgets::align(env, WIDGET);
    // Beside a ring, the text starts at its side.
    let align = if beside && align == Align::Center {
        Align::Left
    } else {
        align
    };
    lines.draw(canvas, env, text, align);
}
// A ring in the square `at`, with the percentage inside if `percent`
/// and there is room; returns the radius inside it.
fn draw_ring_in(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    at: Rect,
    progress: u8,
    percent: bool,
) -> u32 {
    let radius = u32::from(at.width.min(at.height) / 2);
    if radius < 4 {
        return 0;
    }
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    let thick = thickness(env, WIDGET, at.width.min(at.height));
    draw_ring(canvas, env, look, center, radius, thick, progress);
    let inner = radius.saturating_sub(thick + (radius / 8).max(1));
    if percent && inner >= 8 {
        draw_ring_percent(canvas, env, look, center, inner, progress);
    }
    inner
}

/// The ring round a small cover, the title, the author and the
/// percentage below it.
fn draw_round(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, area: Rect, book: &Book) {
    let style = UiStyle::new(env);
    // The percentage first: where there is room for one line, that one.
    let mut lines = Lines::new();
    lines.add(style.small, percent_text(env, book.percent()), look.faint);
    lines.lines.extend(texts(env, look, book).lines);
    let layout = ring_layout(env, area, &mut lines);
    let ring = layout.ring;
    let inner = draw_ring_in(canvas, env, look, ring, book.percent(), false) as u16;
    // The cover inside the ring, as large as fits the circle.
    if inner >= 8 {
        let (w, h) = cover_size(book, inner * 2 * 56 / 100, inner * 2 * 83 / 100);
        let at = Rect::new(
            ring.x + (ring.width - w) / 2,
            ring.y + (ring.height - h) / 2,
            w,
            h,
        );
        draw_cover(canvas, env, look, at, book);
    }
    if let Some((text, beside)) = layout.text {
        let align = match widgets::align(env, WIDGET) {
            Align::Center if beside => Align::Left,
            align => align,
        };
        lines.draw(canvas, env, text, align);
    }
}

// The title and the author, and below them the percentage with a bar.
fn draw_bar_and_text(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, area: Rect, book: &Book) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let gap = style.gap as u16;
    let align = widgets::align(env, WIDGET);
    let mut lines = texts(env, look, book);
    let row = widgets::line_height(env, style.small);
    lines.fit(env, area.height.saturating_sub(row + gap));
    let text_h = lines.height(env);
    let total = (text_h + gap + row).min(area.height);
    let top = area.y + (area.height - total) / 2;
    lines.draw(
        canvas,
        env,
        Rect::new(area.x, top, area.width, text_h),
        align,
    );
    let y = (top + total).saturating_sub(row);
    let percent = percent_text(env, book.percent());
    let row_area = Rect::new(area.x, y, area.width, row);
    let text_w = font.measure(style.small, &percent) as u16;
    paperpaw_sdk::ui::draw_label(
        canvas,
        &font,
        style.small,
        f32::from(area.x),
        row_area,
        &percent,
        look.faint,
    );
    let bar_h = (thickness(env, WIDGET, row * 4) as u16).min(row);
    let shift = (text_w + gap).min(area.width);
    let bar = Rect::new(
        area.x + shift,
        y + (row - bar_h) / 2,
        area.width - shift,
        bar_h,
    );
    draw_bar(canvas, env, look, bar, book.percent());
}
