//! The UI kit: a plugin whose screens draw every building block of the
//! SDK's `ui` module, so `paperpaw-dev ui-kit` shows them all (through
//! the gallery) on every device, in every look, light and dark, with the
//! focus on some and not on others.
//!
//! Each screen is a page of the kit: text and messages, rows, buttons and
//! segments, layout and cards, an address and an offer, a dialog, a
//! choice. The
//! texts are the kit's own (English alone): what each part is called.

use std::borrow::Cow;

use paperpaw_core::KernelBuilder;
use paperpaw_platform::{Color, Rect};
use paperpaw_sdk::builtin::params;
use paperpaw_sdk::ui::{self, SliderFocus, SliderView, TextRole, UiStyle};
use paperpaw_sdk::widgets::Frame;
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, Params, Plugin, RegisterError, Registrar, Screen,
    ScreenSpec, Version,
};

/// The plugin's id.
pub const PLUGIN: &str = "ui-kit";

/// Adds the kit to the app (an [`AddPlugins`](crate::AddPlugins)).
pub fn add(app: KernelBuilder) -> KernelBuilder {
    app.plugin(UiKit)
}

/// The pages, by screen id and title key.
const PAGES: [(&str, &str); 7] = [
    ("ui-kit.text", "ui-kit.text"),
    ("ui-kit.rows", "ui-kit.rows"),
    ("ui-kit.controls", "ui-kit.controls"),
    ("ui-kit.layout", "ui-kit.layout"),
    ("ui-kit.share", "ui-kit.share"),
    ("ui-kit.dialog", "ui-kit.dialog"),
    ("ui-kit.choice", "ui-kit.choice"),
];

const TEXTS: &str = "[meta]
language = en
name = English
plural = one-other

[messages]
ui-kit.plugin.name = UI kit
ui-kit.plugin.description = Every building block of the SDK's user interface.
ui-kit.text = Text and messages
ui-kit.rows = Rows
ui-kit.controls = Buttons and segments
ui-kit.layout = Layout and cards
ui-kit.share = Address and offers
ui-kit.dialog = Dialog
ui-kit.choice = Choice
ui-kit.option.light = A light
ui-kit.option.calm = Calm
ui-kit.option.dark = Dark
";

struct UiKit;

impl Plugin for UiKit {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(PLUGIN), "UI kit", Version::new(0, 1, 0))
            .description("Every building block of the SDK's user interface.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_translations(Cow::Borrowed(TEXTS))?;
        for (index, (id, title)) in PAGES.into_iter().enumerate() {
            let factory: paperpaw_sdk::ScreenFactory = if index == PAGES.len() - 1 {
                Box::new(|env, _| Box::new(choice(env)))
            } else {
                Box::new(move |env, _| Box::new(Page::new(env, index)))
            };
            r.add_screen(ScreenSpec::new(Id::new_static(id), title, factory))?;
        }
        Ok(())
    }
}

/// The choice page: the SDK's choice overlay, three options.
fn choice(env: &Env<'_>) -> ui::choice::Choice {
    let params = Params::new()
        .with(params::TITLE, env.i18n.tr("ui-kit.choice"))
        .with("labels", "ui-kit.option.")
        .with("options", "light,calm,dark");
    ui::choice::Choice::new(env, &params)
}

/// A page of the kit.
struct Page {
    style: UiStyle,
    index: usize,
}

impl Page {
    fn new(env: &Env<'_>, index: usize) -> Self {
        Self {
            style: UiStyle::new(env),
            index,
        }
    }
}

impl Screen for Page {
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    /// Only *Back* does anything: the kernel's.
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let display = env.device.display;
        let screen = Rect::new(0, 0, display.width, display.height);
        canvas.fill(screen, Color::WHITE);
        let (_, title) = PAGES[self.index];
        let mut stack = ui::Stack::new(screen, 0);
        let header = stack.take(style.header as u16);
        let page = format!("{} / {}", self.index + 1, PAGES.len());
        ui::draw_header(canvas, env, &style, header, env.i18n.tr(title), Some(&page));
        let margin = style.margin as u16;
        let rest = stack.rest();
        let body = Rect::new(
            margin,
            rest.y + margin,
            rest.width.saturating_sub(2 * margin),
            rest.height.saturating_sub(2 * margin),
        );
        match self.index {
            0 => text(canvas, env, &style, body),
            1 => rows(
                canvas,
                env,
                &style,
                Rect::new(0, rest.y, display.width, rest.height),
            ),
            2 => controls(canvas, env, &style, body),
            3 => layout(canvas, env, &style, body),
            4 => share(canvas, env, &style, body),
            _ => dialog(canvas, env, &style),
        }
    }
}

