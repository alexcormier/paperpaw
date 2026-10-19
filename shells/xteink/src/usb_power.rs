//! Outer power on the ESP32-S3 devices (the X4 Pro and the X4 Classic),
//! whose USB comes through pogo pins: both boards use it.

use esp_hal::delay::Delay;
use esp_hal::gpio::Input;

/// Whether the device is on outer power. It has no pin that says so: it
/// counts as on USB while it charges (the charging pin low) or while a
/// computer talks to it (the USB frames' number moves on, a frame a
/// millisecond), so it stays awake and in reach of the computer then.
pub struct UsbPower {
    charging: Input<'static>,
    /// When the frames were last looked at (ms), and whether they moved.
    seen: core::cell::Cell<(u64, bool)>,
}

/// The frames are looked at once in so many milliseconds.
const LOOK_MS: u64 = 1000;
/// After starting, the device counts as on USB this long: a computer
/// takes a moment to find it and send frames, and in a light sleep it
/// could not (the USB unit sleeps too).
const START_MS: u64 = 8000;

impl UsbPower {
    pub fn new(charging: Input<'static>) -> Self {
        Self {
            charging,
            seen: core::cell::Cell::new((0, false)),
        }
    }

    pub fn is_high(&self) -> bool {
        if self.charging.is_low() {
            return true;
        }
        let now = esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis();
        if now < START_MS {
            return true;
        }
        let (at, talking) = self.seen.get();
        if at != 0 && now - at < LOOK_MS {
            return talking;
        }
        let talking = computer();
        self.seen.set((now, talking));
        talking
    }

    pub fn is_low(&self) -> bool {
        !self.is_high()
    }
}

/// Whether a computer sends USB frames: their number moves on within 3 ms.
fn computer() -> bool {
    let frame = || {
        esp_hal::peripherals::USB_DEVICE::regs()
            .fram_num()
            .read()
            .sof_frame_index()
            .bits()
    };
    let first = frame();
    Delay::new().delay_millis(3);
    frame() != first
}
