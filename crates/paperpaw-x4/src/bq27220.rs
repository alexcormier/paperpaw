//! The X3's fuel gauge: a BQ27220 on I²C (address 0x55), which knows the
//! battery's charge itself, as the X4 Pro's CW2017 does. It answers TI's
//! standard commands, each a little-endian 16-bit register: the voltage
//! at 0x08 (millivolts), the average current at 0x0C (signed
//! milliamperes, positive while the battery charges) and the charge at
//! 0x2C (percent). The X3 has no pin that says it is on USB: charging is
//! a current into the battery, as the FreeInk SDK (MIT) reads it
//! (`BatteryMonitor.cpp`).
//!
//! The gauge keeps the battery's design capacity in its RAM: after it
//! lost power (the battery run flat or cut off) it says TI's 3000 mAh,
//! and the charge it counts is wrong for the X3's 650 mAh cell.
//! [`load_capacity`] writes the cell's capacity back where it differs, as
//! TI's technical reference (SLUUBD4A, 6.1) says: unsealed, full access,
//! CONFIG UPDATE, the data memory's word written with its block's
//! checksum, then left with a reinit and sealed again.
//!
//! That sequence follows the FreeInk SDK's and Papyrix's
//! (`lib/BatteryMonitor/src/BatteryMonitor.cpp`), which found what the
//! X3's gauge needs beyond the reference (keys 1.5 s apart, a word in one
//! write, the block chosen again after its sum was read):
//!
//! MIT License. Copyright (c) 2026 FreeInk. Copyright (c) 2026 Pavel
//! Liashkov (Papyrix). Permission is hereby granted, free of charge, to
//! any person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the Software without
//! restriction, including without limitation the rights to use, copy,
//! modify, merge, publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software is furnished
//! to do so, subject to the following conditions: The above copyright
//! notice and this permission notice shall be included in all copies or
//! substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS
//! IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
//! NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
//! PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS
//! OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
//! ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
//! OTHER DEALINGS IN THE SOFTWARE.

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;

pub const ADDRESS: u8 = 0x55;

/// The X3's cell.
pub const X3_CAPACITY_MAH: u16 = 650;

const CONTROL: u8 = 0x00;
const VOLTAGE: u8 = 0x08;
const CURRENT: u8 = 0x0C;
const FULL_CHARGE_CAPACITY: u8 = 0x12;
const STATE_OF_CHARGE: u8 = 0x2C;
const OPERATION_STATUS: u8 = 0x3A;
const DESIGN_CAPACITY: u8 = 0x3C;
const MAC_CONTROL: u8 = 0x3E;
const MAC_DATA: u8 = 0x40;
/// The block's checksum in its low byte, its length in the high one.
const MAC_DATA_SUM: u8 = 0x60;

/// Data memory: the learned full charge capacity and the design capacity
/// (big-endian words).
const LEARNED_FCC: u16 = 0x929D;
const DESIGN_CAPACITY_MEMORY: u16 = 0x929F;
/// What the gauge says after it lost power.
const TI_DEFAULT_MAH: u16 = 3000;

/// Subcommands: the keys (unsealed, then full access), CONFIG UPDATE in
/// and out (with a reinit once something was written), sealing.
const UNSEAL: [u16; 2] = [0x0414, 0x3672];
const FULL_ACCESS: [u16; 2] = [0xFFFF, 0xFFFF];
const ENTER_CONFIG: u16 = 0x0090;
const EXIT_REINIT: u16 = 0x0091;
const EXIT: u16 = 0x0092;
const SEAL: u16 = 0x0030;
/// In [`OPERATION_STATUS`]: in CONFIG UPDATE; the security in bits 2:1
/// (3 sealed, 2 unsealed, 1 full access).
const IN_CONFIG: u16 = 0x0400;
const SEALED: u16 = 3;
const UNSEALED: u16 = 2;

