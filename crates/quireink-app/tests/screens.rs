//! What the standard app's screens show, compared with what they showed
//! when last looked at (`tests/snapshots`): each part a screen describes
//! (rows, buttons, headings) with its label and state, in order. A change
//! to what a screen shows (a row gone, another order, a label, a switch's
//! state) fails here; places on the screen are left out, so a change of
//! fonts does not. After a change that is meant, the files are written
//! anew with `QUIREINK_SNAPSHOTS=update`.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;
use std::path::PathBuf;

use quireink_core::{Effects, Kernel};
use quireink_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use quireink_sdk::builtin::{actions, params, slots};
use quireink_sdk::{Id, MemoryStorage, Params};

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

const BOOK: &str = "books/alice-in-wonderland.txt";

fn fonts() -> quireink_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return quireink_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return quireink_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    /// The app on a card with two books, past its first-run setup.
    fn new() -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(
                "books/die-verwandlung.txt",
                include_str!("../../../samples/library/books/die-verwandlung.txt"),
            )
            // The first-run setup was done.
            .with(".quireink/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n");
        let mut kernel = quireink_app::install(builder, fonts()).build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second.
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn open_book(&mut self) {
        self.now += 100;
        let params = Params::new().with(params::PATH, BOOK);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
    }

    /// What the screen shows: its id, then a line for each part.
    fn described(&mut self) -> String {
        // What is shown is drawn first.
        self.kernel.render(&mut |_, _| {});
        let mut text = String::new();
        let screen = self.kernel.active_screen().map_or("", |id| id.as_str());
        writeln!(text, "screen {screen}").unwrap();
        for part in self.kernel.describe() {
            let mut state = String::new();
            if part.focused {
                state.push_str(" (focused)");
            }
            match part.on {
                Some(true) => state.push_str(" (on)"),
                Some(false) => state.push_str(" (off)"),
                None => {}
            }
            let label = part.label.replace('\n', " / ");
            writeln!(text, "{:?} \"{label}\"{state}", part.role).unwrap();
        }
        text
    }

    /// Compares what the screen shows with `tests/snapshots/{name}.txt`.
    fn check(&mut self, name: &str) {
        let shown = self.described();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var("QUIREINK_SNAPSHOTS").as_deref() == Ok("update") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: QUIREINK_SNAPSHOTS=update", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown, kept,
            "{name} shows something else now (QUIREINK_SNAPSHOTS=update if meant)"
        );
    }
}

#[test]
fn home() {
    let mut app = App::new();
    app.check("home");
}

#[test]
fn library() {
    let mut app = App::new();
    app.run(actions::LIBRARY);
    app.check("library");
}

#[test]
fn main_menu_and_tools() {
    let mut app = App::new();
    app.run(actions::MENU);
    app.check("main-menu");
    app.run(actions::BACK);
    app.run(actions::TOOLS);
    app.check("tools");
}

#[test]
fn settings_and_a_group() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    app.check("settings");
    // Past the search, the first group.
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    app.check("settings-reading");
}

#[test]
fn reading_and_its_menus() {
    let mut app = App::new();
    app.open_book();
    app.check("reader");
    app.run(actions::CONFIRM);
    app.check("reader-menu");
    app.run(actions::BACK);
    app.run(Id::new_static("reader.quick-settings"));
    app.check("reader-layout");
}

/// The home screen's widgets read what they show when it shows again:
/// after reading, the book read is the one to continue.
#[test]
fn home_shows_the_book_read_last() {
    let mut app = App::new();
    assert!(app.described().contains("Start reading"));
    app.open_book();
    app.run(actions::HOME);
    let shown = app.described();
    assert!(shown.starts_with("screen home.dashboard"), "{shown}");
    assert!(shown.contains("Continue reading"), "{shown}");
    assert!(shown.contains("Alice"), "{shown}");
}
