//! Tests of the book widgets: what they choose and count, and that each
//! draws, in every frame and at several sizes.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

extern crate std;

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_i18n::{Catalog, Translator};
use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::widgets::{Widget, Widgets, categories};
use paperpaw_sdk::{
    Actions, Battery, Capabilities, Color, Contribution, DeviceInfo, DisplayInfo, Env, EnvParts,
    Fonts, Formats, Hyphenation, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Menus,
    Metrics, PixelFormat, Plugin, Plugins, Rect, Registrar, SettingKind, SettingSpec, SettingValue,
    Settings, StatusItems, SystemStatus, WallClock,
};

use super::highlight::{self, Found};
use super::next;
use super::numbers::Numbers;
use super::reading::{Position, chapter_left_text, left_text, wrap_words};
use super::series::{self, Member};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: paperpaw_platform::Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

struct NoKeys;

impl paperpaw_sdk::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// What an `Env` is made of: the widget pack and the default fonts
/// registered, a library on a storage in memory.
struct Harness {
    settings: Settings,
    i18n: Translator,
    fonts: Fonts,
    formats: Formats,
    hyphenation: Hyphenation,
    menus: Menus,
    actions: Actions,
    plugins: Plugins,
    status: SystemStatus,
    usage: paperpaw_sdk::usage::Usage,
    plugin_usage: core::cell::RefCell<Vec<paperpaw_sdk::usage::PluginUsage>>,
    status_items: StatusItems,
    text_filters: paperpaw_sdk::filters::TextFilters,
    decorators: paperpaw_sdk::decorations::Decorators,
    widgets: Widgets,
    setup: paperpaw_sdk::setup::SetupSteps,
    services: paperpaw_sdk::services::Services,
    secrets: paperpaw_sdk::secrets::Secrets,
    storage: MemoryStorage,
}

impl Harness {
    fn new(storage: MemoryStorage) -> Self {
        let mut h = Harness {
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(64 * 1024),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus {
                clock: Some(WallClock {
                    at: Instant(0),
                    // Wednesday, 14 October 2026, 10:41 UTC.
                    unix_millis: 1_791_974_460_000,
                    utc_offset_minutes: Some(0),
                }),
                battery: Some(Battery {
                    percent: 64,
                    charging: true,
                    millivolts: None,
                }),
            },
            usage: Default::default(),
            plugin_usage: Default::default(),
            status_items: StatusItems::new(),
            text_filters: Default::default(),
            decorators: Default::default(),
            widgets: Widgets::new(),
            setup: Default::default(),
            services: Default::default(),
            secrets: Default::default(),
            storage,
        };
        // The look of the UI, as the kernel defines it.
        for (id, value) in [
            (core_settings::UI_SIZE.as_str(), 12),
            (core_settings::UI_ROW.as_str(), 160),
            (core_settings::UI_MARGIN.as_str(), 4),
            (core_settings::UI_LINES.as_str(), 2),
            (core_settings::BATTERY_LOW.as_str(), 15),
        ] {
            h.settings
                .define(SettingSpec::new(
                    Id::parse(id).unwrap(),
                    "x",
                    SettingKind::Int { min: 0, max: 400 },
                    SettingValue::Int(value),
                ))
                .unwrap();
        }
        let mut r = Registrar::new(Id::new_static("fonts-default"));
        paperpaw_plugin_fonts_default::DefaultFonts
            .register(&mut r)
            .unwrap();
        h.apply(r);
        let mut r = Registrar::new(crate::PLUGIN);
        crate::Widgets.register(&mut r).unwrap();
        h.apply(r);
        h.i18n.set_language("en");
        h
    }

