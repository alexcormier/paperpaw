//! *The page with a picture*: the page read stays while the device
//! sleeps, a picture over it whose clear parts show it, nothing else
//! changed; as see-through as asked; only where it is to be used, the
//! cover or the message otherwise; and pictures that other plugins offer
//! as presets chosen in its picker.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    Color, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_sleep::{Sleep, overlay};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Params, Plugin, Priority, RegisterError,
    Registrar, Screen, ScreenSpec, Version,
};

const X4: DeviceInfo = DeviceInfo {
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

const SCREEN: &str = "sleep.screen";
/// 64 × 64: clear around a black ring with a white middle.
const RING: &[u8] = include_bytes!("data/ring.png");
/// Where the ring goes in the middle at half the width: 240 × 240.
const BOX: (usize, usize, usize) = (120, 280, 240);

/// A page of black lines, every eighth row, bound as the reader.
struct Page;

struct Lines;

impl Screen for Lines {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let display = env.device.display;
        canvas.clear(Color::WHITE);
        for y in (0..i32::from(display.height)).step_by(8) {
            canvas.fill_rect(0, y, u32::from(display.width), 1, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Page {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("page"), "Page", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("page.screen"),
            "Page",
            Box::new(|_, _| Box::new(Lines)),
        ))?;
        r.bind_slot(
            slots::READER,
            Id::new_static("page.screen"),
            Priority::DEFAULT,
        )
    }
}

/// Offers its owl as a picture over the page, in a corner.
struct Owls;

static OWL: [Preset; 1] = [Preset::fixed(
    "owls.owl",
    overlay::KIND,
    "Owl",
    "",
    "sleep.overlay-picture = owls/owl.png\nsleep.overlay-position = sleep.position.top-left",
)];

impl Plugin for Owls {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("owls"), "Owls", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_presets(&OWL)
    }
}

struct Device {
    kernel: Kernel,
    now: u64,
    /// What the panel shows, a row after the other.
    shown: Vec<Color>,
}

impl Device {
    /// With `settings` in `[sleep]`, the overlay's pictures in `sleep/`,
    /// reading if `reading`.
    fn new(settings: &str, reading: bool) -> Self {
        let storage = MemoryStorage::new()
            .with("sleep/ring.png", RING)
            .with("owls/owl.png", RING);
        let kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(format!(
                "[core]\nlanguage = en\n[sleep]\nshow = sleep.show.overlay\n{settings}\n"
            ))
            .sleep(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Sleep)
            .plugin(Page)
            .plugin(Owls)
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            shown: vec![Color::WHITE; 480 * 800],
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        if reading {
            device.now += 100;
            let effects =
                device
                    .kernel
                    .open_slot(Instant(device.now), slots::READER, Params::new());
            device.settle(effects);
        }
        device.render();
        device
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn render(&mut self) -> &[Color] {
        let shown = &mut self.shown;
        self.kernel.render(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    let at = (usize::from(top) + usize::from(y)) * 480 + usize::from(x);
                    shown[at] = view.pixel(x, y);
                }
            }
        });
        &self.shown
    }

    fn texts(&mut self) -> Vec<String> {
        self.render();
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    fn says_sleeping(&mut self) -> bool {
        self.texts().iter().any(|t| t == "Sleeping")
    }
}

/// The pixel at `(x, y)` of `shown`.
fn at(shown: &[Color], x: usize, y: usize) -> Color {
    shown[y * 480 + x]
}

#[test]
fn a_picture_goes_over_the_page_which_shows_around_it() {
    let mut device = Device::new("", true);
    let page = device.render().to_vec();
    assert_eq!(at(&page, 0, 8), Color::BLACK);
    assert_eq!(at(&page, 0, 9), Color::WHITE);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    let shown = device.render().to_vec();
    let (left, top, side) = BOX;
    // Outside the picture nothing changed, nothing dimmed.
    for (i, (now, before)) in shown.iter().zip(&page).enumerate() {
        let (x, y) = (i % 480, i / 480);
        if !(left..left + side).contains(&x) || !(top..top + side).contains(&y) {
            assert_eq!(now, before, "{x},{y}");
        }
    }
    // Its clear corners show the page; the ring is black, the middle
    // white over the lines.
    for y in top..top + 20 {
        assert_eq!(
            at(&shown, left + 2, y),
            at(&page, left + 2, y),
            "corner {y}"
        );
    }
    let middle = left + side / 2;
    assert_eq!(at(&shown, left + 40, top + side / 2), Color::BLACK);
    let line = (top + side / 2) / 8 * 8;
    assert_eq!(at(&page, middle, line), Color::BLACK);
    assert_eq!(at(&shown, middle, line), Color::WHITE);
    assert!(!device.says_sleeping());
    // Waking, the page is as it was.
    device.now += 60_000;
    let effects = device.kernel.resume(Instant(device.now));
    device.settle(effects);
    assert_ne!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    assert_eq!(device.render(), &page[..]);
}

