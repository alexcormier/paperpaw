//! The vocabulary's widgets on the home screen, with the real kernel,
//! the home screen and the default fonts: each shows the words, in every
//! frame, and says what to do while there are none.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat, Storage,
};

/// An X4: black and white, buttons.
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

/// A card with three words, the newest last.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    let words = "serendipity\ten\t3\t100\tbooks/alice.epub\tAlice\tIt was pure serendipity.\n\
                 Ungeziefer\tde\t1\t20\tbooks/kafka.epub\tDie Verwandlung\tZu einem ungeheueren Ungeziefer verwandelt.\n\
                 curious\ten\t5\t9\tbooks/alice.epub\tAlice\tCuriouser and curiouser!\n";
    storage
        .write(".paperpaw/plugins/vocabulary/words.txt", words.as_bytes())
        .unwrap();
    storage
}

/// The home screen with `page` as its first page and the settings of
/// `config` (lines of the `[vocabulary]` section): what it describes,
/// and how many pixels are black.
fn home(storage: MemoryStorage, page: &str, config: &str) -> (String, usize) {
    home_on(storage, page, config, Device::Clock)
}

/// What the device has besides the card.
#[derive(Clone, Copy, PartialEq)]
enum Device {
    /// The time.
    Clock,
    /// No clock, nor a source of random numbers.
    Nothing,
    /// No clock, but random numbers (always the same ones).
    Random,
}

/// Random numbers that always draw 1.
fn one(buf: &mut [u8]) -> bool {
    buf.fill(0);
    buf[0] = 1;
    true
}

/// [`home`] on a device with `device`.
fn home_on(storage: MemoryStorage, page: &str, config: &str, device: Device) -> (String, usize) {
    let user = format!(
        "[core]\nlanguage = en\n[home]\ngesture-tips = false\npage-1 = \"{page}\"\n[vocabulary]\n{config}\n"
    );
    let mut builder = Kernel::builder(DEVICE);
    if device == Device::Random {
        builder = builder.random(one);
    }
    let mut kernel = builder
        .storage(storage)
        .user_config(user)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_vocabulary::Vocabulary)
        .build()
        .unwrap();
    let status = SystemStatus {
        clock: (device == Device::Clock).then_some(WallClock {
            at: Instant(0),
            unix_millis: 1_791_967_260_000,
            utc_offset_minutes: Some(120),
        }),
        battery: None,
    };
    kernel.set_status(Instant(0), status);
    let mut effects = kernel.start(Instant(0));
    let mut now = 0;
    for _ in 0..1000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 1000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    let mut black = 0;
    kernel.render(&mut |_, view| {
        for y in 0..view.height {
            for x in 0..view.width {
                black += usize::from(view.pixel(x, y).luma() < 128);
            }
        }
    });
    let mut text = String::new();
    for part in kernel.describe() {
        text.push_str(&part.label);
        text.push('\n');
    }
    (text, black)
}

#[test]
fn the_word_of_the_day_shows_its_sentence() {
    let (shown, _) = home(card(), "vocabulary.widget.word 0/0 4x2", "");
    assert!(shown.contains("Word of the day"), "{shown}");
    // One of the three, with its sentence.
    let words = [
        ("serendipity", "It was pure"),
        ("Ungeziefer", "Zu einem"),
        ("curious", "Curiouser"),
    ];
    assert!(
        words
            .iter()
            .any(|(w, s)| shown.contains(&format!("{w}\n")) && shown.contains(s)),
        "{shown}"
    );
    let (shown, _) = home(
        card(),
        "vocabulary.widget.word 0/0 4x2",
        "widget.word.pick = vocabulary.pick.newest\nwidget.word.book = true",
    );
    assert!(shown.contains("Newest word"), "{shown}");
    assert!(shown.contains("curious\n"), "{shown}");
    assert!(shown.contains("from Alice"), "{shown}");
}

/// Without a clock the word of the day is not stuck on the oldest word:
/// one drawn at random, else the newest.
#[test]
fn without_a_clock_the_word_of_the_day_still_changes() {
    let page = "vocabulary.widget.word 0/0 4x2";
    let (shown, _) = home_on(card(), page, "", Device::Nothing);
    assert!(
        shown.contains(
            "curious
"
        ),
        "{shown}"
    );
    let (shown, _) = home_on(card(), page, "", Device::Random);
    assert!(
        shown.contains(
            "Ungeziefer
"
        ),
        "{shown}"
    );
}

#[test]
fn the_words_saved_are_counted() {
    let (shown, _) = home(
        card(),
        "vocabulary.widget.saved 0/0 4x2",
        "widget.saved.books = true\nwidget.saved.languages = true",
    );
    assert!(shown.contains("Vocabulary"), "{shown}");
    assert!(shown.contains("3 words"), "{shown}");
    assert!(shown.contains("Newest: curious"), "{shown}");
    assert!(shown.contains("from 2 books · in 2 languages"), "{shown}");
}

#[test]
fn without_words_they_say_where_words_come_from() {
    for widget in ["vocabulary.widget.word", "vocabulary.widget.saved"] {
        let (shown, _) = home(MemoryStorage::new(), &format!("{widget} 0/0 4x2"), "");
        assert!(shown.contains("Words you look up"), "{widget}: {shown}");
    }
}

#[test]
fn both_draw_in_every_frame() {
    let (_, empty) = home(card(), "", "");
    for widget in ["word", "saved"] {
        for frame in [
            "core.frame.none",
            "core.frame.line",
            "core.frame.card",
            "core.frame.dark",
        ] {
            for size in ["4x2", "2x2", "1x1"] {
                let page = format!("vocabulary.widget.{widget} 0/0 {size}");
                let config = format!("widget.{widget}.frame = {frame}");
                let (_, black) = home(card(), &page, &config);
                assert!(black > empty + 50, "{widget} {size} in {frame}");
            }
        }
    }
}
