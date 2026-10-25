//! Lists draw again only the rows a focus left and came to
//! (`Rows::request`, `ListView::request`): a step on the X4 then takes a
//! fraction of a whole frame. What the panel shows after such steps is
//! kept here as the panel keeps it, the rows of each frame put over the
//! last, and must be what drawing the whole screen anew shows: nothing
//! left of a focus moved away, nothing outside the rows changed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, FrameView, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Id, MemoryStorage};

/// An X4 as the emulator's profile has it: buttons, no touch.
const DEVICE: DeviceInfo = DeviceInfo {
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

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The app, and the screen as the panel shows it: a byte a pixel.
struct App {
    kernel: Kernel,
    now: u64,
    panel: Vec<u8>,
}

/// Puts the rows of `view` at `y` over `screen`.
fn put(screen: &mut [u8], y: u16, view: &FrameView<'_>) {
    let width = usize::from(DEVICE.display.width);
    for row in 0..view.height {
        let at = (usize::from(y) + usize::from(row)) * width;
        for x in 0..view.width {
            screen[at + usize::from(x)] = view.pixel(x, row).r;
        }
    }
}

impl App {
    fn new() -> Self {
        let storage = MemoryStorage::new()
            .with(
                "books/alice-in-wonderland.txt",
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
        let effects = kernel.start(Instant(0));
        let pixels = usize::from(DEVICE.display.width) * usize::from(DEVICE.display.height);
        let mut app = Self {
            kernel,
            now: 0,
            panel: vec![0; pixels],
        };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second, then shows
    /// what changed.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        let panel = &mut self.panel;
        self.kernel.render(&mut |y, view| put(panel, y, &view));
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// Fails where the panel shows something else than the whole screen
    /// drawn anew.
    fn check(&mut self, after: &str) {
        let mut whole = vec![0; self.panel.len()];
        assert!(
            self.kernel
                .capture(&mut |y, view| put(&mut whole, y, &view))
        );
        let width = usize::from(DEVICE.display.width);
        if let Some(at) = self.panel.iter().zip(&whole).position(|(a, b)| a != b) {
            let screen = self.kernel.active_screen().map(|id| id.to_string());
            panic!(
                "after {after} on {screen:?}: the panel differs from the screen drawn anew at \
                 x {}, y {}",
                at % width,
                at / width
            );
        }
    }

    /// Steps down `count` times, then back up, checking each step.
    fn walk(&mut self, count: usize) {
        for n in 0..count {
            self.run(actions::DOWN);
            self.check(&format!("down {}", n + 1));
        }
        for n in 0..count {
            self.run(actions::UP);
            self.check(&format!("up {}", n + 1));
        }
    }
}

#[test]
fn settings_lists_drawn_in_part_show_what_a_whole_frame_does() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    app.check("opening the settings");
    app.walk(12);
    // Reading, then its first part (Text), with numbers, switches and
    // choices.
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    app.check("opening Reading");
    app.walk(8);
    app.run(actions::CONFIRM);
    app.check("opening a part of Reading");
    app.walk(14);
}
