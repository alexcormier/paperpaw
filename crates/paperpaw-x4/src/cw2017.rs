//! The X4 Pro's fuel gauge: a CW2017 on I²C (address 0x63), which knows
//! the battery's charge itself (unlike the X4, whose firmware reads the
//! voltage and guesses).
//!
//! Its registers: the chip's state at 0x00, the cell's voltage at 0x02 (14
//! bits, 312.5 µV each), the charge in percent at 0x04 (and a 256th of a
//! percent at 0x05), the temperature at 0x06 (half degrees from −40 °C),
//! its mode at 0x08, the flag that a profile was given at 0x0B, and the
//! battery's profile (80 bytes) at 0x10. The gauge keeps them only while
//! the battery is connected: cut off (the battery run flat, or taken out),
//! it starts asleep with no profile and measures nothing, all zeros.
//! [`start`] gives it the profile and wakes it, as the stock firmware did.

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;

pub const ADDRESS: u8 = 0x63;

const STATE: u8 = 0x00;
const VOLTAGE: u8 = 0x02;
const MODE: u8 = 0x08;
const SOC_ALERT: u8 = 0x0B;
const PROFILE: u8 = 0x10;
/// In [`SOC_ALERT`]: the profile was given.
const PROFILE_GIVEN: u8 = 0x80;
/// Modes: measuring, restarting, asleep.
const ACTIVE: u8 = 0x00;
const RESTART: u8 = 0x30;
const SLEEP: u8 = 0xF0;
/// How often, 20 ms apart, the gauge is asked whether it runs after a
/// restart.
const STARTS_WAITED: u32 = 50;

/// The X4 Pro's battery as the gauge models it: the profile the stock
/// firmware gives it, as Papyrix (MIT) has it.
pub const X4_PRO_PROFILE: [u8; 80] = [
    0x50, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xaa, 0xbf, 0xb5, 0xb4, 0xa4, 0x9c, 0xeb, 0xe2,
    0xdf, 0xe5, 0xca, 0xa0, 0x8a, 0x62, 0x53, 0x48, 0x40, 0x3a, 0x32, 0xb1, 0xae, 0xda, 0xb5, 0xff,
    0xff, 0xff, 0xe8, 0xdb, 0xd9, 0xd6, 0xd4, 0xd2, 0xd0, 0xcb, 0xc3, 0xbc, 0x9e, 0x87, 0x7b, 0x71,
    0x72, 0x7c, 0x8c, 0xa3, 0xb7, 0xc8, 0xa5, 0x4f, 0x00, 0x00, 0xab, 0x02, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x23,
];

/// What the gauge says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub percent: u8,
    pub millivolts: u16,
}

/// What [`start`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// It was running with the profile already.
    Running,
    /// It was asleep, or had another profile: given this one and woken.
    Woken,
}

/// Reads the voltage and the charge.
pub fn read<I: I2c>(i2c: &mut I) -> Option<Reading> {
    let mut raw = [0u8; 3];
    i2c.write_read(ADDRESS, &[VOLTAGE], &mut raw).ok()?;
    Some(parse(raw))
}

/// Makes the gauge measure with `profile`: where it is asleep or holds
/// another profile, writes this one and restarts it, then waits up to a
/// second until it runs. `None` when it does not answer, or does not run.
pub fn start<I: I2c>(i2c: &mut I, profile: &[u8; 80], delay: &mut impl DelayNs) -> Option<Started> {
    let mode = register(i2c, MODE)?;
    let given = register(i2c, SOC_ALERT)? & PROFILE_GIVEN != 0;
    let mut held = [0u8; 80];
    i2c.write_read(ADDRESS, &[PROFILE], &mut held).ok()?;
    let same = given && held == *profile;
    if same && mode == ACTIVE && running(register(i2c, STATE)?) {
        return Some(Started::Running);
    }
    if !same {
        let mut write = [0u8; 81];
        write[0] = PROFILE;
        write[1..].copy_from_slice(profile);
        i2c.write(ADDRESS, &write).ok()?;
        i2c.write(ADDRESS, &[SOC_ALERT, PROFILE_GIVEN]).ok()?;
    }
    for mode in [SLEEP, RESTART, ACTIVE] {
        i2c.write(ADDRESS, &[MODE, mode]).ok()?;
        delay.delay_ms(20);
    }
    for _ in 0..STARTS_WAITED {
        if register(i2c, STATE).is_some_and(running) {
            return Some(Started::Woken);
        }
        delay.delay_ms(20);
    }
    None
}

