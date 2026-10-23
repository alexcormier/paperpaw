//! The on-screen keyboard.
//!
//! Bound to the `core.keyboard` slot: any plugin that needs text (search,
//! a setting, a note) opens the slot and gets the text back as an event
//! (see [`slots::KEYBOARD`]). It is an overlay at the bottom of the screen
//! with a text field and the keys, used by tapping or with the buttons:
//! left and right move along a row, page forward and back (or up and
//! down) between rows, confirm presses the key, back cancels and a long
//! press on confirm finishes.
//!
//! - Layouts: the setting *Keyboard layouts* chooses one or more of the
//!   built-in ones (see `layouts`); by default the UI language's, which
//!   a translation brings under `keyboard.letters`. With several, a key
//!   with the layout's code (e.g. `DE`) switches between them.
//! - Accents and other variants: a long press on a letter (with buttons:
//!   a long press on confirm) shows its variants, e.g. `é è ê ë` for `e`,
//!   and German quotes on `"`.
//! - When there is text on the clipboard, *Paste* in the text field
//!   inserts it, and so does a long press in the field (with buttons: up
//!   from the first row).
//! - Where there is a network and a screen for it (slot
//!   `core.remote-keyboard`, e.g. the `transfer` plugin's), *Phone* in the
//!   text field lets one type the text on a phone instead: handy for
//!   passwords and keys.
//!
//! - A password (mode `password`) shows as dots and is never sent along
//!   with *Phone*. While one is typed, the keyboard keeps the edges and
//!   every button to itself but *Sleep*, refresh and night mode: nothing
//!   opens over it, e.g. over the screen lock's.
//!
//! Bind another screen to `core.keyboard` for a different keyboard
//! altogether.

#![no_std]

extern crate alloc;

mod choose;
mod layouts;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::{actions, events, groups, params, slots};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest, Params, Plugin,
    Priority, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec, SettingSpec,
    Version,
};

pub const PLUGIN: Id = Id::new_static("keyboard");
pub const SCREEN: Id = Id::new_static("keyboard.screen");
/// The layouts to type with: ids separated by commas, in the order the
/// layout key switches through them.
pub const LAYOUTS_SETTING: Setting<Text> = Setting::new("keyboard.layouts");
const CHOOSE_SCREEN: Id = Id::new_static("keyboard.choose");
/// The clipboard's text, asked for by *Paste*.
const PASTED: Event<Typed> = Event::new("keyboard.pasted");
/// The text typed on another device, asked for by *Phone*.
const REMOTE_TEXT: Event<Typed> = Event::new("keyboard.remote-text");

/// Longest text the keyboard accepts, in characters: room for long web
/// addresses and keys; what goes past it is cut, and the keyboard says so.
const MAX_CHARS: usize = 2000;
/// What moves the focus between the keys.
const MOVES: [Id; 8] = [
    actions::UP,
    actions::DOWN,
    actions::LEFT,
    actions::RIGHT,
    actions::NEXT,
    actions::PREVIOUS,
    actions::PREV_PAGE,
    actions::NEXT_PAGE,
];

/// What passes the keyboard while a password is typed.
const PASSING: [Id; 3] = [
    actions::SLEEP,
    actions::REFRESH_SCREEN,
    actions::TOGGLE_NIGHT_MODE,
];

/// Holding Select (as the key map binds it), or the menu where a key
/// map binds that instead: the variants of a letter, else done.
fn held(action: &Id) -> bool {
    *action == actions::LONG_CONFIRM || *action == actions::MENU
}

/// The rows of the symbol layer (the last one also gets *Delete*).
const SYMBOLS: [&str; 3] = ["1234567890", "@#&_-+()/%", "*\"':;!?"];

/// The focus "row" of the buttons in the text field.
const PASTE_ROW: usize = usize::MAX;

/// A button in the text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldButton {
    /// Type the text on another device.
    Phone,
    /// Insert the clipboard's text.
    Paste,
}

impl FieldButton {
    fn label(self) -> &'static str {
        match self {
            FieldButton::Phone => "keyboard.phone",
            FieldButton::Paste => "keyboard.paste",
        }
    }
}

/// The keyboard plugin.
#[derive(Debug, Default)]
pub struct Keyboard;

