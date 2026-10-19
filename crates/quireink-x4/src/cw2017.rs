//! The X4 Pro's fuel gauge: a CW2017 on I²C (address 0x63), which knows
//! the battery's charge itself (unlike the X4, whose firmware reads the
//! voltage and guesses).
//!
//! Its registers: the cell's voltage at 0x02 (14 bits, 312.5 µV each), the
//! charge in percent at 0x04 (and a 256th of a percent at 0x05), the
//! temperature at 0x06 (half degrees from −40 °C). The battery's profile
//! stays in the gauge while the battery is connected; the stock firmware
//! wrote it.

use embedded_hal::i2c::I2c;

pub const ADDRESS: u8 = 0x63;

const VOLTAGE: u8 = 0x02;

/// What the gauge says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub percent: u8,
    pub millivolts: u16,
}

/// Reads the voltage and the charge.
pub fn read<I: I2c>(i2c: &mut I) -> Option<Reading> {
    let mut raw = [0u8; 3];
    i2c.write_read(ADDRESS, &[VOLTAGE], &mut raw).ok()?;
    Some(parse(raw))
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

    #[test]
    fn readings_are_understood() {
        // As read from an X4 Pro on its charger.
        let r = parse([0x36, 0x37, 0x64]);
        assert_eq!(r.percent, 100);
        assert_eq!(r.millivolts, 4337);
    }
}
