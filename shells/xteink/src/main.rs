//! PaperPaw on the Xteink X4 (ESP32-C3), the X4 Pro (ESP32-S3, with a
//! touch screen) and the X4 Classic (ESP32-S3, the X4's buttons): the
//! kernel with the standard app, its screen, buttons, battery and SD card.
//! The feature `x4`, `x4-pro` or `x4-classic` says which.
//!
//! - **Board**: which pin does what, in one place (`board`, for the X4 Pro
//!   `board_pro`, for the X4 Classic `board_classic`; the card of both
//!   `card_pro`).
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
#[cfg(feature = "x4-classic")]
#[path = "board_classic.rs"]
mod board;
#[cfg(feature = "x4")]
mod card;
#[cfg(feature = "esp32s3")]
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
mod probe;
mod provision;
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
mod usb_lines;
#[cfg(feature = "esp32s3")]
mod usb_power;
mod volume;
mod wifi;

#[cfg(any(
    all(feature = "x4", feature = "esp32s3"),
    all(feature = "x4-pro", feature = "x4-classic"),
    all(
        feature = "esp32s3",
        not(any(feature = "x4-pro", feature = "x4-classic"))
    ),
))]
compile_error!("build for one device: the feature `x4`, `x4-pro` or `x4-classic`");

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
#[cfg(not(feature = "x4-pro"))]
use paperpaw_x4::buttons;
#[cfg(feature = "x4")]
use paperpaw_x4::display;
#[cfg(feature = "esp32s3")]
use paperpaw_x4::uc8179 as display;

use crate::board::Board;
use crate::firmware::{Firmware, State};

// PaperPaw's version and which firmware this is, read from updates.
esp_bootloader_esp_idf::esp_app_desc!(
    env!("PAPERPAW_VERSION"),
    ota::PROJECT,
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
/// Until then, from the first moment, the X4 starts again if its start
/// hangs for this long: room for a display that does not answer (each
/// wait for it ends in an error after 10 s).
const START_WATCHDOG_S: u64 = 60;

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

/// The X4 Classic: the X4's buttons, the X4 Pro's screen and memory.
#[cfg(feature = "x4-classic")]
const DEVICE: DeviceInfo = DeviceInfo {
    id: "xteink-x4-classic",
    name: "Xteink X4 Classic",
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
        touch: false,
        pen: false,
        keys: &buttons::ALL,
    },
    memory: MemoryClass::Large,
};

/// Random bytes from the chip's generator: fully random while the
/// radio is on, pseudo-random before (see `esp_hal::rng`).
fn random(buf: &mut [u8]) -> bool {
    esp_hal::rng::Rng::new().read(buf);
    true
}

/// How this start came, from the chip's reason for its reset: a sleep
/// that cut the power ends by the power coming again.
fn started() -> paperpaw_sdk::power::Start {
    use esp_hal::rtc_cntl::SocResetReason;
    use paperpaw_sdk::power::Start;
    match power::last_run().2 {
        Some(SocResetReason::ChipPowerOn) => Start::PowerOn,
        Some(SocResetReason::CoreDeepSleep) => Start::Wake,
        Some(_) => Start::Restart,
        None => Start::Unknown,
    }
}

