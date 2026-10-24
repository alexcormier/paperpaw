//! The UI kit: a plugin whose screens draw every building block of the
//! SDK's `ui` module, so `paperpaw-dev ui-kit` shows them all (through
//! the gallery) on every device, in every look, light and dark, with the
//! focus on some and not on others.
//!
//! Each screen is a page of the kit: text and messages, rows, buttons and
//! segments, layout and cards, an address and an offer, a dialog,
//! fields, ticks, chips and long text, date and time, a choice, a yes/no
//! question. The
//! texts are the kit's own (English alone): what each part is called.

use std::borrow::Cow;

use paperpaw_core::KernelBuilder;
use paperpaw_platform::{Color, Rect};
use paperpaw_sdk::builtin::params;
use paperpaw_sdk::ui::slider::SliderFocus;
use paperpaw_sdk::ui::slider::SliderView;
use paperpaw_sdk::ui::{self, TextRole, UiStyle};
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
const PAGES: [(&str, &str); 20] = [
    ("ui-kit.text", "ui-kit.text"),
    ("ui-kit.messages", "ui-kit.messages"),
    ("ui-kit.empties", "ui-kit.empties"),
    ("ui-kit.rows", "ui-kit.rows"),
    ("ui-kit.values", "ui-kit.values"),
    ("ui-kit.noted-rows", "ui-kit.noted-rows"),
    ("ui-kit.numbers", "ui-kit.numbers"),
    ("ui-kit.sliders", "ui-kit.sliders"),
    ("ui-kit.controls", "ui-kit.controls"),
    ("ui-kit.segments", "ui-kit.segments"),
    ("ui-kit.layout", "ui-kit.layout"),
    ("ui-kit.share", "ui-kit.share"),
    ("ui-kit.offer", "ui-kit.offer"),
    ("ui-kit.dialog", "ui-kit.dialog"),
    ("ui-kit.inputs", "ui-kit.inputs"),
    ("ui-kit.ticks", "ui-kit.ticks"),
    ("ui-kit.chips", "ui-kit.chips"),
    ("ui-kit.pickers", "ui-kit.pickers"),
    ("ui-kit.choice", "ui-kit.choice"),
    ("ui-kit.confirm", "ui-kit.confirm"),
];

const TEXTS: &str = "[meta]
language = en
name = English
plural = one-other

[messages]
ui-kit.plugin.name = UI kit
ui-kit.plugin.description = Every building block of the SDK's user interface.
ui-kit.text = Text and messages
ui-kit.rows = Switch rows
ui-kit.values = Value rows
ui-kit.noted-rows = Rows with a note
ui-kit.numbers = Numbers
ui-kit.sliders = Sliders
ui-kit.controls = Buttons
ui-kit.segments = Segments
ui-kit.layout = Layout and cards
ui-kit.share = Address to open on a phone
ui-kit.offer = Offer to go online
ui-kit.dialog = Dialog
ui-kit.messages = Headings and messages
ui-kit.empties = Empty states
ui-kit.inputs = Text fields
ui-kit.ticks = Ticked rows
ui-kit.chips = Chips and long text
ui-kit.pickers = Date and time
ui-kit.choice = Choice
ui-kit.confirm = Delete the book?
ui-kit.option.light = A light
ui-kit.option.calm = Calm
ui-kit.option.dark = Dark
ui-kit.confirm.text = It goes from the library and the card.
ui-kit.confirm.yes = Delete
ui-kit.confirm.no = Keep
ui-kit.role.display = Display
ui-kit.role.headline = Headline
ui-kit.role.head = Head
ui-kit.role.title = Title
ui-kit.role.body = Body text
ui-kit.role.small = Small text
ui-kit.role.caption = Caption
ui-kit.heading = Heading
ui-kit.note = A note line, small and gray.
ui-kit.message = A message, in a sentence.
ui-kit.empty = Empty
ui-kit.empty.text = What would be here, and how it gets filled.
ui-kit.switch.focused = Hyphenate words
ui-kit.switch = Page numbers in the status line
ui-kit.value = Space between paragraphs
ui-kit.value.value = Medium
ui-kit.noted = Sync automatically
ui-kit.noted.value = On
ui-kit.noted.note = Sent when a book opens.
ui-kit.stepper = Size of the interface text
ui-kit.slider = Brightness of the front light
ui-kit.tab.books = Books
ui-kit.tab.shelves = Shelves
ui-kit.tab.notes = Notes
ui-kit.button.plain = Plain
ui-kit.button.on = On
ui-kit.button.focused = Focused
ui-kit.button.faint = Faint
ui-kit.frame.line = Line
ui-kit.frame.card = Card
ui-kit.frame.dark = Dark
ui-kit.address.hint = Scan the code or type the address.
ui-kit.address.last = Last: a book received.
ui-kit.dialog.title = Delete the book?
ui-kit.dialog.text = Its notes and highlights stay in the library.
ui-kit.field.focused = Name of the device
ui-kit.field = Server address
ui-kit.field.placeholder = For example sync.example.org
ui-kit.check.focused = Sync reading positions
ui-kit.check = Count reading time
ui-kit.check.off = Keep the light on while reading
ui-kit.chip.all = All
ui-kit.chip.reading = Reading
ui-kit.chip.unread = Unread
ui-kit.chip.finished = Finished
ui-kit.chip.shelves = Shelves
ui-kit.long-text = A text longer than its room scrolls: up and down a few lines, the page buttons a page, by touch a swipe or a tap in the upper or lower half. A bar on the right says where the lines shown are in the whole. This line and those after it come into view as it scrolls. One more. And the last.
";