    fn apply(&mut self, r: Registrar) {
        for contribution in r.into_contributions() {
            match contribution {
                Contribution::Setting(spec) => self.settings.define(*spec).unwrap(),
                Contribution::FixedSettings(list) => {
                    for spec in list {
                        self.settings.define_fixed(spec).unwrap();
                    }
                }
                Contribution::SettingGroup(group) => self.settings.add_group(group).unwrap(),
                Contribution::Widget(widget) => self.widgets.add(*widget),
                Contribution::Translations(text) => {
                    self.i18n.add(Catalog::parse(text).unwrap());
                }
                Contribution::Font(font) => self.fonts.add(*font).unwrap(),
                Contribution::FontBinding {
                    role,
                    font,
                    priority,
                } => self.fonts.propose(role, font, priority),
                _ => {}
            }
        }
    }

    fn set(&mut self, id: &str, value: SettingValue) {
        self.settings.set(id, value).unwrap();
    }

    fn env(&self) -> Env<'_> {
        Env::from(EnvParts {
            device: &DEVICE,
            now: Instant(1000),
            metrics: Metrics {
                dpi: DEVICE.display.dpi,
            },
            settings: &self.settings,
            i18n: &self.i18n,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.plugin_usage,
            status_items: &self.status_items,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            stylesheets: &[],
            book_data: &[],
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            keys: &NoKeys,
            caps: Capabilities::default(),
            wifi: None,
            firmware: None,
            clipboard: false,
            slots: &[],
            screens: &[],
            font_dirs: &[],
            secrets: &self.secrets,
            storage: &self.storage,
        })
    }
}

/// A cover: a decoded picture (`PPGI`, its width and height, rows of
/// gray), dark at the top, light at the bottom.
const COVER: &str = ".paperpaw/covers/tales.gray";
const COVER_SIZE: (u32, u32) = (300, 450);

fn cover() -> Vec<u8> {
    let (w, h) = COVER_SIZE;
    let mut data = b"PPGI".to_vec();
    data.extend_from_slice(&w.to_le_bytes());
    data.extend_from_slice(&h.to_le_bytes());
    for y in 0..h {
        data.extend((0..w).map(|x| ((y * 255 / h + x) % 256) as u8));
    }
    data
}

/// A library: a series of four, the third being read, the first read,
/// each with a cover; two other books, favourites, a highlight.
fn library() -> MemoryStorage {
    let mut storage = MemoryStorage::new().with(COVER, cover());
    let mut records = Vec::new();
    let mut add = |storage: MemoryStorage,
                   path: &str,
                   title: &str,
                   series: &str,
                   n: &str,
                   progress: Option<u8>| {
        let storage = storage.with(path, "a book");
        let book = library::Book {
            size: 6,
            indexed: true,
            title: Some(title.to_string()),
            author: Some("Ann Author".to_string()),
            series: (!series.is_empty()).then(|| series.to_string()),
            series_index: (!n.is_empty()).then(|| n.to_string()),
            cover: (!series.is_empty()).then(|| ImageRef {
                path: COVER.to_string(),
                width: COVER_SIZE.0,
                height: COVER_SIZE.1,
            }),
            progress,
            ..library::Book::default()
        };
        library::save(&storage, path, &book);
        records.push(catalog::Record::of(path, &book));
        storage
    };
    storage = add(
        storage,
        "books/s1.epub",
        "The First",
        "Tales",
        "1",
        Some(100),
    );
    storage = add(storage, "books/s2.epub", "The Second", "Tales", "2", None);
    storage = add(
        storage,
        "books/s3.epub",
        "The Third",
        "Tales",
        "3",
        Some(40),
    );
    storage = add(storage, "books/s4.epub", "The Fourth", "Tales", "4", None);
    storage = add(storage, "books/a.epub", "Alone", "", "", None);
    storage = add(storage, "books/b.epub", "Begun", "", "", Some(10));
    catalog::write(&storage, &records);
    let _ = paperpaw_sdk::Storage::write(
        &storage,
        ".paperpaw/library/opened.txt",
        b"books/s3.epub\nbooks/b.epub\nbooks/s1.epub",
    );
    library::set_favourite(&storage, "books/a.epub", true);
    library::set_favourite(&storage, "books/s1.epub", true);
    paperpaw_sdk::annotations::add(
        &storage,
        "books/s3.epub",
        paperpaw_sdk::annotations::Annotation {
            mark: paperpaw_sdk::annotations::Mark {
                kind: paperpaw_sdk::annotations::Kind::Highlight,
                start: Locator::new(2, 10),
                end: Locator::new(2, 60),
                noted: false,
            },
            percent: 30,
            created: 0,
            updated: 0,
            text: "A sentence worth keeping, of some length.".to_string(),
            note: String::new(),
        },
    );
    Position {
        path: "books/s3.epub".to_string(),
        page: Some(40),
        pages: Some(100),
        chapter: "Chapter Three, in Which Something Happens at Last".to_string(),
        chapter_left: Some(6),
        seconds: 600,
        turned: 20,
    }
    .save(&storage);
    storage
}