/// Between two keys: the X3's gauge ignores keys sent back to back, and
/// drops a pair whose second comes more than 4 s after the first.
const KEY_GAP_MS: u32 = 1500;
/// After a block was chosen, and between two writes of the data memory.
const SELECT_MS: u32 = 10;
const BETWEEN_WRITES_MS: u32 = 500;
/// How often the status is asked, 100 ms apart, while CONFIG UPDATE is
/// entered or left (5 s).
const POLLS: u32 = 50;
const POLL_MS: u32 = 100;
/// Tries at leaving CONFIG UPDATE and sealing.
const CLOSE_TRIES: u32 = 3;

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

/// A word written in one write: the X3's gauge ignores a subcommand split
/// in two.
fn write_word<I: I2c>(i2c: &mut I, command: u8, value: u16) -> Option<()> {
    let [low, high] = value.to_le_bytes();
    i2c.write(ADDRESS, &[command, low, high]).ok()
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
/// gauge does not answer. A full battery on the cable has no current: the
/// X3 takes the USB connection too (`usb_power`).
pub fn charging<I: I2c>(i2c: &mut I) -> Option<bool> {
    register(i2c, CURRENT).map(|raw| raw as i16 > 0)
}

/// What [`load_capacity`] found and did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    /// The gauge had it already (or a capacity it was given otherwise).
    Kept,
    /// Written, the gauge left as it was found: sealed, out of CONFIG
    /// UPDATE.
    Written,
    /// Not written, or not checked; the gauge sealed again as far as it
    /// answered (the next start tries again).
    Failed,
}

/// A learned full charge capacity too high for a cell of `mah`: learned
/// against TI's default, as a learning cycle moves it at most 256 mAh.
fn too_high(learned: u16, mah: u16) -> bool {
    learned > mah + mah / 4
}

/// Gives the gauge the cell's capacity `mah` where it says TI's default
/// (or learned too high a full charge from it). Bounded: some 20 s at the
/// most, with `delay` waiting between the steps; only where it is needed,
/// so at a start after the battery was cut off. Whatever fails after the
/// keys, the gauge is left CONFIG UPDATE and sealed again.
pub fn load_capacity<I: I2c, D: DelayNs>(i2c: &mut I, delay: &mut D, mah: u16) -> Capacity {
    let (Some(design), Some(status)) = (
        register(i2c, DESIGN_CAPACITY),
        register(i2c, OPERATION_STATUS),
    ) else {
        return Capacity::Failed;
    };
    let security = (status >> 1) & 3;
    // An earlier load cut short (written, not closed): closed now.
    if design == mah && (status & IN_CONFIG != 0 || security != SEALED) {
        return if close(i2c, delay, true) {
            Capacity::Written
        } else {
            Capacity::Failed
        };
    }
    let needed = if design == mah {
        register(i2c, FULL_CHARGE_CAPACITY).is_some_and(|learned| too_high(learned, mah))
    } else {
        design == TI_DEFAULT_MAH
    };
    if !needed {
        return Capacity::Kept;
    }
    let opened = open(i2c, delay, security);
    let mut wrote = false;
    if opened {
        // The learned capacity first, the design capacity a moment after:
        // the gauge can miss a block chosen right after a write.
        wrote |= write_memory(i2c, delay, LEARNED_FCC, mah, |v| too_high(v, mah)) == Some(true);
        delay.delay_ms(BETWEEN_WRITES_MS);
        wrote |= write_memory(i2c, delay, DESIGN_CAPACITY_MEMORY, mah, |v| {
            v == TI_DEFAULT_MAH
        }) == Some(true);
    }
    let closed = close(i2c, delay, wrote);
    let holds = register(i2c, DESIGN_CAPACITY) == Some(mah);
    if opened && closed && holds {
        Capacity::Written
    } else {
        Capacity::Failed
    }
}

