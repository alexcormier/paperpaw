//! The header of screens and the pager below lists, with their arrows.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// A black header bar with a title and an optional right-aligned note.
pub fn draw_header(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    title: &str,
    note: Option<&str>,
) {
    let part = super::look::HeaderPart { area };
    let ink = match style.look.header(canvas, style, part) {
        Some(ink) => ink,
        None => {
            style.header_background(canvas, area);
            style.header_colors().1
        }
    };
    let Some(font) = style.font(env) else {
        return;
    };
    let mut canvas = canvas.clipped(area);
    let margin = style.margin as f32;
    let mut right = f32::from(area.x) + f32::from(area.width) - margin;
    let note = note.map(|n| fit(&font, style.small, n, f32::from(area.width) / 2.0));
    if let Some(note) = note.as_deref() {
        right -= font.measure(style.small, note) + margin;
    }
    let (size, title) = title_fitting(&font, style, title, right - f32::from(area.x) - margin);
    let band = style.header_band(area);
    if let Some(note) = note.as_deref() {
        let (x, baseline) = (right + margin, cap_baseline(&font, band, style.small));
        draw_label_on(
            &mut canvas,
            &font,
            style.small,
            x,
            area,
            baseline,
            note,
            ink,
        );
    }
    let (x, baseline) = (f32::from(area.x) + margin, cap_baseline(&font, band, size));
    draw_label_on(&mut canvas, &font, size, x, area, baseline, &title, ink);
}

/// A screen's header bar for touch screens too: a back arrow on the left
/// and, for lists with several pages, arrows around the page number on the
/// right, so every screen can be left and paged without buttons or hidden
/// gestures. Without touch the arrows are left out (the buttons do that).
///
/// Screens draw it with [`Header::draw`] and pass taps to [`Header::tap`],
/// which tells the action to run (`core.back`, `core.prev-page`,
/// `core.next-page`); the screen handles paging like the page buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub area: Rect,
    /// The note right of the title has the focus (drawn white on black),
    /// for screens where buttons move the focus to it, e.g. a sort order
    /// that opens a menu as a tap on it does.
    pub note_focused: bool,
    /// The note does something when tapped or confirmed (the screen
    /// handles it): told as a button, the header's height high, a
    /// fingertip for touch. Otherwise it is a text, e.g. "3 hits".
    pub note_button: bool,
    /// Shows the back arrow.
    back: bool,
    /// Shows arrows for paging.
    arrows: bool,
}

/// The parts of a header, from [`Header::layout`].
pub(super) struct HeaderParts {
    back: Option<Rect>,
    /// Previous page, page label, next page.
    pager: Option<(Rect, Rect, Rect)>,
    title: Rect,
    /// Where the middles of the back, previous and next arrows are drawn:
    /// the back arrow's ink starts at the margin, the next arrow's ends
    /// there, as the content of the screen does.
    arrows: (i32, i32, i32),
}

impl Header {
    /// A header at the top of the screen; `back` is `false` for root
    /// screens such as the home screen.
    pub fn new(env: &Env<'_>, style: &UiStyle, back: bool) -> Self {
        let touch = env.device.input.touch;
        Self {
            area: Rect::new(0, 0, env.device.display.width, style.header as u16),
            note_focused: false,
            note_button: false,
            back: back && touch,
            arrows: touch,
        }
    }

    /// Page `page.0` (0-based) of `page.1`, e.g. "2 / 5".
    fn page_label(env: &Env<'_>, page: (usize, usize)) -> String {
        let (shown, pages) = (page.0.saturating_add(1), page.1);
        env.i18n.format(
            "core.page-of",
            &[
                ("page", paperpaw_i18n::Arg::Int(shown as i64)),
                ("pages", paperpaw_i18n::Arg::Int(pages as i64)),
            ],
        )
    }

