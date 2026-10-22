//! A dialog over a screen: a framed box with a title, a few lines of text
//! and a row of buttons, the same space all round and between its parts
//! (the style's margin). Where it goes on the screen is the caller's
//! ([`Dialog::place`]); what the buttons do too ([`Dialog::hit`], and the
//! focus moved with [`Segments::step`](super::Segments::step)'s rule:
//! left and right).
//!
//! ```
//! # fn demo(env: &paperpaw_sdk::Env<'_>) {
//! use paperpaw_sdk::ui::{Dialog, UiStyle};
//!
//! let style = UiStyle::new(env);
//! let mut dialog = Dialog::new(env, &style, "Delete the book?", &["It goes for good."], &["Delete", "Keep"]);
//! let room = env.device.display.height - dialog.height();
//! dialog.place(room / 2);
//! # }
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};

use crate::Id;
use crate::builtin::actions;
use crate::env::Env;
use crate::widgets::Frame;

use super::UiStyle;

/// A dialog: where its box and buttons are, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    panel: Rect,
    title: String,
    lines: Vec<String>,
    labels: Vec<String>,
    buttons: Vec<Rect>,
    /// The height of the title's line and of a line of text.
    title_line: u16,
    text_line: u16,
    /// The height of the screen: the box stays within it.
    room: u16,
}

impl Dialog {
    /// A dialog as wide as the screen less its margins, as tall as
    /// `title`, `text` (each a paragraph, wrapped) and `buttons` need, at
    /// the top of the screen until [placed](Self::place). Never taller
    /// than the screen less its margins: text that does not fit is cut,
    /// its last line ending in an ellipsis, the buttons kept.
    pub fn new(
        env: &Env<'_>,
        style: &UiStyle,
        title: &str,
        text: &[&str],
        buttons: &[&str],
    ) -> Self {
        let display = env.device.display;
        let margin = style.margin as u16;
        let width = display.width.saturating_sub(2 * margin);
        let inner = f32::from(width.saturating_sub(2 * margin));
        let (title_line, text_line, mut lines) = match style.font(env) {
            Some(font) => {
                let lines = text
                    .iter()
                    .flat_map(|paragraph| {
                        let wrapped = font.wrap(style.text, paragraph, inner);
                        if wrapped.is_empty() {
                            return alloc::vec![String::new()];
                        }
                        wrapped
                            .into_iter()
                            .map(|range| paragraph[range].to_string())
                            .collect::<Vec<_>>()
                    })
                    .collect();
                (
                    font.line_metrics(style.title).line_height as u16,
                    font.line_metrics(style.text).line_height as u16,
                    lines,
                )
            }
            None => (0, 0, Vec::new()),
        };
        lines.shrink_to_fit();
        let mut dialog = Self {
            panel: Rect::new(margin, 0, width, 0),
            title: title.to_string(),
            lines,
            labels: buttons.iter().map(|b| b.to_string()).collect(),
            buttons: Vec::new(),
            title_line,
            text_line,
            room: display.height,
        };
        dialog.lay_out(style, 0);
        dialog
    }

    /// Its height: the margin above, between and below its parts.
    pub fn height(&self) -> u16 {
        self.panel.height
    }

    /// Its box.
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// Puts its top at `y`, or higher where it would not end on the
    /// screen.
    pub fn place(&mut self, y: u16) {
        let y = y.min(self.room.saturating_sub(self.panel.height));
        let dy = i32::from(y) - i32::from(self.panel.y);
        self.panel.y = y;
        for button in &mut self.buttons {
            button.y = (i32::from(button.y) + dy).max(0) as u16;
        }
    }

    /// Where button `index` is.
    pub fn button(&self, index: usize) -> Option<Rect> {
        self.buttons.get(index).copied()
    }