const IDS: [&str; 8] = [
    "widgets.progress",
    "widgets.reading",
    "widgets.next",
    "widgets.series",
    "widgets.favourites",
    "widgets.highlight",
    "widgets.library",
    "widgets.battery",
];

/// The pixels of `widget` drawn into `area` of a page with `env`.
fn pixels(env: &Env<'_>, widget: &dyn Widget, area: Rect) -> Vec<u8> {
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.clear(Color::WHITE);
        widget.draw(&mut canvas, env, area);
    }
    let view = fb.view();
    let mut pixels = Vec::new();
    for y in area.y..area.y + area.height {
        pixels.extend((area.x..area.x + area.width).map(|x| view.pixel(x, y).luma()));
    }
    pixels
}

/// Each way the widgets with covers or long texts can be drawn.
const STYLES: [(&str, &str, &str); 7] = [
    ("widgets.progress", "style", "widgets.progress.beside"),
    ("widgets.progress", "style", "widgets.progress.round"),
    ("widgets.progress", "style", "widgets.progress.bar"),
    ("widgets.favourites", "layout", "widgets.books.layout.grid"),
    ("widgets.reading", "frame", "core.frame.line"),
    ("widgets.next", "frame", "core.frame.line"),
    ("widgets.highlight", "size", "core.size.fit"),
];

/// Placed, a widget has made what it draws (its covers small, its text
/// in lines) and draws without reading anything: the same with the
/// storage gone.
#[test]
fn placed_widgets_draw_from_memory() {
    let mut h = Harness::new(library());
    let mut bare = Harness::new(MemoryStorage::new());
    let styles = STYLES
        .iter()
        .map(|&(id, key, value)| (id, Some((key, value))));
    for (id, style) in IDS.iter().map(|&id| (id, None)).chain(styles) {
        if let Some((key, value)) = style {
            let value = SettingValue::Text(Cow::Borrowed(value));
            h.set(&alloc::format!("{id}.{key}"), value.clone());
            bare.set(&alloc::format!("{id}.{key}"), value);
        }
        for area in [
            Rect::new(10, 10, 230, 200),
            Rect::new(10, 10, 460, 200),
            Rect::new(10, 10, 460, 600),
        ] {
            let env = h.env();
            let mut widget = env.widgets.make(id, &env).unwrap();
            widget.place(&env, area);
            let drawn = pixels(&env, &*widget, area);
            assert!(drawn.iter().any(|&p| p < 128), "{id} {style:?} {area:?}");
            let without = pixels(&bare.env(), &*widget, area);
            assert!(
                drawn == without,
                "{id} {style:?} {area:?} reads as it draws"
            );
        }
    }
}