    fn layout(&self, env: &Env<'_>, style: &UiStyle, page: Option<(usize, usize)>) -> HeaderParts {
        let a = self.area;
        let square = a.height;
        let margin = style.margin as u16;
        let (reach, ink) = chevron_extent(style);
        let start = i32::from(a.x) + i32::from(margin);
        let end = i32::from(a.x) + i32::from(a.width) - i32::from(margin);
        // The back arrow where the content's icons are, the title where
        // the labels after them begin (as far as the arrow leaves room).
        let back_x = start + reach;
        let mut left = if self.back {
            let column = (style.text * ICON_SCALE) as u16 + style.gap as u16;
            a.x + margin + column.max(ink as u16 + style.gap as u16)
        } else {
            a.x + margin
        };
        let back = self
            .back
            .then(|| Rect::new(a.x, a.y, left.saturating_sub(a.x), a.height));
        let mut right = a.x + a.width;
        let mut arrows = (back_x, 0, end - reach - 1);
        let pager = match (page, style.font(env)) {
            (Some(page), Some(font)) if page.1 > 1 => {
                let label = font.measure(style.small, &Self::page_label(env, page)) as u16;
                let (next, prev_end) = if self.arrows {
                    // A finger's room round the arrow, out to the edge.
                    let arrow = square * 4 / 5;
                    let x = (arrows.2 - i32::from(arrow / 2)).max(i32::from(a.x)) as u16;
                    (Rect::new(x, a.y, right.saturating_sub(x), a.height), arrow)
                } else {
                    (Rect::new(right - margin, a.y, 0, a.height), 0)
                };
                let label = Rect::new(next.x.saturating_sub(label), a.y, label, a.height);
                let prev = Rect::new(label.x.saturating_sub(prev_end), a.y, prev_end, a.height);
                arrows.1 = i32::from(prev.x) + i32::from(prev.width) / 2;
                right = prev.x;
                Some((prev, label, next))
            }
            _ => None,
        };
        if self.back {
            left = left.min(right);
        }
        HeaderParts {
            back,
            pager,
            title: Rect::new(left, a.y, right.saturating_sub(left), a.height),
            arrows,
        }
    }

