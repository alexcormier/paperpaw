//! A text field in a row ([`draw_text_field`], edited on the keyboard's
//! screen with [`edit_text`]), rows ticked or not of a list where several
//! may be ([`draw_check_row`]), and text longer than its room, scrolled
//! with the buttons and by touch ([`TextView`]).

use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_graphics::{Canvas, Role};
use paperpaw_platform::{Color, Point, Rect};

use crate::builtin::actions;
use crate::env::Env;
use crate::event::Event;
use crate::gesture::{Direction, Gesture, GestureKind};
use crate::replies::Typed;
use crate::{Command, Id};

use super::{Ink, UiStyle};

/// Where a text field's box goes in its row: the right part, a margin in
/// from the row's end, as tall as a button.
pub fn text_field_box(style: &UiStyle, row: Rect) -> Rect {
    let margin = style.margin as u16;
    let height = super::button_height(style, row);
    let width = (row.width / 2).saturating_sub(margin);
    Rect::new(
        (row.x + row.width).saturating_sub(width + margin),
        row.y + (row.height - height) / 2,
        width,
        height,
    )
}

/// Draws a one-line text field in `row`: `title` on the left, `value` in a
/// framed box on the right (`placeholder`, gray, while it is empty),
/// shortened to fit. With `focused`, the row is focused (draw the focus
/// first, as for other rows). *Select* or a tap opens the keyboard
/// ([`edit_text`]); the value comes back as the reply.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_field(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    row: Rect,
    focused: bool,
    title: &str,
    value: &str,
    placeholder: &str,
) {
    let field = text_field_box(style, row);
    canvas.describe(Role::Field, value, field, focused, None);
    let Some(font) = style.font(env) else {
        return;
    };
    let ink = style.focus_ink(focused);
    let margin = style.margin as u16;
    let title_room = Rect::new(
        row.x + margin,
        row.y,
        field.x.saturating_sub(row.x + 2 * margin),
        row.height,
    );
    let title = super::fit(&font, style.text, title, f32::from(title_room.width));
    super::draw_label(
        canvas,
        &font,
        style.text,
        f32::from(title_room.x),
        title_room,
        &title,
        ink,
    );
    let paper = if focused && style.dark_focus() {
        Color::BLACK
    } else {
        Color::WHITE
    };
    let radius = style.radius(u32::from(field.height));
    canvas.fill_round(field, radius, paper);
    canvas.stroke_round(field, super::button_border(style), radius, ink);
    let inside = Rect::new(
        field.x + margin / 2,
        field.y,
        field.width.saturating_sub(margin),
        field.height,
    );
    let (text, color) = if value.is_empty() {
        let muted = if focused && style.dark_focus() {
            Color::WHITE
        } else {
            style.ink(Ink::Muted)
        };
        (placeholder, muted)
    } else {
        (value, ink)
    };
    let shown = super::fit(&font, style.text, text, f32::from(inside.width));
    super::draw_label(
        canvas,
        &font,
        style.text,
        f32::from(inside.x),
        inside,
        &shown,
        color,
    );
}

/// What opens the keyboard for a text field titled `title` with `value`
/// in it; what is typed comes back as `reply` ([`ask::text`](crate::ask::text)).
pub fn edit_text(title: &str, value: &str, reply: &Event<Typed>) -> Command {
    crate::ask::text(title, reply).initial(value).command()
}

/// Where the box of a ticked row goes: at the row's left, a margin in, as
/// a switch is tall.
pub fn check_rect(style: &UiStyle, row: Rect) -> Rect {
    let side = (row.height * 7 / 10 * 3 / 4).max(8);
    Rect::new(
        row.x + style.margin as u16,
        row.y + (row.height - side) / 2,
        side,
        side,
    )
}

