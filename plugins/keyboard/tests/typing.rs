//! Typing with the on-screen keyboard and the real kernel, on a device
//! with buttons only (like the X4) and on a touch screen: every letter of
//! a layout typed gives the text, capitals and variants, symbols and
//! digits, *Delete* on an empty field, the longest text, switching
//! layouts (and the next keyboard keeping the layout), cancelling, and
//! text that comes from elsewhere (a phone) cleaned to one line.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::semantics::{self, Part, Role};
use paperpaw_sdk::{
    ActionSpec, Command, Env, Id, Manifest, MemoryStorage, Params, Plugin, RegisterError,
    Registrar, Version,
};
use proptest::prelude::*;

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

/// A touch screen as large as the X4's.
const TOUCH: DeviceInfo = DeviceInfo {
    id: "test-touch",
    name: "Test touch",
    input: InputCaps {
        buttons: false,
        touch: true,
        pen: false,
        keys: &[],
    },
    ..BUTTONS
};

/// The answer the questions in these tests ask for.
const TYPED: Event<Typed> = Event::new("test.typed");
/// What the keyboard hears as text typed on a phone.
const REMOTE_TEXT: &str = "keyboard.remote-text";
/// Puts text on the clipboard.
const COPY: Id = Id::new_static("listener.copy");
/// Longest text the keyboard takes, in characters.
const MAX_CHARS: usize = 2000;

/// Keeps every answer.
struct Listener(Rc<RefCell<Vec<Typed>>>);

impl Plugin for Listener {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("listener"),
            "Listener",
            Version::new(0, 1, 0),
        )
        .description("Hears the typed text.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // A layout of its own, with variants of its own.
        r.contribute(
            Id::new_static("keyboard.layouts"),
            "dutch.layout; NL; qwertyuiop asdfghjkl zxcvbnm; i=ïí".into(),
        )?;
        // Far too big, and ids that are there already.
        let rows = ["abcdefghijklmnopqrstuvwxyz"; 30].join(" ");
        let variants = "é".repeat(500);
        r.contribute(
            Id::new_static("keyboard.layouts"),
            format!(
                "huge.layout; HUGE; {rows}; a={variants}\nkeyboard.layout.de; XX; abc\ndutch.layout; NX; abc"
            )
            .into(),
        )?;
        let heard = self.0.clone();
        r.on(TYPED, move |_, typed, _| {
            heard.borrow_mut().push(typed.clone())
        })?;
        // Copies text with a line break and a tab to the clipboard.
        r.add_action(ActionSpec::new(COPY, "Copy").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::SetClipboard("  pasted\nline\tend ".into()));
            },
        ))
    }
}

struct Typing {
    kernel: Kernel,
    now: u64,
    answers: Rc<RefCell<Vec<Typed>>>,
}

impl Typing {
    fn new(device: DeviceInfo) -> Self {
        Self::with_config(device, "")
    }

