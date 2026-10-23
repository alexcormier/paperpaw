//! The keyboard's size (`keyboard.size`) on the X4 Pro and the Boox Go 7:
//! larger sizes give higher keys, no size goes below a finger's least
//! key or past half the screen, Medium is the keyboard as before, and
//! with buttons only the size changes nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{Id, MemoryStorage, Metrics, Params};

/// The X4 Pro as the emulator's profile has it: touch and buttons.
const X4_PRO: DeviceInfo = DeviceInfo {
    id: "test-x4-pro",
    name: "Test X4 Pro",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 217,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

/// The Boox Go 7: a larger touch screen, finer.
const BOOX: DeviceInfo = DeviceInfo {
    id: "test-boox",
    name: "Test Boox",
    display: DisplayInfo {
        width: 1264,
        height: 1680,
        dpi: 300,
        format: PixelFormat::Gray8,
        ..X4_PRO.display
    },
    input: InputCaps {
        buttons: false,
        touch: true,
        pen: true,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

/// The X4: buttons only.
const X4: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    ..X4_PRO
};

const SIZES: [&str; 3] = [
    "keyboard.size.small",
    "keyboard.size.medium",
    "keyboard.size.large",
];

/// The keyboard as drawn on `device` with `config`: its parts.
fn keyboard(device: DeviceInfo, config: &str) -> Vec<Part> {
    let mut kernel = Kernel::builder(device)
        .storage(MemoryStorage::new())
        .user_config(format!(
            "[core]\nlanguage = en\n[help]\nshow = false\n{config}"
        ))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Keyboard)
        .build()
        .unwrap();
    let mut effects = kernel.start(Instant(0));
    settle(&mut kernel, &mut effects);
    let params = Params::new().with(params::TITLE, "Name");
    let mut effects = kernel.open_slot(Instant(100), slots::KEYBOARD, params);
    settle(&mut kernel, &mut effects);
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
    kernel.render(&mut |_, _| {});
    kernel.describe()
}

fn settle(kernel: &mut Kernel, effects: &mut Effects) {
    let mut now = 0;
    while let Some(at) = effects.wake_at() {
        if at.0 > now + 1000 {
            return;
        }
        now = now.max(at.0);
        *effects = kernel.tick(Instant(now));
    }
}

fn key(parts: &[Part], label: &str) -> Part {
    parts
        .iter()
        .find(|p| p.role == Role::Button && p.label == label)
        .unwrap_or_else(|| panic!("no key says {label:?}"))
        .clone()
}

/// What a key takes from one row to the next, in pixels: its height
/// where it is tapped, seam included.
fn step(parts: &[Part]) -> u16 {
    key(parts, "a").area.y - key(parts, "q").area.y
}

/// How high the keyboard is, from the top of its title to the bottom.
fn height(device: DeviceInfo, parts: &[Part]) -> u16 {
    let top = parts
        .iter()
        .find(|p| p.label == "Name")
        .expect("the title")
        .area
        .y;
    device.display.height - top
}

fn sized(device: DeviceInfo, size: &str) -> Vec<Part> {
    keyboard(device, &format!("[keyboard]\nsize = {size}\n"))
}

#[test]
fn larger_sizes_give_higher_keys_and_never_below_a_finger() {
    for device in [X4_PRO, BOOX] {
        let least = Metrics::new(device.display.dpi).mm(4) as u16;
        let steps: Vec<u16> = SIZES.iter().map(|s| step(&sized(device, s))).collect();
        assert!(steps[0] < steps[1], "{}: {steps:?}", device.id);
        assert!(steps[1] < steps[2], "{}: {steps:?}", device.id);
        for size in SIZES {
            let parts = sized(device, size);
            assert!(
                step(&parts) >= least,
                "{} {size}: {}",
                device.id,
                step(&parts)
            );
            let height = height(device, &parts);
            assert!(
                height <= device.display.height / 2,
                "{} {size}: {height}",
                device.id
            );
            // The keys stay as wide as before: the size changes heights.
            assert_eq!(
                key(&parts, "q").area.width,
                key(&keyboard(device, ""), "q").area.width
            );
        }
    }
}

#[test]
fn round_keys_take_about_30_40_and_50_percent() {
    for device in [X4_PRO, BOOX] {
        let least = Metrics::new(device.display.dpi).mm(4) as u16;
        for (size, near) in SIZES.iter().zip([30, 40, 50]) {
            let parts = sized(device, size);
            let share = u32::from(height(device, &parts)) * 100 / u32::from(device.display.height);
            // Where the keys are as small as a finger allows, the
            // keyboard is as low as it can be.
            let floored = step(&parts) == least && share > near;
            assert!(
                floored || share.abs_diff(near) <= 2,
                "{} {size}: {share} %",
                device.id
            );
        }
    }
}

#[test]
fn medium_is_the_keyboard_as_before() {
    for device in [X4_PRO, BOOX] {
        assert_eq!(
            sized(device, "keyboard.size.medium"),
            keyboard(device, ""),
            "{}",
            device.id
        );
    }
}

#[test]
fn the_size_leaves_keys_chosen_with_buttons_alone() {
    let before = keyboard(X4, "");
    for size in SIZES {
        assert_eq!(sized(X4, size), before, "{size}");
    }
}

#[test]
fn the_floor_wins_where_a_size_cannot_be_reached() {
    // A small screen of large pixels: Small would go below a finger.
    let coarse = DeviceInfo {
        display: DisplayInfo {
            width: 320,
            height: 480,
            dpi: 160,
            ..X4_PRO.display
        },
        ..X4_PRO
    };
    let least = Metrics::new(160).mm(4) as u16;
    let small = sized(coarse, "keyboard.size.small");
    assert!(step(&small) >= least, "{}", step(&small));
}