/// Text in each role, a heading, a note line, a message and an empty
/// state.
fn text(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let Some(font) = style.font(env) else {
        return;
    };
    let mut stack = ui::Stack::new(area, style.gap as u16);
    for (role, name) in [
        (TextRole::Display, "Display"),
        (TextRole::Headline, "Headline"),
        (TextRole::Head, "Head"),
        (TextRole::Title, "Title"),
        (TextRole::Body, "Body text"),
        (TextRole::Small, "Small text"),
        (TextRole::Caption, "Caption"),
    ] {
        let size = style.size(role);
        let height = font.line_metrics(size).line_height as u16;
        let row = stack.take(height);
        ui::draw_label(
            canvas,
            &font,
            size,
            f32::from(row.x),
            row,
            name,
            Color::BLACK,
        );
    }
    let heading = stack.take(ui::heading_height(env, style));
    ui::draw_heading(canvas, env, style, heading, "Heading");
    let note = stack.take(font.line_metrics(style.small).line_height as u16);
    ui::draw_note_line(canvas, env, style, note, "A note line, small and gray.");
    let message = stack.take(font.line_metrics(style.text).line_height as u16 * 2);
    let empty = stack.rest();
    ui::draw_message(
        canvas,
        env,
        style,
        message,
        "A message: what happened, in a sentence.",
    );
    ui::draw_empty(
        canvas,
        env,
        style,
        empty,
        "books",
        "Empty",
        "What would be here, and how it gets filled.",
    );
}

/// Setting rows: switches on and off, a value, a note, a stepper, a
/// slider, a progress bar; the second of each kind focused.
fn rows(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let row = style.row as u16;
    let mut stack = ui::Stack::new(area, 0);
    let focused = stack.take(row);
    ui::draw_focus(canvas, style, focused);
    ui::draw_switch_row(
        canvas,
        env,
        style,
        focused,
        true,
        "A switch, focused",
        "",
        true,
    );
    let plain = stack.take(row);
    ui::draw_switch_row(canvas, env, style, plain, false, "A switch", "", false);
    ui::draw_separator(canvas, style, plain, 0);
    let value = stack.take(row);
    ui::draw_value_row(canvas, env, style, value, false, "A value", "Medium");
    ui::draw_separator(canvas, style, value, 0);
    let noted = stack.take(row + row / 2);
    ui::draw_noted_row(
        canvas,
        env,
        style,
        noted,
        false,
        "A noted row",
        "On",
        "What it does, below it.",
    );
    ui::draw_separator(canvas, style, noted, 0);
    let stepper = stack.take(row);
    ui::draw_stepper(canvas, env, style, stepper, "A stepper", "12", "100", false);
    let slider = stack.take(ui::slider::row_height(style, false));
    let view = SliderView::new(slider, 40, "40 %")
        .title("A slider, focused")
        .focus(SliderFocus::Focused);
    ui::draw_focus(canvas, style, slider);
    ui::Slider::new(0, 100).draw_row(canvas, env, style, &view);
    let margin = style.margin as u16;
    let progress = stack.take(row);
    let bar = Rect::new(
        margin,
        progress.y + progress.height / 3,
        progress.width.saturating_sub(2 * margin),
        progress.height / 3,
    );
    ui::draw_progress(canvas, style, bar, 620, Color::BLACK);
}

