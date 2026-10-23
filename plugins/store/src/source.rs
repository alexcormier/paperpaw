//! Where a plugin's source is: its address as a QR code to open on a
//! phone, and written out below it.

use alloc::string::{String, ToString};

use paperpaw_sdk::builtin::params;
use paperpaw_sdk::qr::QrCode;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::SOURCE_SCREEN;

/// The command that shows `url`, the source of plugin `name`.
pub(crate) fn open(name: &str, url: &str) -> Command {
    Command::OpenScreen(
        SOURCE_SCREEN,
        Params::new()
            .with(params::TITLE, name.to_string())
            .with(params::TEXT, url.to_string()),
    )
}

pub(crate) struct SourceScreen {
    style: UiStyle,
    header: ui::Header,
    title: String,
    url: String,
    qr: Option<QrCode>,
}

impl SourceScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let url = params.get(params::TEXT).unwrap_or_default().to_string();
        Self {
            header: ui::Header::new(env, &style, true),
            style,
            title: params.get(params::TITLE).unwrap_or_default().to_string(),
            qr: QrCode::new(url.as_bytes()),
            url,
        }
    }
}

impl Screen for SourceScreen {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("store.source");
        self.header.draw(canvas, env, &style, title, None, None);
        let Some(font) = style.font(env) else {
            return;
        };
        let display = env.device.display;
        let margin = style.margin as u16;
        let width = display.width.saturating_sub(2 * margin);
        let mut y = self.header.area().height + style.gap as u16 * 2;
        // Who it is from, then the code, then the address in full.
        let line = font.line_metrics(style.text).line_height as u16;
        let name = Rect::new(margin, y, width, line);
        ui::draw_label(
            canvas,
            &font,
            style.text,
            f32::from(margin),
            name,
            &self.title,
            Color::BLACK,
        );
        y += line + style.gap as u16;
        if let Some(qr) = &self.qr {
            // Three quarters of the width, at most half the height.
            let side = (width * 3 / 4).min(display.height / 2);
            let area = Rect::new((display.width - side) / 2, y, side, side);
            if let Some(drawn) = ui::draw_qr(canvas, qr, area) {
                y = drawn.y + drawn.height + style.gap as u16;
            }
        }
        let shown = self.url.strip_prefix("https://").unwrap_or(&self.url);
        let small = font.line_metrics(style.small).line_height;
        for text in [shown, env.i18n.tr("store.source.help")] {
            for range in font.wrap(style.small, text, f32::from(width)) {
                let part = text[range].trim_end();
                let x =
                    f32::from(margin) + (f32::from(width) - font.measure(style.small, part)) / 2.0;
                let at = Rect::new(margin, y, width, small as u16);
                let color = if text == shown {
                    Color::BLACK
                } else {
                    style.faint(80)
                };
                ui::draw_label(canvas, &font, style.small, x, at, part, color);
                y += small as u16;
            }
            y += style.gap as u16;
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        Handled::No
    }
}