/// The layouts to type with, chosen on their screen.
static CHOSEN: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().editor(CHOOSE_SCREEN);
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed(
    LAYOUTS_SETTING.static_str(),
    "keyboard.setting.layouts",
    paperpaw_sdk::SettingKind::List,
    paperpaw_sdk::SettingValue::Text(Cow::Borrowed(layouts::AUTO)),
    groups::SYSTEM,
)
.with_extra(&CHOSEN)
.described("keyboard.setting.layouts.help")];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Keyboard {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Keyboard", Version::new(0, 1, 0))
            .description("Enters text on the screen, by touch or with the buttons.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // The layout in use; the next keyboard opens with it.
        let current = Rc::new(Cell::new(0usize));
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "keyboard.title",
            Box::new(move |env, params| {
                Box::new(KeyboardScreen::new(env, params, current.clone()))
            }),
        ))?;
        r.bind_slot(slots::KEYBOARD, SCREEN, Priority::DEFAULT)?;
        r.add_settings(&SETTINGS)?;
        r.add_screen(ScreenSpec::new(
            CHOOSE_SCREEN,
            "keyboard.setting.layouts",
            Box::new(|env, _| Box::new(choose::ChooseScreen::new(env))),
        ))?;
        // The first-run setup: the layouts to type with.
        r.add_setup_step(
            paperpaw_sdk::setup::SetupStepSpec::new(
                Id::new_static("keyboard.setup"),
                "",
                paperpaw_sdk::setup::SetupPage::Setting(LAYOUTS_SETTING.into()),
            )
            .order(60),
        )?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    Letters,
    Symbols,
    /// Only digits, for numbers (mode `number`).
    Digits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    Shift,
    Delete,
    Space,
    Layer(Layer),
    /// Switches to the next chosen layout.
    NextLayout,
    Done,
}

/// A key and its width in units (a letter is 2).
type Slot = (Key, u8);

/// The least width of a letter's key, in millimetres: on a narrow screen
/// (the X4) its rows are split rather than their keys made narrower.
const LEAST_KEY_MM: u32 = 6;

/// `row` in rows of at most `most` units, as even as can be and in their
/// order ("qwert" above "yuiop"); as it is where it fits.
fn split(row: Vec<Slot>, most: u32) -> Vec<Vec<Slot>> {
    let total: u32 = row.iter().map(|(_, w)| u32::from(*w)).sum();
    if most == 0 || total <= most {
        return alloc::vec![row];
    }
    let parts = total.div_ceil(most);
    let mut out: Vec<Vec<Slot>> = Vec::with_capacity(parts as usize);
    let mut used = 0;
    for slot in row {
        // A new row once this one holds its share.
        let part = (used * parts / total) as usize;
        if out.len() <= part {
            out.push(Vec::new());
        }
        used += u32::from(slot.1);
        if let Some(last) = out.last_mut() {
            last.push(slot);
        }
    }
    out
}

/// The rows of a layer; `letters` are the rows of the layout, `switch`
/// adds the key for the next layout; rows wider than `most` units are
/// split (see [`split`]; 0 for none).
fn rows(letters: &str, layer: Layer, switch: bool, most: u32) -> Vec<Vec<Slot>> {
    let chars = |row: &str| -> Vec<Slot> { row.chars().map(|c| (Key::Char(c), 2)).collect() };
    match layer {
        Layer::Letters | Layer::Symbols => {
            let mut rows: Vec<Vec<Slot>> = if layer == Layer::Letters {
                letters.split_whitespace().map(chars).collect()
            } else {
                SYMBOLS.iter().map(|row| chars(row)).collect()
            };
            if let Some(last) = rows.last_mut() {
                if layer == Layer::Letters {
                    last.insert(0, (Key::Shift, 3));
                }
                last.push((Key::Delete, 3));
            }
            let mut rows: Vec<Vec<Slot>> =
                rows.into_iter().flat_map(|row| split(row, most)).collect();
            let other = if layer == Layer::Letters {
                Layer::Symbols
            } else {
                Layer::Letters
            };
            let mut bottom = alloc::vec![(Key::Layer(other), 3)];
            // The key to the next layout as wide as the one to symbols, so
            // its code fits.
            if switch && layer == Layer::Letters {
                bottom.push((Key::NextLayout, 3));
            }
            let space = if bottom.len() > 1 { 7 } else { 10 };
            bottom.extend([
                (Key::Char(','), 2),
                (Key::Space, space),
                (Key::Char('.'), 2),
                (Key::Done, 3),
            ]);
            rows.push(bottom);
            rows
        }
        Layer::Digits => {
            let mut rows: Vec<Vec<Slot>> = ["123", "456", "789"].iter().map(|r| chars(r)).collect();
            rows.push(alloc::vec![
                (Key::Delete, 2),
                (Key::Char('0'), 2),
                (Key::Done, 2),
            ]);
            rows
        }
    }
}

/// The variants of a letter, shown above it.
struct Popup {
    chars: Vec<char>,
    area: Rect,
    /// The variant chosen with buttons.
    focus: Option<usize>,
}

impl Popup {
    fn key(&self, index: usize) -> Rect {
        let width = self.area.width / self.chars.len().max(1) as u16;
        Rect::new(
            self.area.x + width * index as u16,
            self.area.y,
            width,
            self.area.height,
        )
    }
}