/// The logo widget with a picture for the logo: the picture is read and
/// made small where the widget is placed, and drawn from memory.
#[test]
fn a_logo_picture_draws_from_memory() {
    let logo = ".paperpaw/plugins/mine/logo.png";
    let storage = library();
    // Larger than it is drawn: made small, not larger.
    let mut png = paperpaw_sdk::image::PngWriter::new(&storage, logo, 256, 256).unwrap();
    for y in 0..256u32 {
        let row: Vec<u8> = (0..256u32)
            .map(|x| if (x / 32 + y / 32) % 2 == 0 { 0 } else { 255 })
            .collect();
        png.row(&storage, logo, &row).unwrap();
    }
    let mut h = Harness::new(storage);
    let mut bare = Harness::new(MemoryStorage::new());
    for h in [&mut h, &mut bare] {
        h.settings
            .define(SettingSpec::new(
                Id::new_static("core.logo"),
                "x",
                SettingKind::Text,
                SettingValue::Text(Cow::Borrowed(logo)),
            ))
            .unwrap();
        h.set(
            "widgets.logo.size",
            SettingValue::Text(Cow::Borrowed("core.size.large")),
        );
    }
    let env = h.env();
    let area = Rect::new(10, 10, 460, 150);
    let mut widget = env.widgets.make("widgets.logo", &env).unwrap();
    widget.place(&env, area);
    let drawn = pixels(&env, &*widget, area);
    let drawn_logo = paperpaw_sdk::logo::Logo::read(&env, 10, 10);
    assert!(matches!(drawn_logo, paperpaw_sdk::logo::Logo::Picture(_)));
    assert!(drawn.iter().any(|&p| p < 128));
    assert!(
        pixels(&bare.env(), &*widget, area) == drawn,
        "reads as it draws"
    );
}

/// Texts laid out when their widget is placed (a highlight, a note, the
/// quote of the day) show as those laid out as they are drawn unplaced.
#[test]
fn texts_draw_as_laid_out() {
    let mut h = Harness::new(library());
    let note = "If found, please return to Ann Author, who will be very glad to have it \
                back and will gladly lend you another book in return.";
    h.set(
        "widgets.note.text",
        SettingValue::Text(Cow::Owned(note.to_string())),
    );
    h.set(
        "widgets.note.heading",
        SettingValue::Text(Cow::Borrowed("Mine")),
    );
    let env = h.env();
    for id in ["widgets.highlight", "widgets.note", "widgets.quote"] {
        for area in [Rect::new(10, 10, 230, 200), Rect::new(10, 10, 460, 200)] {
            let unplaced = env.widgets.make(id, &env).unwrap();
            let mut placed = env.widgets.make(id, &env).unwrap();
            placed.place(&env, area);
            let drawn = pixels(&env, &*placed, area);
            assert!(drawn.iter().any(|&p| p < 128), "{id} {area:?}");
            assert!(drawn == pixels(&env, &*unplaced, area), "{id} {area:?}");
        }
    }
}

/// Draws widget `id` into `area` of a page; how many pixels are dark.
fn draw(h: &Harness, id: &str, area: Rect) -> usize {
    let env = h.env();
    let widget = env.widgets.make(id, &env).unwrap();
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.clear(Color::WHITE);
        widget.draw(&mut canvas, &env, area);
    }
    let view = fb.view();
    let mut dark = 0;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            dark += usize::from(view.pixel(x, y).luma() < 128);
        }
    }
    dark
}

#[test]
fn every_widget_draws_in_every_frame_and_size() {
    let mut h = Harness::new(library());
    for id in IDS {
        assert!(h.widgets.has(id), "{id}");
        let kind = h.widgets.category(id).unwrap();
        assert!(categories::ALL.contains(&kind), "{id}: {kind}");
        for frame in paperpaw_sdk::widgets::look::FRAMES {
            h.set(
                &alloc::format!("{id}.frame"),
                SettingValue::Text(Cow::Borrowed(frame)),
            );
            for area in [
                Rect::new(10, 10, 110, 95),
                Rect::new(10, 10, 230, 200),
                Rect::new(10, 10, 460, 200),
                Rect::new(10, 10, 460, 600),
            ] {
                let dark = draw(&h, id, area);
                assert!(dark > 50, "{id} {frame} {area:?}: {dark}");
            }
        }
    }
}