/// Draws a box, ticked (`on`) or not, in `rect`; `inverted` on a focused
/// (black) row. A look of a plugin's own may paint it
/// ([`Painters::check`](super::look::Painters::check)).
pub fn draw_check(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, on: bool, inverted: bool) {
    canvas.describe(Role::Check, "", rect, false, Some(on));
    let part = super::look::CheckPart { rect, on, inverted };
    if style.look.check(canvas, style, part) {
        return;
    }
    let (ink, paper) = if inverted {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    let border = super::button_border(style);
    let radius = (style.corner / 2).min(u32::from(rect.height) / 4);
    if !on {
        canvas.fill_round(rect, radius, paper);
        canvas.stroke_round(rect, border, radius, ink);
        return;
    }
    canvas.fill_round(rect, radius, ink);
    // The tick: down to the lower third, then up to the right.
    let side = i32::from(rect.width);
    let (x, y) = (i32::from(rect.x), i32::from(rect.y));
    let width = (border * 2).max(2);
    let low = Point::new(x + side * 2 / 5, y + side * 7 / 10);
    canvas.draw_line(Point::new(x + side / 4, y + side / 2), low, width, paper);
    canvas.draw_line(
        low,
        Point::new(x + side * 3 / 4, y + side * 3 / 10),
        width,
        paper,
    );
}

/// Draws a row of a list where several may be ticked: its box on the
/// left ([`draw_check`]), `title` beside it, `note` small below where
/// given. Draw the focus first where it is focused, as for other rows.
#[allow(clippy::too_many_arguments)]
pub fn draw_check_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    row: Rect,
    focused: bool,
    title: &str,
    note: &str,
    on: bool,
) {
    let check = check_rect(style, row);
    draw_check(canvas, style, check, on, focused && style.dark_focus());
    let Some(font) = style.font(env) else {
        return;
    };
    let ink = style.focus_ink(focused);
    let x = check.x + check.width + style.margin as u16;
    let width = (row.x + row.width).saturating_sub(x + style.margin as u16);
    if note.is_empty() {
        let text = super::fit(&font, style.text, title, f32::from(width));
        let at = Rect::new(x, row.y, width, row.height);
        super::draw_label(canvas, &font, style.text, f32::from(x), at, &text, ink);
        return;
    }
    let line = font.line_metrics(style.text).line_height as u16;
    let small = font.line_metrics(style.small).line_height as u16;
    let top = row.y + row.height.saturating_sub(line + small) / 2;
    let text = super::fit(&font, style.text, title, f32::from(width));
    super::draw_label(
        canvas,
        &font,
        style.text,
        f32::from(x),
        Rect::new(x, top, width, line),
        &text,
        ink,
    );
    let muted = if focused && style.dark_focus() {
        Color::WHITE
    } else {
        style.ink(Ink::Muted)
    };
    let note = super::fit(&font, style.small, note, f32::from(width));
    let at = Rect::new(x, top + line, width, small);
    super::draw_label(canvas, &font, style.small, f32::from(x), at, &note, muted);
}

/// Text longer than its room, a page of lines at a time: the up and down
/// buttons scroll a few lines, the page buttons a page; by touch, a swipe
/// up or down scrolls a page, a tap in the lower or upper half too. The
/// caller keeps the text; the view keeps where its lines begin and end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextView {
    area: Rect,
    /// Each line: where it lies in the text.
    lines: Vec<Range<u32>>,
    line: u16,
    /// The first line shown.
    top: usize,
}

impl TextView {
    /// `text` (paragraphs apart by line breaks) wrapped into `area` in the
    /// style's text size.
    pub fn new(env: &Env<'_>, style: &UiStyle, area: Rect, text: &str) -> Self {
        let Some(font) = style.font(env) else {
            return Self::of_lines(area, 1, Vec::new());
        };
        let width = f32::from(area.width.saturating_sub(style.gap as u16));
        let mut lines = Vec::new();
        let mut start = 0;
        for paragraph in text.split('\n') {
            let wrapped = font.wrap(style.text, paragraph, width);
            if wrapped.is_empty() {
                lines.push(start as u32..start as u32);
            }
            for range in wrapped {
                lines.push((start + range.start) as u32..(start + range.end) as u32);
            }
            start += paragraph.len() + 1;
        }
        lines.shrink_to_fit();
        let line = font.line_metrics(style.text).line_height as u16;
        Self::of_lines(area, line, lines)
    }

    pub(crate) fn of_lines(area: Rect, line: u16, lines: Vec<Range<u32>>) -> Self {
        Self {
            area,
            lines,
            line: line.max(1),
            top: 0,
        }
    }

    /// The lines that fit at once (at least one).
    pub fn rows(&self) -> usize {
        usize::from(self.area.height / self.line).max(1)
    }

    /// The first line shown.
    pub fn top(&self) -> usize {
        self.top
    }

    /// The first line the last page begins with.
    fn last_top(&self) -> usize {
        self.lines.len().saturating_sub(self.rows())
    }

    /// Scrolls `by` lines (back where negative), within the text; whether
    /// it moved.
    pub fn scroll(&mut self, by: isize) -> bool {
        let to = self.top.saturating_add_signed(by).min(self.last_top());
        let moved = to != self.top;
        self.top = to;
        moved
    }

    /// Scrolls as `action` says: up and down a third of a page, the page
    /// buttons (and previous and next) a page; whether it moved. Other
    /// actions are not its (`false`, and nothing moves).
    pub fn on_action(&mut self, action: &Id) -> bool {
        let page = self.rows() as isize;
        let few = (page / 3).max(1);
        let by = if *action == actions::DOWN {
            few
        } else if *action == actions::UP {
            -few
        } else if [actions::NEXT_PAGE, actions::NEXT].contains(action) {
            page
        } else if [actions::PREV_PAGE, actions::PREVIOUS].contains(action) {
            -page
        } else {
            return false;
        };
        self.scroll(by)
    }