    /// Draws the header: the title, an optional note right of it (e.g. a
    /// sort order) and the page number of a paged list (`page` is the
    /// 0-based page and the number of pages).
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        title: &str,
        note: Option<&str>,
        page: Option<(usize, usize)>,
    ) {
        let parts = self.layout(env, style, page);
        // A note that only tells something is said with the title ("„word“,
        // 3 hits"); one that does something is a button of its own.
        let told = match note {
            Some(note) if !(self.note_button || self.note_focused) => {
                alloc::format!("{title}, {note}")
            }
            _ => String::from(title),
        };
        canvas.describe(Role::Heading, &told, self.area, false, None);
        style.header_background(canvas, self.area);
        let ink = style.header_colors().1;
        let Some(font) = style.font(env) else {
            return;
        };
        // Everything centred in the band by its ink: the arrows by their
        // drawing, each text by its capitals.
        let band = style.header_band(self.area);
        let arrow = |canvas: &mut Canvas<'_>, x: i32, icon: &str, left: bool, color: Color| {
            let (_, y) = center(band);
            draw_arrow(canvas, env, style, (x, y), icon, left, color);
        };
        let t = parts.title;
        let margin = style.margin as f32;
        let mut right = f32::from(t.x) + f32::from(t.width) - margin;
        let note = note.map(|n| fit(&font, style.small, n, f32::from(t.width) / 2.0));
        let pad = style.gap as f32 / 2.0;
        let note_x = note.as_deref().map(|note| {
            right -= font.measure(style.small, note);
            let x = right;
            right -= margin + pad;
            x
        });
        let (size, title) = title_fitting(&font, style, title, right - f32::from(t.x));
        let label = page
            .filter(|_| parts.pager.is_some())
            .map(|page| Self::page_label(env, page));
        let baseline = |size: f32| cap_baseline(&font, band, size);
        let (back_x, prev_x, next_x) = parts.arrows;
        if parts.back.is_some() {
            arrow(canvas, back_x, crate::icons::BACK, true, ink);
        }
        if let (Some((_, at, _)), Some(page), Some(text)) = (parts.pager, page, &label) {
            if self.arrows {
                // At the first or last page, the arrow is gray.
                let color = |end: bool| if end { style.ink(Ink::Disabled) } else { ink };
                arrow(canvas, prev_x, crate::icons::PREV, true, color(page.0 == 0));
                arrow(
                    canvas,
                    next_x,
                    crate::icons::NEXT,
                    false,
                    color(page.0 + 1 >= page.1),
                );
            }
            let x = f32::from(at.x);
            let y = baseline(style.small);
            draw_label_on(canvas, &font, style.small, x, at, y, text, ink);
        }
        let mut canvas = canvas.clipped(t);
        if let (Some(note), Some(x)) = (note.as_deref(), note_x) {
            let width = font.measure(style.small, note);
            // Focused: white on black, a little room around it.
            let high = font.line_metrics(style.small).line_height as u16 + style.gap as u16;
            let around = Rect::new(
                (x - pad) as u16,
                band.y + band.height.saturating_sub(high) / 2,
                (width + 2.0 * pad) as u16,
                high,
            );
            if self.note_button || self.note_focused {
                // As high as the bar, where a tap on it is taken.
                let target = Rect::new(around.x, band.y, around.width, band.height);
                canvas.describe(Role::Button, note, target, self.note_focused, None);
            }
            let note_ink = if self.note_focused {
                canvas.fill_round(around, style.radius(u32::from(around.height)), ink);
                style.header_colors().0
            } else {
                ink
            };
            let y = baseline(style.small);
            draw_label_on(&mut canvas, &font, style.small, x, t, y, note, note_ink);
        }
        let x = f32::from(t.x);
        let y = baseline(size);
        draw_label_on(&mut canvas, &font, size, x, t, y, &title, ink);
    }

    /// The action a tap at `point` stands for: back, or the previous or
    /// next page. `None` outside the header or on its title.
    pub fn tap(
        &self,
        env: &Env<'_>,
        style: &UiStyle,
        point: Point,
        page: Option<(usize, usize)>,
    ) -> Option<crate::Id> {
        if !self.area.contains(point) {
            return None;
        }
        let parts = self.layout(env, style, page);
        if parts.back.is_some_and(|r| r.contains(point)) {
            return Some(actions::BACK);
        }
        let (prev, label, next) = parts.pager.filter(|_| self.arrows)?;
        if prev.contains(point) || (label.contains(point) && point.x < label_mid(label)) {
            Some(actions::PREV_PAGE)
        } else if next.contains(point) || label.contains(point) {
            Some(actions::NEXT_PAGE)
        } else {
            None
        }
    }

    /// Whether `point` is on the page number between the arrows, e.g. to
    /// go to a page of a long list (the screen asks before
    /// [`Header::tap`], which turns a page there).
    pub fn on_page_number(
        &self,
        env: &Env<'_>,
        style: &UiStyle,
        point: Point,
        page: Option<(usize, usize)>,
    ) -> bool {
        self.area.contains(point)
            && self
                .layout(env, style, page)
                .pager
                .is_some_and(|(_, label, _)| label.contains(point))
    }

    /// Whether `point` is on the header's title (e.g. to open a menu).
    pub fn on_title(
        &self,
        env: &Env<'_>,
        style: &UiStyle,
        point: Point,
        page: Option<(usize, usize)>,
    ) -> bool {
        self.layout(env, style, page).title.contains(point)
    }
}

/// A bar under a list without a header (a menu, a popup) that shows the
/// page and, on touch screens, has arrows to turn pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pager {
    pub area: Rect,
    arrows: bool,
}

impl Pager {
    pub fn new(env: &Env<'_>, area: Rect) -> Self {
        Self {
            area,
            arrows: env.device.input.touch,
        }
    }

    /// The height of a pager.
    pub fn height(style: &UiStyle) -> u16 {
        (style.row * 3 / 4) as u16
    }