    /// A kernel on `device` with the user's configuration `config`.
    fn with_config(device: DeviceInfo, config: &str) -> Self {
        let answers = Rc::new(RefCell::new(Vec::new()));
        let kernel = Kernel::builder(device)
            .storage(MemoryStorage::new())
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Keyboard)
            .plugin(Listener(answers.clone()))
            .build()
            .unwrap();
        let mut typing = Self {
            kernel,
            now: 0,
            answers,
        };
        let effects = typing.kernel.start(Instant(0));
        typing.settle(effects);
        typing
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    /// Opens the keyboard as `ask::text` does, with `extra` parameters.
    fn open(&mut self, extra: &[(&'static str, &str)]) {
        let mut params = Params::new()
            .with(params::TITLE, "Name")
            .with(params::REPLY, TYPED.as_str());
        for (key, value) in extra {
            params.set(*key, *value);
        }
        self.now += 100;
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::KEYBOARD, params);
        self.settle(effects);
        assert_eq!(
            self.kernel.active_screen().map(Id::as_str),
            Some("keyboard.screen")
        );
    }

    /// Holds the button `name` down as long as a long press takes.
    fn hold_button(&mut self, name: &'static str) {
        use paperpaw_platform::{ButtonState, InputEvent};
        for (state, wait) in [(ButtonState::Pressed, 0), (ButtonState::Released, 1500)] {
            self.now += 100 + wait;
            let effects = self
                .kernel
                .handle(Instant(self.now), InputEvent::Button { name, state });
            self.settle(effects);
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    /// The part that says exactly `label`.
    fn part(&mut self, label: &str) -> Option<Part> {
        self.parts()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == label)
    }

    /// The text in the field (the text after the title), as shown.
    fn field(&mut self) -> String {
        self.parts()
            .into_iter()
            .find(|p| p.role == Role::Field)
            .map(|p| p.label)
            .unwrap_or_default()
    }

    /// What the buttons' focus is on: the last focused part drawn (a
    /// variant over the key it belongs to).
    fn focused(&mut self) -> Option<String> {
        semantics::focused(&self.parts()).map(|p| p.label.clone())
    }

    /// Moves the focus with *Next* (through every key, row after row)
    /// to the key that says `label` and presses it.
    fn press_by_buttons(&mut self, label: &str) {
        for _ in 0..80 {
            if self.focused().as_deref() == Some(label) {
                self.run(actions::CONFIRM);
                return;
            }
            self.run(actions::NEXT);
        }
        panic!("no key says {label:?}");
    }

    /// The middle of the part that says `label`.
    fn center(&mut self, label: &str) -> Point {
        let area = self
            .part(label)
            .unwrap_or_else(|| panic!("nothing says {label:?}"))
            .area;
        Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        )
    }

    fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent {
            id: 0,
            phase,
            point,
            tool: PointerTool::Finger,
            pressure: 1000,
        });
        let effects = self.kernel.handle(Instant(self.now), event);
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    fn tap_at(&mut self, at: Point) {
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    fn tap(&mut self, label: &str) {
        let at = self.center(label);
        self.tap_at(at);
    }

    /// A finger held on the part that says `label`.
    fn hold(&mut self, label: &str) {
        let at = self.center(label);
        self.finger(PointerPhase::Down, at);
        self.now += 1500;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
        self.finger(PointerPhase::Up, at);
    }

    /// The texts answered so far.
    fn answers(&self) -> Vec<String> {
        self.answers
            .borrow()
            .iter()
            .map(|t| t.text.clone())
            .collect()
    }

    /// The one answer given.
    fn answer(&self) -> String {
        let answers = self.answers();
        assert_eq!(answers.len(), 1, "{answers:?}");
        answers[0].clone()
    }

    /// Text arriving as if typed on a phone.
    fn typed_on_phone(&mut self, text: &str) {
        self.now += 100;
        let typed = Typed::new(text.to_string(), Params::new());
        let effects = self.kernel.send_event(
            Instant(self.now),
            Id::new_static(REMOTE_TEXT),
            typed.to_params(),
        );
        self.settle(effects);
    }
}

/// The letters of the English layout, row after row.
const ENGLISH: &str = "qwertyuiopasdfghjklzxcvbnm";

#[test]
fn every_letter_typed_by_buttons_gives_the_text() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[]);
    // The focus starts on the first key, as buttons need one.
    assert_eq!(typing.focused().as_deref(), Some("q"));
    for c in ENGLISH.chars() {
        typing.press_by_buttons(&c.to_string());
    }
    typing.press_by_buttons("OK");
    assert_eq!(typing.answer(), ENGLISH);
    assert_ne!(
        typing.kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
}

#[test]
fn every_letter_typed_by_touch_gives_the_text() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    // No focus while tapping.
    assert_eq!(typing.focused(), None);
    for c in ENGLISH.chars() {
        typing.tap(&c.to_string());
    }
    typing.tap("OK");
    assert_eq!(typing.answer(), ENGLISH);
}