/// The keys from `security` on, then CONFIG UPDATE; whether the gauge is
/// in it.
fn open<I: I2c, D: DelayNs>(i2c: &mut I, delay: &mut D, security: u16) -> bool {
    let keys: &[u16] = match security {
        SEALED => &[UNSEAL[0], UNSEAL[1], FULL_ACCESS[0], FULL_ACCESS[1]],
        UNSEALED => &FULL_ACCESS,
        _ => &[],
    };
    for &key in keys.iter().chain(&[ENTER_CONFIG]) {
        if write_word(i2c, CONTROL, key).is_none() {
            return false;
        }
        delay.delay_ms(if key == ENTER_CONFIG {
            POLL_MS
        } else {
            KEY_GAP_MS
        });
    }
    poll(i2c, delay, true)
}

/// Asks the status until CONFIG UPDATE is entered (`entered`) or left;
/// whether it was within [`POLLS`].
fn poll<I: I2c, D: DelayNs>(i2c: &mut I, delay: &mut D, entered: bool) -> bool {
    for _ in 0..POLLS {
        if register(i2c, OPERATION_STATUS).is_some_and(|s| (s & IN_CONFIG != 0) == entered) {
            return true;
        }
        delay.delay_ms(POLL_MS);
    }
    false
}

/// Leaves CONFIG UPDATE (with a reinit if `reinit`) and seals; whether the
/// gauge then says it is sealed and out of it.
fn close<I: I2c, D: DelayNs>(i2c: &mut I, delay: &mut D, reinit: bool) -> bool {
    for _ in 0..CLOSE_TRIES {
        let in_config = register(i2c, OPERATION_STATUS).is_none_or(|s| s & IN_CONFIG != 0);
        if in_config {
            let exit = if reinit { EXIT_REINIT } else { EXIT };
            if write_word(i2c, CONTROL, exit).is_none() || !poll(i2c, delay, false) {
                continue;
            }
        }
        if write_word(i2c, CONTROL, SEAL).is_none() {
            continue;
        }
        delay.delay_ms(POLL_MS);
        if register(i2c, OPERATION_STATUS)
            .is_some_and(|s| (s >> 1) & 3 == SEALED && s & IN_CONFIG == 0)
        {
            return true;
        }
    }
    false
}

