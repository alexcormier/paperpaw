//! The first-run guide with the real kernel, by the buttons of an X4:
//! *Back* on the first page asks before skipping it, and a choice of
//! many works without the settings plugin.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_setup::{SCREEN, Setup};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo {
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
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

const DONE: &str = ".paperpaw/plugins/setup/done";
const CHOICE: Id = Id::new_static("setup.choice");

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new() -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .plugin(Setup)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        assert_eq!(kernel.active_screen(), Some(&SCREEN), "the first start");
        Self { kernel, now: 0 }
    }

    fn press(&mut self, name: &'static str) {
        self.now += 200;
        let at = Instant(self.now);
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    fn done(&self) -> bool {
        self.kernel.storage().exists(DONE)
    }
}

#[test]
fn back_on_the_first_page_asks_before_skipping() {
    let mut test = Test::new();
    test.press("back");
    assert_eq!(test.kernel.active_screen(), Some(&CHOICE), "asked");
    assert!(!test.done());
    // Going on (the first answer): the guide stays.
    test.press("confirm");
    assert_eq!(test.kernel.active_screen(), Some(&SCREEN));
    assert!(!test.done());
    // A second stray *Back* closes the question only.
    test.press("back");
    test.press("back");
    assert_eq!(test.kernel.active_screen(), Some(&SCREEN));
    // Skipping, the second answer.
    test.press("back");
    test.press("down");
    test.press("confirm");
    assert!(!test.kernel.screens().any(|s| *s == SCREEN), "skipped");
    assert!(test.done());
}

#[test]
fn a_choice_of_many_steps_on_without_the_settings_plugin() {
    let mut test = Test::new();
    // Language, buttons, then the time zone (the device has no clock).
    test.press("confirm");
    test.press("confirm");
    let before = test.kernel.settings().text("core.time-zone").to_string();
    // Up to its card, and choose.
    test.press("up");
    test.press("confirm");
    assert_eq!(test.kernel.active_screen(), Some(&SCREEN));
    assert_ne!(test.kernel.settings().text("core.time-zone"), before);
}