#[test]
fn shift_makes_one_capital() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    typing.tap("Shift");
    // The keys show capitals now.
    assert!(typing.part("H").is_some());
    assert!(typing.part("h").is_none());
    typing.tap("H");
    // Only the next letter.
    typing.tap("i");
    typing.tap("Shift");
    typing.tap("Shift");
    typing.tap("x");
    typing.tap("OK");
    assert_eq!(typing.answer(), "Hix");
}

#[test]
fn symbols_digits_and_space_by_buttons() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[]);
    typing.press_by_buttons("a");
    typing.press_by_buttons("Space");
    typing.press_by_buttons("123");
    for c in "1@#9?".chars() {
        typing.press_by_buttons(&c.to_string());
    }
    typing.press_by_buttons("ABC");
    typing.press_by_buttons("b");
    typing.press_by_buttons(",");
    typing.press_by_buttons(".");
    typing.press_by_buttons("OK");
    assert_eq!(typing.answer(), "a 1@#9?b,.");
}

#[test]
fn number_mode_has_only_digits() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::MODE, "number")]);
    assert!(typing.part("q").is_none());
    assert!(typing.part("123").is_none());
    for c in "4072".chars() {
        typing.tap(&c.to_string());
    }
    typing.tap("Delete");
    typing.tap("0");
    typing.tap("OK");
    assert_eq!(typing.answer(), "4070");
}

#[test]
fn delete_on_an_empty_field_does_nothing() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[]);
    for _ in 0..3 {
        typing.press_by_buttons("Delete");
    }
    typing.press_by_buttons("a");
    typing.press_by_buttons("OK");
    assert_eq!(typing.answer(), "a");
}

#[test]
fn delete_takes_whole_characters_of_the_initial_text() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::TEXT, "Grüße 😀")]);
    typing.tap("Delete");
    typing.tap("Delete");
    typing.tap("Delete");
    typing.tap("OK");
    assert_eq!(typing.answer(), "Grüß");
}

#[test]
fn holding_delete_clears_the_text() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::TEXT, "old text")]);
    typing.hold("Delete");
    typing.tap("n");
    typing.tap("OK");
    assert_eq!(typing.answer(), "n");
}

#[test]
fn text_stops_at_the_longest_allowed() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    let at = typing.center("a");
    for _ in 0..MAX_CHARS + 5 {
        typing.tap_at(at);
    }
    typing.tap("OK");
    assert_eq!(typing.answer(), "a".repeat(MAX_CHARS));
}

#[test]
fn an_initial_text_longer_than_allowed_is_kept_but_not_added_to() {
    let long = "x".repeat(MAX_CHARS + 10);
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::TEXT, &long)]);
    typing.tap("a");
    typing.tap("OK");
    assert_eq!(typing.answer(), long);
}

#[test]
fn variants_by_touch_and_by_buttons() {
    let mut touch = Typing::new(TOUCH);
    touch.open(&[]);
    touch.hold("e");
    // The variants show above the key; one is chosen by a tap.
    assert!(touch.part("é").is_some());
    assert!(touch.part("è").is_some());
    touch.tap("é");
    // The variants are gone, the one chosen is in the field.
    assert!(touch.part("è").is_none());
    assert_eq!(touch.field(), "é");
    touch.tap("Shift");
    touch.hold("U");
    touch.tap("Ü");
    touch.tap("OK");
    assert_eq!(touch.answer(), "éÜ");

    let mut buttons = Typing::new(BUTTONS);
    buttons.open(&[]);
    // To the `e`, then a long press on confirm.
    for _ in 0..2 {
        buttons.run(actions::RIGHT);
    }
    assert_eq!(buttons.focused().as_deref(), Some("e"));
    buttons.hold_button("confirm");
    assert_eq!(buttons.focused().as_deref(), Some("è"));
    buttons.run(actions::RIGHT);
    assert_eq!(buttons.focused().as_deref(), Some("é"));
    buttons.run(actions::CONFIRM);
    // Back to the keys: the focus is on the `e` again.
    assert_eq!(buttons.focused().as_deref(), Some("e"));
    // Holding again opens them, and once more closes them; Back closes
    // the variants only.
    buttons.hold_button("confirm");
    assert_eq!(buttons.focused().as_deref(), Some("è"));
    buttons.hold_button("confirm");
    assert_eq!(buttons.focused().as_deref(), Some("e"));
    buttons.hold_button("confirm");
    buttons.run(actions::BACK);
    assert_eq!(
        buttons.kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
    buttons.press_by_buttons("OK");
    assert_eq!(buttons.answer(), "é");
}

/// A finger held near the screen's edges, where swipes in from the edge
/// begin, still makes a long press: `a` (left edge) and `.` (the bottom
/// row, all of it within the bottom edge's strip) show their variants.
#[test]
fn holding_a_letter_at_the_edge_shows_its_variants() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    typing.hold("a");
    assert!(typing.part("ä").is_some(), "no variants for `a`");
    typing.tap("ä");
    typing.hold(".");
    assert!(typing.part("…").is_some(), "no variants for `.`");
    typing.tap("…");
    typing.tap("OK");
    assert_eq!(typing.answer(), "ä…");
}