const TEXTS_DE: &str = "[meta]
language = de
name = Deutsch
plural = one-other

[messages]
ui-kit.plugin.name = UI-Baukasten
ui-kit.plugin.description = Jeder Baustein der Benutzeroberfläche des SDK.
ui-kit.text = Text und Meldungen
ui-kit.rows = Schalterzeilen
ui-kit.values = Wertzeilen
ui-kit.noted-rows = Zeilen mit Hinweis
ui-kit.numbers = Zahlen
ui-kit.sliders = Schieberegler
ui-kit.controls = Schaltflächen
ui-kit.segments = Segmente
ui-kit.layout = Anordnung und Karten
ui-kit.share = Adresse zum Öffnen am Handy
ui-kit.offer = Angebot, online zu gehen
ui-kit.dialog = Dialog
ui-kit.messages = Überschriften und Meldungen
ui-kit.empties = Leere Zustände
ui-kit.inputs = Textfelder
ui-kit.ticks = Häkchen-Zeilen
ui-kit.chips = Chips und langer Text
ui-kit.pickers = Datum und Uhrzeit
ui-kit.choice = Auswahl
ui-kit.confirm = Das Buch endgültig von der Speicherkarte löschen?
ui-kit.option.light = Hell
ui-kit.option.calm = Ruhig
ui-kit.option.dark = Dunkel
ui-kit.confirm.text = Es verschwindet aus der Bibliothek und von der Speicherkarte.
ui-kit.confirm.yes = Endgültig löschen
ui-kit.confirm.no = Behalten
ui-kit.role.display = Anzeige
ui-kit.role.headline = Schlagzeile
ui-kit.role.head = Kopfzeile
ui-kit.role.title = Titel
ui-kit.role.body = Fließtext
ui-kit.role.small = Kleiner Text
ui-kit.role.caption = Bildunterschrift
ui-kit.heading = Überschrift
ui-kit.note = Eine Hinweiszeile, klein und grau.
ui-kit.message = Eine Meldung, in einem Satz.
ui-kit.empty = Leer
ui-kit.empty.text = Was hier stünde und wie es sich füllt.
ui-kit.switch.focused = Silbentrennung am Zeilenende verwenden
ui-kit.switch = Seitenzahlen in der Statusleiste anzeigen
ui-kit.value = Abstand zwischen den Absätzen
ui-kit.value.value = Mittel
ui-kit.noted = Automatisch mit dem Server abgleichen
ui-kit.noted.value = An
ui-kit.noted.note = Beim Öffnen eines Buches.
ui-kit.stepper = Schriftgröße der Benutzeroberfläche
ui-kit.slider = Helligkeit der Frontbeleuchtung
ui-kit.tab.books = Bücher
ui-kit.tab.shelves = Regale
ui-kit.tab.notes = Notizen
ui-kit.button.plain = Normal
ui-kit.button.on = An
ui-kit.button.focused = Fokus
ui-kit.button.faint = Schwach
ui-kit.frame.line = Linie
ui-kit.frame.card = Karte
ui-kit.frame.dark = Dunkel
ui-kit.address.hint = Scanne den Code oder tippe die Adresse ein.
ui-kit.address.last = Zuletzt: ein Buch empfangen.
ui-kit.dialog.title = Das Buch endgültig von der Speicherkarte löschen?
ui-kit.dialog.text = Seine Notizen und Markierungen bleiben in der Bibliothek erhalten.
ui-kit.field.focused = Name des Geräts im Netzwerk für die Übertragung
ui-kit.field = Adresse des Synchronisationsservers
ui-kit.field.placeholder = Zum Beispiel sync.example.org
ui-kit.check.focused = Lesepositionen mit anderen Geräten abgleichen
ui-kit.check = Lesezeit für die Statistik zählen
ui-kit.check.off = Frontbeleuchtung beim Lesen eingeschaltet lassen
ui-kit.chip.all = Alle
ui-kit.chip.reading = Wird gelesen
ui-kit.chip.unread = Ungelesen
ui-kit.chip.finished = Abgeschlossen
ui-kit.chip.shelves = Regale
ui-kit.long-text = Ein Text, der länger ist als sein Platz, lässt sich verschieben: hoch und runter ein paar Zeilen, die Blättertasten eine Seite, per Touch ein Wischen oder ein Tippen in die obere oder untere Hälfte. Ein Balken rechts zeigt, wo die sichtbaren Zeilen im Ganzen liegen. Diese Zeile und die folgenden kommen beim Verschieben in Sicht. Noch eine. Und die letzte.
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
        r.add_translations(Cow::Borrowed(TEXTS_DE))?;
        for (index, (id, title)) in PAGES.into_iter().enumerate() {
            let factory: paperpaw_sdk::ScreenFactory = match id {
                "ui-kit.choice" => Box::new(|env, _| Box::new(choice(env))),
                "ui-kit.confirm" => Box::new(|env, _| Box::new(confirm(env))),
                _ => Box::new(move |env, _| Box::new(Page::new(env, index))),
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

/// The confirm page: the SDK's yes/no question, the focus on No.
fn confirm(env: &Env<'_>) -> ui::confirm::Confirm {
    let params = Params::new()
        .with(params::TITLE, env.i18n.tr("ui-kit.confirm"))
        .with(params::TEXT, env.i18n.tr("ui-kit.confirm.text"))
        .with(ui::confirm::YES_LABEL, env.i18n.tr("ui-kit.confirm.yes"))
        .with(ui::confirm::NO_LABEL, env.i18n.tr("ui-kit.confirm.no"));
    ui::confirm::Confirm::new(env, &params)
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
        let title = PAGES[self.index].1;
        let mut stack = ui::Stack::new(screen, 0);
        let margin = style.margin();
        let header = stack.take(style.header());
        let page = format!("{} / {}", self.index + 1, PAGES.len());
        ui::draw_header(canvas, env, &style, header, env.i18n.tr(title), Some(&page));
        let rest = stack.rest();
        let body = Rect::new(
            margin,
            rest.y + margin,
            rest.width.saturating_sub(2 * margin),
            rest.height.saturating_sub(2 * margin),
        );
        let whole = Rect::new(0, rest.y, display.width, rest.height);
        match PAGES[self.index].0 {
            "ui-kit.text" => text(canvas, env, &style, body),
            "ui-kit.messages" => messages(canvas, env, &style, body),
            "ui-kit.empties" => empty(canvas, env, &style, body),
            "ui-kit.rows" => rows(canvas, env, &style, whole),
            "ui-kit.values" => values(canvas, env, &style, whole),
            "ui-kit.noted-rows" => noted(canvas, env, &style, whole),
            "ui-kit.numbers" => numbers(canvas, env, &style, whole),
            "ui-kit.sliders" => sliders(canvas, env, &style, whole),
            "ui-kit.controls" => controls(canvas, env, &style, body),
            "ui-kit.segments" => segments(canvas, env, &style, body),
            "ui-kit.layout" => layout(canvas, env, &style, body),
            "ui-kit.share" => share(canvas, env, &style, body),
            "ui-kit.offer" => offer(canvas, env, &style, body),
            "ui-kit.dialog" => dialog(canvas, env, &style),
            "ui-kit.inputs" => inputs(canvas, env, &style, whole),
            "ui-kit.ticks" => ticks(canvas, env, &style, whole),
            "ui-kit.chips" => chips(canvas, env, &style, whole),
            _ => pickers(canvas, env, &style, body),
        }
    }
}

/// Text in each role, a heading, a note line, a message and an empty
/// state.
fn text(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let Some(font) = style.font(env) else {
        return;
    };
    let mut stack = ui::Stack::new(area, style.gap());
    for (role, name) in [
        (TextRole::Display, env.i18n.tr("ui-kit.role.display")),
        (TextRole::Headline, env.i18n.tr("ui-kit.role.headline")),
        (TextRole::Head, env.i18n.tr("ui-kit.role.head")),
        (TextRole::Title, env.i18n.tr("ui-kit.role.title")),
        (TextRole::Body, env.i18n.tr("ui-kit.role.body")),
        (TextRole::Small, env.i18n.tr("ui-kit.role.small")),
        (TextRole::Caption, env.i18n.tr("ui-kit.role.caption")),
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
}

/// A heading, a note line, a message and an empty state.
fn messages(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let mut stack = ui::Stack::new(area, style.gap());
    let heading = stack.take(ui::heading_height(env, style));
    ui::draw_heading(canvas, env, style, heading, env.i18n.tr("ui-kit.heading"));
    // Each as high as all its words need.
    let text = env.i18n.tr("ui-kit.note");
    let note = stack.take(ui::note_line_height(env, style, width, text));
    ui::draw_note_line(canvas, env, style, note, text);
    let text = env.i18n.tr("ui-kit.message");
    let message = stack.take(ui::message_height(env, style, width, text));
    ui::draw_message(canvas, env, style, message, text);
}

/// An empty state across the page.
fn empty(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    ui::draw_empty(
        canvas,
        env,
        style,
        area,
        "books",
        env.i18n.tr("ui-kit.empty"),
        env.i18n.tr("ui-kit.empty.text"),
    );
}

/// Switch rows, on (focused) and off.
fn rows(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, 0);
    // Each row as tall as its title needs: titles are never shortened.
    let title = tr("ui-kit.switch.focused");
    let focused = stack.take(ui::switch_row_height(env, style, width, title, ""));
    ui::draw_focus(canvas, style, focused);
    ui::draw_switch_row(canvas, env, style, focused, true, title, "", true);
    let title = tr("ui-kit.switch");
    let plain = stack.take(ui::switch_row_height(env, style, width, title, ""));
    ui::draw_switch_row(canvas, env, style, plain, false, title, "", false);
    ui::draw_separator(canvas, style, plain, 0);
}

/// A value row.
fn values(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, 0);
    let (title, value) = (tr("ui-kit.value"), tr("ui-kit.value.value"));
    let row = stack.take(ui::value_row_height(env, style, width, title, value));
    ui::draw_value_row(canvas, env, style, row, false, title, value);
    ui::draw_separator(canvas, style, row, 0);
}