/// Writes `mah` into the data memory's word at `address` where `wanted`
/// says its value is one to replace, the block's checksum adjusted by the
/// bytes that change; whether it wrote (`None` where the gauge did not
/// answer).
fn write_memory<I: I2c, D: DelayNs>(
    i2c: &mut I,
    delay: &mut D,
    address: u16,
    mah: u16,
    wanted: impl Fn(u16) -> bool,
) -> Option<bool> {
    write_word(i2c, MAC_CONTROL, address)?;
    delay.delay_ms(SELECT_MS);
    let old = register(i2c, MAC_DATA)?;
    let sum = register(i2c, MAC_DATA_SUM)?;
    // The data memory's words are big-endian.
    if !wanted(old.swap_bytes()) {
        return Some(false);
    }
    let [old_low, old_high] = old.to_le_bytes();
    let [new_high, new_low] = mah.to_be_bytes();
    let checksum = (sum as u8)
        .wrapping_add(old_low)
        .wrapping_add(old_high)
        .wrapping_sub(new_high)
        .wrapping_sub(new_low);
    // Reading the sum moves the X3's gauge to the next block: this one
    // chosen again.
    write_word(i2c, MAC_CONTROL, address)?;
    delay.delay_ms(SELECT_MS);
    write_word(i2c, MAC_DATA, mah.swap_bytes())?;
    write_word(i2c, MAC_DATA_SUM, (sum & 0xFF00) | u16::from(checksum))?;
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_hal::i2c::{ErrorKind, ErrorType, Operation, SevenBitAddress};

    /// A gauge: its registers, its security and CONFIG UPDATE, its data
    /// memory (two words in one block with a checksum), what it was sent.
    struct Gauge {
        voltage: u16,
        current: i16,
        percent: u16,
        design: u16,
        learned: u16,
        security: u16,
        in_config: bool,
        /// Keys of a pair received so far.
        keys: alloc::vec::Vec<u16>,
        /// The block chosen, and the words written into it not yet summed.
        chosen: u16,
        pending: Option<u16>,
        /// The block's bytes' sum, as the checksum is 255 minus it.
        checksum: u8,
        /// Answers nothing from this subcommand on (a gauge gone).
        dead_from: Option<u16>,
        dead: bool,
        /// Stays in CONFIG UPDATE once entered (an exit it ignores).
        stuck: bool,
        sent: alloc::vec::Vec<u16>,
    }

    impl Gauge {
        fn new(design: u16, learned: u16) -> Self {
            let mut gauge = Self {
                voltage: 3987,
                current: 412,
                percent: 76,
                design,
                learned,
                security: SEALED,
                in_config: false,
                keys: alloc::vec::Vec::new(),
                chosen: 0,
                pending: None,
                checksum: 0,
                dead_from: None,
                dead: false,
                stuck: false,
                sent: alloc::vec::Vec::new(),
            };
            gauge.checksum = gauge.block_checksum(design, learned);
            gauge
        }

        fn block_checksum(&self, design: u16, learned: u16) -> u8 {
            let bytes = [design.to_be_bytes(), learned.to_be_bytes()];
            255u8.wrapping_sub(bytes.iter().flatten().fold(0u8, |a, b| a.wrapping_add(*b)))
        }

        fn status(&self) -> u16 {
            (self.security << 1) | if self.in_config { IN_CONFIG } else { 0 }
        }

        fn word(&self) -> u16 {
            match self.chosen {
                LEARNED_FCC => self.learned,
                DESIGN_CAPACITY_MEMORY => self.design,
                _ => 0,
            }
        }

        fn subcommand(&mut self, value: u16) {
            self.sent.push(value);
            if self.dead_from == Some(value) {
                self.dead = true;
                return;
            }
            match value {
                0x0414 | 0x3672 | 0xFFFF => {
                    self.keys.push(value);
                    if self.keys == UNSEAL {
                        self.security = UNSEALED;
                        self.keys.clear();
                    } else if self.keys == FULL_ACCESS && self.security == UNSEALED {
                        self.security = 1;
                        self.keys.clear();
                    }
                }
                ENTER_CONFIG if self.security == 1 => self.in_config = true,
                EXIT_REINIT | EXIT if !self.stuck => self.in_config = false,
                SEAL if !self.in_config => self.security = SEALED,
                _ => {}
            }
        }

        fn receive(&mut self, bytes: &[u8]) {
            let value = u16::from_le_bytes([bytes[1], bytes[2]]);
            match bytes[0] {
                CONTROL => self.subcommand(value),
                MAC_CONTROL => {
                    self.chosen = value;
                    self.pending = None;
                }
                MAC_DATA if self.in_config => self.pending = Some(value.swap_bytes()),
                MAC_DATA_SUM => {
                    let Some(new) = self.pending.take() else {
                        return;
                    };
                    let (design, learned) = match self.chosen {
                        LEARNED_FCC => (self.design, new),
                        _ => (new, self.learned),
                    };
                    // Taken only with the right checksum.
                    if value as u8 == self.block_checksum(design, learned) {
                        self.design = design;
                        self.learned = learned;
                        self.checksum = value as u8;
                    }
                }
                _ => {}
            }
        }

        fn answer(&self, command: u8) -> u16 {
            match command {
                VOLTAGE => self.voltage,
                CURRENT => self.current as u16,
                STATE_OF_CHARGE => self.percent,
                OPERATION_STATUS => self.status(),
                DESIGN_CAPACITY => self.design,
                FULL_CHARGE_CAPACITY => self.learned,
                MAC_DATA => self.word().swap_bytes(),
                MAC_DATA_SUM => 0x2400 | u16::from(self.checksum),
                _ => 0,
            }
        }
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
            if self.dead {
                return Err(ErrorKind::Other);
            }
            let mut command = 0;
            for operation in operations {
                match operation {
                    Operation::Write(bytes) if bytes.len() == 3 => self.receive(bytes),
                    Operation::Write(bytes) => command = bytes[0],
                    Operation::Read(buffer) => {
                        buffer.copy_from_slice(&self.answer(command).to_le_bytes());
                    }
                }
            }
            Ok(())
        }
    }

    /// Counts the time waited.
    struct Waited(u32);

    impl DelayNs for Waited {
        fn delay_ns(&mut self, ns: u32) {
            self.0 += ns / 1_000_000;
        }
    }

    #[test]
    fn it_reads_the_charge_and_whether_it_charges() {
        let mut gauge = Gauge::new(650, 600);
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
        let mut gauge = Gauge::new(650, 600);
        gauge.voltage = 0;
        assert_eq!(read(&mut gauge), None);
        // A charge over 100 % is 100 %.
        assert_eq!(reading(4200, 102).map(|r| r.percent), Some(100));
    }

    #[test]
    fn the_cell_s_capacity_is_kept_where_the_gauge_has_it() {
        let mut gauge = Gauge::new(650, 640);
        assert_eq!(
            load_capacity(&mut gauge, &mut Waited(0), X3_CAPACITY_MAH),
            Capacity::Kept
        );
        assert!(gauge.sent.is_empty(), "{:04X?}", gauge.sent);
        // One given otherwise is not touched either.
        let mut other = Gauge::new(1200, 1100);
        assert_eq!(
            load_capacity(&mut other, &mut Waited(0), X3_CAPACITY_MAH),
            Capacity::Kept
        );
        assert!(other.sent.is_empty());
    }

    #[test]
    fn ti_s_default_is_replaced_and_the_gauge_sealed_again() {
        // After it lost power: 3000 mAh, and a full charge learned from it.
        let mut gauge = Gauge::new(TI_DEFAULT_MAH, 2744);
        let mut waited = Waited(0);
        assert_eq!(
            load_capacity(&mut gauge, &mut waited, X3_CAPACITY_MAH),
            Capacity::Written
        );
        assert_eq!((gauge.design, gauge.learned), (650, 650));
        assert_eq!((gauge.security, gauge.in_config), (SEALED, false));
        // The keys in order, apart; out with a reinit, sealed.
        assert_eq!(
            gauge.sent,
            [
                0x0414,
                0x3672,
                0xFFFF,
                0xFFFF,
                ENTER_CONFIG,
                EXIT_REINIT,
                SEAL
            ]
        );
        assert!(
            waited.0 >= 4 * KEY_GAP_MS && waited.0 < 20_000,
            "{}",
            waited.0
        );
    }

    #[test]
    fn a_load_cut_short_is_closed() {
        let mut gauge = Gauge::new(650, 650);
        gauge.security = 1;
        gauge.in_config = true;
        assert_eq!(
            load_capacity(&mut gauge, &mut Waited(0), X3_CAPACITY_MAH),
            Capacity::Written
        );
        assert_eq!((gauge.security, gauge.in_config), (SEALED, false));
    }

    #[test]
    fn a_failure_never_leaves_it_open() {
        // The gauge goes quiet after CONFIG UPDATE is entered: nothing
        // written, and whatever it answers it is not left there by us.
        let mut gauge = Gauge::new(TI_DEFAULT_MAH, 2744);
        gauge.stuck = true;
        let mut waited = Waited(0);
        assert_eq!(
            load_capacity(&mut gauge, &mut waited, X3_CAPACITY_MAH),
            Capacity::Failed
        );
        // It was asked to leave, every try, and waiting stayed bounded.
        assert!(gauge.sent.iter().filter(|&&s| s == EXIT_REINIT).count() >= 1);
        assert!(waited.0 < 30_000, "{}", waited.0);
        // A gauge that stops answering at the keys: given up at once,
        // nothing written.
        let mut gone = Gauge::new(TI_DEFAULT_MAH, 2744);
        gone.dead_from = Some(0x3672);
        assert_eq!(
            load_capacity(&mut gone, &mut Waited(0), X3_CAPACITY_MAH),
            Capacity::Failed
        );
        assert_eq!(gone.design, TI_DEFAULT_MAH);
    }
}