#[test]
fn the_page_shows_through_the_picture_as_much_as_asked() {
    let dark_in_ring = |settings: &str| {
        let mut device = Device::new(settings, true);
        device.run(actions::SLEEP);
        let shown = device.render().to_vec();
        let (left, top, _) = BOX;
        // A square of the ring between two lines of the page.
        (top + 113..top + 119)
            .flat_map(|y| (left + 30..left + 50).map(move |x| (x, y)))
            .filter(|&(x, y)| at(&shown, x, y) == Color::BLACK)
            .count()
    };
    assert_eq!(dark_in_ring(""), 120);
    let half = dark_in_ring("overlay-see-through = 50");
    assert!((40..=80).contains(&half), "{half}");
}

#[test]
fn size_and_place_are_settings() {
    let mut device = Device::new(
        "overlay-size = 20\noverlay-position = sleep.position.bottom-right",
        true,
    );
    let page = device.render().to_vec();
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    // 96 pixels, inside the margin of the bottom right corner.
    let changed: Vec<(usize, usize)> = shown
        .iter()
        .zip(&page)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| (i % 480, i / 480))
        .collect();
    assert!(!changed.is_empty());
    assert!(
        changed
            .iter()
            .all(|&(x, y)| x >= 480 - 96 - 40 && y >= 800 - 96 - 40)
    );
    assert!(changed.iter().all(|&(x, y)| x < 480 - 8 && y < 800 - 8));
}

#[test]
fn only_where_it_is_used_else_the_cover_or_the_message() {
    // Not reading: the cover, of which there is none.
    let mut device = Device::new("overlay-when = sleep.when.reading", false);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    assert!(device.says_sleeping());
    let mut device = Device::new("overlay-when = sleep.when.reading", true);
    device.run(actions::SLEEP);
    assert!(!device.says_sleeping());
    let mut device = Device::new("overlay-when = sleep.when.elsewhere", true);
    device.run(actions::SLEEP);
    assert!(device.says_sleeping());
    // No picture to show.
    let mut device = Device::new("folder = empty", true);
    device.run(actions::SLEEP);
    assert!(device.says_sleeping());
}

#[test]
fn plugins_offer_pictures_in_the_picker() {
    let mut device = Device::new("", true);
    device.now += 100;
    let effects = device
        .kernel
        .open_screen(Instant(device.now), overlay::PICK, Params::new());
    device.settle(effects);
    let texts = device.texts();
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("From the pictures folder")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Owl"), "{texts:?}");
    // The owl: its picture and its place.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let settings = device.kernel.settings();
    assert_eq!(settings.text(overlay::PICTURE.as_str()), "owls/owl.png");
    assert_eq!(
        settings.text(overlay::POSITION.as_str()),
        "sleep.position.top-left"
    );
    assert_ne!(
        device.kernel.active_screen(),
        Some(&overlay::PICK),
        "closed once chosen"
    );
    // Asleep, it is in the top left corner.
    let page = device.render().to_vec();
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    assert_ne!(at(&shown, 60, 140), at(&page, 60, 140));
    assert_eq!(&shown[600 * 480..], &page[600 * 480..]);
    // And back to the folder.
    device.now += 60_000;
    let effects = device.kernel.resume(Instant(device.now));
    device.settle(effects);
    let effects = device
        .kernel
        .open_screen(Instant(device.now), overlay::PICK, Params::new());
    device.settle(effects);
    device.run(actions::CONFIRM);
    assert_eq!(device.kernel.settings().text(overlay::PICTURE.as_str()), "");
}