/// A row with a note below its title and value.
fn noted(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, 0);
    let (title, value, note) = (
        tr("ui-kit.noted"),
        tr("ui-kit.noted.value"),
        tr("ui-kit.noted.note"),
    );
    let noted = stack.take(ui::noted_row_height(env, style, width, title, value, note));
    ui::draw_noted_row(canvas, env, style, noted, false, title, value, note);
}

/// A stepper and a progress bar.
fn numbers(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let row = style.row();
    let mut stack = ui::Stack::new(area, 0);
    // Its title beside the − and +, or above them where it needs the room.
    let title = env.i18n.tr("ui-kit.stepper");
    let lines = ui::stepper_lines(env, style, area.width, row, title, "100");
    let stepper = stack.take(lines.height);
    ui::draw_stepper_lines(
        canvas,
        env,
        style,
        lines.at(stepper),
        title,
        "12",
        "100",
        false,
    );
    let margin = style.margin();
    let progress = stack.take(row);
    let bar = Rect::new(
        margin,
        progress.y + progress.height / 3,
        progress.width.saturating_sub(2 * margin),
        progress.height / 3,
    );
    ui::draw_progress(canvas, style, bar, 620, Color::BLACK);
}

/// A slider, focused.
fn sliders(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let title = env.i18n.tr("ui-kit.slider");
    let height = ui::slider::row_height_for(env, style, area.width, title, "100 %", false);
    let slider = ui::Stack::new(area, 0).take(height);
    let view = SliderView::new(slider, 40, "40 %")
        .title(title)
        .room("100 %")
        .focus(SliderFocus::Focused);
    ui::draw_focus(canvas, style, slider);
    ui::slider::Slider::new(0, 100).draw_row(canvas, env, style, &view);
}

