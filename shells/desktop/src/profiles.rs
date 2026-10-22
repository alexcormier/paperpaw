//! Built-in device profiles.
//!
//! A profile describes everything the emulator needs to behave like a
//! device: screen, pixel format, input hardware, buttons and the heap the
//! kernel may use. The Xteink profiles build the kernel as the firmware
//! does (`shells/xteink/src/main.rs`), the Boox Go 7 as the Android shell
//! does (`shells/android/src/app.rs`); a change there belongs here too.
//! The emulator keeps the kernel's own clipboard on every profile.

use crate::light::Light;
use paperpaw_platform::{
    ColorPanel, DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation, buttons,
};

/// The X4's front buttons, left to right, as `paperpaw-x4` names them.
const X4_ROW: [&str; 4] = [
    buttons::BACK,
    buttons::CONFIRM,
    buttons::LEFT,
    buttons::RIGHT,
];

/// Every button of the X4 and the X4 Classic.
const X4_BUTTONS: [&str; 7] = [
    buttons::PAGE_BACK,
    buttons::PAGE_FORWARD,
    buttons::LEFT,
    buttons::RIGHT,
    buttons::CONFIRM,
    buttons::BACK,
    buttons::POWER,
];

/// The firmware's setting defaults (the X4, X4 Pro and X4 Classic).
const X4_SETTINGS: [(&str, &str); 10] = [
    // A 4.3" screen: smaller text, so menus and pages hold enough.
    ("core.ui-size", "9"),
    ("core.font-size", "9"),
    // A full refresh takes 3.5 s on its panel: screens change fast.
    ("core.flash-screens", "false"),
    // On USB the radio stays on in a network joined before.
    ("wifi.while-charging", "true"),
    // A small screen: calm status lines, the time and the battery at the
    // edges; while reading the time at the left, the page at the right
    // (and the mark of reading aloud beside the time while it reads).
    (
        "reader.status.bottom-left",
        "core.status.clock, reader.status.aloud, lock.touch-status",
    ),
    ("reader.status.bottom-center", ""),
    ("reader.status.bottom-right", "reader.status.page"),
    ("home.status.left", "core.status.clock"),
    ("home.status.center", ""),
    ("home.status.right", "lock.touch-status, core.status.battery"),
];

/// The firmware's memory for glyphs of font packs on the card.
const X4_GLYPH_CACHE: usize = 12 * KIB;

/// The heap of the ESP32-S3 devices: 8 MB of PSRAM and 256 KiB of the
/// chip's own, with about a fifth more here (pointers of 8 bytes, not 4).
const S3_HEAP: usize = 10 * 1024 * KIB;

/// An emulated device.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub device: DeviceInfo,
    /// Physical buttons the device has; other keys are ignored.
    pub buttons: &'static [&'static str],
    /// Heap available to the kernel and plugins, if limited.
    pub heap_budget: Option<usize>,
    /// Memory for runtime-rasterized glyphs.
    pub glyph_cache: usize,
    /// A mark shows at once while slow work goes on (see
    /// `KernelBuilder::busy_mark`), where the panel refreshes on its own.
    pub busy_mark: bool,
    /// Black and white are shown in the colours of the settings (see
    /// `KernelBuilder::colors`).
    pub colors: bool,
    /// Use bitmap font packs read in place, like small devices without
    /// runtime font rendering; otherwise scalable fonts.
    pub font_packs: bool,
    /// Draw frames in bands of this many rows (see
    /// `KernelBuilder::frame_rows`); `None` keeps the whole frame in RAM.
    pub frame_rows: Option<u16>,
    /// Rows passed from the frame start and end at multiples of this (see
    /// `KernelBuilder::row_align`): 8 where the panel controller takes 8
    /// of them in a byte.
    pub row_align: u16,
    /// Device-specific setting defaults, as `(id, value)`.
    pub settings: &'static [(&'static str, &'static str)],
    /// The device speaks text aloud (emulated: the text is logged and
    /// takes as long as it would to speak).
    pub speech: bool,
    /// PaperPaw manages Wi-Fi itself, as on the X4 (emulated: made-up
    /// networks, see `wifi`); elsewhere the system does.
    pub wifi: bool,
    /// What plugin libraries call the device (see
    /// `KernelBuilder::catalog_device`): `None` where PaperPaw has no
    /// build for it yet, and its plugin store offers nothing.
    pub catalog_device: Option<&'static str>,
    /// PaperPaw shows its sleep screen and the device sleeps (the power
    /// button); Android shows its own.
    pub sleep: bool,
    /// The buttons in a row below the screen, left to right (see
    /// `KernelBuilder::button_row`).
    pub button_row: &'static [&'static str],
    /// The device's front light, if it has one.
    pub light: Option<Light>,
    /// One-line description for `--list-profiles`.
    pub summary: &'static str,
}

