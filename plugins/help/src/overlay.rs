//! The hints over a screen, and *Show hints again*.
//!
//! Over the screen (which shows through), each hint with a zone is a
//! frame around that part with its text in a box: in the middle of it, or
//! beside it where the part is narrow. The others are lines in a box with
//! *Got it* and *No more hints* below them, at the bottom, the middle or
//! the top of the screen, wherever it covers the zones least. Drawn once,
//! without pictures.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::help::Hint;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen, ShowsThrough,
};

use crate::SHOW;

/// The two buttons of the box.
const GOT_IT: usize = 0;
const NO_MORE: usize = 1;

pub(crate) struct Overlay {
    style: UiStyle,
    screen: String,
    /// Each zone's frame, its text and the box the text is in.
    zones: Vec<(Rect, String, Rect)>,
    /// The box: the hints without a zone, *Got it*, *No more hints*.
    dialog: ui::Dialog,
    focus: usize,
}

impl Overlay {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let screen = String::from(params.get(params::ID).unwrap_or_default());
        let display = env.device.display;
        let whole = Rect::new(0, 0, display.width, display.height);
        let (mut zones, mut lines) = (Vec::new(), Vec::new());
        for Hint { text, zone, .. } in paperpaw_sdk::help::hints(env, &screen) {
            let text = String::from(env.i18n.tr(&text));
            match zone {
                Some(zone) => zones.push((zone.on(whole), text, Rect::new(0, 0, 0, 0))),
                None => lines.push(text),
            }
        }
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let buttons = [env.i18n.tr("help.got-it"), env.i18n.tr("help.no-more")];
        let dialog = ui::Dialog::new(env, &style, env.i18n.tr("help.title"), &lines, &buttons);
        let mut overlay = Self {
            style,
            screen,
            zones,
            dialog,
            focus: GOT_IT,
        };
        overlay.lay_out(env);
        overlay
    }

    /// The zones' frames and their texts' boxes, then where the box goes.
    fn lay_out(&mut self, env: &Env<'_>) {
        let style = self.style;
        let display = env.device.display;
        let Some(font) = style.font(env) else {
            return;
        };
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let small = font.line_metrics(style.small).line_height as u16;
        for (zone, text, label) in &mut self.zones {
            *zone = Rect::new(
                zone.x + gap,
                zone.y + gap,
                zone.width.saturating_sub(2 * gap),
                zone.height.saturating_sub(2 * gap),
            );
            // A narrow part (an edge) has its text beside it, on the side
            // with more room.
            let narrow = u32::from(zone.width) * 4 < u32::from(display.width);
            let width = if narrow {
                display.width / 2
            } else {
                zone.width.saturating_sub(gap)
            };
            let lines = font.wrap(style.small, text, f32::from(width.saturating_sub(2 * gap)));
            let height = small * lines.len() as u16 + gap * 2;
            let x = if !narrow {
                zone.x + gap / 2
            } else if u32::from(zone.x) + u32::from(zone.width) / 2 < u32::from(display.width) / 2 {
                (zone.x + zone.width + gap).min(display.width.saturating_sub(width + margin))
            } else {
                zone.x.saturating_sub(width + gap).max(margin)
            };
            let y = (zone.y + zone.height.saturating_sub(height) / 2)
                .min(display.height.saturating_sub(height));
            *label = Rect::new(x, y, width, height);
        }
        // Where the box hides least of the zones, their texts above all.
        let height = self.dialog.height().min(display.height);
        let room = display.height.saturating_sub(height);
        let covered = |y: u16| -> u32 {
            let mut panel = self.dialog.panel();
            panel.y = y;
            let area = |r: &Rect| {
                panel
                    .intersection(r)
                    .map_or(0, |o| u32::from(o.width) * u32::from(o.height))
            };
            self.zones
                .iter()
                .map(|(zone, _, label)| area(zone) + 8 * area(label))
                .sum()
        };
        let y = [room.saturating_sub(margin), room / 2, margin.min(room / 2)]
            .into_iter()
            .min_by_key(|y| covered(*y))
            .unwrap_or(room / 2);
        self.dialog.place(y);
    }

    /// Closes, the screen's hints seen; `more` false switches all off.
    fn close(&mut self, more: bool, cx: &mut Cx<'_, '_>) {
        if !more {
            cx.command(Command::set(&SHOW, false));
        }
        cx.command(Command::Back);
    }
}

impl Screen for Overlay {
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            let key = if self.focus == GOT_IT {
                "help.got-it"
            } else {
                "help.no-more"
            };
            paperpaw_sdk::Hint::Text(env.i18n.tr(key).into())
        } else if *action == actions::BACK || *action == actions::LEFT || *action == actions::RIGHT
        {
            paperpaw_sdk::Hint::Default
        } else {
            paperpaw_sdk::Hint::Hidden
        }
    }

    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        let display = env.device.display;
        Some(Rect::new(0, 0, display.width, display.height))
    }

    /// The screen it explains shows below it.
    fn shows_through(&self) -> ShowsThrough {
        ShowsThrough::FullScreen
    }

    /// Seen as it shows: never again, unless asked for.
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        crate::mark_seen(cx.env().storage, &self.screen);
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let line_w = style.line.max(2) * 2;
        // The zones: a frame each, its text in its box.
        let small = font.line_metrics(style.small).line_height as u16;
        for (zone, text, label) in &self.zones {
            canvas.stroke_rect(
                i32::from(zone.x),
                i32::from(zone.y),
                u32::from(zone.width),
                u32::from(zone.height),
                line_w,
                Color::BLACK,
            );
            let label = *label;
            let width = f32::from(label.width.saturating_sub(2 * style.gap as u16));
            let wrapped = font.wrap(style.small, text, width);
            canvas.fill(label, Color::WHITE);
            ui::stroke_round(canvas, &style, label, style.line, Color::BLACK);
            let mut y = label.y + style.gap as u16;
            for range in wrapped {
                let part = &text[range];
                let x = f32::from(label.x)
                    + (f32::from(label.width) - font.measure(style.small, part)).max(0.0) / 2.0;
                let row = Rect::new(label.x, y, label.width, small);
                ui::draw_label(canvas, &font, style.small, x, row, part, Color::BLACK);
                y += small;
            }
        }
        self.dialog.draw(canvas, env, &style, Some(self.focus));
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            let more = self.focus == GOT_IT;
            self.close(more, cx);
        } else if let Some(focus) = self.dialog.step(self.focus, action) {
            self.focus = focus;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        } else {
            // Back, and every other button: closed, the hints seen.
            self.close(true, cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap {
            let more = self.dialog.hit(gesture.start) != Some(NO_MORE);
            self.close(more, cx);
        }
        Handled::Yes
    }
}

/// *Show hints again*: done as it opens, and said; any tap or button
/// closes it.
pub(crate) struct Again {
    style: UiStyle,
}

impl Again {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        crate::again(env.storage);
        Self {
            style: UiStyle::new(env),
        }
    }
}

impl Screen for Again {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let display = env.device.display;
        let area = Rect::new(0, 0, display.width, display.height);
        canvas.fill(area, Color::WHITE);
        ui::draw_message(
            canvas,
            env,
            &self.style,
            area,
            env.i18n.tr("help.again-done"),
        );
    }

    fn on_action(&mut self, _: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        cx.command(Command::Back);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind == GestureKind::Tap {
            cx.command(Command::Back);
        }
        Handled::Yes
    }
}
