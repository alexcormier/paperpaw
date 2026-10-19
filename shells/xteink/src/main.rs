//! PaperPaw on the Xteink X4 (ESP32-C3) and the X4 Pro (ESP32-S3, with a
//! touch screen): the kernel with the standard app, its screen, buttons,
//! battery and SD card. The feature `x4` or `x4-pro` says which.
//!
//! - **Board**: which pin does what, in one place (`board`, for the X4 Pro
//!   `board_pro`; its card `card_pro`).
//! - **Screen**: the kernel draws in bands of [`FRAME_ROWS`] rows; each
//!   goes straight into the display controller's memory, which keeps the
//!   frame (see `paperpaw_x4::display`).
//! - **Buttons**: read in a timer's interrupt and debounced, so presses
//!   count while PaperPaw is busy (`input`, `paperpaw_x4::buttons`).
//! - **Storage**: the SD card's FAT file system, with long file names
//!   (`paperpaw_fat`). Without a card, PaperPaw starts with nothing to
//!   read.
//! - **Sleep**: the sleep screen stays shown; the battery's power is cut,
//!   and the power button starts the X4 again, where it was.
//! - **Main loop**: `firmware`; remote control over USB for testing
//!   (`remote`) with the `remote` feature, on by default.

#![no_std]
#![no_main]

extern crate alloc;

#[cfg(feature = "x4")]
mod board;
#[cfg(feature = "x4-pro")]
#[path = "board_pro.rs"]
mod board;
#[cfg(feature = "x4")]
mod card;
#[cfg(feature = "x4-pro")]
#[path = "card_pro.rs"]
mod card;
mod clock;
mod crash;
mod device;
mod firmware;
#[cfg(feature = "remote")]
mod gray_test;
mod http;
mod input;
#[cfg(feature = "x4-pro")]
mod light;
mod link;
mod native;
mod net;
mod network;
mod ota;
mod power;
#[cfg(feature = "remote")]
mod remote;
mod roots;
mod safe_mode;
mod serve;
mod sockets;
mod stack;
mod tls;
#[cfg(feature = "remote")]
mod traced;
mod volume;
mod wifi;

#[cfg(all(feature = "x4", feature = "x4-pro"))]
compile_error!("build for one device: the feature `x4` or `x4-pro`");

use alloc::boxed::Box;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::main;
use esp_hal::rtc_cntl::sleep::LowPower;
use esp_hal::timer::PeriodicTimer;
use log::{info, warn};
use paperpaw_core::Kernel;
#[cfg(feature = "x4-pro")]
use paperpaw_platform::buttons as names;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, Storage,
};
use paperpaw_sdk::builtin::settings;
#[cfg(feature = "x4")]
use paperpaw_x4::buttons;
#[cfg(feature = "x4")]
use paperpaw_x4::display;
#[cfg(feature = "x4-pro")]
use paperpaw_x4::uc8179 as display;

use crate::board::Board;
use crate::firmware::{Firmware, State};

// PaperPaw's version and which firmware this is, read from updates.
esp_bootloader_esp_idf::esp_app_desc!(
    env!("PAPERPAW_VERSION"),
    "paperpaw-xteink",
    esp_bootloader_esp_idf::BUILD_TIME,
    esp_bootloader_esp_idf::BUILD_DATE,
    esp_bootloader_esp_idf::ESP_IDF_COMPATIBLE_VERSION,
    esp_bootloader_esp_idf::MMU_PAGE_SIZE,
    0,
    u16::MAX,
    esp_bootloader_esp_idf::SECURE_VERSION
);

/// Heap for PaperPaw: what the emulator's X4 profile allows.
const HEAP_SIZE: usize = 192 * 1024;
/// More heap in the memory the bootloader used (65 KB are free after it):
/// room for the frame kept for the panel (see `paperpaw_x4::sent`).
const RECLAIMED_SIZE: usize = 64 * 1024;
/// Rows the kernel draws at once: 11.3 KiB of black and white. Fewer
/// bands draw faster: each costs the screens a pass over what they show.
const FRAME_ROWS: u16 = 192;
/// The X4 starts again if its main loop does not come by for this long
/// (the longest work, a book's first opening, takes some 5 s): by the
/// RTC's watchdog (counting in light sleep too), and the timer group's.
const WATCHDOG_S: (u64, u64) = (20, 30);