#[test]
fn a_long_press_on_confirm_without_variants_finishes() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[(params::TEXT, "abc")]);
    // On `q`, which has no variants.
    assert_eq!(typing.focused().as_deref(), Some("q"));
    typing.hold_button("confirm");
    assert_eq!(typing.answer(), "abc");
}

/// Typing a password (as for Wi-Fi on the X4), holding Select still
/// shows a letter's variants and, without any, ends the entry.
#[test]
fn holding_select_works_while_typing_a_password() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[(params::MODE, "password")]);
    for _ in 0..2 {
        typing.run(actions::RIGHT);
    }
    assert_eq!(typing.focused().as_deref(), Some("e"));
    typing.hold_button("confirm");
    assert_eq!(typing.focused().as_deref(), Some("è"));
    typing.run(actions::CONFIRM);
    typing.run(actions::LEFT);
    typing.run(actions::LEFT);
    assert_eq!(typing.focused().as_deref(), Some("q"));
    typing.hold_button("confirm");
    // Done: the keyboard is gone (a password goes to the one who asked).
    assert_ne!(
        typing.kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
}

/// Holding Select before any key has the focus ends the entry at once,
/// as it does on a key without variants (it does not only show the focus).
#[test]
fn holding_select_without_a_focus_finishes() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::TEXT, "abc")]);
    assert_eq!(typing.focused(), None);
    typing.hold_button("confirm");
    assert_eq!(typing.answer(), "abc");
}

#[test]
fn back_cancels_without_an_answer() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[(params::TEXT, "kept?")]);
    typing.press_by_buttons("a");
    typing.run(actions::BACK);
    assert!(typing.answers().is_empty());
    assert_ne!(
        typing.kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
}

#[test]
fn the_answer_carries_the_question_context() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[("shelf", "3")]);
    typing.tap("OK");
    let answers = typing.answers.borrow();
    assert_eq!(answers[0].context.get("shelf"), Some("3"));
}

#[test]
fn buttons_move_round_the_rows() {
    let mut typing = Typing::new(BUTTONS);
    typing.open(&[]);
    // On this narrow screen the rows are split ("qwert" above "yuiop").
    // Left from the first key wraps to the end of the row.
    typing.run(actions::LEFT);
    assert_eq!(typing.focused().as_deref(), Some("t"));
    typing.run(actions::RIGHT);
    assert_eq!(typing.focused().as_deref(), Some("q"));
    // Down to the row below, near the same place.
    typing.run(actions::DOWN);
    assert_eq!(typing.focused().as_deref(), Some("y"));
    // Previous from the first key of a row: the end of the row above.
    typing.run(actions::PREVIOUS);
    assert_eq!(typing.focused().as_deref(), Some("t"));
    // Up from the first row: the bottom row, below (no buttons in the
    // field without a clipboard or network).
    typing.run(actions::UP);
    assert_eq!(typing.focused().as_deref(), Some("OK"));
    // Right from the end of a row wraps to its start.
    typing.run(actions::RIGHT);
    assert_eq!(typing.focused().as_deref(), Some("123"));
    // Down from the bottom row: the top row.
    typing.run(actions::DOWN);
    assert_eq!(typing.focused().as_deref(), Some("q"));
    // Next from the end of the last row: the first key.
    typing.run(actions::UP);
    typing.run(actions::LEFT);
    assert_eq!(typing.focused().as_deref(), Some("OK"));
    typing.run(actions::NEXT);
    assert_eq!(typing.focused().as_deref(), Some("q"));
}