/// Buttons in each state, a faint one and icon buttons.
fn controls(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let (gap, width) = (style.gap(), area.width);
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, style.margin());
    // Side by side where every word fits, else in more rows: never cut.
    let labels = [
        tr("ui-kit.button.plain"),
        tr("ui-kit.button.on"),
        tr("ui-kit.button.focused"),
    ];
    let row = stack.take(ui::button_cells_height(env, style, width, &labels, gap));
    for (i, (cell, label)) in ui::button_cells(env, style, row, &labels, gap)
        .into_iter()
        .zip(labels)
        .enumerate()
    {
        ui::draw_button(canvas, env, style, cell, label, i == 1, i == 2);
    }
    // A faint button, then two icon buttons (no words of their own).
    let faint = [tr("ui-kit.button.faint")];
    let row = stack.take(ui::button_cells_height(env, style, width, &faint, gap));
    ui::draw_faint_button(canvas, env, style, row, faint[0], false);
    let side = ui::Segments::height(style);
    let row = stack.take(side);
    for (i, (name, on)) in [("books", false), ("folder", true)].into_iter().enumerate() {
        let button = Rect::new(row.x + i as u16 * (side + gap), row.y, side, side);
        let ink = ui::draw_icon_button(canvas, style, button, name, on, on);
        let centre = (
            i32::from(button.x + side / 2),
            i32::from(button.y + side / 2),
        );
        ui::draw_icon_at(canvas, env, name, style.text(), centre, ink);
    }
}