struct KeyboardScreen {
    /// The opening parameters, sent back with the text.
    params: Params,
    title: String,
    text: String,
    layer: Layer,
    /// The chosen layouts (code, letter rows, variants they add) and the
    /// one in use.
    layouts: Vec<layouts::Chosen>,
    current: Rc<Cell<usize>>,
    /// Shown as dots (mode `password`).
    password: bool,
    /// The next letter is a capital.
    shift: bool,
    rows: Vec<Vec<Slot>>,
    /// The key chosen with buttons, as `(row, index)`; none while tapping.
    focus: Option<(usize, usize)>,
    /// The buttons in the text field (*Phone*, *Paste*) where they work,
    /// left to right.
    buttons: Vec<(FieldButton, Rect)>,
    popup: Option<Popup>,
    style: UiStyle,
    region: Rect,
    field: Rect,
    keys: Rect,
    key_height: u16,
    /// The most units a row of keys holds (see [`rows`]).
    most: u32,
}

impl KeyboardScreen {
    fn new(env: &Env<'_>, params: &Params, current: Rc<Cell<usize>>) -> Self {
        let style = UiStyle::new(env);
        let layer = if params.get(params::MODE) == Some("number") {
            Layer::Digits
        } else {
            Layer::Letters
        };
        let layouts = layouts::chosen(env, env.settings.read(&LAYOUTS_SETTING));
        if current.get() >= layouts.len() {
            current.set(0);
        }
        let display = env.device.display;
        // As many units in a row as leave a letter (2 units) its least
        // width.
        let unit = (env.metrics.mm(LEAST_KEY_MM) / 2).max(1);
        let most = (u32::from(display.width) - 2 * style.margin) / unit;
        let letters = &layouts[current.get()].1;
        // As high as the layer with the most rows needs, so that turning
        // to the other keeps the keys where they are.
        let other = match layer {
            Layer::Letters => rows(letters, Layer::Symbols, false, most).len(),
            _ => 0,
        };
        let rows = rows(letters, layer, layouts.len() > 1, most);
        let count = rows.len().max(other).max(4);
        // Keys and the field's buttons grow on touch screens, and their
        // rows with them.
        let key_height = style.touch_row(style.row) as u16;
        let field_height = (style.small * 1.5) as u16 + key_height;
        let gap = style.gap as u16 / 2;
        // The last row's keys keep the side margin from the bottom edge.
        let room = ui::edge_room(&style, key_height);
        // On the screen whatever the rows: keys get smaller first.
        let room_for_keys = display
            .height
            .saturating_sub(field_height.saturating_add(gap * 2 + room));
        let keys_height = key_height.saturating_mul(count as u16).min(room_for_keys);
        let height = (field_height + keys_height + gap * 2 + room).min(display.height);
        let region = Rect::new(0, display.height - height, display.width, height);
        let field = Rect::new(0, region.y + gap, display.width, field_height);
        let keys = Rect::new(
            0,
            (region.y + region.height).saturating_sub(keys_height + room),
            display.width,
            keys_height,
        );
        let mut screen = Self {
            title: params.get(params::TITLE).unwrap_or_default().to_string(),
            text: params.get(params::TEXT).unwrap_or_default().to_string(),
            params: params.clone(),
            password: params.get(params::MODE) == Some("password"),
            layer,
            layouts,
            current,
            shift: false,
            rows,
            focus: (!env.device.input.touch).then_some((0, 0)),
            buttons: Vec::new(),
            popup: None,
            style,
            region,
            field,
            keys,
            // Smaller where the screen holds the rows only so.
            key_height: keys_height / count as u16,
            most,
        };
        screen.buttons = screen.field_buttons(env);
        screen
    }

    /// The box of the text field: on the margins, like its title above
    /// and the keys below.
    fn text_box(&self) -> Rect {
        let f = self.field;
        let title_height = (self.style.small * 1.5) as u16;
        let m = self.style.margin as u16;
        Rect::new(
            f.x + m,
            f.y + title_height,
            f.width.saturating_sub(2 * m),
            f.height - title_height,
        )
    }

    /// Keys are drawn inset by this inside their area (room for the
    /// focus frame), so their areas reach this far over the margins.
    fn key_inset(&self) -> u32 {
        ui::button_border(&self.style) * 3
    }

    /// The buttons at the right end of the text field: *Phone* where text
    /// can come from another device, *Paste* when there is text on the
    /// clipboard.
    fn field_buttons(&self, env: &Env<'_>) -> Vec<(FieldButton, Rect)> {
        let Some(font) = self.style.font(env) else {
            return Vec::new();
        };
        let mut kinds = Vec::new();
        if env.caps.network && env.has_slot(&slots::REMOTE_KEYBOARD) {
            kinds.push(FieldButton::Phone);
        }
        if env.clipboard {
            kinds.push(FieldButton::Paste);
        }
        let b = self.text_box();
        let height = ui::button_height(&self.style, b);
        let pad = (b.height - height) / 2;
        // From the right: each as wide as its label (rounded up) and the
        // button's padding.
        let mut right = b.x + b.width - pad;
        let mut buttons: Vec<(FieldButton, Rect)> = kinds
            .into_iter()
            .rev()
            .map(|kind| {
                let label = env.i18n.tr(kind.label());
                let label_width = (font.measure(self.style.small, label) + 1.0) as u16;
                let width = label_width + self.style.gap as u16 * 3;
                let area = Rect::new(right - width, b.y + pad, width, height);
                right -= width + pad;
                (kind, area)
            })
            .collect();
        buttons.reverse();
        buttons
    }