/// Whether the chip's state says it measures (0x0D or 0x0F; asleep it
/// says 0xA0).
fn running(state: u8) -> bool {
    state & 0xFD == 0x0D
}

fn register<I: I2c>(i2c: &mut I, at: u8) -> Option<u8> {
    let mut value = [0u8];
    i2c.write_read(ADDRESS, &[at], &mut value).ok()?;
    Some(value[0])
}

/// The reading from registers 0x02 to 0x04.
fn parse(raw: [u8; 3]) -> Reading {
    let cell = u32::from(u16::from_be_bytes([raw[0], raw[1]]) & 0x3FFF);
    Reading {
        percent: raw[2].min(100),
        // 312.5 µV each.
        millivolts: (cell * 5 / 16) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::i2c::{ErrorType, Operation};

    #[test]
    fn readings_are_understood() {
        // As read from an X4 Pro on its charger.
        let r = parse([0x36, 0x37, 0x64]);
        assert_eq!(r.percent, 100);
        assert_eq!(r.millivolts, 4337);
    }

    /// A gauge: its registers, which it reads and writes from the address
    /// written first, as the CW2017 does. Restarted to active with a
    /// profile, it runs.
    struct Gauge {
        registers: [u8; 256],
        at: usize,
        writes: usize,
    }

    impl Gauge {
        /// As the X4 Pro's was after its battery was cut: asleep, empty.
        fn cut_off() -> Self {
            let mut registers = [0u8; 256];
            registers[usize::from(STATE)] = 0xA0;
            registers[usize::from(MODE)] = SLEEP;
            Self {
                registers,
                at: 0,
                writes: 0,
            }
        }
    }

    impl ErrorType for Gauge {
        type Error = Infallible;
    }

    impl I2c for Gauge {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Infallible> {
            assert_eq!(address, ADDRESS);
            for operation in operations {
                match operation {
                    Operation::Write(bytes) => {
                        self.at = usize::from(bytes[0]);
                        for (i, b) in bytes[1..].iter().enumerate() {
                            self.registers[self.at + i] = *b;
                            self.writes += 1;
                        }
                        if bytes.len() > 1 && self.at == usize::from(MODE) {
                            let ready = self.registers[usize::from(SOC_ALERT)] & PROFILE_GIVEN != 0;
                            let state = &mut self.registers[usize::from(STATE)];
                            *state = if bytes[1] == ACTIVE && ready {
                                0x0D
                            } else {
                                0xA0
                            };
                        }
                    }
                    Operation::Read(into) => {
                        for (i, b) in into.iter_mut().enumerate() {
                            *b = self.registers[self.at + i];
                        }
                    }
                }
            }
            Ok(())
        }
    }

    struct NoWait;

    impl DelayNs for NoWait {
        fn delay_ns(&mut self, _: u32) {}
    }

    #[test]
    fn a_gauge_cut_off_gets_its_profile_and_wakes() {
        let mut gauge = Gauge::cut_off();
        assert_eq!(
            read(&mut gauge).unwrap().millivolts,
            0,
            "asleep, it measures nothing"
        );
        assert_eq!(
            start(&mut gauge, &X4_PRO_PROFILE, &mut NoWait),
            Some(Started::Woken)
        );
        assert_eq!(&gauge.registers[0x10..0x60], &X4_PRO_PROFILE[..]);
        assert_eq!(gauge.registers[usize::from(MODE)], ACTIVE);
        // Running with it, it is left alone: nothing written.
        let writes = gauge.writes;
        assert_eq!(
            start(&mut gauge, &X4_PRO_PROFILE, &mut NoWait),
            Some(Started::Running)
        );
        assert_eq!(gauge.writes, writes);
    }

    #[test]
    fn a_gauge_that_never_runs_is_reported() {
        let mut stubborn = StubbornGauge(Gauge::cut_off());
        assert_eq!(start(&mut stubborn, &X4_PRO_PROFILE, &mut NoWait), None);
    }

    /// A gauge whose state never leaves asleep.
    struct StubbornGauge(Gauge);

    impl ErrorType for StubbornGauge {
        type Error = Infallible;
    }

    impl I2c for StubbornGauge {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Infallible> {
            self.0.transaction(address, operations)?;
            self.0.registers[usize::from(STATE)] = 0xA0;
            Ok(())
        }
    }
}