    /// The button at `point`.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.buttons.iter().position(|b| b.contains(point))
    }

    /// Where the focus goes from button `focus` with `action`: left and
    /// right (and previous and next) along the buttons. `None` for other
    /// actions.
    pub fn step(&self, focus: usize, action: &Id) -> Option<usize> {
        let last = self.buttons.len().checked_sub(1)?;
        if [actions::LEFT, actions::PREVIOUS].contains(action) {
            Some(focus.saturating_sub(1))
        } else if [actions::RIGHT, actions::NEXT].contains(action) {
            Some((focus + 1).min(last))
        } else {
            None
        }
    }

    fn lay_out(&mut self, style: &UiStyle, y: u16) {
        let margin = style.margin as u16;
        let button_h = if self.labels.is_empty() {
            0
        } else {
            super::Segments::height(style)
        };
        // What the text may take: the screen less its margins, the box's
        // space round and between its parts, its title and its buttons.
        let fixed = [self.title_line, button_h, 1]
            .iter()
            .filter(|p| **p > 0)
            .count() as u16;
        let room = self
            .room
            .saturating_sub(2 * margin)
            .saturating_sub(margin.saturating_mul(fixed + 1))
            .saturating_sub(self.title_line)
            .saturating_sub(button_h);
        let fit = usize::from(room / self.text_line.max(1));
        if self.lines.len() > fit {
            self.lines.truncate(fit);
            if let Some(last) = self.lines.last_mut() {
                last.push('…');
            }
        }
        let text = self.text_height();
        let parts = [self.title_line, text, button_h];
        let shown = parts.iter().filter(|p| **p > 0).count() as u16;
        let height = parts
            .iter()
            .fold(0u16, |sum, p| sum.saturating_add(*p))
            .saturating_add(margin.saturating_mul(shown + 1));
        self.panel = Rect::new(self.panel.x, y, self.panel.width, height);
        let inside = super::card_inside(style, self.panel, Frame::Line);
        let row = super::Stack::new(inside, margin).take_bottom(button_h);
        self.buttons = super::columns(row, self.labels.len(), margin);
    }

    /// The height of its text's lines.
    fn text_height(&self) -> u16 {
        let lines = u16::try_from(self.lines.len()).unwrap_or(u16::MAX);
        self.text_line.saturating_mul(lines)
    }

    /// Draws it, white with a frame (as the look draws frames), the
    /// button `focus` focused.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        focus: Option<usize>,
    ) {
        canvas.fill(self.panel, Color::WHITE);
        let inside = super::draw_card(canvas, style, self.panel, Frame::Line);
        let Some(font) = style.font(env) else {
            return;
        };
        let mut stack = super::Stack::new(inside, style.margin as u16);
        if self.title_line > 0 {
            let row = stack.take(self.title_line);
            let x = f32::from(row.x)
                + (f32::from(row.width) - font.measure(style.title, &self.title)).max(0.0) / 2.0;
            super::draw_label(
                canvas,
                &font,
                style.title,
                x,
                row,
                &self.title,
                Color::BLACK,
            );
        }
        if !self.lines.is_empty() {
            let text = stack.take(self.text_height());
            for (i, line) in self.lines.iter().enumerate() {
                let row = Rect::new(
                    text.x,
                    text.y + i as u16 * self.text_line,
                    text.width,
                    self.text_line,
                );
                super::draw_label(
                    canvas,
                    &font,
                    style.text,
                    f32::from(row.x),
                    row,
                    line,
                    Color::BLACK,
                );
            }
        }
        for (i, (rect, label)) in self.buttons.iter().zip(&self.labels).enumerate() {
            super::draw_button(canvas, env, style, *rect, label, false, focus == Some(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    fn dialog(lines: usize, buttons: usize) -> Dialog {
        let mut dialog = Dialog {
            panel: Rect::new(20, 0, 440, 0),
            title: String::from("Title"),
            lines: (0..lines).map(|_| String::from("line")).collect(),
            labels: (0..buttons).map(|_| String::from("button")).collect(),
            buttons: Vec::new(),
            title_line: 30,
            text_line: 20,
            room: 800,
        };
        dialog.lay_out(&STYLE, 0);
        dialog
    }

    #[test]
    fn a_dialog_keeps_one_space_all_round() {
        let margin = STYLE.margin as u16;
        let button = crate::ui::Segments::height(&STYLE);
        let d = dialog(2, 2);
        assert_eq!(d.height(), 4 * margin + 30 + 40 + button);
        let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
        // The buttons a margin from the box's sides and bottom, and apart.
        assert_eq!(first.x, 20 + margin);
        assert_eq!(second.right(), u32::from(20 + 440 - margin));
        assert_eq!(second.x, first.right() as u16 + margin);
        assert_eq!(first.bottom(), u32::from(d.height() - margin));
    }

    #[test]
    fn a_placed_dialog_moves_its_buttons_and_hits_them() {
        let mut d = dialog(1, 2);
        d.place(300);
        let second = d.button(1).unwrap();
        assert_eq!(d.panel().y, 300);
        assert_eq!(
            d.hit(Point::new(i32::from(second.x) + 1, i32::from(second.y) + 1)),
            Some(1)
        );
        assert_eq!(d.hit(Point::new(0, 0)), None);
        assert_eq!(d.step(0, &actions::RIGHT), Some(1));
        assert_eq!(d.step(1, &actions::RIGHT), Some(1));
        assert_eq!(d.step(1, &actions::CONFIRM), None);
    }

    #[test]
    fn a_dialog_too_tall_for_the_x4_keeps_its_buttons_on_the_screen() {
        let margin = STYLE.margin as u16;
        let mut d = dialog(500, 2);
        assert!(d.height() <= 800 - 2 * margin, "{}", d.height());
        assert!(d.lines.len() < 500);
        assert!(d.lines.last().unwrap().ends_with('…'));
        // Placed low: it moves up so all of it shows.
        d.place(700);
        assert!(d.panel().bottom() <= 800);
        for button in [d.button(0).unwrap(), d.button(1).unwrap()] {
            assert!(button.bottom() <= u32::from(d.panel().bottom() as u16 - margin));
        }
        // Lines beyond what any number holds: no overflow.
        assert!(dialog(70_000, 1).height() <= 800);
    }

    #[test]
    fn without_text_or_buttons_no_space_is_kept_for_them() {
        let margin = STYLE.margin as u16;
        assert_eq!(dialog(0, 0).height(), 2 * margin + 30);
    }
}