/// The heap and the stack free now, for diagnostics and plugins
/// (`paperpaw_sdk::memory`).
fn memory() -> Option<paperpaw_sdk::memory::MemoryUse> {
    let (stack_free, stack_size) = stack::left();
    let bytes = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let free = esp_alloc::HEAP.free();
    Some(
        paperpaw_sdk::memory::MemoryUse::new(bytes(free), bytes(free + esp_alloc::HEAP.used()))
            .with_stack(bytes(stack_free), bytes(stack_size)),
    )
}

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
    // `init` stopped the watchdogs: a start that hangs begins again.
    power::watch_start(START_WATCHDOG_S);
    // The PSRAM of the ESP32-S3 devices first, so PaperPaw's memory comes
    // from there and the chip's own stays for the radio and DMA, which
    // need it.
    #[cfg(feature = "esp32s3")]
    // SAFETY: only taken here; the board does not use it.
    esp_alloc::psram_allocator!(
        unsafe { esp_hal::peripherals::PSRAM::steal() },
        esp_hal::psram
    );
    // The heap before the board: the board keeps its bus there.
    esp_alloc::heap_allocator!(size: HEAP_SIZE);
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: RECLAIMED_SIZE);
    esp_println::logger::init_logger_from_env();
    power::note_start();
    let (place, naps, reason) = power::last_run();
    info!("PaperPaw starting (reset: {reason:?}, last at {place}, naps {naps})");
    // Before anything that could fail or hang, the board too: an update
    // on trial that failed is undone here (a restart into the firmware
    // before it). One handle on the flash, for updates and native
    // plugins.
    // SAFETY: only taken here; the board does not use it.
    let flash = unsafe { esp_hal::peripherals::FLASH::steal() };
    let flash: ota::Flash = alloc::rc::Rc::new(core::cell::RefCell::new(
        esp_bootloader_esp_idf::partitions::FlashStorage::new(flash),
    ));
    let ota = ota::Ota::start(flash.clone());
    // Taken only now: an update undone above restarts into the firmware
    // before it, which keeps the message of the update's panic.
    let panicked = crash::take_last();
    if let Some(message) = &panicked {
        warn!("the last run panicked: {message}");
    }
    let board = Board::take(p);

    let storage = card::mount(board.card);
    // What the card keeps under PaperPaw's old name, moved before anything
    // is written to its folder (the kernel does it again, finding nothing).
    paperpaw_sdk::data::take_over_old_name(storage.as_ref());
    // Wi-Fi the web flasher wrote with the firmware: to the card, for the
    // kernel to keep at its start.
    provision::take(&flash, storage.as_ref());
    // For bug reports: on the card, and said over USB.
    if let Some(message) = &panicked {
        crash::keep(storage.as_ref(), message);
    }
    #[cfg(feature = "remote")]
    remote::note_panic(panicked);
    let mut panel = board.panel;
    // For reports from devices with other controllers: what it answered.
    info!(
        "display: {}, driven as {}",
        board.display,
        panel.controller()
    );
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
    // The clock chip of the ESP32-S3 devices kept the time while it was
    // off.
    #[cfg(feature = "esp32s3")]
    if let Some(unix_ms) = board.clock_ms {
        clock::set(now(), unix_ms);
    }
    // Plugins the web flasher wrote behind the image, onto the card first.
    let staged = native::bundle::stage(&flash, storage.as_ref()).unwrap_or_default();
    let code = code_plugins(ota.as_ref(), &flash);
    let kernel = kernel(storage, ota.as_ref(), code, staged);
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
        // Lines from a computer on USB; the log goes out the same way.
        usb: usb_lines::UsbLines::new(board.usb),
        #[cfg(feature = "remote")]
        remote: remote::Remote,
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

/// What this firmware loads, as plugins see it ([`paperpaw_sdk::Env`]),
/// with the plugins of the web flasher not installed.
fn native(
    not_installed: alloc::vec::Vec<paperpaw_sdk::native::NotInstalled>,
) -> paperpaw_sdk::native::Native {
    let mut native = crate::native::this_firmware();
    native.not_installed = not_installed;
    native
}

/// Loads nothing: in safe mode the kernel asks for no code.
struct NoCode;

impl paperpaw_core::CodeLoader for NoCode {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &paperpaw_core::CodeFile<'_>,
    ) -> Result<Box<dyn paperpaw_sdk::Plugin>, paperpaw_sdk::PluginProblem> {
        Err(paperpaw_sdk::PluginProblem::SafeMode)
    }
}

