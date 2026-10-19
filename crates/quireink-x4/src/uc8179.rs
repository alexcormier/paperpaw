//! The X4 Pro's display: an 800 × 480 panel driven by a UC8179 controller
//! over SPI, with the same calls as [`display`](crate::display), so the
//! firmware drives either.
//!
//! Unlike the X4's SSD1677, the UC8179 takes a frame whole, line by line
//! (its *new* memory, `DTM2`), and compares it with the frame shown (its
//! *old* memory, `DTM1`) for a fast refresh. The X4 Pro has the memory to
//! keep the frame (it has PSRAM): [`Display::write_rows`] draws QuireInk's
//! bands into it, and each refresh sends it whole. After a refresh the
//! frame goes into *old* too, for the next fast one to compare with.
//!
//! The controller addresses 800 × 600 pixels (`TRES`); the panel shows
//! the first 480 lines, sent last line first. A byte holds 8 pixels of a
//! line, the leftmost in the highest bit, set for white. QuireInk draws
//! the screen upright (480 × 800): the panel turned, the screen's row `y`
//! the controller's column `y`, its column `x` the controller's line
//! `479 - x` (as seen on the device: the other way it shows mirrored).
//!
//! The refreshes use the panel's own waveforms (`PSR` 0x1F), chosen by
//! the temperature told (`TSSET`): 30 °C drives every pixel (full and
//! half), 90 °C the fast waveform, only where *new* and *old* differ.
//! These sequences are the ones the FreeInk SDK records for the X4 Pro
//! (MIT licence), as Papyrix uses them.
//!
//! Four grays ([`Display::write_gray_rows`], [`Display::refresh_gray`])
//! come in two steps, as in the FreeInk SDK: the frame is shown in black
//! and white first, every gray black; then the SDK's gray waveforms
//! (`PSR` 0x3F, from the LUT registers) lighten the grays, chosen by a
//! code in *old* and *new*: 00 not at all (black), 11 a little (dark
//! gray), 10 more (light gray); white, already white, takes 11 too. The next
//! refresh after them drives every pixel back to black or white, as a
//! half refresh if a fast one was asked (the SDK's waveforms for a fast
//! one leave the grays a little light).

use alloc::vec;
use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use quireink_platform::{FrameView, PixelFormat, Rotation};

pub use crate::display::{Error, PANEL_HEIGHT, PANEL_WIDTH, Planes, Refresh};

/// The screen QuireInk draws upright (see [`Display::size`] for it
/// turned).
pub const WIDTH: u16 = PANEL_HEIGHT;
pub const HEIGHT: u16 = PANEL_WIDTH;

/// Lines the controller addresses; those past the panel's are white.
const ADDRESSED_LINES: u16 = 600;
/// Bytes of a line.
const LINE: usize = PANEL_WIDTH as usize / 8;
/// Longest wait for the controller, e.g. a full refresh.
const BUSY_TIMEOUT_MS: u32 = 30_000;

/// The controller's commands used here.
mod cmd {
    pub const PANEL_SETTING: u8 = 0x00;
    pub const POWER_OFF: u8 = 0x02;
    pub const POWER_OFF_SEQUENCE: u8 = 0x03;
    pub const POWER_ON: u8 = 0x04;
    pub const BOOSTER_SOFT_START: u8 = 0x06;
    pub const DEEP_SLEEP: u8 = 0x07;
    pub const OLD: u8 = 0x10;
    pub const REFRESH: u8 = 0x12;
    pub const NEW: u8 = 0x13;
    pub const VCOM_DATA_INTERVAL: u8 = 0x50;
    pub const RESOLUTION: u8 = 0x61;
    pub const GATE_SOURCE_START: u8 = 0x65;
    pub const PARTIAL_IN: u8 = 0x91;
    pub const PARTIAL_OUT: u8 = 0x92;
    pub const CASCADE: u8 = 0xE0;
    pub const GATE_SCAN: u8 = 0xE1;
    pub const POWER_SAVING: u8 = 0xE3;
    pub const TEMPERATURE: u8 = 0xE5;
}

/// The border and data interval while refreshing, and at rest.
const ACTIVE_INTERVAL: [u8; 2] = [0x29, 0x07];
const IDLE_INTERVAL: [u8; 2] = [0xA9, 0x07];

/// Bytes of a waveform register; the tables below give the first 12, the
/// rest are 0.
const LUT_LEN: usize = 42;

