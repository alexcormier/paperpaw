//! Outer power on the ESP32-S3 devices (the X4 Pro and the X4 Classic),
//! whose USB comes through pogo pins, and on the X3, whose pin for it is
//! its I²C bus now: they all use it.

use esp_hal::delay::Delay;
use esp_hal::gpio::Input;

/// Whether the device is on outer power. It has no pin that says so: it
/// counts as on USB while it charges (the charging pin low) or while a
/// computer talks to it (the USB frames' number moves on, a frame a
/// millisecond), so it stays awake and in reach of the computer then.
pub struct UsbPower {
    /// Low while charging; the X3 has none (its fuel gauge knows, but
    /// the input interrupt owns the bus: uncertain without a device
    /// whether a charger alone keeps it awake).
    charging: Option<Input<'static>>,
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
    #[cfg_attr(
        feature = "x3",
        expect(dead_code, reason = "the X3 has no charging pin")
    )]
    pub fn new(charging: Input<'static>) -> Self {
        Self {
            charging: Some(charging),
            seen: core::cell::Cell::new((0, false)),
        }
    }

    /// Without a charging pin: on USB while a computer talks to it (and
    /// for a while after starting).
    #[cfg_attr(feature = "esp32s3", expect(dead_code, reason = "the X3's"))]
    pub fn without_charging() -> Self {
        Self {
            charging: None,
            seen: core::cell::Cell::new((0, false)),
        }
    }

    pub fn is_high(&self) -> bool {
        if self.charging.as_ref().is_some_and(Input::is_low) {
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