/// Segments as tabs and as a choice of sizes, one focused.
fn segments(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let mut stack = ui::Stack::new(area, style.margin());
    // Side by side where every word fits, else in more rows: never cut.
    let tabs = ["ui-kit.tab.books", "ui-kit.tab.shelves", "ui-kit.tab.notes"];
    let row = stack.take(ui::Segments::height_for(env, style, width, &tabs));
    ui::Segments::fitted(env, style, row, tabs).draw(canvas, env, style, 0, None);
    let sizes = ["S", "M", "L", "XL"];
    let row = stack.take(ui::Segments::height_for(env, style, width, &sizes));
    ui::Segments::fitted(env, style, row, sizes).draw(canvas, env, style, 1, Some(3));
}

/// Cards in each frame, and a grid with the focus on one cell.
fn layout(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let Some(font) = style.font(env) else {
        return;
    };
    let gap = style.gap();
    let mut stack = ui::Stack::new(area, style.margin());
    let cards = stack.take(area.height / 3);
    let frames = [
        (Frame::Line, env.i18n.tr("ui-kit.frame.line")),
        (Frame::Card, env.i18n.tr("ui-kit.frame.card")),
        (Frame::Dark, env.i18n.tr("ui-kit.frame.dark")),
    ];
    for (cell, (frame, name)) in ui::columns(cards, 3, gap).into_iter().zip(frames) {
        let inside = ui::draw_card(canvas, style, cell, frame);
        let ink = if frame == Frame::Dark {
            Color::WHITE
        } else {
            Color::BLACK
        };
        // The frame's name at the top of its card, a gap in from its edges.
        let _ = inside;
        let gap = style.gap();
        let line = font.line_metrics(style.small()).line_height as u16;
        let row = Rect::new(
            cell.x + gap,
            cell.y + gap,
            cell.width.saturating_sub(2 * gap),
            line,
        );
        let x = f32::from(row.x)
            + (f32::from(row.width) - font.measure(style.small(), name)).max(0.0) / 2.0;
        ui::draw_label(canvas, &font, style.small(), x, row, name, ink);
    }
    let rest = stack.rest();
    let grid = ui::CellGrid::new(rest, 3, (rest.height / 3).saturating_sub(gap), gap);
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
        let size = style.title();
        let x = f32::from(cell.x) + (f32::from(cell.width) - font.measure(size, &label)) / 2.0;
        ui::draw_label(canvas, &font, size, x, cell, &label, ink);
    }
}

