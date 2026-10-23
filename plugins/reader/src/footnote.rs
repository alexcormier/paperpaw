//! A footnote over the page (an overlay, in the slot `core.footnote`):
//! the note's text, *Close* and *Go to the note*. Opened when a link that
//! looks like a note mark ("1", "[2]", "*") is followed and the setting
//! `core.footnotes` says so; parameters `text`, `path` (the book) and
//! `url` (the link).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

/// The most of the screen a note covers.
const SHARE: f32 = 0.5;

pub(crate) struct FootnoteScreen {
    style: UiStyle,
    region: Rect,
    lines: Vec<String>,
    /// *Close* and *Go to the note*.
    buttons: [Rect; 2],
    /// The button the buttons point at (none after a touch).
    focus: Option<usize>,
    path: String,
    href: String,
}

/// The keys of its buttons' words: close, then follow the link.
const BUTTONS: [&str; 2] = ["reader.footnote.close", "reader.footnote.go"];

impl FootnoteScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let margin = style.margin();
        let gap = style.gap();
        let text = params.get(params::TEXT).unwrap_or_default();
        let width = f32::from(display.width.saturating_sub(2 * margin));
        let row = style.row();
        let most = (f32::from(display.height) * SHARE) as u16;
        let (lines, line_height) = match style.font(env) {
            Some(font) => {
                let line = font.line_metrics(style.text()).line_height as u16;
                let room = most.saturating_sub(row + 3 * gap) / line.max(1);
                let ranges = font.wrap(style.text(), text, width);
                let keep = ranges.len().min(room.max(1) as usize);
                let mut lines: Vec<String> = ranges[..keep]
                    .iter()
                    .map(|r| text[r.clone()].trim_end().to_string())
                    .collect();
                if ranges.len() > keep
                    && let Some(last) = lines.last_mut()
                {
                    let start = ranges[keep - 1].start;
                    *last = ui::fit(&font, style.text(), text[start..].trim(), width);
                }
                (lines, line)
            }
            None => (Vec::new(), row),
        };
        // The buttons' row grows with them on touch screens.
        let button_row = style.touch_row(u32::from(style.control_row())) as u16;
        // Its buttons keep the side margin from the bottom edge too.
        let room = ui::edge_room(&style, button_row);
        // Side by side where their words fit, else one below the other.
        let labels = BUTTONS.map(|key| env.i18n.tr(key));
        let half = display.width / 2;
        let half_width = half.saturating_sub(margin + gap / 2);
        let inner = display.width.saturating_sub(2 * margin);
        let side_by_side = labels
            .iter()
            .all(|label| ui::button_slot_height(env, &style, half_width, label) <= button_row);
        let slots = if side_by_side {
            [button_row; 2]
        } else {
            labels.map(|label| ui::button_slot_height(env, &style, inner, label))
        };
        let buttons_height = if side_by_side {
            button_row
        } else {
            slots[0] + gap + slots[1]
        };
        let height = gap * 3 - gap / 2 + line_height * lines.len() as u16 + buttons_height + room;
        let region = Rect::new(
            0,
            display.height.saturating_sub(height),
            display.width,
            height,
        );
        let buttons_y = region.y + height - buttons_height - room;
        let buttons = if side_by_side {
            [
                Rect::new(margin, buttons_y, half_width, button_row),
                Rect::new(half + gap / 2, buttons_y, half_width, button_row),
            ]
        } else {
            [
                Rect::new(margin, buttons_y, inner, slots[0]),
                Rect::new(margin, buttons_y + slots[0] + gap, inner, slots[1]),
            ]
        };
        Self {
            focus: (!env.device.input.touch).then_some(0),
            style,
            region,
            lines,
            buttons,
            path: params.get(params::PATH).unwrap_or_default().to_string(),
            href: params.get(params::URL).unwrap_or_default().to_string(),
        }
    }

    fn press(&self, index: usize, cx: &mut Cx<'_, '_>) {
        cx.command(Command::Back);
        if index == 1 {
            cx.command(events::FOLLOW_LINK.emit(events::FollowLink::new(
                self.path.clone(),
                self.href.clone(),
            )));
        }
    }
}

impl Screen for FootnoteScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let r = self.region;
        canvas.fill(r, Color::WHITE);
        canvas.fill(
            Rect::new(r.x, r.y, r.width, (u32::from(style.line()) * 2) as u16),
            Color::BLACK,
        );
        let Some(font) = style.font(env) else {
            return;
        };
        let line_height = font.line_metrics(style.text()).line_height as u16;
        let mut y = r.y + style.gap();
        for line in &self.lines {
            let area = Rect::new(r.x, y, r.width, line_height);
            let x = f32::from(r.x) + style.margin() as f32;
            ui::draw_label(canvas, &font, style.text(), x, area, line, Color::BLACK);
            y += line_height;
        }
        for (i, (slot, label)) in self.buttons.iter().zip(BUTTONS).enumerate() {
            let label = env.i18n.tr(label);
            ui::draw_button(
                canvas,
                env,
                &style,
                ui::button_in_slot(&style, *slot),
                label,
                false,
                self.focus == Some(i),
            );
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if [
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            self.focus = Some(0);
        } else if [
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            self.focus = Some(1);
        } else if *a == actions::CONFIRM {
            self.press(self.focus.unwrap_or(0), cx);
            return Handled::Yes;
        } else if *a == actions::BACK || *a == actions::MENU {
            cx.command(Command::Back);
            return Handled::Yes;
        } else {
            return Handled::No;
        }
        cx.request_redraw();
        cx.request_refresh(paperpaw_sdk::RefreshMode::Fast);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let at = gesture.start;
        if let Some(i) = self.buttons.iter().position(|b| b.contains(at)) {
            self.press(i, cx);
        } else {
            // Anywhere else: done reading the note.
            cx.command(Command::Back);
        }
        Handled::Yes
    }
}

/// Whether a link's text looks like a note mark: short, or a number in
/// brackets, or a symbol such as `*` or `†`.
pub(crate) fn note_like(label: &str) -> bool {
    let t = label.trim();
    !t.is_empty()
        && (t.chars().count() <= 3
            || t.chars().count() <= 6
                && t.chars()
                    .all(|c| c.is_ascii_digit() || "[]()*†‡§¹²³⁴⁵⁶⁷⁸⁹⁰".contains(c)))
}

#[cfg(test)]
mod tests {
    use super::note_like;

    #[test]
    fn note_marks() {
        for mark in ["1", "12", "[3]", "(14)", "*", "†", "[123]"] {
            assert!(note_like(mark), "{mark}");
        }
        for text in ["CHAPTER I.", "Notes", "", "the end"] {
            assert!(!note_like(text), "{text}");
        }
    }
}
