//! The X4 Classic's board ("X4 v2", X4C): which of the ESP32-S3's pins
//! and units does what, the one place that says so (as `board.rs` for the
//! X4, `board_pro.rs` for the X4 Pro). It has the X4 Pro's chip, glass,
//! card, gauge and clock, other pins for the panel, no touch and no light,
//! and the X4's seven buttons, each on a pin of its own. From the FreeInk
//! SDK's board table and the Papyrix firmware's notes (both read from the
//! stock firmware); not yet tried on one.
//!
//! | Part | Connection |
//! |---|---|
//! | 4.3" panel, 800 × 480, SSD1677, UC8179 or UC8279 controller (by unit, found at start) | SPI: clock GPIO 12, data out GPIO 11, select GPIO 13; data/command GPIO 14, reset GPIO 10, busy GPIO 18 |
//! | microSD card | SDMMC, one data line: clock GPIO 41, command GPIO 42, data GPIO 40; its power GPIO 6, on while low |
//! | Fuel gauge (CW2017), clock (BM8563) | I²C: data GPIO 39, clock GPIO 38 |
//! | Back, confirm, left, right (below the screen) | GPIO 9, 8, 5 and 2, low while pressed |
//! | Side buttons (up, down) | GPIO 0 and GPIO 7, low while pressed |
//! | Power button | GPIO 3, low while pressed |
//! | Charging | GPIO 21 |
//! | The panel's and the other parts' power | held on by GPIO 1, cut when it goes low |
//!
//! It has 16 MB of flash and 8 MB of PSRAM (`main` makes it the heap);
//! its USB comes through a pogo adapter. A motion sensor (QMI8658, I²C
//! 0x6B) is not used yet.

use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::{FLASH, GPIO40, GPIO41, GPIO42, LPWR, Peripherals, RTC_TIMER, SDHOST};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use paperpaw_x4::buttons::State;
use paperpaw_x4::panel::Device;
use paperpaw_x4::probe::{Controller, Probe};
use paperpaw_x4::{bm8563, cw2017};

use crate::input::Touch;
pub use crate::usb_power::UsbPower;

/// The panel's SPI speed.
const PANEL_MHZ: u32 = 20;
/// The I²C bus's speed, as on the X4 Pro.
const I2C_KHZ: u32 = 100;
/// The card's power is off this long first, as the stock firmware does,
/// so the card starts afresh.
const CARD_OFF_MS: u32 = 80;

/// The panel, on its own SPI bus.
pub type Panel = paperpaw_x4::panel::Panel<
    ExclusiveDevice<Spi<'static, Blocking>, Output<'static>, Delay>,
    Output<'static>,
    Output<'static>,
    Input<'static>,
    Delay,
>;

/// The SD card's parts: its bus (its power is on from the start).
pub struct CardParts {
    pub host: SDHOST<'static>,
    pub clock: GPIO41<'static>,
    pub command: GPIO42<'static>,
    pub data: GPIO40<'static>,
    pub delay: Delay,
}

/// What the input interrupt reads: the buttons, and on the I²C bus the
/// fuel gauge.
pub struct Keys {
    pub back: Input<'static>,
    pub confirm: Input<'static>,
    pub left: Input<'static>,
    pub right: Input<'static>,
    pub up: Input<'static>,
    pub down: Input<'static>,
    pub i2c: I2c<'static, Blocking>,
}

impl Keys {
    /// The battery's charge (percent) and voltage (millivolts), from the
    /// fuel gauge.
    pub fn battery(&mut self) -> Option<(u8, u16)> {
        cw2017::read(&mut self.i2c).map(|r| (r.percent, r.millivolts))
    }

    /// The buttons and the power button down (`power_down`); no readings
    /// to look into.
    pub fn read(&mut self, power_down: bool) -> (State, u32) {
        (
            State::keys([
                self.back.is_low(),
                self.confirm.is_low(),
                self.left.is_low(),
                self.right.is_low(),
                self.up.is_low(),
                self.down.is_low(),
                power_down,
            ]),
            0,
        )
    }

    /// Sets the clock chip, which keeps the time while the X4 Classic is
    /// off.
    pub fn set_clock(&mut self, unix_ms: i64) {
        let _ = bm8563::write(&mut self.i2c, &bm8563::Time::from_unix_ms(unix_ms));
    }

    /// The X4 Classic has no touch screen.
    pub fn touch(&mut self, _: &mut dyn FnMut(Touch)) {}

    /// The X4 Classic has no home key (its Back is a button).
    pub fn home_down(&self) -> bool {
        false
    }
}