/// Buttons in each state, an icon button, segments as tabs and as a
/// choice.
fn controls(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let gap = style.gap as u16;
    let height = ui::Segments::height(style);
    let mut stack = ui::Stack::new(area, style.margin as u16);
    let tabs = stack.take(height);
    ui::Segments::new(style, tabs, 3).draw(
        canvas,
        env,
        style,
        &["Books", "Shelves", "Notes"],
        0,
        None,
    );
    let buttons = stack.take(height);
    let labels = ["Plain", "On", "Focused"];
    for (i, (cell, label)) in ui::columns(buttons, 3, gap)
        .into_iter()
        .zip(labels)
        .enumerate()
    {
        ui::draw_button(canvas, env, style, cell, label, i == 1, i == 2);
    }
    let more = stack.take(height);
    if let [faint, icon, icon_on] = ui::columns(more, 3, gap)[..] {
        ui::draw_faint_button(canvas, env, style, faint, "Faint", false);
        let side = icon.height;
        for (cell, name, on) in [(icon, "books", false), (icon_on, "folder", true)] {
            let button = Rect::new(cell.x, cell.y, side, side);
            let ink = ui::draw_icon_button(canvas, style, button, name, on, on);
            let centre = (
                i32::from(button.x + side / 2),
                i32::from(button.y + side / 2),
            );
            ui::draw_icon_at(canvas, env, name, style.text, centre, ink);
        }
    }
    let segments = stack.take(height);
    ui::Segments::new(style, segments, 4).draw(
        canvas,
        env,
        style,
        &["S", "M", "L", "XL"],
        1,
        Some(3),
    );
}

/// Cards in each frame, and a grid with the focus on one cell.
fn layout(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let Some(font) = style.font(env) else {
        return;
    };
    let gap = style.gap as u16;
    let mut stack = ui::Stack::new(area, style.margin as u16);
    let cards = stack.take(area.height / 3);
    let frames = [
        (Frame::Line, "Line"),
        (Frame::Card, "Card"),
        (Frame::Dark, "Dark"),
    ];
    for (cell, (frame, name)) in ui::columns(cards, 3, gap).into_iter().zip(frames) {
        let inside = ui::draw_card(canvas, style, cell, frame);
        let ink = if frame == Frame::Dark {
            Color::WHITE
        } else {
            Color::BLACK
        };
        let line = font.line_metrics(style.text).line_height as u16;
        let row = Rect::new(inside.x, inside.y, inside.width, line);
        ui::draw_label(canvas, &font, style.text, f32::from(row.x), row, name, ink);
    }
    let rest = stack.rest();
    let grid = ui::Grid::new(rest, 3, (rest.height / 3).saturating_sub(gap), gap);
    let focus = 4;
    for index in 0..grid.per_page() {
        let cell = grid.cell(index);
        if index == focus {
            ui::draw_focus(canvas, style, cell);
        } else {
            ui::draw_card(canvas, style, cell, Frame::Line);
        }
        let ink = if index == focus {
            Color::WHITE
        } else {
            Color::BLACK
        };
        let label = format!("{}", index + 1);
        let size = style.title;
        let x = f32::from(cell.x) + (f32::from(cell.width) - font.measure(size, &label)) / 2.0;
        ui::draw_label(canvas, &font, size, x, cell, &label, ink);
    }
}

/// An address with its code, and the offer to go online.
fn share(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let [top, bottom] = halves(area, style.margin as u16);
    let url = "http://192.168.4.1:8080/?key=3f9a";
    let qr = paperpaw_sdk::qr::QrCode::new(url.as_bytes());
    ui::draw_address(
        canvas,
        env,
        style,
        top,
        url,
        qr.as_ref(),
        "Scan the code or type the address.",
        Some("Last: a book received."),
    );
    ui::OfflineOffer::new().draw(canvas, env, style, bottom);
}

/// A dialog in the middle of the screen, its first button focused.
fn dialog(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle) {
    let mut dialog = ui::Dialog::new(
        env,
        style,
        "Delete the book?",
        &["It goes from the library and the card.", "Its notes stay."],
        &["Delete", "Keep"],
    );
    let room = env.device.display.height.saturating_sub(dialog.height());
    dialog.place(room / 2);
    dialog.draw(canvas, env, style, Some(0));
}

/// `area` in two halves one above the other, `gap` apart.
fn halves(area: Rect, gap: u16) -> [Rect; 2] {
    let mut stack = ui::Stack::new(area, gap);
    let top = stack.take(area.height.saturating_sub(gap) / 2);
    [top, stack.rest()]
}