/// The waveforms for four grays (registers 0x20 to 0x24: VCOM, then by
/// *old* and *new* bit: 11, 01, 10, 00), from the panel showing every
/// gray black: 11 lightens one frame, 01 and 10 two, 00 none.
const GRAY_LUTS: [[u8; 13]; 5] = [
    [
        0x20, 0x00, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x21, 0x08, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x22, 0x20, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x23, 0x20, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
    [
        0x24, 0x00, 0x02, 0x02, 0x01, 0x01, 0x01, 0, 0, 0, 0, 0, 0x01,
    ],
];

/// A picture of the panel as [`Display`] keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plane {
    /// The frame in black and white.
    Frame,
    /// The grays' code in *old*: set but for black.
    Old,
    /// In *new*: set for dark gray and white.
    New,
}

/// What [`Display::wait`] does once the controller is done refreshing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    /// Leave the partial mode a fast refresh entered.
    partial: bool,
    power_off: bool,
}

/// The display, on its SPI device (which selects the chip) and pins.
#[derive(Debug)]
pub struct Display<SPI, DC, RST, BUSY, D> {
    spi: SPI,
    dc: DC,
    reset: RST,
    busy: BUSY,
    delay: D,
    /// The frame, line by line as the panel lies (480 lines of 100 bytes).
    frame: Vec<u8>,
    /// The grays' codes for *old* and *new*, laid out as `frame`; empty
    /// until grays are first written.
    old: Vec<u8>,
    new: Vec<u8>,
    /// The panel shows grays: the next fast refresh drives them away.
    gray_shown: bool,
    /// The controller's *old* memory holds the frame shown, for a fast
    /// refresh to compare with.
    old_valid: bool,
    powered: bool,
    rotation: Rotation,
    pending: Option<Pending>,
}

