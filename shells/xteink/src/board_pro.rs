//! The X4 Pro's board: which of the ESP32-S3's pins and units does what,
//! the one place that says so (as `board.rs` for the X4). Checked on the
//! device; the community's Papyrix firmware documents them alike.
//!
//! | Part | Connection |
//! |---|---|
//! | 4.3" panel, 800 × 480, UC8179 controller | SPI: clock GPIO 12, data out GPIO 11, select GPIO 13; data/command GPIO 18, reset GPIO 14, busy GPIO 6 (low while busy) |
//! | microSD card | SDMMC, one data line: clock GPIO 41, command GPIO 42, data GPIO 40; its power GPIO 5, on while low |
//! | Touch (GT911), fuel gauge (CW2017), clock (BM8563) | I²C: data GPIO 39, clock GPIO 38; the touch's interrupt GPIO 10, reset GPIO 4, power GPIO 2 (on while low) |
//! | Side buttons (up, down) | GPIO 0 and GPIO 7, low while pressed |
//! | Power button | GPIO 3, low while pressed |
//! | Charging | GPIO 21 |
//! | Front light | GPIO 8 (cool) and GPIO 9 (warm), by PWM |
//! | The battery's power | held on by GPIO 1, cut when it goes low |
//!
//! It has 8 MB of PSRAM besides its 512 KB of RAM (`main` makes it the
//! heap).

use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Flex, Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals::{FLASH, GPIO40, GPIO41, GPIO42, LPWR, Peripherals, RTC_TIMER, SDHOST};
use esp_hal::spi::Mode;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use quireink_x4::buttons::State;
use quireink_x4::gt911::{self, Transform};
use quireink_x4::uc8179::Display;
use quireink_x4::{bm8563, cw2017};

use crate::input::Touch;

/// The panel's SPI speed.
const PANEL_MHZ: u32 = 20;
/// The I²C bus's speed: as the touch controller wants it.
const I2C_KHZ: u32 = 100;

/// The panel, on its own SPI bus.
pub type Panel = Display<
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

/// What the input interrupt reads: the side buttons, and on the I²C bus
/// the touch screen and the fuel gauge.
pub struct Keys {
    pub up: Input<'static>,
    pub down: Input<'static>,
    pub i2c: I2c<'static, Blocking>,
    /// High while the touch controller has a report (kept as an input; the
    /// controller is asked by time).
    #[expect(dead_code, reason = "held so the line stays an input")]
    pub touch_interrupt: Flex<'static>,
    /// The touch controller's frame, once it answered.
    touch_frame: Option<(u16, u16)>,
    /// The finger followed: its id and where it was (on the screen).
    finger: Option<(u8, u16, u16)>,
    /// Readings since the touch controller was last asked.
    since_touch: u8,
    /// The home key (a touch key below the screen) is down.
    home: bool,
}

/// The touch controller is asked every so many readings (5 ms each).
const TOUCH_EVERY: u8 = 2;

/// How the touch controller's frame (480 × 800, as the screen) lies on the
/// upright screen.
const TOUCH: Transform = Transform {
    from: (TOUCH_WIDTH, TOUCH_HEIGHT),
    to: (quireink_x4::uc8179::WIDTH, quireink_x4::uc8179::HEIGHT),
    swap: false,
    mirror_x: false,
    mirror_y: false,
};
const TOUCH_WIDTH: u16 = 480;
const TOUCH_HEIGHT: u16 = 800;

impl Keys {
    /// The battery's charge (percent) and voltage (millivolts), from the
    /// fuel gauge.
    pub fn battery(&mut self) -> Option<(u8, u16)> {
        cw2017::read(&mut self.i2c).map(|r| (r.percent, r.millivolts))
    }

    /// The side buttons and the power button down (`power_down`); no
    /// readings to look into.
    pub fn read(&mut self, power_down: bool) -> (State, u32) {
        (
            State::pins(self.up.is_low(), self.down.is_low(), power_down),
            0,
        )
    }

    /// Sets the clock chip, which keeps the time while the X4 Pro is off.
    pub fn set_clock(&mut self, unix_ms: i64) {
        let _ = bm8563::write(&mut self.i2c, &bm8563::Time::from_unix_ms(unix_ms));
    }

