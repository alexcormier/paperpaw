//! Lines a computer sends over the USB cable (the USB serial unit the
//! log goes out on, and the web flasher uses): the cable transfer's
//! (`PPT/1 …`, for the kernel, see `paperpaw_core` `Kernel::cable_line`)
//! in every build, and the test remote control's in builds with the
//! `remote` feature. Nothing is kept but the line coming.

use alloc::vec::Vec;

use esp_hal::peripherals::USB_DEVICE;

/// A line is never longer: a cable's or the remote's `data` line with
/// 512 bytes in hexadecimal.
pub const MAX_LINE: usize = 1100;

/// A line longer than [`MAX_LINE`] came: dropped whole, not carried out
/// cut.
pub struct TooLong;

/// Reads the lines, a byte at a time.
///
/// It reads the USB unit's receive buffer itself: esp-hal's driver resets
/// the unit when it starts, which cuts the connection the log goes out on.
pub struct UsbLines {
    _usb: USB_DEVICE<'static>,
    line: Vec<u8>,
    /// The line coming is longer than [`MAX_LINE`]: dropped whole.
    too_long: bool,
}

impl UsbLines {
    pub fn new(usb: USB_DEVICE<'static>) -> Self {
        Self {
            _usb: usb,
            line: Vec::new(),
            too_long: false,
        }
    }

    /// The next byte the computer sent, if any.
    fn read_byte(&mut self) -> Option<u8> {
        let regs = USB_DEVICE::regs();
        regs.ep1_conf()
            .read()
            .serial_out_ep_data_avail()
            .bit_is_set()
            .then(|| regs.ep1().read().rdwr_byte().bits())
    }

    /// The next whole line that came, if one did.
    pub fn next_line(&mut self) -> Option<Result<Vec<u8>, TooLong>> {
        while let Some(byte) = self.read_byte() {
            match byte {
                b'\n' | b'\r' if self.too_long => {
                    self.too_long = false;
                    return Some(Err(TooLong));
                }
                b'\n' | b'\r' if self.line.is_empty() => {}
                b'\n' | b'\r' => return Some(Ok(core::mem::take(&mut self.line))),
                _ if self.too_long => {}
                _ if self.line.len() < MAX_LINE => self.line.push(byte),
                _ => {
                    self.line.clear();
                    self.too_long = true;
                }
            }
        }
        None
    }

    /// Whether part of a line has come.
    pub fn in_line(&self) -> bool {
        !self.line.is_empty() || self.too_long
    }
}
