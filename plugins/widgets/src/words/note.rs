//! A note: a text the reader writes ("If found, please return to …", a
//! reminder), small or large, with a heading of its own if wanted. A tap
//! edits it on the keyboard; so does its setting.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use quireink_sdk::ask;
use quireink_sdk::event::Event;
use quireink_sdk::replies::Typed;
use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{
    Canvas, Command, Env, Id, Rect, RegisterError, Registrar, SettingSpec, SettingValue,
};

use super::frame_height;
use super::text::{self, FACES};

pub(super) const ID: Id = Id::new_static("widgets.note");
/// What the note says.
const TEXT: Setting<Text> = Setting::new("widgets.note.text");
/// A small heading above it, if any.
const HEADING: Setting<Text> = Setting::new("widgets.note.heading");
/// Its font: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.note.face");
/// The text typed for it on the keyboard.
const TYPED: Event<Typed> = Event::new("widgets.note.typed");

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&ID, "widgets.note"))?;
    r.add_setting(
        SettingSpec::text(&TEXT, "widgets.note.setting.text", "")
            .group(ID)
            .description("widgets.note.setting.text.help"),
    )?;
    r.add_setting(
        SettingSpec::text(&HEADING, "widgets.note.setting.heading", "")
            .group(ID)
            .description("widgets.note.setting.heading.help"),
    )?;
    r.add_setting(
        SettingSpec::choice(
            &FACE,
            "widgets.setting.face",
            FACES.iter().map(|s| (*s).into()).collect(),
            FACES[0],
        )
        .group(ID),
    )?;
    r.add_setting(widgets::size_setting(&ID, Size::Fit, true))?;
    r.add_setting(widgets::align_setting(&ID, Align::Center))?;
    r.add_setting(widgets::frame_setting(&ID, Frame::None))?;
    r.on(TYPED, |_, typed, out| {
        let text = typed.text.trim();
        out.push(Command::SetSetting(
            TEXT.id().clone(),
            SettingValue::Text(Cow::Owned(String::from(text))),
        ));
    })?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.note", |_| Box::new(Note::default()))
            .category(widgets::categories::PERSONAL),
    )
}

/// The note: its text is a setting, read as it is drawn.
#[derive(Default)]
struct Note {
    /// The size it was last laid out at, in that area.
    fitted: Cell<Option<(Rect, f32)>>,
}

impl Widget for Note {
    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let style = UiStyle::new(env);
        let size = widgets::size(env, ID.as_str()).text(&style);
        let lines = text::font(env, env.settings.read(&FACE)).map_or(1, |font| {
            let width = f32::from(width.saturating_sub(frame_height(env, ID.as_str())));
            text::count_lines(&font, size, env.settings.read(&TEXT), width).clamp(1, 4)
        });
        let heading = heading_height(env);
        widgets::line_height(env, size) * lines as u16 + heading + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let note = env.settings.read(&TEXT).trim();
        if note.is_empty() {
            let key = if env.device.input.touch {
                "widgets.note.none.touch"
            } else {
                "widgets.note.none"
            };
            super::draw_empty(canvas, env, &look, env.i18n.tr(key));
            return;
        }
        let mut area = look.area;
        let style = UiStyle::new(env);
        let Some(font) = text::font(env, env.settings.read(&FACE)) else {
            return;
        };
        // The heading where a line of the note fits below it.
        let heading = env.settings.read(&HEADING).trim();
        let least = text::line_height(&font, style.small) as u16;
        if !heading.is_empty() && area.height >= heading_height(env) + least {
            area = widgets::draw_caption(canvas, env, ID.as_str(), &look, heading);
        }
        let width = f32::from(area.width);
        let size = match self.fitted.get() {
            Some((at, size)) if at == area => size,
            _ => {
                let size = match widgets::size(env, ID.as_str()) {
                    Size::Fit => text::largest(style.small, style.title * 2.4, |size| {
                        let lines = text::count_lines(&font, size, note, width) as f32;
                        lines * text::line_height(&font, size) <= f32::from(area.height)
                    }),
                    size => size.text(&style),
                };
                self.fitted.set(Some((area, size)));
                size
            }
        };
        let line = text::line_height(&font, size);
        let fits = ((f32::from(area.height) / line) as usize).max(1);
        let lines = text::lines(&font, size, note, width, fits);
        let total = (line * lines.len() as f32) as u16;
        let top = area.y + area.height.saturating_sub(total) / 2;
        let align = widgets::align(env, ID.as_str());
        text::draw_lines(canvas, &font, size, &lines, area, top, align, look.ink);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let question = ask::text(env.i18n.tr("widgets.note.setting.text"), &TYPED)
            .initial(env.settings.read(&TEXT));
        alloc::vec![WidgetTarget {
            area,
            command: question.command(),
        }]
    }
}

/// The height of the heading with its gap; 0 without one.
fn heading_height(env: &Env<'_>) -> u16 {
    if env.settings.read(&HEADING).trim().is_empty() {
        0
    } else {
        widgets::caption_height(env)
    }
}