#[cfg(feature = "x4")]
const DEVICE: DeviceInfo = DeviceInfo {
    id: "xteink-x4",
    name: "Xteink X4",
    display: DisplayInfo {
        width: display::WIDTH,
        height: display::HEIGHT,
        dpi: 220,
        // What the panel shows until its four grays come: drawing in gray
        // only to send black and white costs time and memory.
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg90,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &buttons::ALL,
    },
    memory: MemoryClass::Small,
};

/// The X4 Pro: touch besides its page and power buttons, and the memory
/// of larger devices (8 MB of PSRAM).
#[cfg(feature = "x4-pro")]
const DEVICE: DeviceInfo = DeviceInfo {
    id: "xteink-x4-pro",
    name: "Xteink X4 Pro",
    display: DisplayInfo {
        width: display::WIDTH,
        height: display::HEIGHT,
        dpi: 217,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg90,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &[
            names::PAGE_BACK,
            names::PAGE_FORWARD,
            names::HOME,
            names::POWER,
        ],
    },
    memory: MemoryClass::Large,
};

/// Microseconds since start, for measuring what plugins take.
pub fn micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

fn now() -> Instant {
    Instant(
        esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis(),
    )
}

#[main]
fn main() -> ! {
    stack::paint();
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    // The X4 Pro's PSRAM first, so PaperPaw's memory comes from there and
    // the chip's own stays for the radio and DMA, which need it.
    #[cfg(feature = "x4-pro")]
    // SAFETY: only taken here; the board does not use it.
    esp_alloc::psram_allocator!(
        unsafe { esp_hal::peripherals::PSRAM::steal() },
        esp_hal::psram
    );
    // The heap before the board: the board keeps its bus there.
    esp_alloc::heap_allocator!(size: HEAP_SIZE);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: RECLAIMED_SIZE);
    let board = Board::take(p);
    esp_println::logger::init_logger_from_env();
    power::note_start();
    let panicked = crash::take_last();
    let (place, naps, reason) = power::last_run();
    info!("PaperPaw starting (reset: {reason:?}, last at {place}, naps {naps})");
    if let Some(message) = &panicked {
        warn!("the last run panicked: {message}");
    }
    // Before anything else: an update on trial that failed is undone here
    // (a restart into the firmware before it).
    let ota = ota::Ota::start(board.flash);

    let storage = card::mount(board.card);
    // For bug reports: on the card, and said over USB.
    if let Some(message) = &panicked {
        crash::keep(storage.as_ref(), message);
    }
    #[cfg(feature = "remote")]
    remote::note_panic(panicked);
    let mut panel = board.panel;
    if let Err(e) = panel.init() {
        warn!("display: {e:?}");
    }
    power::watch(
        esp_hal::rtc_cntl::Rtc::new(board.rtc),
        board.timers1.wdt,
        WATCHDOG_S,
    );
    // The radio (on when the kernel says so) and what goes over it.
    let network = network::Network::new(wifi::Parts {
        timer: board.timers1.timer0,
        interrupt: board.radio.1,
        wifi: board.radio.0,
    });
    input::start(input::Sampler::new(
        PeriodicTimer::new(board.timers.timer0),
        board.keys,
        board.power_button,
    ));
    // The X4 Pro's clock chip kept the time while it was off.
    #[cfg(feature = "x4-pro")]
    if let Some(unix_ms) = board.clock_ms {
        clock::set(now(), unix_ms);
    }
    let code = code_plugins(ota.as_ref());
    let kernel = kernel(storage, ota.as_ref(), code);
    // Light sleeps while idle on battery, the deep one at the end.
    power::init(LowPower::new(board.low_power));
    Firmware {
        kernel,
        panel,
        network,
        ota,
        power_latch: board.power_latch,
        usb_power: board.usb_power,
        delay: board.delay,
        // Commands from a computer on USB; the log goes out the same way.
        #[cfg(feature = "remote")]
        remote: remote::Remote::new(board.usb),
        #[cfg(feature = "x4-pro")]
        light: board.light,
        state: State::default(),
    }
    .run()
}