const KIB: usize = 1024;

pub const PROFILES: &[Profile] = &[
    Profile {
        device: DeviceInfo {
            id: "xteink-x4",
            name: "Xteink X4",
            display: DisplayInfo {
                width: 480,
                height: 800,
                dpi: 220,
                // As the firmware draws: black and white, what the panel
                // shows until its four grays (the controller's LUTs) come.
                format: PixelFormat::Mono1,
                rotation: Rotation::Deg90,
                color: None,
            },
            input: InputCaps {
                buttons: true,
                touch: false,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Small,
        },
        buttons: &X4_BUTTONS,
        // The firmware's heap: 192 KiB, and 64 KiB in the memory the boot
        // loader used (two regions: no block larger than 192 KiB). The
        // same things take about a fifth more here (pointers of 8 bytes,
        // not 4), so the emulator runs out a little before the X4 would.
        heap_budget: Some(256 * KIB),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        // 192 of 800 rows: 11.3 KiB instead of 47 KiB, five bands a frame.
        // The controller takes 8 rows (turned) in a byte.
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        catalog_device: Some("x4"),
        sleep: true,
        button_row: &X4_ROW,
        light: None,
        summary: "ESP32-C3, 4.3\" 800x480, buttons only, 256 KiB heap budget",
    },
    Profile {
        device: DeviceInfo {
            id: "xteink-x4-pro",
            name: "Xteink X4 Pro",
            display: DisplayInfo {
                width: 480,
                height: 800,
                dpi: 217,
                format: PixelFormat::Mono1,
                rotation: Rotation::Deg90,
                color: None,
            },
            input: InputCaps {
                buttons: true,
                touch: true,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        // Previous/next, a touch home key and power (reset is hardware).
        buttons: &[
            buttons::PAGE_BACK,
            buttons::PAGE_FORWARD,
            buttons::HOME,
            buttons::POWER,
        ],
        heap_budget: Some(S3_HEAP),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        // The controller takes 8 rows (turned) in a byte.
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        catalog_device: Some("x4-pro"),
        sleep: true,
        button_row: &[],
        // As the X4 Pro's shell has it: brightness in single percents,
        // cool and warm LEDs mixed in fives.
        light: Some(Light {
            max: 100,
            step: 1,
            percent: true,
            warmth: Some(5),
        }),
        summary: "ESP32-S3 with 8 MB PSRAM, 4.3\" 800x480, touch + buttons, 10 MiB heap budget",
    },
    Profile {
        device: DeviceInfo {
            id: "xteink-x4-classic",
            name: "Xteink X4 Classic",
            display: DisplayInfo {
                width: 480,
                height: 800,
                dpi: 217,
                format: PixelFormat::Mono1,
                rotation: Rotation::Deg90,
                color: None,
            },
            input: InputCaps {
                buttons: true,
                touch: false,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        // The X4's buttons, the X4 Pro's screen and memory.
        buttons: &X4_BUTTONS,
        heap_budget: Some(S3_HEAP),
        glyph_cache: X4_GLYPH_CACHE,
        busy_mark: true,
        colors: false,
        font_packs: true,
        frame_rows: Some(192),
        row_align: 8,
        settings: &X4_SETTINGS,
        speech: false,
        wifi: true,
        catalog_device: Some("x4-classic"),
        sleep: true,
        button_row: &X4_ROW,
        light: None,
        summary: "ESP32-S3 with 8 MB PSRAM, 4.3\" 800x480, buttons only, 10 MiB heap budget",
    },
    Profile {
        device: DeviceInfo {
            id: "boox-go-7",
            name: "Boox Go 7",
            display: DisplayInfo {
                width: 1264,
                height: 1680,
                // As the Android shell draws on gray e-ink.
                dpi: 300,
                format: PixelFormat::Gray8,
                rotation: Rotation::Deg0,
                color: None,
            },
            input: InputCaps {
                buttons: true,
                touch: true,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        // The page buttons and Android's back; the system keeps power.
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::BACK],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: true,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: true,
        wifi: false,
        catalog_device: Some("android"),
        sleep: false,
        button_row: &[],
        // Unverified: Onyx's levels from 0 to 255, warm and cool LEDs;
        // the Android shell takes the device's range, in steps of 1.
        light: Some(Light {
            max: 255,
            step: 1,
            percent: false,
            warmth: Some(1),
        }),
        summary: "Android, 7\" Carta 1300 1264x1680, touch + page buttons",
    },
    Profile {
        device: DeviceInfo {
            id: "kobo-libra-colour",
            name: "Kobo Libra Colour",
            display: DisplayInfo {
                width: 1264,
                height: 1680,
                dpi: 300,
                format: PixelFormat::Rgb888,
                rotation: Rotation::Deg0,
                color: Some(ColorPanel {
                    color_divisor: 2,
                    saturation_percent: 55,
                }),
            },
            input: InputCaps {
                buttons: true,
                touch: true,
                pen: true,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        summary: "Linux, 7\" Kaleido 3 colour 1264x1680, touch + page buttons",
    },
    Profile {
        device: DeviceInfo {
            id: "kindle-paperwhite-5",
            name: "Kindle Paperwhite (11th gen)",
            display: DisplayInfo {
                width: 1236,
                height: 1648,
                dpi: 300,
                format: PixelFormat::Gray4,
                rotation: Rotation::Deg0,
                color: None,
            },
            input: InputCaps {
                buttons: false,
                touch: true,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        buttons: &[buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        summary: "Linux, 6.8\" 1236x1648, touch only",
    },
    Profile {
        device: DeviceInfo {
            id: "kobo-clara-bw",
            name: "Kobo Clara BW",
            display: DisplayInfo {
                width: 1072,
                height: 1448,
                dpi: 300,
                format: PixelFormat::Gray4,
                rotation: Rotation::Deg0,
                color: None,
            },
            input: InputCaps {
                buttons: false,
                touch: true,
                pen: false,
                keys: &[],
            },
            memory: MemoryClass::Large,
        },
        buttons: &[buttons::POWER],
        heap_budget: None,
        glyph_cache: 256 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        settings: &[],
        speech: false,
        wifi: false,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        summary: "Linux, 6\" 1072x1448, touch only",
    },
    Profile {
        device: DeviceInfo {
            id: "two-buttons",
            name: "Two-button e-reader",
            display: DisplayInfo {
                width: 600,
                height: 800,
                dpi: 167,
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
        },
        buttons: &[buttons::PAGE_BACK, buttons::PAGE_FORWARD, buttons::POWER],
        heap_budget: None,
        glyph_cache: 64 * KIB,
        busy_mark: false,
        colors: false,
        font_packs: false,
        frame_rows: None,
        row_align: 1,
        // Only two buttons: everything works with forward and back.
        settings: &[("core.keymap-preset", "core.two-buttons")],
        speech: false,
        wifi: false,
        catalog_device: None,
        sleep: true,
        button_row: &[],
        light: None,
        summary: "Generic 6\" 600x800, only page buttons (also: Bluetooth page turners)",
    },
];

pub fn find(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.device.id == id)
}
