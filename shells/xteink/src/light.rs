//! The X4 Pro's front light: cool LEDs on GPIO 8 and warm ones on GPIO 9,
//! each dimmed by PWM (the LEDC unit: 10 kHz, 10 bits). The settings
//! `x4.frontlight` (how bright) and `x4.warmth` (how much of it warm,
//! see `device`) are shown; the eye sees brightness about as the square
//! of the duty, so the levels follow a curve, and the lowest step is
//! still very dim.

use alloc::boxed::Box;

use esp_hal::gpio::DriveMode;
use esp_hal::ledc::channel::{self, ChannelHW, ChannelIFace};
use esp_hal::ledc::timer::{self, TimerIFace};
use esp_hal::ledc::{LSGlobalClkSource, Ledc, LowSpeed};
use esp_hal::peripherals::{GPIO8, GPIO9, LEDC};
use esp_hal::time::Rate;

/// The PWM's resolution: duty from 0 to 1023.
const MAX_DUTY: u32 = 1023;

pub struct Light {
    cool: channel::Channel<'static, LowSpeed>,
    warm: channel::Channel<'static, LowSpeed>,
    /// The brightness and warmth shown, in percent.
    shown: Option<(u8, u8)>,
}

impl Light {
    /// The light, off; `None` if the PWM unit cannot be set up.
    pub fn new(ledc: LEDC<'static>, cool: GPIO8<'static>, warm: GPIO9<'static>) -> Option<Self> {
        let mut ledc = Ledc::new(ledc);
        ledc.set_global_slow_clock(LSGlobalClkSource::APBClk);
        let ledc: &'static Ledc<'static> = Box::leak(Box::new(ledc));
        let mut pwm = ledc.timer::<LowSpeed>(timer::Number::Timer0);
        pwm.configure(timer::config::Config {
            duty: timer::config::Duty::Duty10Bit,
            clock_source: timer::LSClockSource::APBClk,
            frequency: Rate::from_khz(10),
        })
        .ok()?;
        let pwm: &'static timer::Timer<'static, LowSpeed> = Box::leak(Box::new(pwm));
        let channel = |number, pin: esp_hal::gpio::AnyPin<'static>| {
            let mut c = ledc.channel(number, pin);
            c.configure(channel::config::Config {
                timer: pwm,
                duty_pct: 0,
                drive_mode: DriveMode::PushPull,
            })
            .ok()
            .map(|()| c)
        };
        Some(Self {
            cool: channel(channel::Number::Channel0, cool.into())?,
            warm: channel(channel::Number::Channel1, warm.into())?,
            shown: None,
        })
    }

    /// Shows `level` percent of brightness, `warmth` percent of it warm.
    pub fn show(&mut self, level: i64, warmth: i64) {
        let level = level.clamp(0, 100) as u8;
        let warmth = warmth.clamp(0, 100) as u8;
        if self.shown == Some((level, warmth)) {
            return;
        }
        self.shown = Some((level, warmth));
        let total = duty(level);
        let warm = total * u32::from(warmth) / 100;
        self.warm.set_duty_hw(warm);
        self.cool.set_duty_hw(total - warm);
    }

    /// Off, e.g. asleep; [`Light::show`] brings it back.
    pub fn off(&mut self) {
        self.shown = None;
        self.cool.set_duty_hw(0);
        self.warm.set_duty_hw(0);
    }
}

/// The duty for `level` percent: the square, so low levels stay fine; at
/// least 1 when on.
fn duty(level: u8) -> u32 {
    if level == 0 {
        return 0;
    }
    let level = u32::from(level);
    (MAX_DUTY * level * level / 10_000).max(1)
}