#[test]
fn every_style_draws() {
    let mut h = Harness::new(library());
    let area = Rect::new(0, 0, 460, 220);
    for (id, key, values) in [
        (
            "widgets.progress",
            "style",
            &[
                "widgets.progress.beside",
                "widgets.progress.round",
                "widgets.progress.ring",
                "widgets.progress.bar",
            ][..],
        ),
        (
            "widgets.battery",
            "style",
            &[
                "widgets.battery.lying",
                "widgets.battery.standing",
                "widgets.battery.ring",
            ][..],
        ),
        (
            "widgets.favourites",
            "layout",
            &["widgets.books.layout.row", "widgets.books.layout.grid"][..],
        ),
    ] {
        for value in values {
            h.set(
                &alloc::format!("{id}.{key}"),
                SettingValue::Text(Cow::Borrowed(value)),
            );
            assert!(draw(&h, id, area) > 50, "{id} {value}");
        }
    }
}

#[test]
fn widgets_show_what_the_library_knows() {
    let h = Harness::new(library());
    let env = h.env();
    // Tapped, the progress opens the book being read.
    let widget = env.widgets.make("widgets.progress", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].command, super::open("books/s3.epub"));
    // The series: each book a target, in order.
    let widget = env.widgets.make("widgets.series", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 460, 300));
    let opened: Vec<_> = targets.iter().map(|t| t.command.clone()).collect();
    assert_eq!(
        opened,
        [
            "books/s1.epub",
            "books/s2.epub",
            "books/s3.epub",
            "books/s4.epub"
        ]
        .map(super::open)
    );
    // Next up: the next in the series not read.
    let widget = env.widgets.make("widgets.next", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets[0].command, super::open("books/s4.epub"));
    // The favourites, the newest first.
    let widget = env.widgets.make("widgets.favourites", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 460, 300));
    let opened: Vec<_> = targets.iter().map(|t| t.command.clone()).collect();
    assert_eq!(opened, ["books/s1.epub", "books/a.epub"].map(super::open));
    // The highlight opens its book at its place.
    let widget = env.widgets.make("widgets.highlight", &env).unwrap();
    let targets = widget.targets(&env, Rect::new(0, 0, 200, 100));
    assert_eq!(targets.len(), 1);
}

#[test]
fn series_go_by_their_numbers() {
    assert!(super::series_rank("2") < super::series_rank("2.5"));
    assert!(super::series_rank("2.5") < super::series_rank("10"));
    assert_eq!(super::series_rank("3,25"), 3250);
    assert_eq!(super::series_rank(""), u64::MAX);
    let member = |path: &str, title: &str, number: &str, read: bool| Member {
        path: path.to_string(),
        title: title.to_string(),
        number: number.to_string(),
        read,
    };
    let mut members = vec![
        member("c", "Cee", "10", false),
        member("x", "Extra", "", false),
        member("a", "Ay", "2", true),
        member("b", "Bee", "2.5", false),
    ];
    series::order(&mut members);
    let paths: Vec<&str> = members.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(paths, ["a", "b", "c", "x"]);
    // Next: the first after the current one not read.
    members[1].read = true;
    assert_eq!(
        next::next_in_series(&members, "a").map(|m| m.path.as_str()),
        Some("c")
    );
    assert_eq!(next::next_in_series(&members, "x"), None);
    assert_eq!(next::next_in_series(&members, "elsewhere"), None);
    // Which show of a long series: round the book being read.
    assert_eq!(series::window(10, 4, Some(6), true), 5..9);
    assert_eq!(series::window(10, 4, Some(9), true), 6..10);
    assert_eq!(series::window(10, 4, Some(6), false), 0..4);
    assert_eq!(series::window(3, 6, None, true), 0..3);
}

#[test]
fn chance_picks_another_book_each_day() {
    let books: Vec<String> = (0..50).map(|i| alloc::format!("books/{i}.epub")).collect();
    let first = |day: i64| {
        let mut kept = Vec::new();
        for path in &books {
            next::keep_lowest(&mut kept, 3, next::chance(path, day), path);
        }
        assert_eq!(kept.len(), 3);
        assert!(kept.windows(2).all(|w| w[0].0 <= w[1].0));
        kept[0].1.clone()
    };
    // The same all day; not the same every day.
    assert_eq!(first(20_000), first(20_000));
    let days: std::collections::BTreeSet<String> = (20_000..20_010).map(first).collect();
    assert!(days.len() > 3, "{days:?}");
}