    /// Where a finger went down, moved or was lifted since the last report,
    /// for `push`. One finger is followed: the first.
    pub fn touch(&mut self, push: &mut dyn FnMut(Touch)) {
        self.since_touch += 1;
        if self.since_touch < TOUCH_EVERY {
            return;
        }
        self.since_touch = 0;
        if self.touch_frame.is_none() {
            self.touch_frame = gt911::probe(&mut self.i2c).ok();
            if self.touch_frame.is_none() {
                return;
            }
        }
        let mut points = [gt911::Point { id: 0, x: 0, y: 0 }; gt911::MAX_POINTS];
        let Ok(Some(report)) = gt911::touches(&mut self.i2c, &mut points) else {
            return;
        };
        if report.key != self.home {
            self.home = report.key;
            push(Touch::Home(report.key));
        }
        let first = (report.count > 0).then(|| points[0]);
        match (self.finger, first) {
            (Some((_, x, y)), None) => {
                self.finger = None;
                push(Touch::Up(x, y));
            }
            (finger, Some(p)) => {
                let (x, y) = TOUCH.apply(p.x, p.y);
                match finger {
                    None => push(Touch::Down(x, y, (p.x, p.y))),
                    Some((id, fx, fy)) if id == p.id && (fx, fy) != (x, y) => {
                        push(Touch::Move(x, y))
                    }
                    Some((id, fx, fy)) if id != p.id => {
                        // Another finger: the first was lifted.
                        push(Touch::Up(fx, fy));
                        push(Touch::Down(x, y, (p.x, p.y)));
                    }
                    Some(_) => {}
                }
                self.finger = Some((p.id, x, y));
            }
            (None, None) => {}
        }
    }
}

/// Whether the X4 Pro is on outer power. It has no pin that says so: it
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
/// After starting, the X4 Pro counts as on USB this long: a computer
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

/// The X4 Pro's parts, each once.
pub struct Board {
    /// Holds the battery's power on; low, it is cut.
    pub power_latch: Output<'static>,
    /// The panel, still to be started.
    pub panel: Panel,
    pub card: CardParts,
    pub keys: Keys,
    pub power_button: Input<'static>,
    pub usb_power: UsbPower,
    /// The front light, off.
    pub light: Option<crate::light::Light>,
    /// The time the clock chip kept while the X4 Pro was off (ms since
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
    /// Takes the chip's parts apart. The battery's power is held at once:
    /// it stays on only while the power button is down otherwise.
    #[expect(
        clippy::expect_used,
        reason = "without its buses the X4 Pro can do nothing; a panic shows why"
    )]
    pub fn take(p: Peripherals) -> Self {
        let mut power_latch = Output::new(p.GPIO1, Level::High, OutputConfig::default());
        // Held through resets: flashing restarts the chip into its boot
        // loader, which would let the power go; held, the X4 Pro stays on
        // and needs no button held while it is flashed.
        power_latch.set_pad_hold(true);
        let delay = Delay::new();

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
        let panel = Display::new(
            ExclusiveDevice::new(spi, select, delay).expect("display SPI"),
            Output::new(p.GPIO18, Level::High, OutputConfig::default()),
            Output::new(p.GPIO14, Level::High, OutputConfig::default()),
            Input::new(p.GPIO6, InputConfig::default()),
            delay,
        );

        // The touch controller: powered (pins keep their level once their
        // driver is gone), then reset with its interrupt line low, which
        // sets its address to 0x5D; the line is its output after.
        let _ = Output::new(p.GPIO2, Level::Low, OutputConfig::default());
        delay.delay_millis(50);
        let mut reset = Output::new(p.GPIO4, Level::Low, OutputConfig::default());
        let mut touch_interrupt = Flex::new(p.GPIO10);
        touch_interrupt.set_low();
        touch_interrupt.set_output_enable(true);
        delay.delay_millis(10);
        reset.set_high();
        delay.delay_millis(60);
        touch_interrupt.set_output_enable(false);
        touch_interrupt.set_input_enable(true);
        delay.delay_millis(50);
        let i2c = I2c::new(
            p.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(I2C_KHZ)),
        )
        .expect("I2C")
        .with_sda(p.GPIO39)
        .with_scl(p.GPIO38);
        let mut i2c = i2c;
        let clock_ms = bm8563::read(&mut i2c).map(|t| t.unix_ms());

        // The card's power, on (low) for good.
        let _ = Output::new(p.GPIO5, Level::Low, OutputConfig::default());
        let pulled = InputConfig::default().with_pull(Pull::Up);
        Self {
            power_latch,
            panel,
            card: CardParts {
                host: p.SDHOST,
                clock: p.GPIO41,
                command: p.GPIO42,
                data: p.GPIO40,
                delay,
            },
            keys: Keys {
                up: Input::new(p.GPIO0, pulled),
                down: Input::new(p.GPIO7, pulled),
                i2c,
                touch_interrupt,
                touch_frame: None,
                finger: None,
                since_touch: 0,
                home: false,
            },
            power_button: Input::new(p.GPIO3, pulled),
            light: crate::light::Light::new(p.LEDC, p.GPIO8, p.GPIO9),
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