    /// What a field button does.
    fn field_button(&mut self, kind: FieldButton, cx: &mut Cx<'_, '_>) {
        match kind {
            FieldButton::Paste => self.paste(cx),
            FieldButton::Phone => {
                // The text so far goes along, but never a password.
                let text = if self.password {
                    String::new()
                } else {
                    self.text.clone()
                };
                let question = ask::text(self.title.clone(), &REMOTE_TEXT).initial(text);
                let question = if self.password {
                    question.password()
                } else {
                    question
                };
                cx.command(question.remote_command());
            }
        }
    }

    /// Where key `index` of row `row` is (the field's buttons for
    /// [`PASTE_ROW`]).
    fn key_area(&self, row: usize, index: usize) -> Rect {
        if row == PASTE_ROW {
            return self.buttons.get(index).map_or(self.field, |b| b.1);
        }
        let keys = &self.rows[row];
        let total: u32 = keys.iter().map(|(_, w)| u32::from(*w)).sum::<u32>().max(1);
        // The keys' visible edges on the margins.
        let outside = self.style.margin.saturating_sub(self.key_inset());
        let width = u32::from(self.keys.width) - 2 * outside;
        let x0 = u32::from(self.keys.x) + outside;
        let before: u32 = keys[..index].iter().map(|(_, w)| u32::from(*w)).sum();
        let left = x0 + width * before / total;
        let right = x0 + width * (before + u32::from(keys[index].1)) / total;
        let y = self.keys.y + self.key_height * row as u16;
        Rect::new(left as u16, y, (right - left) as u16, self.key_height)
    }

    fn key_at(&self, point: paperpaw_sdk::Point) -> Option<(usize, usize)> {
        if let Some(index) = self.buttons.iter().position(|b| b.1.contains(point)) {
            return Some((PASTE_ROW, index));
        }
        (0..self.rows.len()).find_map(|row| {
            (0..self.rows[row].len())
                .find(|&index| self.key_area(row, index).contains(point))
                .map(|index| (row, index))
        })
    }