#[test]
fn new_books_are_those_not_seen() {
    let books = vec![(1, "one".to_string()), (5, "five".to_string())];
    // The first time, none is new.
    assert!(next::new_books(None, &books, Vec::new()).is_empty());
    let seen = [1, 2, 3];
    let newest = next::new_books(Some(&seen), &books, vec!["older".to_string()]);
    assert_eq!(newest, ["five", "older"]);
}

#[test]
fn a_highlight_a_day() {
    let books: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
    let found = |path: &str| -> Vec<Found> {
        if path == "b" {
            return Vec::new();
        }
        (0..5)
            .map(|i| {
                let at = Locator::new(i, 0);
                (at, at, alloc::format!("{path}{i}"), String::new())
            })
            .collect()
    };
    let pick = |seed: u64| highlight::pick(&books, seed, &mut |p| found(p)).unwrap();
    // The same for the same day, never from a book without any.
    assert_eq!(pick(7), pick(7));
    let picks: std::collections::BTreeSet<String> = (0..40).map(|d| pick(d).1.2).collect();
    assert!(picks.len() > 4);
    assert!(picks.iter().all(|t| !t.starts_with('b')));
    assert!(highlight::pick(&[], 1, &mut |p| found(p)).is_none());
}

#[test]
fn the_library_counted() {
    let numbers = Numbers::count(
        10,
        [Some(100), Some(40), None, Some(0), Some(100)].into_iter(),
    );
    assert_eq!(
        numbers,
        Numbers {
            books: 10,
            read: 2,
            reading: 1,
            unread: 7
        }
    );
    // Books read outside the library count too.
    assert_eq!(Numbers::count(1, [Some(100), Some(5)].into_iter()).books, 2);
    let h = Harness::new(library());
    let env = h.env();
    let mut widget = env.widgets.make("widgets.library", &env).unwrap();
    assert!(!widget.update(&env));
    assert!(!widget.live());
}

#[test]
fn time_left_from_the_pages_read() {
    let mut position = Position {
        pages: Some(200),
        page: Some(50),
        ..Position::default()
    };
    // Leafing and pauses do not count.
    position.turned(500);
    position.turned(3_600_000);
    assert_eq!(position.turned, 0);
    for _ in 0..9 {
        position.turned(30_000);
    }
    assert_eq!(position.left(), None);
    position.turned(30_000);
    // 150 pages at 30 seconds.
    assert_eq!(position.left(), Some(4500));
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    assert_eq!(left_text(&env, 4500), "About 1 h 15 min left");
    assert_eq!(left_text(&env, 600), "About 10 min left");
    // Kept and read again, the chapter too.
    let storage = MemoryStorage::new();
    position.path = "books/a.epub".to_string();
    position.chapter = "Down the Rabbit-Hole".to_string();
    position.chapter_left = Some(12);
    position.save(&storage);
    assert_eq!(Position::load(&storage), position);
    // 12 pages at 30 seconds.
    assert_eq!(position.chapter_seconds(), Some(360));
}

#[test]
fn reading_ini_of_every_version_is_read() {
    // Version 1, as written before the chapter was kept.
    let storage = MemoryStorage::new().with(
        ".paperpaw/plugins/widgets/reading.ini",
        include_str!("../../testdata/reading.ini.v1"),
    );
    let position = Position::load(&storage);
    assert_eq!(position.path, "books/a.epub");
    assert_eq!((position.page, position.pages), (Some(34), Some(210)));
    assert_eq!((position.seconds, position.turned), (600, 20));
    assert_eq!(position.chapter, "");
    assert_eq!(position.chapter_left, None);
    assert_eq!(position.chapter_seconds(), None);
}