/// An address with its code, and the offer to go online.
fn share(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let url = "http://192.168.4.1:8080/?key=3f9a";
    let qr = paperpaw_sdk::qr::QrCode::new(url.as_bytes());
    ui::draw_address(
        canvas,
        env,
        style,
        area,
        url,
        qr.as_ref(),
        env.i18n.tr("ui-kit.address.hint"),
        Some(env.i18n.tr("ui-kit.address.last")),
    );
}

/// The offer to go online, as a screen for the network shows it.
fn offer(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    ui::offline::OfflineOffer::new().draw(canvas, env, style, area);
}

/// A dialog in the middle of the screen, its first button focused.
fn dialog(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle) {
    let mut dialog = ui::Dialog::new(
        env,
        style,
        env.i18n.tr("ui-kit.dialog.title"),
        &[
            env.i18n.tr("ui-kit.confirm.text"),
            env.i18n.tr("ui-kit.dialog.text"),
        ],
        &[
            env.i18n.tr("ui-kit.confirm.yes"),
            env.i18n.tr("ui-kit.confirm.no"),
        ],
    );
    let room = env.device.display.height.saturating_sub(dialog.height());
    dialog.place(room / 2);
    dialog.draw(canvas, env, style, Some(0));
}

/// Text fields and ticked rows.
fn inputs(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, 0);
    // Each row as tall as its title needs: titles are never shortened.
    let title = tr("ui-kit.field.focused");
    let focused = stack.take(ui::text_field_height(env, style, width, title));
    ui::draw_focus(canvas, style, focused);
    ui::draw_text_field(canvas, env, style, focused, true, title, "Ada", "");
    let title = tr("ui-kit.field");
    let empty = stack.take(ui::text_field_height(env, style, width, title));
    let placeholder = tr("ui-kit.field.placeholder");
    ui::draw_text_field(canvas, env, style, empty, false, title, "", placeholder);
}

/// Rows where several may be ticked, the first focused.
fn ticks(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let width = area.width;
    let tr = |key: &'static str| env.i18n.tr(key);
    let mut stack = ui::Stack::new(area, 0);
    for (on, focus, key) in [
        (true, true, "ui-kit.check.focused"),
        (false, false, "ui-kit.check.off"),
    ] {
        let title = tr(key);
        let at = stack.take(ui::check_row_height(env, style, width, title, ""));
        if focus {
            ui::draw_focus(canvas, style, at);
        }
        ui::draw_check_row(canvas, env, style, at, focus, title, "", on);
    }
}

/// Chips, wrapping where their words need, and text longer than its room.
fn chips(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let margin = style.margin();
    let width = area.width;
    let mut stack = ui::Stack::new(area, 0);
    let labels = [
        "ui-kit.chip.all",
        "ui-kit.chip.reading",
        "ui-kit.chip.unread",
        "ui-kit.chip.finished",
        "ui-kit.chip.shelves",
    ];
    let inner = width.saturating_sub(2 * margin);
    let chips_height = ui::Chips::height_for(env, style, inner, &labels);
    let chips_row = stack.take(chips_height + 2 * margin);
    let inside = Rect::new(margin, chips_row.y + margin, inner, chips_height);
    ui::Chips::new(env, style, inside, labels).draw(canvas, env, style, 1, Some(2));
    let rest = stack.rest();
    let text_area = Rect::new(
        margin,
        rest.y + margin,
        rest.width.saturating_sub(2 * margin),
        rest.height.saturating_sub(2 * margin),
    );
    let text = env.i18n.tr("ui-kit.long-text");
    let mut view = ui::TextView::new(env, style, text_area, text);
    view.scroll(1);
    view.draw(canvas, env, style, text);
}

/// A date and a time, the day and the minutes focused.
fn pickers(canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect) {
    let mut stack = ui::Stack::new(area, style.margin() * 2);
    let date = stack.take(ui::DatePicker::height(style));
    let mut picker = ui::DatePicker::new(style, date, paperpaw_sdk::time::Date::new(2026, 10, 22));
    picker.set_focus(0);
    picker.draw(canvas, env, style);
    let time = stack.take(ui::TimePicker::height(style));
    let mut picker = ui::TimePicker::new(style, time, paperpaw_sdk::time::Time::new(22, 30));
    picker.set_focus(1);
    picker.draw(canvas, env, style);
}
