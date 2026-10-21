//! *Page with a picture*: the page read stays, a picture over it, its
//! clear parts showing the page (a PNG's transparency) and all of it as
//! see-through as the reader likes.
//!
//! The picture comes from the pictures folder (one after the other, as
//! for *My pictures*) or from a file a preset names: any plugin, also a
//! data plugin's `presets.ini`, offers pictures as presets of kind
//! [`KIND`] (`sleep.overlay-picture = plugins/owls/owl.png`, with its
//! place and size if it likes). Settings: where it goes, how large it
//! is, how much the page shows through, and when it is used (always,
//! while reading, or elsewhere; else the book's cover shows).

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
    SettingValue,
};

/// The kind of the presets that offer pictures.
pub const KIND: &str = "sleep.overlay";
/// The picture: a file, or empty for the pictures folder.
pub const PICTURE: Setting<Text> = Setting::new("sleep.overlay-picture");
/// Where it goes: one of [`POSITIONS`].
pub const POSITION: Setting<Text> = Setting::new("sleep.overlay-position");
pub const POSITIONS: [&str; 7] = [
    "sleep.position.center",
    "sleep.position.top",
    "sleep.position.bottom",
    "sleep.position.top-left",
    "sleep.position.top-right",
    "sleep.position.bottom-left",
    "sleep.position.bottom-right",
];
/// How large: percent of the screen's shorter side.
pub const SIZE: Setting<i64> = Setting::new("sleep.overlay-size");
/// How much the page shows through the picture, in percent.
pub const SEE_THROUGH: Setting<i64> = Setting::new("sleep.overlay-see-through");
/// When it is used: one of [`WHEN`].
pub const USED: Setting<Text> = Setting::new("sleep.overlay-when");
pub const WHEN: [&str; 3] = [
    "sleep.when.always",
    "sleep.when.reading",
    "sleep.when.elsewhere",
];
/// The screen choosing the picture: the folder's, or a preset's.
pub const PICK: Id = Id::new_static("sleep.pick-overlay");

/// Whether the picture goes over the page now: as [`USED`] says, by
/// whether a book is read below.
pub(crate) fn used(env: &Env<'_>) -> bool {
    let reading = env
        .stack
        .last()
        .and_then(|entry| entry.slot.as_ref())
        .is_some_and(|slot| *slot == slots::READER);
    match env.settings.text(USED.as_str()) {
        w if w == WHEN[1] => reading,
        w if w == WHEN[2] => !reading,
        _ => true,
    }
}

/// The picture, decoded with its transparency, as large as it may show.
pub(crate) fn picture(env: &Env<'_>, data: &str) -> Option<ImageRef> {
    let chosen = env.settings.text(PICTURE.as_str()).trim().to_string();
    let path = if chosen.is_empty() {
        crate::next_from_folder(env)?
    } else {
        chosen
    };
    let side = u32::from(box_side(env));
    paperpaw_sdk::image::picture_with_alpha(
        env.storage,
        &path,
        data,
        paperpaw_sdk::image::Fit::side(side),
    )
}

/// The side of the square the picture fits in.
fn box_side(env: &Env<'_>) -> u16 {
    let display = env.device.display;
    let percent = env.settings.read(&SIZE).clamp(10, 100) as u32;
    (u32::from(display.width.min(display.height)) * percent / 100) as u16
}

/// Where `image` goes on the screen.
pub(crate) fn place(env: &Env<'_>, image: &ImageRef) -> Rect {
    let display = env.device.display;
    let side = box_side(env);
    let (w, h) = ui::fit_image(image, side, side);
    let margin = UiStyle::new(env).margin as u16;
    let (left, right) = (margin, display.width.saturating_sub(w + margin));
    let (top, bottom) = (margin, display.height.saturating_sub(h + margin));
    let (mid_x, mid_y) = (
        display.width.saturating_sub(w) / 2,
        display.height.saturating_sub(h) / 2,
    );
    let position = env.settings.text(POSITION.as_str());
    let (x, y) = match POSITIONS.iter().position(|p| *p == position) {
        Some(1) => (mid_x, top),
        Some(2) => (mid_x, bottom),
        Some(3) => (left, top),
        Some(4) => (right, top),
        Some(5) => (left, bottom),
        Some(6) => (right, bottom),
        _ => (mid_x, mid_y),
    };
    Rect::new(x, y, w, h)
}

/// How opaque the picture is drawn (0 to 255).
pub(crate) fn opacity(env: &Env<'_>) -> u8 {
    let through = env.settings.read(&SEE_THROUGH).clamp(0, 100) as u32;
    (255 * (100 - through) / 100) as u8
}

/// Chooses where the picture comes from: the pictures folder, or one of
/// the presets of kind [`KIND`] (applied as presets are).
pub(crate) struct Picker {
    /// The presets' ids and titles; the folder first, without an id.
    rows: Vec<(Option<Id>, String)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl Picker {
    pub(crate) fn new(env: &Env<'_>, _: &Params) -> Self {
        let mut rows = Vec::from([(None, env.i18n.tr("sleep.overlay.folder").to_string())]);
        rows.extend(
            env.settings
                .presets()
                .of_kind(KIND)
                .map(|p| (Some(p.id.clone()), env.i18n.tr(&p.title).to_string())),
        );
        let style = UiStyle::new(env);
        let display = env.device.display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height - top);
        Self {
            rows,
            list: ListView::new(area, style.row).focus_on_keys(env),
            style,
            header,
        }
    }

    /// Whether row `row` is the picture chosen now.
    fn chosen(&self, env: &Env<'_>, row: usize) -> bool {
        match &self.rows[row].0 {
            None => env.settings.text(PICTURE.as_str()).trim().is_empty(),
            Some(id) => env
                .settings
                .presets()
                .get(id.as_str())
                .is_some_and(|preset| {
                    let changes = preset.changes(env.settings, &[]);
                    paperpaw_sdk::presets::applied(env.settings, &changes)
                }),
        }
    }

    fn pick(&self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.rows.get(row) {
            Some((None, _)) => cx.command(Command::SetSetting(
                PICTURE.id().clone(),
                SettingValue::Text(Cow::Borrowed("")),
            )),
            Some((Some(id), _)) => {
                if let Some(preset) = env.settings.presets().get(id.as_str()) {
                    let changes = preset.changes(env.settings, &[]);
                    for command in paperpaw_sdk::presets::commands(env.settings, &changes) {
                        cx.command(command);
                    }
                }
            }
            None => return,
        }
        cx.command(Command::Back);
    }
}

impl Screen for Picker {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.rows.len();
        let title = env.i18n.tr("sleep.setting.overlay-picture");
        self.header
            .draw(canvas, env, &style, title, None, self.list.paging(len));
        let on = env.i18n.tr("core.state.on");
        let chosen: Vec<bool> = (0..len).map(|row| self.chosen(env, row)).collect();
        let rows = &self.rows;
        self.list
            .draw(canvas, &style, len, |canvas, row, rect, focused| {
                let value = if chosen[row] { on } else { "" };
                ui::draw_value_row(canvas, env, &style, rect, focused, &rows[row].1, value);
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            match self.list.confirm() {
                Some(row) => self.pick(row, cx),
                None => self.list.request(ListChange::Focus, cx),
            }
            return Handled::Yes;
        }
        match self.list.navigate(action, self.rows.len()) {
            Some(change) => {
                change.request(cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.rows.len());
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.rows.len()) {
            Some(row) => {
                self.list.set_focus(row);
                self.pick(row, cx);
                cx.request_refresh(RefreshMode::Fast);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

impl core::fmt::Debug for Picker {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Picker")
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}