    /// Scrolls as `gesture` says: a swipe up a page on, down a page back;
    /// a tap in the lower half a page on, in the upper half back. Whether
    /// it moved.
    pub fn on_gesture(&mut self, gesture: &Gesture) -> bool {
        if !self.area.contains(gesture.start) {
            return false;
        }
        let page = self.rows() as isize;
        let middle = i32::from(self.area.y) + i32::from(self.area.height) / 2;
        let by = match gesture.kind {
            GestureKind::Swipe(Direction::Up) => page,
            GestureKind::Swipe(Direction::Down) => -page,
            GestureKind::Tap if gesture.start.y >= middle => page,
            GestureKind::Tap => -page,
            _ => return false,
        };
        self.scroll(by)
    }

    /// Draws the lines shown of `text` (the text it was made of), and on
    /// the right a thin bar where they are in the whole, where there is
    /// more than a page.
    pub fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, text: &str) {
        let Some(font) = style.font(env) else {
            return;
        };
        let shown = self.lines.iter().skip(self.top).take(self.rows());
        for (i, range) in shown.enumerate() {
            let y = self.area.y + i as u16 * self.line;
            let at = Rect::new(self.area.x, y, self.area.width, self.line);
            let part = text
                .get(range.start as usize..range.end as usize)
                .unwrap_or_default();
            super::draw_label(
                canvas,
                &font,
                style.text,
                f32::from(at.x),
                at,
                part,
                Color::BLACK,
            );
        }
        if self.lines.len() > self.rows() {
            let bar = (style.line.max(1) * 2) as u16;
            let x = (self.area.x + self.area.width).saturating_sub(bar);
            let whole = u32::from(self.area.height);
            let all = self.lines.len() as u32;
            let length = (whole * self.rows() as u32 / all).max(u32::from(bar) * 4) as u16;
            let y = self.area.y + (whole * self.top as u32 / all) as u16;
            canvas.fill(
                Rect::new(x, y, bar, length.min(self.area.height)),
                style.ink(Ink::Muted),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;
    use paperpaw_platform::PointerTool;

    fn view(lines: usize) -> TextView {
        let ranges = (0..lines as u32).map(|i| i..i).collect();
        TextView::of_lines(Rect::new(0, 100, 300, 200), 20, ranges)
    }

    fn gesture(kind: GestureKind, y: i32) -> Gesture {
        Gesture::new(
            kind,
            Point::new(50, y),
            Point::new(50, y),
            PointerTool::Finger,
        )
    }

    #[test]
    fn a_text_view_scrolls_by_buttons_within_the_text() {
        let mut v = view(25);
        assert_eq!(v.rows(), 10);
        assert!(v.on_action(&actions::DOWN));
        assert_eq!(v.top(), 3);
        assert!(v.on_action(&actions::NEXT_PAGE));
        assert_eq!(v.top(), 13);
        // Not past the last page.
        assert!(v.on_action(&actions::NEXT_PAGE));
        assert_eq!(v.top(), 15);
        assert!(!v.on_action(&actions::NEXT_PAGE));
        assert!(v.on_action(&actions::PREV_PAGE));
        assert_eq!(v.top(), 5);
        assert!(!v.on_action(&actions::CONFIRM));
        // Text that fits does not scroll.
        assert!(!view(5).on_action(&actions::DOWN));
    }

    #[test]
    fn a_text_view_scrolls_by_touch() {
        let mut v = view(25);
        assert!(v.on_gesture(&gesture(GestureKind::Swipe(Direction::Up), 150)));
        assert_eq!(v.top(), 10);
        assert!(v.on_gesture(&gesture(GestureKind::Tap, 120)));
        assert_eq!(v.top(), 0);
        assert!(v.on_gesture(&gesture(GestureKind::Tap, 280)));
        assert_eq!(v.top(), 10);
        // Outside its area: not its.
        assert!(!v.on_gesture(&gesture(GestureKind::Tap, 50)));
    }

    #[test]
    fn a_text_field_s_box_and_a_check_lie_in_their_row() {
        let row = Rect::new(0, 40, 480, 60);
        let field = text_field_box(&STYLE, row);
        assert!(
            field.x > row.width / 2 - 1
                && field.right() <= u32::from(row.width - STYLE.margin as u16)
        );
        assert!(field.y >= row.y && field.bottom() <= row.bottom());
        let check = check_rect(&STYLE, row);
        assert_eq!(check.x, STYLE.margin as u16);
        assert_eq!(check.width, check.height);
        assert!(check.bottom() <= row.bottom());
    }

    #[test]
    fn editing_a_field_asks_for_text_with_its_value() {
        const NAMED: Event<Typed> = Event::new("demo.named");
        let command = edit_text("Name", "Ada", &NAMED);
        assert!(
            matches!(command, Command::OpenSlot(..) | Command::OpenScreen(..)),
            "{command:?}"
        );
    }
}