/// Whether code plugins are loaded at this start, after the key for safe
/// mode was looked for and the failed starts counted.
fn code_plugins(ota: Option<&ota::Ota>, flash: &ota::Flash) -> CodePlugins {
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
    #[cfg(feature = "esp32s3")]
    let place = {
        let _ = flash;
        native::Place::default()
    };
    #[cfg(feature = "x4")]
    let place = native::Place::new(flash.clone());
    CodePlugins::Load(native::Loader::new(place))
}

/// The status lines of the X4s by default (see `setting_default_text`):
/// fewer items than on large screens.
const CALM_STATUS: [(&str, &str); 6] = [
    (
        "reader.status.bottom-left",
        "core.status.clock, reader.status.aloud, lock.touch-status",
    ),
    ("reader.status.bottom-center", ""),
    ("reader.status.bottom-right", "reader.status.page"),
    ("home.status.left", "core.status.clock"),
    ("home.status.center", ""),
    (
        "home.status.right",
        "lock.touch-status, core.status.battery",
    ),
];

/// The kernel with the standard app, as the X4 wants it.
#[expect(
    clippy::expect_used,
    reason = "the plugins are built in: a kernel that cannot be built is a bug to see at once"
)]
fn kernel(
    storage: Box<dyn Storage>,
    ota: Option<&ota::Ota>,
    code: CodePlugins,
    staged: native::bundle::Staged,
) -> Kernel {
    #[cfg(feature = "remote")]
    let storage: Box<dyn Storage> = Box::new(traced::Traced(storage));
    let builder = Kernel::builder(DEVICE)
        .storage_boxed(storage)
        .clock(micros)
        // Fully random while the radio is on, as when serving.
        .random(random)
        .memory(memory)
        .started(started())
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
    // The X4's row of hints under the screen (the X4 Classic has its
    // buttons); the X4 Pro is touched.
    #[cfg(feature = "x4")]
    let builder = builder.button_row(&buttons::ROW).catalog_device("x4");
    #[cfg(feature = "x4-pro")]
    let builder = builder.catalog_device("x4-pro");
    #[cfg(feature = "x4-classic")]
    let builder = builder
        .button_row(&buttons::ROW)
        .catalog_device("x4-classic");
    let builder = builder
        // A full refresh takes 3.5 s on its panel: screens change fast.
        .setting_default(&settings::FLASH_SCREENS, false)
        .setting_default(&settings::UI_SIZE, 9)
        .setting_default(&settings::FONT_SIZE, 9);
    // A small screen: calm status lines, the time and the page while
    // reading (and the mark of reading aloud while it reads), the time
    // and the battery elsewhere. Every item stays to be chosen.
    let builder = CALM_STATUS
        .iter()
        .fold(builder, |builder, (id, value)| {
            builder.setting_default_text(id, value)
        })
        // PaperPaw switches the radio (`network`); on USB it stays on in a
        // network joined before, for transfers and for trying things.
        .wifi(true)
        .setting_default(&paperpaw_plugin_wifi::WHILE_CHARGING, true);
    let builder = match ota {
        Some(ota) => builder.firmware(ota.firmware()),
        None => builder,
    };
    let builder = match code {
        CodePlugins::Load(loader) => {
            builder.code_plugins(native(staged.not_installed), Box::new(loader))
        }
        CodePlugins::SafeMode(safe_mode) => builder
            .code_plugins(native(staged.not_installed), Box::new(NoCode))
            .safe_mode(safe_mode),
        CodePlugins::None => builder,
    };
    let builder = staged
        .installs
        .into_iter()
        .fold(builder, |builder, (id, folder)| {
            builder.install_plugin(id, folder)
        });
    // The plugins the store offers as files are not built in: the
    // firmware stays lean, and the reader installs those wanted.
    let builder = paperpaw_app::install_with(
        builder,
        paperpaw_app::Fonts::Packs,
        paperpaw_app::StorePlugins::Left,
    )
    .plugin(device::X4);
    let mut kernel = builder.build().expect("kernel");
    for warning in kernel.take_warnings() {
        warn!("{warning}");
    }
    kernel
}
