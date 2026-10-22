//! A row of chips, one of them chosen: a few filters or options side by
//! side, each as wide as its word needs ([`Chips`]). Where the choices
//! should share the width evenly, use [`Segments`](super::Segments).

use alloc::vec::Vec;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Point, Rect};

use crate::Id;
use crate::builtin::actions;
use crate::env::Env;

use super::UiStyle;

/// A row of chips, one of them chosen: each a button
/// ([`draw_button`](super::draw_button)) as wide as its label needs with
/// the room a button keeps at its sides
/// ([`button_width`](super::button_width)), at least as wide as high, the
/// style's gap apart, from the left of the area. A chip that does not fit is cut to what is left of the area;
/// those after it are not shown. With the buttons, left and right move
/// the focus, *Select* chooses.
///
/// Made once with the layout (it measures the labels); drawing and
/// hitting allocate nothing. The default is a row of no chips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chips {
    /// One per label; empty for those not shown.
    cells: Vec<Rect>,
}

impl Chips {
    /// The chips of `labels` in `area`, measured in the UI font at the
    /// size [`draw_button`](super::draw_button) sets them, centred in the
    /// area's height.
    pub fn new(env: &Env<'_>, style: &UiStyle, area: Rect, labels: &[&str]) -> Self {
        let font = style.font(env);
        let widths = labels.iter().map(|label| {
            let single = label.chars().count() == 1;
            let size = if single { style.text } else { style.small };
            let text = match &font {
                Some(font) => font.measure(size, label),
                // No font: about half the size a character.
                None => label.chars().count() as f32 * size / 2.0,
            };
            // Rounded up, so the word keeps all its room.
            let width = super::button_width(style, text);
            width as u16 + u16::from(width > f32::from(width as u16))
        });
        Self::from_widths(style, area, widths)
    }

    /// The chips in `area` with the label widths `widths` already known
    /// (the room at their sides included): as [`Chips::new`].
    pub(crate) fn from_widths(
        style: &UiStyle,
        area: Rect,
        widths: impl IntoIterator<Item = u16>,
    ) -> Self {
        let height = Self::height(style).min(area.height);
        let y = area.y + (area.height - height) / 2;
        let gap = style.gap as u16;
        let right = area.x.saturating_add(area.width);
        let mut x = area.x;
        let cells = widths
            .into_iter()
            .map(|width| {
                let width = width.max(height).min(right.saturating_sub(x));
                if width == 0 {
                    return Rect::new(right, y, 0, 0);
                }
                let cell = Rect::new(x, y, width, height);
                x = x.saturating_add(width).saturating_add(gap).min(right);
                cell
            })
            .collect();
        Self { cells }
    }

    /// The height of a row of chips: a button's in a row that can be
    /// touched, as [`Segments`](super::Segments) are.
    pub fn height(style: &UiStyle) -> u16 {
        super::Segments::height(style)
    }

    /// Where chip `index` is; none for one not shown (past the area).
    pub fn cell(&self, index: usize) -> Option<Rect> {
        self.cells.get(index).copied().filter(|c| c.width > 0)
    }

    /// The chip at `point`.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.cells.iter().position(|c| c.contains(point))
    }

    /// How many chips are shown: those that fit, at least in part.
    pub fn shown(&self) -> usize {
        self.cells.iter().take_while(|c| c.width > 0).count()
    }

    /// Where the focus goes from `focus` with `action`: left and right
    /// (and previous and next), staying among the chips shown. `None` for
    /// other actions.
    pub fn step(&self, focus: usize, action: &Id) -> Option<usize> {
        let last = self.shown().checked_sub(1)?;
        if [actions::LEFT, actions::PREVIOUS].contains(action) {
            Some(focus.saturating_sub(1).min(last))
        } else if [actions::RIGHT, actions::NEXT].contains(action) {
            Some((focus + 1).min(last))
        } else {
            None
        }
    }

    /// Draws the chips shown with `labels`, `chosen` on and `focus`
    /// (where the focus is on them) focused.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        labels: &[&str],
        chosen: usize,
        focus: Option<usize>,
    ) {
        for (i, (cell, label)) in self.cells.iter().zip(labels).enumerate() {
            if cell.width == 0 {
                break;
            }
            super::draw_button(
                canvas,
                env,
                style,
                *cell,
                label,
                i == chosen,
                focus == Some(i),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    #[test]
    fn chips_go_from_the_left_a_gap_apart_each_its_own_width() {
        let height = Chips::height(&STYLE);
        let gap = STYLE.gap as u16;
        let wide = height * 3;
        let area = Rect::new(10, 20, 1000, height + 10);
        let chips = Chips::from_widths(&STYLE, area, [wide, wide * 2]);
        assert_eq!(chips.cell(0), Some(Rect::new(10, 25, wide, height)));
        assert_eq!(
            chips.cell(1),
            Some(Rect::new(10 + wide + gap, 25, wide * 2, height))
        );
        assert_eq!(chips.cell(2), None);
        // A narrow word's chip is as wide as high.
        let narrow = Chips::from_widths(&STYLE, area, [1]);
        assert_eq!(narrow.cell(0).unwrap().width, height);
    }

    #[test]
    fn a_chip_that_does_not_fit_is_cut_and_those_after_are_not_shown() {
        let height = Chips::height(&STYLE);
        let gap = STYLE.gap as u16;
        let wide = height * 2;
        let area = Rect::new(0, 0, 2 * wide + gap + wide / 2, height);
        let chips = Chips::from_widths(&STYLE, area, [wide, wide, wide, wide]);
        assert_eq!(chips.shown(), 3);
        let cut = chips.cell(2).unwrap();
        assert_eq!((cut.width, cut.right()), (wide / 2 - gap, area.right()));
        assert_eq!(chips.cell(3), None);
        assert_eq!(chips.step(2, &actions::RIGHT), Some(2));
        assert_eq!(chips.step(3, &actions::LEFT), Some(2));
        // Nothing past the area's edge is hit.
        assert_eq!(chips.hit(Point::new(area.right() as i32, 1)), None);
    }

    #[test]
    fn chips_hit_and_step_within_their_row() {
        let height = Chips::height(&STYLE);
        let area = Rect::new(0, 0, 1000, height);
        let chips = Chips::from_widths(&STYLE, area, [height * 2, height * 3, height]);
        let second = chips.cell(1).unwrap();
        assert_eq!(chips.hit(Point::new(i32::from(second.x) + 1, 1)), Some(1));
        // Between chips: none.
        assert_eq!(chips.hit(Point::new(i32::from(second.x) - 1, 1)), None);
        assert_eq!(chips.step(0, &actions::LEFT), Some(0));
        assert_eq!(chips.step(0, &actions::NEXT), Some(1));
        assert_eq!(chips.step(2, &actions::RIGHT), Some(2));
        assert_eq!(chips.step(1, &actions::UP), None);
        assert_eq!(
            Chips::from_widths(&STYLE, area, []).step(0, &actions::LEFT),
            None
        );
    }
}
