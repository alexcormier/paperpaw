//! Buttons and gestures changed on the device, with the real kernel: on
//! an X4 (buttons only) a change that would leave no button to select,
//! go back or move with is refused, everything changed is taken back in
//! one go, and screens of the same name can each be chosen as a place.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_keys::{Keys, OPEN};
use paperpaw_sdk::builtin::CONFIG_FILE;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat,
};

const LIST: Id = Id::new_static("keys.list");
const PICK: Id = Id::new_static("keys.pick");

const fn device(touch: bool) -> DeviceInfo {
    DeviceInfo {
        id: "test",
        name: "Test device",
        display: DisplayInfo {
            width: 480,
            height: 800,
            dpi: 220,
            format: PixelFormat::Gray4,
            rotation: Rotation::Deg0,
            color: None,
        },
        input: InputCaps {
            buttons: true,
            touch,
            pen: false,
            keys: &[],
        },
        memory: MemoryClass::Large,
    }
}

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new(touch: bool) -> Self {
        let mut kernel = Kernel::builder(device(touch))
            .storage(MemoryStorage::new())
            .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
            .plugin(Keys)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut test = Self { kernel, now: 0 };
        let at = test.at();
        test.kernel.run_action(at, OPEN);
        test
    }

    fn at(&mut self) -> Instant {
        self.now += 200;
        Instant(self.now)
    }

    fn press(&mut self, name: &'static str) {
        let at = self.at();
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn presses(&mut self, name: &'static str, times: usize) {
        for _ in 0..times {
            self.press(name);
        }
    }

    /// What the picker sends back when `input` is given `action`.
    fn picked(&mut self, input: &str, action: &str) {
        let at = self.at();
        let params = Params::new()
            .with("input", input.to_string())
            .with("id", action.to_string());
        self.kernel
            .send_event(at, Id::new_static("keys.picked"), params);
    }

    fn config(&self) -> String {
        paperpaw_sdk::read_file(self.kernel.storage(), CONFIG_FILE, 1 << 16)
            .map(|data| String::from_utf8(data).unwrap())
            .unwrap_or_default()
    }

    fn labels(&mut self) -> Vec<String> {
        self.kernel
            .describe()
            .into_iter()
            .map(|part| part.label)
            .collect()
    }
}

#[test]
fn select_cannot_be_taken_from_its_last_button_on_an_x4() {
    let mut test = Test::new(false);
    // Down to *Select* (past the place and the heading), *Nothing* for it.
    test.presses("down", 3);
    test.press("confirm");
    assert_eq!(test.kernel.active_screen(), Some(&PICK));
    test.presses("down", 2);
    test.press("confirm");
    assert_eq!(test.kernel.active_screen(), Some(&LIST));
    assert!(!test.config().contains("button.confirm"), "refused");

    // *Select* still selects: the picker opens again.
    test.press("confirm");
    assert_eq!(test.kernel.active_screen(), Some(&PICK));
}

#[test]
fn going_back_and_moving_either_way_keep_a_button_on_an_x4() {
    let mut test = Test::new(false);
    // *Back* is the only button that goes back.
    test.picked("button.back", "core.nothing");
    assert!(!test.config().contains("button.back"), "refused");
    // Three buttons move on, three move back: the last of each is kept.
    for (inputs, last) in [
        (["button.down", "button.page-forward"], "button.right"),
        (["button.up", "button.page-back"], "button.left"),
    ] {
        for input in inputs {
            test.picked(input, "core.nothing");
            let changed = format!("{input} = core.nothing");
            assert!(test.config().contains(&changed), "{input} may change");
        }
        test.picked(last, "core.nothing");
        assert!(!test.config().contains(last), "{last} is the last");
    }
}

#[test]
fn select_moves_to_another_button_first() {
    let mut test = Test::new(false);
    // Hold *Menu* to select: then *Select* may do something else.
    test.picked("long.menu", "core.confirm");
    test.picked("button.confirm", "core.nothing");
    let config = test.config();
    assert!(config.contains("long.menu = core.confirm"), "{config}");
    assert!(config.contains("button.confirm = core.nothing"), "{config}");
    // Back to how it was: *Select* first, so that held *Menu* may go.
    test.picked("button.confirm", "core.confirm");
    test.picked("long.menu", "core.menu");
    assert!(test.config().contains("long.menu = core.menu"));
}

#[test]
fn a_touch_screen_reaches_everything_without_buttons() {
    let mut test = Test::new(true);
    test.picked("button.confirm", "core.nothing");
    let config = test.config();
    assert!(
        config.contains("button.confirm = core.nothing"),
        "{config:?}"
    );
}

#[test]
fn every_change_is_taken_back_with_the_buttons() {
    let mut test = Test::new(false);
    test.picked("button.menu", "core.home");
    test.picked("long.home", "core.toggle-night-mode");
    assert!(test.config().contains("button.menu = core.home"));
    // Up from the first row: the last, *Reset buttons and gestures*.
    test.press("up");
    test.press("confirm");
    // *Reset* is the first choice.
    test.press("confirm");
    let config = test.config();
    assert!(!config.contains("button.menu"), "{config}");
    assert!(!config.contains("long.home"), "{config}");
    assert_eq!(test.kernel.active_screen(), Some(&LIST));
}

#[test]
fn screens_of_the_same_name_are_each_a_place() {
    let mut test = Test::new(false);
    // *Applies*: the places, then every screen.
    test.press("confirm");
    // The list, its picker and its question share the name; each shows,
    // told apart by its id (cut short where it does not fit).
    let labels = test.labels();
    let alike = labels
        .iter()
        .filter(|l| l.starts_with("Buttons and gestures (keys."))
        .count();
    assert_eq!(alike, 3, "{labels:?}");
}
