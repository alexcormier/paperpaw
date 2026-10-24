//! The X3's fuel gauge: a BQ27220 on I²C (address 0x55), which knows the
//! battery's charge itself, as the X4 Pro's CW2017 does. It answers TI's
//! standard commands, each a little-endian 16-bit register: the voltage
//! at 0x08 (millivolts), the average current at 0x0C (signed
//! milliamperes, positive while the battery charges) and the charge at
//! 0x2C (percent). The X3 has no pin that says it is on USB: charging is
//! a current into the battery, as the FreeInk SDK (MIT) reads it
//! (`BatteryMonitor.cpp`). Uncertain without a device: whether it counts
//! as on USB once the charge stops at 100 % (no current then).

use embedded_hal::i2c::I2c;

pub const ADDRESS: u8 = 0x55;

const VOLTAGE: u8 = 0x08;
const CURRENT: u8 = 0x0C;
const STATE_OF_CHARGE: u8 = 0x2C;

/// What the gauge says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub percent: u8,
    pub millivolts: u16,
}

fn register<I: I2c>(i2c: &mut I, command: u8) -> Option<u16> {
    let mut raw = [0u8; 2];
    i2c.write_read(ADDRESS, &[command], &mut raw).ok()?;
    Some(u16::from_le_bytes(raw))
}

/// Reads the voltage and the charge; `None` where the gauge does not
/// answer, or answers with what no battery says (a voltage of 0).
pub fn read<I: I2c>(i2c: &mut I) -> Option<Reading> {
    let millivolts = register(i2c, VOLTAGE)?;
    let percent = register(i2c, STATE_OF_CHARGE)?;
    reading(millivolts, percent)
}

fn reading(millivolts: u16, percent: u16) -> Option<Reading> {
    (millivolts > 0).then(|| Reading {
        percent: percent.min(100) as u8,
        millivolts,
    })
}

/// Whether the battery charges now (a current into it); `None` where the
/// gauge does not answer.
pub fn charging<I: I2c>(i2c: &mut I) -> Option<bool> {
    register(i2c, CURRENT).map(|raw| raw as i16 > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::i2c::{ErrorKind, ErrorType, Operation, SevenBitAddress};

    /// A gauge answering each command with its value.
    struct Gauge {
        voltage: u16,
        current: i16,
        percent: u16,
    }

    impl ErrorType for Gauge {
        type Error = ErrorKind;
    }

    impl I2c<SevenBitAddress> for Gauge {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), ErrorKind> {
            assert_eq!(address, ADDRESS);
            let mut command = 0;
            for operation in operations {
                match operation {
                    Operation::Write(bytes) => command = bytes[0],
                    Operation::Read(buffer) => {
                        let value = match command {
                            VOLTAGE => self.voltage,
                            CURRENT => self.current as u16,
                            STATE_OF_CHARGE => self.percent,
                            _ => return Err(ErrorKind::Other),
                        };
                        buffer.copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn it_reads_the_charge_and_whether_it_charges() {
        let mut gauge = Gauge {
            voltage: 3987,
            current: 412,
            percent: 76,
        };
        assert_eq!(
            read(&mut gauge),
            Some(Reading {
                percent: 76,
                millivolts: 3987
            })
        );
        assert_eq!(charging(&mut gauge), Some(true));
        gauge.current = -35;
        assert_eq!(charging(&mut gauge), Some(false));
        gauge.current = 0;
        assert_eq!(charging(&mut gauge), Some(false));
    }

    #[test]
    fn what_no_battery_says_is_none() {
        let mut gauge = Gauge {
            voltage: 0,
            current: 0,
            percent: 0,
        };
        assert_eq!(read(&mut gauge), None);
        // A charge over 100 % is 100 %.
        assert_eq!(reading(4200, 102).map(|r| r.percent), Some(100));
    }
}