/// Two layouts chosen: English and German.
const TWO_LAYOUTS: &str = "[keyboard]\nlayouts = keyboard.layout.en, keyboard.layout.de\n";

#[test]
fn the_layout_key_switches_and_the_next_keyboard_keeps_it() {
    let mut typing = Typing::with_config(TOUCH, TWO_LAYOUTS);
    typing.open(&[]);
    // The key shows the layout in use.
    assert!(typing.part("EN").is_some());
    assert!(typing.part("ü").is_none());
    typing.tap("EN");
    assert!(typing.part("DE").is_some());
    typing.tap("ü");
    typing.tap("ß");
    // `ß` has no single capital: it stays as it is.
    typing.tap("Shift");
    typing.tap("ß");
    typing.tap("OK");
    assert_eq!(typing.answer(), "üßß");
    // The next keyboard opens with German.
    typing.open(&[]);
    assert!(typing.part("DE").is_some());
    assert!(typing.part("z").is_some());
    typing.tap("DE");
    assert!(typing.part("EN").is_some());
}

#[test]
fn the_layout_key_by_buttons() {
    let mut typing = Typing::with_config(BUTTONS, TWO_LAYOUTS);
    typing.open(&[]);
    typing.press_by_buttons("EN");
    typing.press_by_buttons("ö");
    typing.press_by_buttons("y");
    typing.press_by_buttons("OK");
    assert_eq!(typing.answer(), "öy");
}

#[test]
fn every_letter_of_every_drawable_layout_types_itself() {
    let layouts = [
        "de", "fr", "es", "pt", "sv", "da", "cs", "tr", "ru", "uk", "el",
    ];
    for code in layouts {
        let config = format!("[keyboard]\nlayouts = keyboard.layout.{code}\n");
        let mut typing = Typing::with_config(TOUCH, &config);
        typing.open(&[]);
        // A layout the fonts cannot draw falls back to the language's.
        let letters: Vec<String> = typing
            .parts()
            .into_iter()
            .map(|p| p.label)
            .filter(|l| l.chars().count() == 1 && l.chars().all(char::is_alphabetic))
            .collect();
        assert!(letters.len() >= 26, "{code}: {letters:?}");
        let mut typed = String::new();
        for letter in &letters {
            typing.tap(letter);
            typed.push_str(letter);
        }
        typing.tap("OK");
        assert_eq!(typing.answer(), typed, "{code}");
    }
}

#[test]
fn unknown_layouts_fall_back_to_the_language() {
    let config = "[keyboard]\nlayouts = keyboard.layout.xx, , nonsense\n";
    let mut typing = Typing::with_config(TOUCH, config);
    typing.open(&[]);
    assert!(typing.part("q").is_some());
    assert!(typing.part("w").is_some());
    // One layout: no key to switch.
    assert!(typing.part("EN").is_none());
}

#[test]
fn text_from_a_phone_replaces_the_text_as_one_line() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::TEXT, "before")]);
    typing.typed_on_phone("  first\nsecond\tthird\u{7}  ");
    typing.tap("OK");
    assert_eq!(typing.answer(), "first second third");
}

#[test]
fn text_from_a_phone_is_cut_to_the_longest_allowed() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    typing.typed_on_phone(&"ä".repeat(MAX_CHARS * 2));
    typing.tap("OK");
    assert_eq!(typing.answer(), "ä".repeat(MAX_CHARS));
}