/// The X4 Classic's parts, each once.
pub struct Board {
    /// Holds the power on; low, it is cut.
    pub power_latch: Output<'static>,
    /// The panel, still to be started.
    pub panel: Panel,
    /// What its controller answered at start.
    pub display: Probe,
    pub card: CardParts,
    pub keys: Keys,
    pub power_button: Input<'static>,
    pub usb_power: UsbPower,
    /// The time the clock chip kept while the X4 Classic was off (ms since
    /// 1970, UTC), if it kept one.
    pub clock_ms: Option<i64>,
    pub timers: TimerGroup<'static, esp_hal::peripherals::TIMG0<'static>>,
    pub timers1: TimerGroup<'static, esp_hal::peripherals::TIMG1<'static>>,
    pub rtc: RTC_TIMER<'static>,
    pub low_power: LPWR<'static>,
    pub flash: FLASH<'static>,
    #[cfg(feature = "remote")]
    pub usb: esp_hal::peripherals::USB_DEVICE<'static>,
    pub radio: (
        esp_hal::peripherals::WIFI<'static>,
        esp_hal::peripherals::FROM_CPU_INTR0<'static>,
    ),
    pub delay: Delay,
}

impl Board {
    /// Takes the chip's parts apart. The power is held at once, and
    /// through resets, as on the X4 Pro.
    #[expect(
        clippy::expect_used,
        reason = "without its buses the X4 Classic can do nothing; a panic shows why"
    )]
    pub fn take(mut p: Peripherals) -> Self {
        let mut power_latch = Output::new(p.GPIO1, Level::High, OutputConfig::default());
        power_latch.set_pad_hold(true);
        let delay = Delay::new();

        // Which controller the panel has, asked on its lines before the
        // bus takes them; without a sure answer, the UC8179, as on the
        // X4 Pro (the stock firmware drives all three).
        let display = crate::probe::ask(
            p.GPIO12.reborrow(),
            p.GPIO11.reborrow(),
            p.GPIO13.reborrow(),
            p.GPIO14.reborrow(),
            p.GPIO10.reborrow(),
            p.GPIO18.reborrow(),
            delay,
        );
        let spi = Spi::new(
            p.SPI2,
            SpiConfig::default()
                .with_frequency(Rate::from_mhz(PANEL_MHZ))
                .with_mode(Mode::_0),
        )
        .expect("SPI")
        .with_sck(p.GPIO12)
        .with_mosi(p.GPIO11);
        let select = Output::new(p.GPIO13, Level::High, OutputConfig::default());
        // Its glass and memory are the X4 Pro's.
        let panel = Panel::new(
            Device::X4Pro,
            display.controller().unwrap_or(Controller::Uc8179),
            ExclusiveDevice::new(spi, select, delay).expect("display SPI"),
            Output::new(p.GPIO14, Level::High, OutputConfig::default()),
            Output::new(p.GPIO10, Level::High, OutputConfig::default()),
            Input::new(p.GPIO18, InputConfig::default()),
            delay,
        );

        let mut i2c = I2c::new(
            p.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(I2C_KHZ)),
        )
        .expect("I2C")
        .with_sda(p.GPIO39)
        .with_scl(p.GPIO38);
        let clock_ms = bm8563::read(&mut i2c).map(|t| t.unix_ms());

        // The card's power: off a moment, then on (low) for good (pins
        // keep their level once their driver is gone).
        {
            let mut card_power = Output::new(p.GPIO6, Level::High, OutputConfig::default());
            delay.delay_millis(CARD_OFF_MS);
            card_power.set_low();
        }
        let pulled = InputConfig::default().with_pull(Pull::Up);
        Self {
            power_latch,
            panel,
            display,
            card: CardParts {
                host: p.SDHOST,
                clock: p.GPIO41,
                command: p.GPIO42,
                data: p.GPIO40,
                delay,
            },
            keys: Keys {
                back: Input::new(p.GPIO9, pulled),
                confirm: Input::new(p.GPIO8, pulled),
                left: Input::new(p.GPIO5, pulled),
                right: Input::new(p.GPIO2, pulled),
                up: Input::new(p.GPIO0, pulled),
                down: Input::new(p.GPIO7, pulled),
                i2c,
            },
            power_button: Input::new(p.GPIO3, pulled),
            clock_ms,
            usb_power: UsbPower::new(Input::new(p.GPIO21, pulled)),
            timers: TimerGroup::new(p.TIMG0),
            timers1: TimerGroup::new(p.TIMG1),
            rtc: p.RTC_TIMER,
            low_power: p.LPWR,
            flash: p.FLASH,
            #[cfg(feature = "remote")]
            usb: p.USB_DEVICE,
            radio: (p.WIFI, p.FROM_CPU_INTR0),
            delay,
        }
    }
}