/// How this start treats code plugins (ADR 0024).
enum CodePlugins {
    Load(native::Loader),
    /// Safe mode: none loaded.
    SafeMode(paperpaw_sdk::native::SafeMode),
    /// None this start: an update is on trial, which must show it starts
    /// by itself.
    None,
}

/// What this firmware loads, as plugins see it ([`paperpaw_sdk::Env`]).
fn native() -> paperpaw_sdk::native::Native {
    paperpaw_sdk::native::Native::new("code.ppn")
}

/// Loads nothing: in safe mode the kernel asks for no code.
struct NoCode;

impl paperpaw_core::CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &str,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::SafeMode)
    }
}

/// Whether code plugins are loaded at this start, after the key for safe
/// mode was looked for and the failed starts counted.
fn code_plugins(ota: Option<&ota::Ota>) -> CodePlugins {
    if ota.is_some_and(ota::Ota::on_trial) {
        info!("native: an update on trial starts without code plugins");
        return CodePlugins::None;
    }
    if let Some(safe_mode) = safe_mode::check(input::held(safe_mode::KEY)) {
        warn!(
            "native: safe mode ({:?}), without code plugins",
            safe_mode.reason
        );
        return CodePlugins::SafeMode(safe_mode);
    }
    #[cfg(feature = "x4-pro")]
    let place = native::Place::default();
    // SAFETY: the loader uses the flash only while the kernel is built,
    // before an update can be written (the main loop's).
    #[cfg(feature = "x4")]
    let place = native::Place::new(unsafe { esp_hal::peripherals::FLASH::steal() });
    CodePlugins::Load(native::Loader::new(place))
}

/// The kernel with the standard app, as the X4 wants it.
#[expect(
    clippy::expect_used,
    reason = "the plugins are built in: a kernel that cannot be built is a bug to see at once"
)]
fn kernel(storage: Box<dyn Storage>, ota: Option<&ota::Ota>, code: CodePlugins) -> Kernel {
    #[cfg(feature = "remote")]
    let storage: Box<dyn Storage> = Box::new(traced::Traced(storage));
    let builder = Kernel::builder(DEVICE)
        .storage_boxed(storage)
        .clock(micros)
        // The panel refreshes on its own: a mark shows at once while slow
        // work goes on.
        .busy_mark(true)
        // Built-in packs are read in place; glyphs of packs on the card
        // (other scripts) are kept here once read. Empty until one is.
        .glyph_cache(12 * 1024)
        .frame_rows(FRAME_ROWS)
        .row_align(8)
        // The panel takes bands turned any way (`display`).
        .rotation(true)
        .sleep(true);
    // The X4's row of hints under the screen; the X4 Pro is touched.
    #[cfg(feature = "x4")]
    let builder = builder.button_row(&buttons::ROW);
    let builder = builder
        // A full refresh takes 3.5 s on its panel: screens change fast.
        .setting_default(&settings::FLASH_SCREENS, false)
        .setting_default(&settings::UI_SIZE, 9)
        .setting_default(&settings::FONT_SIZE, 9)
        // PaperPaw switches the radio (`network`); on USB it stays on in a
        // network joined before, for transfers and for trying things.
        .wifi(true)
        .setting_default(&paperpaw_plugin_wifi::WHILE_CHARGING, true);
    let builder = match ota {
        Some(ota) => builder.firmware(ota.firmware()),
        None => builder,
    };
    let builder = match code {
        CodePlugins::Load(loader) => builder.code_plugins(native(), Box::new(loader)),
        CodePlugins::SafeMode(safe_mode) => builder
            .code_plugins(native(), Box::new(NoCode))
            .safe_mode(safe_mode),
        CodePlugins::None => builder,
    };
    let builder = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs).plugin(device::X4);
    let mut kernel = builder.build().expect("kernel");
    for warning in kernel.take_warnings() {
        warn!("{warning}");
    }
    kernel
}