#[test]
fn text_from_a_phone_keeps_only_digits_for_numbers() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::MODE, "number")]);
    typing.typed_on_phone("PIN: 12-34 ٣");
    typing.tap("OK");
    assert_eq!(typing.answer(), "1234");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Whatever arrives from a phone, the answer is one line, trimmed and
    /// no longer than allowed.
    #[test]
    fn text_from_a_phone_is_always_one_short_line(text in any::<String>()) {
        let mut typing = Typing::new(TOUCH);
        typing.open(&[]);
        typing.typed_on_phone(&text);
        typing.tap("OK");
        let answer = typing.answer();
        prop_assert!(answer.chars().count() <= MAX_CHARS);
        prop_assert!(!answer.chars().any(char::is_control), "{:?}", answer);
        let expected: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect::<String>()
            .trim()
            .chars()
            .take(MAX_CHARS)
            .collect();
        prop_assert_eq!(answer, expected);
    }

    /// Any sequence of English letters typed by touch comes back as typed.
    #[test]
    fn letters_typed_by_touch_come_back(text in "[a-z ]{0,24}") {
        let mut typing = Typing::new(TOUCH);
        typing.open(&[]);
        for c in text.chars() {
            if c == ' ' {
                typing.tap("Space");
            } else {
                typing.tap(&c.to_string());
            }
        }
        typing.tap("OK");
        prop_assert_eq!(typing.answer(), text);
    }
}

#[test]
fn paste_inserts_the_clipboard_as_one_line() {
    let mut typing = Typing::new(TOUCH);
    // Nothing on the clipboard: no *Paste*.
    typing.open(&[]);
    assert!(typing.part("Paste").is_none());
    typing.run(actions::BACK);
    typing.run(COPY);
    typing.open(&[(params::TEXT, "a ")]);
    typing.tap("Paste");
    typing.tap("OK");
    assert_eq!(typing.answer(), "a pasted line end");
}

#[test]
fn paste_by_buttons_from_above_the_first_row() {
    let mut typing = Typing::new(BUTTONS);
    typing.run(COPY);
    typing.open(&[]);
    typing.run(actions::UP);
    assert_eq!(typing.focused().as_deref(), Some("Paste"));
    typing.run(actions::CONFIRM);
    // Down again to the key below it, at the right of the first row.
    typing.run(actions::DOWN);
    assert_eq!(typing.focused().as_deref(), Some("t"));
    typing.press_by_buttons("OK");
    assert_eq!(typing.answer(), "pasted line end");
}

#[test]
fn paste_keeps_only_digits_for_numbers() {
    let mut typing = Typing::new(TOUCH);
    typing.run(COPY);
    typing.open(&[(params::MODE, "number")]);
    typing.tap("Paste");
    typing.tap("OK");
    assert_eq!(typing.answer(), "");
}

/// The value of the layouts setting, as saved.
fn layouts(typing: &Typing) -> String {
    typing
        .kernel
        .settings()
        .text("keyboard.layouts")
        .to_string()
}