    /// Draws page `page.0` (0-based) of `page.1`.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        page: (usize, usize),
    ) {
        let a = self.area;
        canvas.fill(a, Color::WHITE);
        if style.separators != Separators::None {
            canvas.fill_rect(
                i32::from(a.x) + style.margin as i32,
                i32::from(a.y),
                u32::from(a.width).saturating_sub(style.margin * 2),
                style.line.max(1),
                style.ink(Ink::Rule),
            );
        }
        let Some(font) = style.font(env) else {
            return;
        };
        let label = Header::page_label(env, page);
        let x = f32::from(a.x) + (f32::from(a.width) - font.measure(style.small, &label)) / 2.0;
        draw_label(canvas, &font, style.small, x, a, &label, Color::BLACK);
        if self.arrows {
            let side = i32::from(a.width) / 4;
            let (_, y) = center(a);
            let dim = |end: bool| {
                if end {
                    style.ink(Ink::Disabled)
                } else {
                    Color::BLACK
                }
            };
            let x = i32::from(a.x);
            let prev = (x + side, y);
            draw_arrow(
                canvas,
                env,
                style,
                prev,
                crate::icons::PREV,
                true,
                dim(page.0 == 0),
            );
            let next = (x + side * 3, y);
            let last = dim(page.0 + 1 >= page.1);
            draw_arrow(canvas, env, style, next, crate::icons::NEXT, false, last);
        }
    }

    /// `core.prev-page` or `core.next-page` for a tap on the left or right
    /// half; `None` outside.
    pub fn tap(&self, point: Point) -> Option<crate::Id> {
        if !self.arrows || !self.area.contains(point) {
            return None;
        }
        Some(if point.x < label_mid(self.area) {
            actions::PREV_PAGE
        } else {
            actions::NEXT_PAGE
        })
    }
}

/// A header's title in `room` pixels: at its size where it fits, else
/// smaller, down to the body's size, before it is shortened (the arrows
/// and the page number beside it keep their room).
fn title_fitting(font: &FontHandle<'_>, style: &UiStyle, title: &str, room: f32) -> (f32, String) {
    let least = style.text.min(style.head);
    let size = crate::widgets::largest_size(style.head, least, |size| {
        font.measure(size, title) <= room + 1.0
    })
    .unwrap_or(least);
    (size, fit(font, size, title, room))
}

pub(super) fn center(area: Rect) -> (i32, i32) {
    (
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) + i32::from(area.height) / 2,
    )
}

/// The arrow of semantic icon `icon` ([`icons::BACK`](crate::icons::BACK),
/// [`PREV`](crate::icons::PREV), [`NEXT`](crate::icons::NEXT)) centred
/// at `at`: the icon a theme gave it, as tall as titles, else a chevron
/// pointing left or right ([`draw_chevron`]).
pub fn draw_arrow(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    at: (i32, i32),
    icon: &str,
    left: bool,
    color: Color,
) {
    if !draw_icon_at(canvas, env, icon, style.title, at, color) {
        draw_chevron(canvas, style, at.0, at.1, left, color);
    }
}

/// An arrow head pointing left or right, centred at `(x, y)` and as tall
/// as a capital letter of titles. Drawn rather than taken from a font, so
/// it is centred exactly and needs no special glyph.
pub fn draw_chevron(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    x: i32,
    y: i32,
    left: bool,
    color: Color,
) {
    let half = (style.title * 0.38) as i32;
    let (_, extent) = chevron_extent(style);
    let thickness = (style.line * 3).max(3);
    let width = extent - thickness as i32;
    for dy in -half..=half {
        let dx = width * dy.abs() / half.max(1);
        let tip = if left {
            x - width / 2 + dx
        } else {
            x + width / 2 - dx
        };
        let start = if left {
            tip
        } else {
            tip - thickness as i32 + 1
        };
        // A tint, so a light (unavailable) arrow shows as gray on panels
        // of black and white alone too.
        if let (Ok(x), Ok(y)) = (u16::try_from(start), u16::try_from(y + dy)) {
            canvas.fill_tint(Rect::new(x, y, thickness as u16, 1), color);
        }
    }
}

/// How far a chevron's ink reaches from the middle it is drawn at to its
/// point, and how wide its ink is (see [`draw_chevron`]).
pub(super) fn chevron_extent(style: &UiStyle) -> (i32, i32) {
    let half = (style.title * 0.38) as i32;
    let width = half * 4 / 5;
    let thickness = (style.line * 3).max(3) as i32;
    (width / 2, width + thickness)
}

pub(super) fn label_mid(label: Rect) -> i32 {
    i32::from(label.x) + i32::from(label.width) / 2
}
