//! Reviewing flash cards with the real kernel and the default fonts: with
//! buttons alone and by touch, the answers kept and written into the
//! deck as the review closes, the widget counting what is due.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    InputEvent, Point, PointerEvent, PointerPhase, PointerTool, Rotation, SystemStatus, WallClock,
};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage,
};

/// An X4: black and white, buttons.
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

/// A touch screen as large.
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

const DIR: &str = ".paperpaw/plugins/vocabulary/cards";
/// The time the device knows, in seconds since 1970.
const NOW: i64 = 1_791_967_260;

/// A deck of two new cards and one due yesterday.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    let write = |name: &str, text: &str| {
        storage
            .write(&format!("{DIR}/{name}"), text.as_bytes())
            .unwrap();
    };
    write("decks.txt", "paperpaw-decks 1\n1\tWords\n");
    let yesterday = NOW - 24 * 3600;
    let seen = NOW - 4 * 24 * 3600;
    write(
        "deck-1.txt",
        &format!(
            "paperpaw-cards 1\n\
             1\t0\t0\t0\t0\t0\t0\tHund\tdog\tDer Hund bellt.\n\
             2\t0\t0\t0\t0\t0\t0\tKatze\tcat\t\n\
             3\t{yesterday}\t{seen}\t317\t528\t1\t0\tMaus\tmouse\t\n"
        ),
    );
    write(
        "due.txt",
        &format!("paperpaw-due 1\n1\t2\t{}=1\n", yesterday / 3600),
    );
    storage
}

struct Reader {
    kernel: Kernel,
    now: u64,
}

impl Reader {
    fn new(device: DeviceInfo, storage: MemoryStorage, config: &str) -> Self {
        let user = format!("[core]\nlanguage = en\n[home]\ngesture-tips = false\n{config}\n");
        let mut kernel = Kernel::builder(device)
            .storage(storage)
            .user_config(user)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(paperpaw_plugin_vocabulary::Vocabulary)
            .build()
            .unwrap();
        kernel.set_status(
            Instant(0),
            SystemStatus {
                clock: Some(WallClock {
                    at: Instant(0),
                    unix_millis: NOW * 1000,
                    utc_offset_minutes: Some(0),
                }),
                battery: None,
            },
        );
        let effects = kernel.start(Instant(0));
        let mut reader = Self { kernel, now: 0 };
        reader.settle(effects);
        reader
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
            match effects.wake_at() {
                Some(at) if at.0 <= self.now + 1000 => {
                    self.now = self.now.max(at.0);
                    effects = self.kernel.tick(Instant(self.now));
                }
                _ => return,
            }
        }
    }

    fn open(&mut self, screen: &str) {
        self.now += 100;
        let effects =
            self.kernel
                .open_screen(Instant(self.now), Id::parse(screen).unwrap(), Params::new());
        self.settle(effects);
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

    fn shown(&mut self) -> String {
        self.parts()
            .iter()
            .map(|p| format!("{}\n", p.label))
            .collect()
    }

    fn tap(&mut self, label: &str) {
        let part = self
            .parts()
            .into_iter()
            .find(|p| p.role == Role::Button && p.label == label)
            .unwrap_or_else(|| panic!("no button {label}"));
        let a = part.area;
        let at = Point::new(
            i32::from(a.x) + i32::from(a.width) / 2,
            i32::from(a.y) + i32::from(a.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event = InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point: at,
                tool: PointerTool::Finger,
                pressure: 1000,
            });
            let effects = self.kernel.handle(Instant(self.now), event);
            // Not while the finger is down: that would be a long press.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn deck(&self) -> String {
        let storage = self.kernel.storage();
        let file = format!("{DIR}/deck-1.txt");
        let size = storage.size(&file).unwrap() as usize;
        let bytes = paperpaw_sdk::read_file(storage, &file, size).unwrap();
        String::from_utf8(bytes).unwrap()
    }
}

#[test]
fn cards_are_reviewed_with_buttons_alone() {
    let mut reader = Reader::new(BUTTONS, card(), "");
    reader.open("vocabulary.review");
    // The card due first, its back hidden.
    let shown = reader.shown();
    assert!(
        shown.contains("Maus\n") && !shown.contains("mouse"),
        "{shown}"
    );
    assert!(shown.contains("3 left"), "{shown}");
    reader.run(actions::CONFIRM);
    let shown = reader.shown();
    assert!(shown.contains("mouse"), "{shown}");
    for answer in ["Again", "Hard", "Good", "Easy"] {
        assert!(shown.contains(&format!("{answer}\n")), "{shown}");
    }
    // When each would come again; the focus on Good.
    assert!(shown.contains("10 min"), "{shown}");
    let focused = reader.parts().into_iter().find(|p| p.focused).unwrap();
    assert_eq!(focused.label, "Good");
    reader.run(actions::CONFIRM);
    // Then the new ones, in their order; Again brings one back.
    assert!(reader.shown().contains("Hund\n"));
    reader.run(actions::CONFIRM);
    reader.run(actions::LEFT);
    reader.run(actions::LEFT);
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("Katze\n"));
    reader.run(actions::CONFIRM);
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("Hund\n"), "Again: Hund once more");
    reader.run(actions::CONFIRM);
    reader.run(actions::CONFIRM);
    assert!(reader.shown().contains("Done: 4 cards reviewed."));
    // Kept as answered, written into the deck as the review closes.
    let storage = reader.kernel.storage();
    assert!(storage.exists(&format!("{DIR}/answers.txt")));
    reader.run(actions::BACK);
    assert!(
        !reader
            .kernel
            .storage()
            .exists(&format!("{DIR}/answers.txt"))
    );
    let deck = reader.deck();
    let reps: Vec<&str> = deck
        .lines()
        .skip(1)
        .map(|l| l.split('\t').nth(5).unwrap())
        .collect();
    assert_eq!(reps, ["2", "1", "2"], "{deck}");
}

#[test]
fn cards_are_reviewed_by_touch() {
    let mut reader = Reader::new(TOUCH, card(), "");
    reader.open("vocabulary.review");
    reader.tap("Show answer");
    let shown = reader.shown();
    assert!(shown.contains("mouse"), "{shown}");
    reader.tap("Easy");
    assert!(reader.shown().contains("Hund\n"));
    // New cards a day: none.
    let mut reader = Reader::new(TOUCH, card(), "[vocabulary]\nnew-per-day = 0");
    reader.open("vocabulary.review");
    reader.tap("Show answer");
    reader.tap("Good");
    assert!(reader.shown().contains("Done: 1 card reviewed."));
}

#[test]
fn the_widget_counts_the_cards_due() {
    let mut reader = Reader::new(
        BUTTONS,
        card(),
        "columns = 4\npage-1 = \"vocabulary.widget.due 0/0 4x2\"",
    );
    let shown = reader.shown();
    assert!(shown.contains("3 cards due"), "{shown}");
    let mut empty = Reader::new(
        BUTTONS,
        MemoryStorage::new(),
        "columns = 4\npage-1 = \"vocabulary.widget.due 0/0 4x2\"",
    );
    let shown = empty.shown();
    assert!(shown.contains("flash cards"), "{shown}");
}

#[test]
fn flash_cards_show_what_is_due_and_the_decks() {
    let mut reader = Reader::new(BUTTONS, card(), "");
    reader.open("vocabulary.cards");
    let shown = reader.shown();
    assert!(shown.contains("3 cards due"), "{shown}");
    assert!(shown.contains("Edit with your phone"), "{shown}");
    assert!(shown.contains("Words, 3 cards"), "{shown}");
}
