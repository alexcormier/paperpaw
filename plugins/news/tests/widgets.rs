//! The headlines widget on the home screen, with the real kernel, the
//! home screen and the default fonts: the headlines fetched last, as many
//! as fit and are wanted, in every frame; without feeds, how to add them.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
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

/// A card with two feeds fetched and five headlines.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    for feed in ["Tagesschau", "Guardian"] {
        storage
            .write(&format!("News/{feed}.html"), b"<p>news</p>")
            .unwrap();
    }
    let headlines = "Budget agreed\tNews/Tagesschau.html\n\
                     Storm warning\tNews/Tagesschau.html\n\
                     Water on the Moon\tNews/Guardian.html\n\
                     A reading garden\tNews/Guardian.html\n\
                     Strike ends\tNews/Tagesschau.html";
    storage
        .write(".paperpaw/plugins/news/headlines.txt", headlines.as_bytes())
        .unwrap();
    storage
}

/// The home screen with `page` as its first page and the settings of
/// `config` (lines of the `[news]` section): what it describes, and how
/// many pixels are black.
fn home(storage: MemoryStorage, page: &str, config: &str) -> (String, usize) {
    let user = format!(
        "[core]\nlanguage = en\n[home]\ngesture-tips = false\npage-1 = \"{page}\"\n[news]\n{config}\n"
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(user)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_news::News)
        .build()
        .unwrap();
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
fn the_newest_headlines_show_as_many_as_wanted() {
    let (shown, _) = home(card(), "news.widget.headlines 0/0 4x4", "");
    assert!(shown.contains("News\n"), "{shown}");
    // Four at first.
    for title in [
        "Budget agreed",
        "Storm warning",
        "Water on the Moon",
        "A reading garden",
    ] {
        assert!(shown.contains(title), "{title}: {shown}");
    }
    assert!(!shown.contains("Strike ends"), "{shown}");
    let (shown, _) = home(
        card(),
        "news.widget.headlines 0/0 4x4",
        "widget.headlines.count = news.count.2\nwidget.headlines.source = true",
    );
    assert!(shown.contains("Storm warning"), "{shown}");
    assert!(!shown.contains("Water on the Moon"), "{shown}");
    assert!(shown.contains("Tagesschau"), "{shown}");
}

#[test]
fn without_feeds_it_says_how_to_add_them() {
    let (shown, _) = home(MemoryStorage::new(), "news.widget.headlines 0/0 4x2", "");
    assert!(shown.contains("Add news feeds"), "{shown}");
}

#[test]
fn it_draws_in_every_frame() {
    let (_, empty) = home(card(), "", "");
    for frame in [
        "core.frame.none",
        "core.frame.line",
        "core.frame.card",
        "core.frame.dark",
    ] {
        for size in ["4x2", "2x2", "4x1"] {
            for lines in ["news.lines.1", "news.lines.2"] {
                let page = format!("news.widget.headlines 0/0 {size}");
                let config =
                    format!("widget.headlines.frame = {frame}\nwidget.headlines.lines = {lines}");
                let (_, black) = home(card(), &page, &config);
                assert!(black > empty + 50, "{size} in {frame}, {lines}");
            }
        }
    }
}