/// The row whose text begins with `name`, with its state after it.
fn row(typing: &mut Typing, name: &str) -> Part {
    typing
        .parts()
        .into_iter()
        .find(|p| p.role == Role::Row && p.label.starts_with(name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn tap_row(typing: &mut Typing, name: &str) {
    let area = row(typing, name).area;
    typing.tap_at(Point::new(
        i32::from(area.x) + i32::from(area.width) / 2,
        i32::from(area.y) + i32::from(area.height) / 2,
    ));
}

#[test]
fn layouts_are_chosen_in_order_and_automatic_stands_alone() {
    let mut typing = Typing::new(TOUCH);
    typing.now += 100;
    let effects = typing.kernel.open_screen(
        Instant(typing.now),
        Id::new_static("keyboard.choose"),
        Params::new(),
    );
    typing.settle(effects);
    // On and off in the keyboard's own words, with no settings plugin here.
    assert!(row(&mut typing, "Automatic").label.ends_with("On"));
    tap_row(&mut typing, "Deutsch");
    assert_eq!(layouts(&typing), "keyboard.layout.de");
    assert!(row(&mut typing, "Automatic").label.ends_with("Off"));
    tap_row(&mut typing, "English");
    assert_eq!(layouts(&typing), "keyboard.layout.de, keyboard.layout.en");
    // Their place in switching shows.
    assert!(row(&mut typing, "English").label.ends_with("On (2)"));
    // Off again: the other stays.
    tap_row(&mut typing, "Deutsch");
    assert_eq!(layouts(&typing), "keyboard.layout.en");
    // The last one off: automatic again, not none.
    tap_row(&mut typing, "English");
    assert_eq!(layouts(&typing), "keyboard.layout.auto");
    tap_row(&mut typing, "Fran");
    tap_row(&mut typing, "Automatic");
    assert_eq!(layouts(&typing), "keyboard.layout.auto");
    // Saved where the user's settings are.
    let saved = paperpaw_sdk::read_file(typing.kernel.storage(), "paperpaw.ini", 1 << 16).unwrap();
    let saved = String::from_utf8(saved).unwrap();
    assert!(saved.contains("keyboard.layout.auto"), "{saved}");
}

#[test]
fn a_live_question_is_answered_on_each_change() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[(params::LIVE, "1")]);
    typing.tap("a");
    typing.tap("b");
    typing.tap("Delete");
    typing.tap("OK");
    let answers = typing.answers.borrow();
    let seen: Vec<(&str, bool)> = answers
        .iter()
        .map(|t| (t.text.as_str(), t.typing))
        .collect();
    assert_eq!(seen, [("a", true), ("ab", true), ("a", true), ("a", false)]);
}

#[test]
fn a_question_not_live_is_answered_once() {
    let mut typing = Typing::new(TOUCH);
    typing.open(&[]);
    typing.tap("a");
    typing.tap("OK");
    assert_eq!(typing.answers.borrow().len(), 1);
    assert!(!typing.answers.borrow()[0].typing);
}

#[test]
fn a_layout_another_plugin_adds_is_typed_with() {
    let config = "[keyboard]\nlayouts = keyboard.layout.en, dutch.layout\n";
    let mut touch = Typing::with_config(TOUCH, config);
    touch.open(&[]);
    // English first, then the added one with its code.
    touch.tap("EN");
    assert!(touch.part("NL").is_some(), "{:?}", touch.parts());
    // Its own variants.
    touch.hold("i");
    touch.tap("ï");
    touch.tap("OK");
    assert_eq!(touch.answer(), "ï");
}

#[test]
fn a_layout_far_too_big_stays_on_the_screen() {
    let config = "[keyboard]\nlayouts = huge.layout\n";
    for device in [TOUCH, BUTTONS] {
        let mut typing = Typing::with_config(device, config);
        typing.open(&[]);
        // Its keys within the screen, a long press on one too.
        let height = i32::from(device.display.height);
        assert!(typing.parts().iter().all(|p| i32::from(p.area.y) < height));
        // A long press on a letter of many variants: no overflow.
        if device.input.touch {
            typing.hold("a");
        }
    }
}

#[test]
fn layouts_named_as_others_are_left_out() {
    let mut typing = Typing::new(BUTTONS);
    typing.now += 100;
    let effects = typing.kernel.open_screen(
        Instant(typing.now),
        Id::new_static("keyboard.choose"),
        Params::new(),
    );
    typing.settle(effects);
    let labels: Vec<String> = typing.parts().into_iter().map(|p| p.label).collect();
    let german = labels.iter().filter(|l| l.starts_with("German")).count();
    let dutch = labels
        .iter()
        .filter(|l| l.starts_with("dutch.layout"))
        .count();
    assert!(german <= 1 && dutch <= 1, "{labels:?}");
}