#[test]
fn whats_left_of_the_chapter_as_room_allows() {
    let mut h = Harness::new(MemoryStorage::new());
    let mut position = Position {
        chapter: "Down the Rabbit-Hole".to_string(),
        chapter_left: Some(12),
        ..Position::default()
    };
    let text = |h: &Harness, position: &Position, timed, titled, width| {
        chapter_left_text(&h.env(), position, timed, titled, 12.0, width).unwrap()
    };
    let wide = 1000;
    assert_eq!(
        text(&h, &position, true, true, wide),
        "12 pages left in the chapter"
    );
    // With a speed, the time too; less where there is less room.
    position.seconds = 300;
    position.turned = 10;
    assert_eq!(
        text(&h, &position, true, true, wide),
        "12 pages left in the chapter · about 6 min"
    );
    assert_eq!(
        text(&h, &position, false, true, wide),
        "12 pages left in the chapter"
    );
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let width = |text: &str| font.measure(12.0, text).ceil() as u16;
    let narrow = width("12 pages left · about 6 min");
    assert_eq!(
        text(&h, &position, true, true, narrow),
        "12 pages left · about 6 min"
    );
    assert_eq!(text(&h, &position, true, true, narrow - 1), "12 pages left");
    assert_eq!(text(&h, &position, true, true, 10), "12 pages left");
    // Without the chapter's title above it, it says which pages are meant.
    assert_eq!(
        text(&h, &position, true, false, 10),
        "12 pages left in the chapter"
    );
    position.chapter_left = Some(1);
    assert_eq!(
        text(&h, &position, false, true, wide),
        "1 page left in the chapter"
    );
    position.chapter_left = Some(0);
    assert_eq!(
        text(&h, &position, true, true, wide),
        "Last page of the chapter"
    );
    position.chapter_left = None;
    let env = h.env();
    assert_eq!(
        chapter_left_text(&env, &position, true, true, 12.0, wide),
        None
    );
    h.i18n.set_language("de");
    position.chapter_left = Some(12);
    assert_eq!(
        text(&h, &position, true, true, wide),
        "Noch 12 Seiten im Kapitel · etwa 6 Min."
    );
    assert_eq!(text(&h, &position, false, true, 10), "Noch 12 Seiten");
}

#[test]
fn long_titles_in_lines_as_even_as_can_be() {
    let h = Harness::new(MemoryStorage::new());
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let width = |text: &str| font.measure(12.0, text).ceil() as u16;
    let title = "Alice's Adventures in Wonderland";
    let wrap = |most, room| wrap_words(&font, 12.0, title, room, most);
    // Where it fits, or one line is all there is room for: as it is.
    assert_eq!(wrap(3, width(title)), [title]);
    assert_eq!(wrap(1, width("Alice's")), [title]);
    // In two, the longer line as short as can be.
    assert_eq!(
        wrap(3, width("Alice's Adventures in")),
        ["Alice's Adventures", "in Wonderland"]
    );
    // Narrower: as many words as fit, the rest last.
    assert_eq!(
        wrap(3, width("Adventures in")),
        ["Alice's", "Adventures in", "Wonderland"]
    );
    assert_eq!(
        wrap(2, width("Adventures in")),
        ["Alice's", "Adventures in Wonderland"]
    );
}

#[test]
fn every_setting_has_a_name_in_both_languages() {
    let mut h = Harness::new(MemoryStorage::new());
    for language in ["en", "de"] {
        h.i18n.set_language(language);
        for spec in h.settings.specs() {
            if !IDS.contains(&h.settings.group_of(spec).as_str()) {
                continue;
            }
            let mut keys = vec![spec.title.to_string()];
            if let SettingKind::Choice(values) = &spec.kind {
                keys.extend(values.iter().map(|v| v.to_string()));
            }
            // The shared look's names come from the kernel.
            for key in keys.iter().filter(|k| !k.starts_with("core.")) {
                assert!(h.i18n.get(key).is_some(), "{language}: {key}");
            }
        }
        for id in IDS {
            assert!(h.i18n.get(h.widgets.title(id).unwrap()).is_some(), "{id}");
        }
    }
}
