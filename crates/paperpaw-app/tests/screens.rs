//! What the standard app's screens show, compared with what they showed
//! when last looked at (`tests/snapshots`): each part a screen describes
//! (rows, buttons, headings) with its label and state, in order. A change
//! to what a screen shows (a row gone, another order, a label, a switch's
//! state) fails here; places on the screen are left out, so a change of
//! fonts does not. After a change that is meant, the files are written
//! anew with `PAPERPAW_SNAPSHOTS=update`.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;
use std::path::PathBuf;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{Id, MemoryStorage, Params};

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

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
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
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n");
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
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
        if std::env::var("PAPERPAW_SNAPSHOTS").as_deref() == Ok("update") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_SNAPSHOTS=update", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown, kept,
            "{name} shows something else now (PAPERPAW_SNAPSHOTS=update if meant)"
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

/// The ready-made layouts of the home screen: each shown on it before it
/// is kept, the one used kept, back leaving all as it was.
#[test]
fn home_layouts_are_shown_then_kept() {
    let mut app = App::new();
    let choose = Id::new_static("home.choose-layout");
    app.run(choose.clone());
    app.check("home-layouts");
    // The covers shown, not kept: back leaves the page as it was.
    app.run(actions::NEXT_PAGE);
    assert!(app.kernel.settings().previewing());
    assert!(
        app.kernel
            .settings()
            .text("home.page-1")
            .contains("home.recent@1")
    );
    app.run(actions::BACK);
    assert!(!app.kernel.settings().previewing());
    assert!(!app.kernel.settings().is_set("home.page-1"));
    // Used, kept: the dashboard, its widgets with looks of their own.
    app.run(choose.clone());
    app.run(actions::NEXT_PAGE);
    app.run(actions::NEXT_PAGE);
    app.run(actions::CONFIRM);
    let settings = app.kernel.settings();
    assert!(!settings.previewing());
    assert!(settings.text("home.page-1").contains("stats.widget.goal@1"));
    assert_eq!(settings.int("home.rows"), 6);
    assert!(settings.is_set("stats.widget.goal@1.frame"));
    let shown = app.described();
    assert!(shown.starts_with("screen home.dashboard"), "{shown}");
    // Calm again: everything back at its defaults.
    app.run(choose);
    app.run(actions::PREV_PAGE);
    app.run(actions::PREV_PAGE);
    app.run(actions::CONFIRM);
    let settings = app.kernel.settings();
    for id in ["home.page-1", "home.rows", "stats.widget.goal@1.frame"] {
        assert!(!settings.is_set(id), "{id}");
    }
}