    /// `c` as typed now: a capital after *Shift* (if it has one capital).
    fn cased(&self, c: char) -> char {
        if !self.shift {
            return c;
        }
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(u), None) => u,
            _ => c,
        }
    }

    fn label(&self, env: &Env<'_>, key: Key) -> String {
        match key {
            Key::Char(c) => self.cased(c).to_string(),
            Key::Layer(Layer::Letters) => env.i18n.tr("keyboard.letters-key").to_string(),
            Key::Layer(_) => env.i18n.tr("keyboard.symbols-key").to_string(),
            Key::NextLayout => self.layouts[self.current.get()].0.clone(),
            Key::Done => env.i18n.tr("keyboard.done").to_string(),
            Key::Shift | Key::Delete | Key::Space => String::new(),
        }
    }

    /// Adds text at the end, as far as there is room.
    fn insert(&mut self, text: &str, cx: &mut Cx<'_, '_>) {
        let room = MAX_CHARS.saturating_sub(self.text.chars().count());
        self.text.extend(text.chars().take(room));
        if text.chars().nth(room).is_some() {
            let env = *cx.env();
            let full = env.i18n.format(
                "keyboard.full",
                &[("count", paperpaw_sdk::Arg::Int(MAX_CHARS as i64))],
            );
            cx.command(Command::Notify(full));
        }
        if self.shift {
            self.shift = false;
            self.redraw(cx, self.keys);
        }
        self.changed(cx);
    }

    fn press(&mut self, key: Key, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match key {
            Key::Char(c) => {
                let c = self.cased(c);
                let mut buffer = [0u8; 4];
                return self.insert(c.encode_utf8(&mut buffer), cx);
            }
            Key::Space => return self.insert(" ", cx),
            Key::Delete => {
                if self.text.pop().is_some() {
                    return self.changed(cx);
                }
            }
            Key::Shift => {
                self.shift = !self.shift;
                self.redraw(cx, self.keys);
            }
            Key::Layer(layer) => {
                self.layer = layer;
                self.rebuild(&env, cx);
            }
            Key::NextLayout => {
                self.current
                    .set((self.current.get() + 1) % self.layouts.len());
                self.rebuild(&env, cx);
            }
            Key::Done => {
                self.finish(cx);
                return;
            }
        }
        self.redraw(cx, self.field);
    }

    /// The keys again, after the layer or layout changed.
    fn rebuild(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        let _ = env;
        self.shift = false;
        let letters = self.layouts[self.current.get()].1.clone();
        self.rows = rows(&letters, self.layer, self.layouts.len() > 1, self.most);
        if let Some((row, index)) = self.focus.filter(|(row, _)| *row != PASTE_ROW) {
            let row = row.min(self.rows.len() - 1);
            self.focus = Some((row, index.min(self.rows[row].len() - 1)));
        }
        self.redraw(cx, self.keys);
    }

    /// Asks for the clipboard's text ([`PASTED`] brings it).
    fn paste(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::ReadClipboard(PASTED.into(), Params::new()));
    }

    /// Text typed from outside (keys of a keyboard, a remote control): as
    /// the keys would add it, without what is no text.
    fn typed(&mut self, text: &str, cx: &mut Cx<'_, '_>) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .filter(|c| self.layer != Layer::Digits || c.is_ascii_digit())
            .collect();
        self.insert(&text, cx);
    }

    /// Inserts pasted text: one line, digits only for numbers.
    fn pasted(&mut self, text: &str, cx: &mut Cx<'_, '_>) {
        let line: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .filter(|c| self.layer != Layer::Digits || c.is_ascii_digit())
            .collect();
        self.insert(line.trim(), cx);
    }

    /// Shows the variants of the letter at `(row, index)`, if it has any.
    fn open_variants(&mut self, row: usize, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        let Some(Key::Char(c)) = self.rows.get(row).and_then(|r| r.get(index)).map(|k| k.0) else {
            return false;
        };
        // The layout's own, then the built-in ones of the letter.
        let own = self.layouts[self.current.get()]
            .2
            .iter()
            .find(|(base, _)| c.to_lowercase().eq(core::iter::once(*base)))
            .map_or("", |(_, variants)| variants.as_str());
        let builtin = layouts::variants(c).unwrap_or_default();
        let mut chars: Vec<char> = Vec::new();
        for v in own.chars().chain(builtin.chars()).map(|v| self.cased(v)) {
            if !chars.contains(&v) && chars.len() < 2 * layouts::MAX_VARIANTS {
                chars.push(v);
            }
        }
        if chars.is_empty() {
            return false;
        }
        let key = self.key_area(row, index);
        let margin = self.style.margin as u16 / 2;
        let room = self.region.width.saturating_sub(2 * margin);
        let width = key
            .width
            .max(self.key_height)
            .saturating_mul(chars.len() as u16)
            .min(room);
        let center = key.x + key.width / 2;
        let x = center
            .saturating_sub(width / 2)
            .clamp(margin, self.region.width - margin - width);
        let y = key.y.saturating_sub(self.key_height).max(self.region.y);
        let focus = self.focus.map(|_| 0);
        self.popup = Some(Popup {
            chars,
            area: Rect::new(x, y, width, self.key_height),
            focus,
        });
        self.redraw(cx, self.region);
        true
    }

    fn close_variants(&mut self, cx: &mut Cx<'_, '_>) {
        if self.popup.take().is_some() {
            self.redraw(cx, self.region);
        }
    }

    /// Shows the changed text, and sends it where asked on each change.
    fn changed(&mut self, cx: &mut Cx<'_, '_>) {
        self.redraw(cx, self.field);
        if self.params.get(params::LIVE).is_some() {
            self.answer(self.text.clone(), true, cx);
        }
    }

    /// Closes the keyboard and sends the text.
    fn finish(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::Back);
        let text = core::mem::take(&mut self.text);
        self.answer(text, false, cx);
    }

    fn answer(&self, text: String, typing: bool, cx: &mut Cx<'_, '_>) {
        if let Some(reply) = self
            .params
            .get(params::REPLY)
            .and_then(|r| Id::parse(r).ok())
        {
            let typed = Typed::new(text, self.params.clone());
            let typed = if typing { typed.while_typing() } else { typed };
            cx.command(Command::Emit(reply, typed.to_params()));
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>, area: Rect) {
        cx.request_redraw_area(area);
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Moves the button focus.
    fn move_focus(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> bool {
        // Only a move (or Select, where no key has the focus yet): others,
        // such as Home, are not the keyboard's even before it has a focus.
        if held(action) || !(MOVES.contains(action) || *action == actions::CONFIRM) {
            return false;
        }
        let Some((row, index)) = self.focus else {
            self.focus = Some((0, 0));
            self.redraw(cx, self.key_area(0, 0));
            return true;
        };
        let a = action;
        let last_row = self.rows.len() - 1;
        let up = *a == actions::UP || *a == actions::PREV_PAGE;
        let down = *a == actions::DOWN || *a == actions::NEXT_PAGE;
        let to = if row == PASTE_ROW {
            // The field's buttons sit above the first row.
            let last = self.buttons.len().saturating_sub(1);
            if down || *a == actions::NEXT && index == last {
                self.nearest(row, index, 0)
            } else if up {
                self.nearest(row, index, last_row)
            } else if *a == actions::RIGHT || *a == actions::NEXT {
                (row, ui::step_focus(index, 1, last + 1))
            } else if *a == actions::LEFT || *a == actions::PREVIOUS {
                (row, ui::step_focus(index, -1, last + 1))
            } else {
                return false;
            }
        } else if *a == actions::LEFT || *a == actions::PREVIOUS {
            match index.checked_sub(1) {
                Some(index) => (row, index),
                None if *a == actions::PREVIOUS => {
                    let row = row.checked_sub(1).unwrap_or(last_row);
                    (row, self.rows[row].len() - 1)
                }
                None => (row, self.rows[row].len() - 1),
            }
        } else if *a == actions::RIGHT || *a == actions::NEXT {
            if index + 1 < self.rows[row].len() {
                (row, index + 1)
            } else if *a == actions::NEXT {
                (if row == last_row { 0 } else { row + 1 }, 0)
            } else {
                (row, 0)
            }
        } else if up {
            match row.checked_sub(1) {
                Some(above) => self.nearest(row, index, above),
                None if !self.buttons.is_empty() => (PASTE_ROW, self.buttons.len() - 1),
                None => self.nearest(row, index, last_row),
            }
        } else if down {
            self.nearest(row, index, if row == last_row { 0 } else { row + 1 })
        } else {
            return false;
        };
        self.redraw(cx, self.key_area(row, index));
        self.redraw(cx, self.key_area(to.0, to.1));
        self.focus = Some(to);
        true
    }

    /// The key of row `to` nearest below the centre of key `index` of `row`.
    fn nearest(&self, row: usize, index: usize, to: usize) -> (usize, usize) {
        let from = self.key_area(row, index);
        let center = u32::from(from.x) + u32::from(from.width) / 2;
        let at = (0..self.rows[to].len())
            .find(|&i| self.key_area(to, i).right() > center)
            .unwrap_or(self.rows[to].len() - 1);
        (to, at)
    }

    /// Buttons while the variants are shown.
    fn on_variants_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let Some(popup) = &mut self.popup else {
            return Handled::No;
        };
        let len = popup.chars.len();
        let focus = popup.focus.unwrap_or(0);
        let a = action;
        let to = if [actions::LEFT, actions::PREVIOUS, actions::UP].contains(a) {
            ui::step_focus(focus, -1, len)
        } else if [actions::RIGHT, actions::NEXT, actions::DOWN].contains(a) {
            ui::step_focus(focus, 1, len)
        } else if *a == actions::CONFIRM {
            let c = popup.chars[focus];
            self.close_variants(cx);
            let mut buffer = [0u8; 4];
            self.insert(c.encode_utf8(&mut buffer), cx);
            return Handled::Yes;
        } else if *a == actions::BACK || held(a) {
            self.close_variants(cx);
            return Handled::Yes;
        } else {
            return Handled::Yes;
        };
        popup.focus = Some(to);
        let area = popup.area;
        self.redraw(cx, area);
        Handled::Yes
    }

    fn draw_field(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // Only in the band it lies in: its text is measured to fit.
        if self.field.intersection(&canvas.clip()).is_none() {
            return;
        }
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let margin = style.margin as f32;
        let f = self.field;
        let title_height = (style.small * 1.5) as u16;
        let title_area = Rect::new(f.x, f.y, f.width, title_height);
        let title = ui::fit(
            &font,
            style.small,
            &self.title,
            f32::from(f.width) - 2.0 * margin,
        );
        ui::draw_label(
            canvas,
            &font,
            style.small,
            margin,
            title_area,
            &title,
            style.faint(90),
        );
        let box_area = self.text_box();
        ui::stroke_round(canvas, &style, box_area, style.line.max(2), Color::BLACK);
        // What is typed in it, for screen readers (dots of a password say
        // nothing).
        canvas.describe(
            paperpaw_sdk::semantics::Role::Field,
            "",
            box_area,
            false,
            None,
        );
        // The end of the text, with a cursor after it; a password as dots.
        let text = if self.password {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        };
        let buttons_width = self.buttons.first().map_or(0.0, |(_, area)| {
            f32::from(box_area.x + box_area.width - area.x) + margin
        });
        let room = f32::from(box_area.width) - margin * 1.5 - buttons_width;
        // The longest end of it that fits, found by halving.
        let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let fits = |n: usize| font.measure(style.text, &text[starts[n]..]) <= room;
        let (mut low, mut high) = (0, starts.len());
        while low < high {
            let mid = (low + high) / 2;
            if fits(mid) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        let start = starts.get(low).copied().unwrap_or(text.len());
        let x = f32::from(box_area.x) + margin / 2.0;
        let shown = &text[start..];
        ui::draw_label(canvas, &font, style.text, x, box_area, shown, Color::BLACK);
        let end = x + font.measure(style.text, shown);
        let cursor = style.text as u32;
        canvas.fill_rect(
            end as i32 + 2,
            i32::from(box_area.y) + (i32::from(box_area.height) - cursor as i32) / 2,
            style.line.max(2),
            cursor,
            Color::BLACK,
        );
        for (index, &(kind, area)) in self.buttons.iter().enumerate() {
            let focused = self.focus == Some((PASTE_ROW, index));
            let label = env.i18n.tr(kind.label());
            ui::draw_button(canvas, env, &style, area, label, false, focused);
        }
    }

    fn draw_key(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, row: usize, index: usize) {
        let style = self.style;
        let key = self.rows[row][index].0;
        let area = self.key_area(row, index);
        // Buttons as everywhere, with room for the focus frame.
        let h = ui::button_height(&style, area);
        let pad = self.key_inset() as u16;
        let r = Rect::new(
            area.x + pad,
            area.y + (area.height - h) / 2,
            area.width.saturating_sub(2 * pad),
            h,
        );
        let focused = self.focus == Some((row, index));
        // Shift while on, and *OK* always: the key that ends typing stands
        // out.
        let active = (key == Key::Shift && self.shift) || key == Key::Done;
        let name = match key {
            Key::Shift => "keyboard.shift",
            Key::Delete => "keyboard.delete",
            Key::Space => "keyboard.space",
            _ => {
                let label = self.label(env, key);
                return ui::draw_button(canvas, env, &style, r, &label, active, focused);
            }
        };
        // The key drawn: an arrow, a space.
        let name = env.i18n.tr(name);
        let ink = ui::draw_icon_button(canvas, &style, r, name, active, focused);
        let size = (style.text * 0.6) as i32;
        let (cx, cy) = (
            i32::from(r.x) + i32::from(r.width) / 2,
            i32::from(r.y) + i32::from(r.height) / 2,
        );
        // The icon a theme gave the key, else drawn here.
        let icon = match key {
            Key::Shift => paperpaw_sdk::icons::SHIFT,
            Key::Delete => paperpaw_sdk::icons::DELETE,
            _ => paperpaw_sdk::icons::SPACE,
        };
        if ui::draw_icon_at(canvas, env, icon, style.text, (cx, cy), ink) {
            return;
        }
        match key {
            Key::Shift => arrow_up(canvas, cx, cy, size, ink),
            Key::Delete => arrow_left(canvas, cx, cy, size, ink),
            Key::Space => {
                let w = i32::from(r.width) / 3;
                let t = style.line.max(2) as i32;
                canvas.fill_rect(cx - w / 2, cy + size / 3, w as u32, t as u32, ink);
                canvas.fill_rect(cx - w / 2, cy, t as u32, (size / 3 + t) as u32, ink);
                canvas.fill_rect(cx + w / 2 - t, cy, t as u32, (size / 3 + t) as u32, ink);
            }
            _ => {}
        }
    }

    fn draw_variants(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let Some(popup) = &self.popup else {
            return;
        };
        let style = self.style;
        let a = popup.area;
        let border = (style.line * 2).max(2);
        let out = border as u16 * 2;
        let frame = Rect::new(
            a.x.saturating_sub(out),
            a.y.saturating_sub(out),
            a.width + 2 * out,
            a.height + 2 * out,
        );
        canvas.fill(frame, Color::WHITE);
        canvas.stroke(frame, border, Color::BLACK);
        for (index, c) in popup.chars.iter().enumerate() {
            let key = popup.key(index);
            let h = ui::button_height(&style, key);
            let pad = ui::button_border(&style) as u16 * 3;
            let r = Rect::new(
                key.x + pad,
                key.y + (key.height - h) / 2,
                key.width.saturating_sub(2 * pad),
                h,
            );
            let focused = popup.focus == Some(index);
            ui::draw_button(canvas, env, &style, r, &c.to_string(), false, focused);
        }
    }
}

/// An arrow pointing up (*Shift*), centred at `(x, y)`.
fn arrow_up(canvas: &mut Canvas<'_>, x: i32, y: i32, size: i32, color: Color) {
    let half = size / 2;
    for dy in 0..=half {
        canvas.fill_rect(x - dy, y - half + dy, (dy * 2 + 1) as u32, 1, color);
    }
    let stem = (size / 4).max(2);
    canvas.fill_rect(x - stem / 2, y, stem as u32, half as u32, color);
}

/// An arrow pointing left (*Delete*), centred at `(x, y)`.
fn arrow_left(canvas: &mut Canvas<'_>, x: i32, y: i32, size: i32, color: Color) {
    let half = size / 2;
    for dx in 0..=half {
        canvas.fill_rect(x - half + dx, y - dx, 1, (dx * 2 + 1) as u32, color);
    }
    let stem = (size / 4).max(2);
    canvas.fill_rect(x, y - stem / 2, half as u32, stem as u32, color);
}

impl Screen for KeyboardScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let r = self.region;
        canvas.fill(r, Color::WHITE);
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (self.style.line * 2).max(2),
            Color::BLACK,
        );
        self.draw_field(canvas, env);
        let clip = canvas.clip();
        for row in 0..self.rows.len() {
            for index in 0..self.rows[row].len() {
                if self.key_area(row, index).intersection(&clip).is_some() {
                    self.draw_key(canvas, env, row, index);
                }
            }
        }
        self.draw_variants(canvas, env);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if self.popup.is_some() {
            return self.on_variants_action(action, cx);
        }
        if *action == actions::CONFIRM {
            match self.focus {
                Some((PASTE_ROW, index)) => {
                    if let Some(&(kind, _)) = self.buttons.get(index) {
                        self.field_button(kind, cx);
                    }
                }
                Some((row, index)) => self.press(self.rows[row][index].0, cx),
                None => {
                    self.move_focus(action, cx);
                }
            }
            return Handled::Yes;
        }
        if held(action) {
            // A long press on confirm: the variants of a letter, else done.
            let opened = self
                .focus
                .is_some_and(|(row, index)| self.open_variants(row, index, cx));
            if !opened {
                self.finish(cx);
            }
            return Handled::Yes;
        }
        if *action == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        if self.move_focus(action, cx) {
            Handled::Yes
        } else if self.password && !PASSING.contains(action) {
            // Typing a password, nothing else runs (see the top).
            Handled::Yes
        } else {
            Handled::No
        }
    }

    /// Typing a password, no swipe in from an edge opens anything over it.
    fn holds_edges(&self) -> bool {
        self.password
    }

    /// A password's keys are not told to plugins as they are pressed.
    fn secret(&self) -> bool {
        self.password
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(pasted) = PASTED.read(event, params) {
            self.pasted(&pasted.text, cx);
        } else if let Some(typed) = REMOTE_TEXT.read(event, params) {
            // Typed on the phone with the text so far: it replaces it.
            self.text.clear();
            self.pasted(&typed.text, cx);
        } else if let Some(typed) = events::TYPE_TEXT.read(event, params) {
            self.typed(&typed.text, cx);
            if typed.done == Some(true) {
                self.finish(cx);
            }
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.region.contains(gesture.start) {
            return Handled::No;
        }
        if let Some(popup) = &self.popup {
            // A variant, or anywhere else to close them.
            let chosen = (0..popup.chars.len())
                .find(|&i| popup.key(i).contains(gesture.start))
                .map(|i| popup.chars[i]);
            self.close_variants(cx);
            if let Some(c) = chosen.filter(|_| gesture.kind == GestureKind::Tap) {
                let mut buffer = [0u8; 4];
                self.insert(c.encode_utf8(&mut buffer), cx);
            }
            return Handled::Yes;
        }
        let key = self.key_at(gesture.start);
        if let Some((r, i)) = self.focus.take() {
            self.redraw(cx, self.key_area(r, i));
        }
        match (gesture.kind, key) {
            (GestureKind::Tap, Some((PASTE_ROW, index))) => {
                if let Some(&(kind, _)) = self.buttons.get(index) {
                    self.field_button(kind, cx);
                }
            }
            // A long press in the text field pastes too, as elsewhere.
            (GestureKind::LongPress, _)
                if self.field.contains(gesture.start)
                    && self.buttons.iter().any(|b| b.0 == FieldButton::Paste) =>
            {
                self.paste(cx);
            }
            (GestureKind::Tap, Some((row, index))) => self.press(self.rows[row][index].0, cx),
            // A long press on *Delete* clears the text.
            (GestureKind::LongPress, Some((row, index)))
                if row != PASTE_ROW && self.rows[row][index].0 == Key::Delete =>
            {
                self.text.clear();
                self.changed(cx);
            }
            // A long press on a letter shows its variants.
            (GestureKind::LongPress, Some((row, index))) if row != PASTE_ROW => {
                self.open_variants(row, index, cx);
            }
            _ => {}
        }
        Handled::Yes
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn several_layouts_get_a_key_to_switch() {
        let letters = "abc def ghi";
        let one = rows(letters, Layer::Letters, false, 0);
        let two = rows(letters, Layer::Letters, true, 0);
        let has = |rows: &Vec<Vec<Slot>>| rows.iter().flatten().any(|(k, _)| *k == Key::NextLayout);
        assert!(!has(&one));
        assert!(has(&two));
        // Rows add up to the same width with or without it.
        let width = |rows: &Vec<Vec<Slot>>| -> u32 {
            rows.last()
                .unwrap()
                .iter()
                .map(|(_, w)| u32::from(*w))
                .sum()
        };
        assert_eq!(width(&one), width(&two));
        // Not on the symbols.
        assert!(!has(&rows(letters, Layer::Symbols, true, 0)));
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    /// A row too wide for a narrow screen in two as even as can be, in
    /// its order; one that fits as it is.
    #[test]
    fn rows_too_wide_are_split_evenly_in_order() {
        let row = rows("qwertyuiop asdfghjkl zxcvbnm", Layer::Letters, false, 16);
        let names = |r: &[Slot]| {
            r.iter()
                .map(|(k, _)| match k {
                    Key::Char(c) => *c,
                    Key::Shift => '^',
                    Key::Delete => '<',
                    _ => '_',
                })
                .collect::<String>()
        };
        let shown: Vec<String> = row.iter().map(|r| names(r)).collect();
        assert_eq!(
            &shown[..6],
            ["qwert", "yuiop", "asdfg", "hjkl", "^zxcv", "bnm<"]
        );
        assert!(
            row[..6]
                .iter()
                .all(|r| r.iter().map(|(_, w)| u32::from(*w)).sum::<u32>() <= 16)
        );
        // Wide enough: the rows as they are.
        assert_eq!(
            rows("qwertyuiop asdfghjkl zxcvbnm", Layer::Letters, false, 40).len(),
            4
        );
    }
}
