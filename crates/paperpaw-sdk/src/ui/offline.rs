//! The offer to go online, on a screen the reader opened for the network
//! (the store, a catalog, a transfer) while the power mode is Offline
//! (see [`power`](crate::power)): once, inline in the screen's own
//! layout, never as a pop-up over the page: "Offline mode. Go online for
//! this?" with *Go online* and *Not now*.
//!
//! *Go online* sends [`Command::GoOnline`]: the
//! network for that screen until it closes; the screen then does again
//! what needed the network. *Not now* leaves the screen as it is without
//! the network (it says so as it would without one). Either way it is
//! not offered again on that screen.
//!
//! ```ignore
//! if self.offer.shows(env) {
//!     self.offer.draw(canvas, env, &style, area);
//! }
//! // In `on_action` and `on_gesture`:
//! if let Some(Offer::Online) = self.offer.on_action(action, cx) {
//!     self.fetch(cx);
//! }
//! ```

use crate::builtin::actions;
use crate::power::PowerLevel;
use crate::{Canvas, Command, Cx, Env, Id, Point, Rect};

use super::{UiStyle, button_height, draw_button, draw_empty};

/// What the reader chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    /// *Go online*: [`Command::GoOnline`] was sent; do again what needed
    /// the network.
    Online,
    /// *Not now*: go on without the network.
    NotNow,
}

/// The offer on one screen: where its buttons are, which has the focus,
/// whether the reader chose already.
#[derive(Debug, Clone, Default)]
pub struct OfflineOffer {
    buttons: [Rect; 2],
    focus: usize,
    chosen: bool,
}

impl OfflineOffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether to show it: the mode is Offline and the reader has not
    /// chosen on this screen yet.
    pub fn shows(&self, env: &Env<'_>) -> bool {
        env.power.level == PowerLevel::Offline && !self.chosen
    }

    /// Draws it in `area`: what it is about, as an empty screen says it,
    /// and the two buttons below, *Go online* first.
    pub fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
        let row = style.touch_row(style.row) as u16;
        let height = button_height(style, Rect::new(0, 0, area.width, row));
        let gap = style.gap as u16 * 2;
        let margin = style.margin as u16 * 2;
        let room = row * 2 + gap;
        let text = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(room + gap),
        );
        draw_empty(
            canvas,
            env,
            style,
            text,
            "wifi-off",
            env.i18n.tr("core.offline.title"),
            env.i18n.tr("core.offline.text"),
        );
        let width = area.width.saturating_sub(2 * margin);
        let top = text.y + text.height + gap;
        for (index, key) in ["core.offline.go-online", "core.offline.not-now"]
            .into_iter()
            .enumerate()
        {
            let y = top + index as u16 * (row + gap / 2) + (row - height) / 2;
            let rect = Rect::new(area.x + margin, y, width, height);
            self.buttons[index] = rect;
            let focused = !env.device.input.touch && self.focus == index;
            draw_button(
                canvas,
                env,
                style,
                rect,
                env.i18n.tr(key),
                index == 0,
                focused,
            );
        }
    }

    /// The buttons with the buttons: up and down (or left and right)
    /// move between them, *Select* chooses; `None` for other actions and
    /// once chosen.
    pub fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Option<Offer> {
        if !self.shows(cx.env()) {
            return None;
        }
        let a = action;
        if [actions::UP, actions::DOWN, actions::LEFT, actions::RIGHT].contains(a)
            || *a == actions::NEXT
            || *a == actions::PREVIOUS
        {
            self.focus = 1 - self.focus;
            cx.request_redraw();
            cx.request_refresh(crate::RefreshMode::Fast);
            return None;
        }
        (*a == actions::CONFIRM).then(|| self.choose(self.focus, cx))
    }

    /// A tap: on a button, what it chooses.
    pub fn on_tap(&mut self, point: Point, cx: &mut Cx<'_, '_>) -> Option<Offer> {
        if !self.shows(cx.env()) {
            return None;
        }
        let index = self.buttons.iter().position(|b| b.contains(point))?;
        Some(self.choose(index, cx))
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> Offer {
        self.chosen = true;
        cx.request_redraw();
        cx.request_refresh(crate::RefreshMode::Partial);
        if index == 0 {
            cx.command(Command::GoOnline);
            Offer::Online
        } else {
            Offer::NotNow
        }
    }
}