impl<SPI, DC, RST, BUSY, D> Display<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, busy: BUSY, delay: D) -> Self {
        Self {
            spi,
            dc,
            reset,
            busy,
            delay,
            frame: vec![0xFF; LINE * usize::from(PANEL_HEIGHT)],
            old: Vec::new(),
            new: Vec::new(),
            gray_shown: false,
            old_valid: false,
            powered: false,
            rotation: Rotation::Deg0,
            pending: None,
        }
    }

    /// How the screen QuireInk draws is turned (as
    /// [`display::Display::set_rotation`](crate::display::Display::set_rotation)).
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.rotation = rotation;
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The width and height of the screen QuireInk draws, turned.
    pub fn size(&self) -> (u16, u16) {
        match self.rotation {
            Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
            Rotation::Deg90 | Rotation::Deg270 => (HEIGHT, WIDTH),
        }
    }

    /// Resets and sets up the controller; the next refresh drives every
    /// pixel.
    pub fn init(&mut self) -> Result<(), Error> {
        for (high, ms) in [(true, 2), (false, 50), (true, 30)] {
            if high {
                self.reset.set_high()
            } else {
                self.reset.set_low()
            }
            .map_err(|_| Error::Pin)?;
            self.delay.delay_ms(ms);
        }
        self.wait_ready()?;
        // The panel's own waveforms from its memory; 800 × 600 addressed.
        self.command(cmd::PANEL_SETTING, &[0x3F, 0x0A])?;
        self.command(cmd::RESOLUTION, &[0x03, 0x20, 0x02, 0x58])?;
        self.command(cmd::GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00])?;
        self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
        self.command(cmd::BOOSTER_SOFT_START, &[0x25, 0x25, 0x3C, 0x25])?;
        self.command(cmd::GATE_SCAN, &[0x02])?;
        self.command(cmd::POWER_SAVING, &[0x22])?;
        self.old_valid = false;
        self.gray_shown = false;
        self.powered = false;
        self.pending = None;
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned) into the frame.
    /// Pixels lighter than middle gray are white. The frame is sent whole
    /// at the refresh; `planes` only matter to the SSD1677, whose frame
    /// lives in its memories: here what goes only into *shown* after a
    /// fast refresh is there already.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        if !planes.new {
            return Ok(());
        }
        let (width, screen_height) = self.size();
        if top >= screen_height || rows.width != width || rows.height == 0 {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        let bpp = usize::from(rows.format.bits_per_pixel());
        let gray = rows.format.gray_levels().is_some() && bpp <= 8;
        let max = (1u32 << bpp.min(8)) - 1;
        for y in 0..height {
            for x in 0..width {
                let light = if gray {
                    let bit = usize::from(x) * bpp;
                    let at = usize::from(y) * rows.stride + bit / 8;
                    let byte = rows.data.get(at).copied().unwrap_or(0xFF);
                    let level = u32::from(byte >> (8 - bpp - bit % 8)) & max;
                    level * 255 >= 128 * max
                } else {
                    let c = rows.pixel(x, y);
                    u16::from(c.r) * 77 + u16::from(c.g) * 150 + u16::from(c.b) * 29 >= 128 * 256
                };
                let place = self.place(x, top + y);
                put(&mut self.frame, place, light);
            }
        }
        Ok(())
    }

    /// The byte and bit of the turned screen's pixel `(x, y)` in a plane.
    fn place(&self, x: u16, y: u16) -> (usize, u8) {
        let (sx, sy) = upright(self.rotation, x, y);
        let (column, line) = panel_place(sx, sy);
        let at = usize::from(line) * LINE + usize::from(column / 8);
        (at, 0x80 >> (column % 8))
    }

    /// Writes screen rows in four grays (2 bits a pixel, 0 black, 3
    /// white; `top` and `rows` as for [`Display::write_rows`]), shown with
    /// [`Display::refresh_gray`]. The frame gets them in black and white,
    /// only white white.
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if top >= screen_height
            || rows.width != width
            || rows.height == 0
            || rows.format != PixelFormat::Gray2
        {
            return Err(Error::Rows);
        }
        if self.old.is_empty() {
            self.old = vec![0; self.frame.len()];
            self.new = vec![0; self.frame.len()];
        }
        let height = rows.height.min(screen_height - top);
        for y in 0..height {
            for x in 0..width {
                let at = usize::from(y) * rows.stride + usize::from(x / 4);
                let byte = rows.data.get(at).copied().unwrap_or(0xFF);
                let level = (byte >> (6 - 2 * (x % 4))) & 0b11;
                let place = self.place(x, top + y);
                put(&mut self.frame, place, level == 3);
                put(&mut self.old, place, level != 0);
                put(&mut self.new, place, level & 1 != 0);
            }
        }
        Ok(())
    }

    /// Shows what [`Display::write_gray_rows`] wrote: the frame in black
    /// and white with a fast refresh, then the grays lightened from it
    /// (about a second more). The next fast refresh drives every pixel.
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        if self.old.is_empty() {
            return self.refresh(Refresh::Half, false);
        }
        self.refresh(Refresh::Fast, false)?;
        self.send(cmd::OLD, Plane::Old, false)?;
        self.send(cmd::NEW, Plane::New, false)?;
        self.command(cmd::PANEL_SETTING, &[0x3F, 0x0A])?;
        self.load_luts(&GRAY_LUTS)?;
        self.command(cmd::VCOM_DATA_INTERVAL, &ACTIVE_INTERVAL)?;
        self.power_on()?;
        self.command(cmd::REFRESH, &[])?;
        let done = self.wait_ready();
        self.command(cmd::VCOM_DATA_INTERVAL, &IDLE_INTERVAL)?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        // Both memories the frame in black and white, the grays black:
        // what the next fast refresh starts from.
        self.send(cmd::OLD, Plane::Frame, false)?;
        self.send(cmd::NEW, Plane::Frame, false)?;
        self.old_valid = true;
        self.gray_shown = true;
        Ok(())
    }

    /// Shows the frame; `power_off` turns the panel's power off after it.
    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.start_refresh(refresh, power_off)?;
        self.wait()
    }

    /// Starts showing the frame as [`Display::refresh`] does, without
    /// waiting for the panel: [`Display::wait`] before anything else.
    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        self.wait()?;
        // A fast refresh compares with the frame shown: without it, every
        // pixel is driven; after grays, as a half refresh.
        let fast = refresh == Refresh::Fast && self.old_valid && !self.gray_shown;
        let refresh = if refresh == Refresh::Fast && self.gray_shown {
            Refresh::Half
        } else {
            refresh
        };
        self.gray_shown = false;
        self.send(cmd::NEW, Plane::Frame, false)?;
        match refresh {
            _ if fast => {}
            Refresh::Half => self.send(cmd::OLD, Plane::Frame, true)?,
            _ => self.fill_old(0xFF)?,
        }
        self.command(cmd::VCOM_DATA_INTERVAL, &ACTIVE_INTERVAL)?;
        self.command(cmd::CASCADE, &[0x02])?;
        self.command(cmd::TEMPERATURE, &[if fast { 0x5A } else { 0x1E }])?;
        self.command(cmd::PANEL_SETTING, &[0x1F, 0x0A])?;
        if fast {
            self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
            self.command(cmd::GATE_SCAN, &[0x02])?;
        }
        self.power_on()?;
        if fast {
            self.command(cmd::PARTIAL_IN, &[])?;
        }
        self.command(cmd::REFRESH, &[])?;
        self.pending = Some(Pending {
            partial: fast,
            power_off,
        });
        Ok(())
    }

    /// Loads waveforms into their registers.
    fn load_luts(&mut self, luts: &[[u8; 13]; 5]) -> Result<(), Error> {
        let mut lut = [0u8; LUT_LEN];
        for row in luts {
            lut[..12].copy_from_slice(&row[1..]);
            self.command(row[0], &lut)?;
        }
        Ok(())
    }

    /// Turns the panel's power on, if it is not.
    fn power_on(&mut self) -> Result<(), Error> {
        if !self.powered {
            self.command(cmd::POWER_ON, &[])?;
            self.wait_ready()?;
            self.powered = true;
        }
        Ok(())
    }

    /// Puts the controller into deep sleep, the picture staying;
    /// [`Display::init`] wakes it again.
    pub fn sleep(&mut self) -> Result<(), Error> {
        self.wait()?;
        if self.powered {
            self.command(cmd::POWER_OFF, &[])?;
            self.wait_ready()?;
            self.powered = false;
        }
        self.old_valid = false;
        self.gray_shown = false;
        self.command(cmd::DEEP_SLEEP, &[0xA5])
    }

    /// Waits for a refresh started by [`Display::start_refresh`] to end,
    /// then keeps the frame as the one shown.
    pub fn wait(&mut self) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let done = self.wait_ready();
        if pending.partial {
            self.command(cmd::PARTIAL_OUT, &[])?;
        }
        self.command(cmd::VCOM_DATA_INTERVAL, &IDLE_INTERVAL)?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        self.send(cmd::OLD, Plane::Frame, false)?;
        self.old_valid = true;
        if pending.power_off {
            self.command(cmd::POWER_OFF, &[])?;
            self.wait_ready()?;
            self.powered = false;
        }
        Ok(())
    }

    /// Waits while the controller is busy (its busy pin low).
    fn wait_ready(&mut self) -> Result<(), Error> {
        for _ in 0..BUSY_TIMEOUT_MS {
            if self.busy.is_high().map_err(|_| Error::Pin)? {
                return Ok(());
            }
            self.delay.delay_ms(1);
        }
        Err(Error::Timeout)
    }

    /// Sends `plane` into memory `command`, inverted if asked: its lines
    /// last first, then the lines past the panel white.
    fn send(&mut self, command: u8, plane: Plane, invert: bool) -> Result<(), Error> {
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let plane = match plane {
            Plane::Frame => &self.frame,
            Plane::Old => &self.old,
            Plane::New => &self.new,
        };
        let mut line = [0u8; LINE];
        for index in (0..usize::from(PANEL_HEIGHT)).rev() {
            let from = &plane[index * LINE..(index + 1) * LINE];
            if invert {
                for (to, byte) in line.iter_mut().zip(from) {
                    *to = !byte;
                }
                self.spi.write(&line).map_err(|_| Error::Spi)?;
            } else {
                self.spi.write(from).map_err(|_| Error::Spi)?;
            }
        }
        line.fill(0xFF);
        for _ in PANEL_HEIGHT..ADDRESSED_LINES {
            self.spi.write(&line).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }

    /// Fills the *old* memory with `value`.
    fn fill_old(&mut self, value: u8) -> Result<(), Error> {
        self.command(cmd::OLD, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let line = [value; LINE];
        for _ in 0..ADDRESSED_LINES {
            self.spi.write(&line).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }

    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Error> {
        self.dc.set_low().map_err(|_| Error::Pin)?;
        self.spi.write(&[command]).map_err(|_| Error::Spi)?;
        if !data.is_empty() {
            self.dc.set_high().map_err(|_| Error::Pin)?;
            self.spi.write(data).map_err(|_| Error::Spi)?;
        }
        Ok(())
    }
}

/// Sets the bit at `place` (from [`Display::place`]) of `plane`, or
/// clears it.
fn put(plane: &mut [u8], (at, bit): (usize, u8), on: bool) {
    if let Some(byte) = plane.get_mut(at) {
        if on {
            *byte |= bit;
        } else {
            *byte &= !bit;
        }
    }
}

/// The upright screen's pixel of the turned screen's `(x, y)`.
fn upright(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg0 => (x, y),
        // Turned clockwise: the turned screen's top is the upright one's
        // right side.
        Rotation::Deg90 => (WIDTH - 1 - y, x),
        Rotation::Deg180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        Rotation::Deg270 => (y, HEIGHT - 1 - x),
    }
}

/// The turned screen's pixel at the upright screen's `(x, y)`: where a
/// touch on the upright screen lands on the screen QuireInk draws.
pub fn turned(rotation: Rotation, x: u16, y: u16) -> (u16, u16) {
    match rotation {
        Rotation::Deg0 => (x, y),
        Rotation::Deg90 => (y, WIDTH - 1 - x),
        Rotation::Deg180 => (WIDTH - 1 - x, HEIGHT - 1 - y),
        Rotation::Deg270 => (HEIGHT - 1 - y, x),
    }
}

/// Where the upright screen's pixel `(x, y)` lies in the frame: its
/// column (bit) and line (the screen's left column the panel's last line).
fn panel_place(x: u16, y: u16) -> (u16, u16) {
    (y, WIDTH - 1 - x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use embedded_hal::spi::{ErrorType, Operation};
    use quireink_platform::PixelFormat;

    /// The controller, simulated: what it was told, and its memories.
    #[derive(Default)]
    struct Controller {
        data: bool,
        command: u8,
        commands: Vec<u8>,
        new: Vec<u8>,
        old: Vec<u8>,
        /// The temperatures told, by refresh.
        temperatures: Vec<u8>,
        /// The memories at each refresh: *old*, then *new*.
        refreshed: Vec<(Vec<u8>, Vec<u8>)>,
    }

    impl Controller {
        fn byte(&mut self, b: u8) {
            if !self.data {
                self.command = b;
                self.commands.push(b);
                match b {
                    cmd::NEW => self.new.clear(),
                    cmd::OLD => self.old.clear(),
                    cmd::REFRESH => self.refreshed.push((self.old.clone(), self.new.clone())),
                    _ => {}
                }
                return;
            }
            match self.command {
                cmd::NEW => self.new.push(b),
                cmd::OLD => self.old.push(b),
                cmd::TEMPERATURE => self.temperatures.push(b),
                _ => {}
            }
        }
    }

    struct Spi(Rc<RefCell<Controller>>);
    struct Dc(Rc<RefCell<Controller>>);
    struct Pin;
    struct Delay;

    impl ErrorType for Spi {
        type Error = core::convert::Infallible;
    }
    impl SpiDevice for Spi {
        fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
            for op in ops {
                if let Operation::Write(bytes) = op {
                    for b in bytes.iter() {
                        self.0.borrow_mut().byte(*b);
                    }
                }
            }
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Dc {
        type Error = core::convert::Infallible;
    }
    impl OutputPin for Dc {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().data = false;
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            self.0.borrow_mut().data = true;
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Pin {
        type Error = core::convert::Infallible;
    }
    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    impl InputPin for Pin {
        fn is_high(&mut self) -> Result<bool, Self::Error> {
            Ok(true)
        }
        fn is_low(&mut self) -> Result<bool, Self::Error> {
            Ok(false)
        }
    }
    impl DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }

    type Simulated = Display<Spi, Dc, Pin, Pin, Delay>;

    fn display() -> (Simulated, Rc<RefCell<Controller>>) {
        let c = Rc::new(RefCell::new(Controller::default()));
        let mut d = Display::new(Spi(c.clone()), Dc(c.clone()), Pin, Pin, Delay);
        d.init().unwrap();
        (d, c)
    }

    /// A band of the upright screen, all black but for one white pixel.
    fn band_with(x: u16, y: u16) -> Vec<u8> {
        let stride = usize::from(WIDTH / 8);
        let mut data = vec![0u8; stride * 8];
        data[usize::from(y) * stride + usize::from(x / 8)] |= 0x80 >> (x % 8);
        data
    }

    fn view(data: &[u8], height: u16) -> FrameView<'_> {
        FrameView {
            width: WIDTH,
            height,
            stride: usize::from(WIDTH / 8),
            format: PixelFormat::Mono1,
            data,
        }
    }

    #[test]
    fn frames_are_sent_whole_last_line_first() {
        let (mut d, c) = display();
        // Screen pixel (5, 3) white: the panel's line 474, column 3.
        let band = band_with(5, 3);
        d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
        d.refresh(Refresh::Full, false).unwrap();
        let c = c.borrow();
        assert_eq!(c.new.len(), LINE * usize::from(ADDRESSED_LINES));
        // Line 474 of 480 is sent as the sixth.
        let line = 5;
        assert_eq!(c.new[line * LINE] & 0x10, 0x10);
        assert_eq!(c.new[line * LINE] & 0x08, 0);
        // The lines past the panel are white.
        assert!(
            c.new[usize::from(PANEL_HEIGHT) * LINE..]
                .iter()
                .all(|b| *b == 0xFF)
        );
        // After the refresh the frame is the old one too.
        assert_eq!(c.old, c.new);
    }

    #[test]
    fn a_fast_refresh_needs_a_frame_shown() {
        let (mut d, c) = display();
        let band = band_with(0, 0);
        d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
        // The first after init drives every pixel.
        d.refresh(Refresh::Fast, false).unwrap();
        d.refresh(Refresh::Fast, false).unwrap();
        let c = c.borrow();
        assert_eq!(c.temperatures, [0x1E, 0x5A]);
        assert!(c.commands.contains(&cmd::PARTIAL_IN));
        assert!(c.commands.contains(&cmd::PARTIAL_OUT));
    }

    /// A gray band of the upright screen, white but for its first four
    /// pixels: black, dark gray, light gray, white.
    fn gray_band() -> Vec<u8> {
        let mut data = vec![0xFF; usize::from(WIDTH / 4) * 8];
        data[0] = 0b00_01_10_11;
        data
    }

    fn gray_view(data: &[u8]) -> FrameView<'_> {
        FrameView {
            width: WIDTH,
            height: 8,
            stride: usize::from(WIDTH / 4),
            format: PixelFormat::Gray2,
            data,
        }
    }

    /// The bit of the upright screen's pixel `(x, 0)` in memory as sent:
    /// the panel's line `479 - x`, sent as the `x`th, its first bit.
    fn bit(memory: &[u8], x: usize) -> u8 {
        memory[x * LINE] >> 7
    }

    #[test]
    fn grays_are_lightened_from_black() {
        let (mut d, c) = display();
        let band = gray_band();
        d.write_gray_rows(0, gray_view(&band)).unwrap();
        d.refresh_gray().unwrap();
        let c = c.borrow();
        let [shown, (old, new)] = &c.refreshed[..] else {
            panic!("two refreshes: {}", c.refreshed.len());
        };
        // First the frame in black and white: only white white.
        let levels = 0..4;
        assert_eq!(
            levels.clone().map(|x| bit(&shown.1, x)).collect::<Vec<_>>(),
            [0, 0, 0, 1]
        );
        // Then the codes: black 00, dark gray 11, light gray 10, white 11.
        assert_eq!(
            levels.clone().map(|x| bit(old, x)).collect::<Vec<_>>(),
            [0, 1, 1, 1]
        );
        assert_eq!(
            levels.map(|x| bit(new, x)).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        // With waveforms of our own.
        assert!((0x20..=0x24).all(|r| c.commands.contains(&r)));
        // After: both memories the frame in black and white.
        assert_eq!(c.old, c.new);
        assert_eq!(c.new, shown.1);
    }

    #[test]
    fn the_refresh_after_grays_drives_every_pixel() {
        let (mut d, c) = display();
        let band = gray_band();
        d.write_gray_rows(0, gray_view(&band)).unwrap();
        d.refresh_gray().unwrap();
        d.refresh(Refresh::Fast, false).unwrap();
        d.refresh(Refresh::Fast, false).unwrap();
        // Fast (the first after init: full), the grays, then a half
        // refresh in place of a fast one, then fast again.
        assert_eq!(c.borrow().temperatures, [0x1E, 0x1E, 0x5A]);
    }

    #[test]
    fn turned_screens_land_upright() {
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let (w, h) = match rotation {
                Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
                _ => (HEIGHT, WIDTH),
            };
            for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
                let (sx, sy) = upright(rotation, x, y);
                assert!(sx < WIDTH && sy < HEIGHT, "{rotation:?} {x},{y}");
                assert_eq!(turned(rotation, sx, sy), (x, y), "{rotation:?}");
            }
        }
    }
}
