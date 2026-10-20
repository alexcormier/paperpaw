//! The UltraChip displays: an 800 × 480 panel driven by a UC8179 or a
//! UC8279 controller over SPI (the X4 Pro and the X4 Classic have either,
//! by batch; see [`probe`](crate::probe)), with the same calls as
//! [`display`](crate::display), so the firmware drives any.
//!
//! The controller keeps two frames: the new one (`DTM2`) and the one
//! shown (`DTM1`), which a fast refresh compares with. Where there is the
//! memory for it (the X4 Pro's PSRAM), the driver keeps the frame too
//! ([`Frame::Kept`]): [`Display::write_rows`] draws PaperPaw's bands into
//! it, each refresh sends it whole, and after a refresh it goes into the
//! frame shown too, for the next fast one to compare with. Where there is
//! not ([`Frame::InController`], a device with the X4's 192 KB), the bands
//! go straight into the controller's frames as the SSD1677's do, each
//! through a window of the controller's memory (`PTL`, entered with
//! `PTIN` and left with `PTOUT`): into the new frame, the one shown or
//! both, as [`Planes`] says (laid out as for the SSD1677).
//!
//! The controller addresses 800 × 600 pixels (`TRES`); the panel shows
//! 480 of the lines: on a UC8179 the first, sent last line first, on a
//! UC8279 the last, in order. A byte holds 8 pixels of a line, the
//! leftmost in the highest bit, set for white. PaperPaw draws the screen
//! upright (480 × 800): the panel turned, the screen's row `y` the
//! controller's column `y`, its column `x` the controller's line `x` on a
//! UC8179 (as seen on the device: the other way it shows mirrored) and
//! `599 - x` on a UC8279.
//!
//! The refreshes use the panel's own waveforms (`PSR` 0x1F on a UC8179,
//! 0x17 on a UC8279), chosen by the temperature told (`TSSET`): 30 °C
//! drives every pixel (full and half), 90 °C the fast waveform, only where
//! the two frames differ. A UC8279 forgets its panel setting when its
//! power comes on (it is set again after) and refreshes fast only within
//! a window. These sequences are the ones the FreeInk SDK records for the
//! X4 Pro from its stock firmware (MIT licence), as Papyrix uses them.
//!
//! Four grays ([`Display::write_gray_rows`], [`Display::refresh_gray`])
//! come in two steps on a UC8179 with its frame kept, as in the FreeInk
//! SDK: the frame is shown in black and white first, every gray black;
//! then the SDK's gray waveforms (`PSR` 0x3F, from the LUT registers)
//! lighten the grays, chosen by a code in *old* and *new*: 00 not at all
//! (black), 11 a little (dark gray), 10 more (light gray); white, already
//! white, takes 11 too. The next refresh after them drives every pixel
//! back to black or white, as a half refresh if a fast one was asked (the
//! SDK's waveforms for a fast one leave the grays a little light). The
//! first step needs the frame kept, the UC8279 waveforms of its own that
//! are not known yet: otherwise grays are shown in black and white.

use alloc::vec;
use alloc::vec::Vec;

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, PixelFormat, Rotation};

use crate::bands::{self, Window};
use crate::display::BUSY_TIMEOUT_MS;
pub use crate::display::{Error, PANEL_HEIGHT, PANEL_WIDTH, Planes, Refresh};

/// The screen PaperPaw draws upright (see [`Display::size`] for it
/// turned).
pub const WIDTH: u16 = PANEL_HEIGHT;
pub const HEIGHT: u16 = PANEL_WIDTH;

/// Lines the controller addresses; those past the panel's are white.
const ADDRESSED_LINES: u16 = 600;
/// Bytes of a line.
const LINE: usize = PANEL_WIDTH as usize / 8;

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
    pub const PLL: u8 = 0x30;
    pub const VCOM_DATA_INTERVAL: u8 = 0x50;
    pub const RESOLUTION: u8 = 0x61;
    pub const GATE_SOURCE_START: u8 = 0x65;
    pub const PARTIAL_WINDOW: u8 = 0x90;
    pub const PARTIAL_IN: u8 = 0x91;
    pub const PARTIAL_OUT: u8 = 0x92;
    pub const CASCADE: u8 = 0xE0;
    pub const GATE_SCAN: u8 = 0xE1;
    pub const POWER_SAVING: u8 = 0xE3;
    pub const TEMPERATURE: u8 = 0xE5;
}

/// Which UltraChip controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Uc8179,
    Uc8279,
}

/// Where the frame is kept between refreshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame {
    /// In the driver (48 KB, 144 KB with grays) and the controller.
    Kept,
    /// In the controller alone.
    InController,
}

/// What differs between the two controllers.
struct Sequences {
    /// The panel setting at start, and for the panel's own waveforms.
    panel: [u8; 2],
    panel_otp: [u8; 2],
    /// Set again after the power comes on (the UC8279 forgets it).
    panel_after_power_on: bool,
    /// The border and data interval while refreshing fully and fast, and
    /// at rest.
    interval_full: &'static [u8],
    interval_fast: &'static [u8],
    interval_idle: Option<&'static [u8]>,
    /// The frame rate, if not the controller's own.
    pll: Option<u8>,
    /// The booster and power saving, if not the controller's own.
    booster: Option<[u8; 4]>,
    power_saving: Option<u8>,
    /// The first line the panel shows; its lines go in order, or last
    /// first.
    first_line: u16,
    lines_back: bool,
    /// A fast refresh needs a window: without one, nothing develops.
    fast_in_window: bool,
    /// Waveforms for four grays from black and white.
    gray: Option<&'static [[u8; 13]; 5]>,
}

const UC8179: Sequences = Sequences {
    panel: [0x3F, 0x0A],
    panel_otp: [0x1F, 0x0A],
    panel_after_power_on: false,
    interval_full: &[0x29, 0x07],
    interval_fast: &[0x29, 0x07],
    interval_idle: Some(&[0xA9, 0x07]),
    pll: None,
    booster: Some([0x25, 0x25, 0x3C, 0x25]),
    power_saving: Some(0x22),
    first_line: 0,
    lines_back: false,
    fast_in_window: false,
    gray: Some(&GRAY_LUTS),
};

const UC8279: Sequences = Sequences {
    panel: [0x37, 0x4D],
    panel_otp: [0x17, 0x4D],
    panel_after_power_on: true,
    interval_full: &[0x97],
    interval_fast: &[0xD7],
    interval_idle: None,
    pll: Some(0x0E),
    booster: None,
    power_saving: None,
    first_line: ADDRESSED_LINES - PANEL_HEIGHT,
    lines_back: true,
    fast_in_window: true,
    gray: None,
};

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
    chip: Chip,
    /// The frame, line by line as the panel lies (480 lines of 100 bytes),
    /// if it is kept; empty if not.
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
    /// A band's bytes on their way, without a frame kept.
    buffer: Vec<u8>,
}

impl<SPI, DC, RST, BUSY, D> Display<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    pub fn new(
        chip: Chip,
        frame: Frame,
        spi: SPI,
        dc: DC,
        reset: RST,
        busy: BUSY,
        delay: D,
    ) -> Self {
        Self {
            spi,
            dc,
            reset,
            busy,
            delay,
            chip,
            frame: match frame {
                Frame::Kept => vec![0xFF; LINE * usize::from(PANEL_HEIGHT)],
                Frame::InController => Vec::new(),
            },
            old: Vec::new(),
            new: Vec::new(),
            gray_shown: false,
            old_valid: false,
            powered: false,
            rotation: Rotation::Deg0,
            pending: None,
            buffer: Vec::new(),
        }
    }

    pub fn chip(&self) -> Chip {
        self.chip
    }

    fn sequences(&self) -> &'static Sequences {
        match self.chip {
            Chip::Uc8179 => &UC8179,
            Chip::Uc8279 => &UC8279,
        }
    }

    fn kept(&self) -> bool {
        !self.frame.is_empty()
    }

    /// How the screen PaperPaw draws is turned: [`Rotation::Deg90`] with
    /// the frame kept is the X4 Pro turned anticlockwise (the right side
    /// of the upright screen on top), without it the X4 turned clockwise
    /// (as [`display::Display::set_rotation`](crate::display::Display::set_rotation)).
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.rotation = rotation;
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The width and height of the screen PaperPaw draws, turned.
    pub fn size(&self) -> (u16, u16) {
        match self.rotation {
            Rotation::Deg0 | Rotation::Deg180 => (WIDTH, HEIGHT),
            Rotation::Deg90 | Rotation::Deg270 => (HEIGHT, WIDTH),
        }
    }

    /// Resets and sets up the controller; the next refresh drives every
    /// pixel. Without the frame kept, both of the controller's frames are
    /// made white.
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
        let s = self.sequences();
        // The panel's own waveforms from its memory; 800 × 600 addressed.
        self.command(cmd::PANEL_SETTING, &s.panel)?;
        self.command(cmd::RESOLUTION, &[0x03, 0x20, 0x02, 0x58])?;
        self.command(cmd::GATE_SOURCE_START, &[0x00, 0x00, 0x00, 0x00])?;
        self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
        if let Some(booster) = s.booster {
            self.command(cmd::BOOSTER_SOFT_START, &booster)?;
        }
        if let Some(pll) = s.pll {
            self.command(cmd::PLL, &[pll])?;
        }
        self.command(cmd::GATE_SCAN, &[0x02])?;
        if let Some(power_saving) = s.power_saving {
            self.command(cmd::POWER_SAVING, &[power_saving])?;
        }
        if !self.kept() {
            self.fill(cmd::OLD, 0xFF)?;
            self.fill(cmd::NEW, 0xFF)?;
        }
        self.old_valid = false;
        self.gray_shown = false;
        self.powered = false;
        self.pending = None;
        Ok(())
    }

    /// Writes screen rows `top..top + rows.height` (a band from
    /// `Kernel::render`, the screen's width as turned) into the frame.
    /// Pixels lighter than middle gray are white. With the frame kept,
    /// it is sent whole at the refresh, and `planes` only say whether the
    /// band is new: what goes only into *shown* after a fast refresh is
    /// there already. Without, the band goes into the controller's
    /// frames `planes` names, starting at a row that is a multiple of 8.
    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        let kept = self.kept();
        if kept && !planes.new {
            return Ok(());
        }
        if top >= screen_height
            || rows.width != width
            || rows.height == 0
            || (!kept && !top.is_multiple_of(8))
        {
            return Err(Error::Rows);
        }
        let height = rows.height.min(screen_height - top);
        let light = bands::light(&rows);
        if !kept {
            return self.write_band(top, &rows, height, planes, &light);
        }
        for y in 0..height {
            for x in 0..width {
                let place = self.place(x, top + y);
                put(&mut self.frame, place, light(x, y));
            }
        }
        Ok(())
    }

    /// Writes a band into the controller's frames, through the window of
    /// its memory the band covers (rows past the band's end white).
    fn write_band(
        &mut self,
        top: u16,
        rows: &FrameView<'_>,
        height: u16,
        planes: Planes,
        light: &dyn Fn(u16, u16) -> bool,
    ) -> Result<(), Error> {
        let s = self.sequences();
        let mut window = Window::of(self.rotation, top, height);
        // The window's lines on a controller whose lines run the other way.
        let first = if s.lines_back {
            window.rows_back = true;
            s.first_line + PANEL_HEIGHT - window.y - window.height
        } else {
            s.first_line + window.y
        };
        let area = partial_window(window.x, window.width, first, window.height);
        for (plane, write) in [(planes.new, cmd::NEW), (planes.shown, cmd::OLD)] {
            if !plane {
                continue;
            }
            self.command(cmd::PARTIAL_IN, &[])?;
            self.command(cmd::PARTIAL_WINDOW, &area)?;
            self.wait_ready()?;
            self.command(write, &[])?;
            let Self {
                spi, dc, buffer, ..
            } = self;
            bands::send(&window, rows, height, light, true, buffer, &mut |bytes| {
                dc.set_high().map_err(|_| Error::Pin)?;
                spi.write(bytes).map_err(|_| Error::Spi)
            })?;
            self.command(cmd::PARTIAL_OUT, &[])?;
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
    /// only white white. Where grays cannot be shown, as by
    /// [`Display::write_rows`] into both frames.
    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        let (width, screen_height) = self.size();
        if top >= screen_height
            || rows.width != width
            || rows.height == 0
            || rows.format != PixelFormat::Gray2
        {
            return Err(Error::Rows);
        }
        if !self.grays() {
            return self.write_rows(top, rows, Planes::BOTH);
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

    /// Whether four grays can be shown: with the frame kept and the
    /// controller's waveforms for them known.
    fn grays(&self) -> bool {
        self.kept() && self.sequences().gray.is_some()
    }

    /// Shows what [`Display::write_gray_rows`] wrote: the frame in black
    /// and white with a fast refresh, then the grays lightened from it
    /// (about a second more). The next fast refresh drives every pixel.
    /// Where grays cannot be shown, a half refresh.
    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        let Some(luts) = self.sequences().gray.filter(|_| self.grays()) else {
            return self.refresh(Refresh::Half, false);
        };
        if self.old.is_empty() {
            return self.refresh(Refresh::Half, false);
        }
        self.refresh(Refresh::Fast, false)?;
        self.send(cmd::OLD, Plane::Old, false)?;
        self.send(cmd::NEW, Plane::New, false)?;
        self.command(cmd::PANEL_SETTING, &UC8179.panel)?;
        self.load_luts(luts)?;
        self.command(cmd::VCOM_DATA_INTERVAL, UC8179.interval_full)?;
        self.power_on()?;
        self.command(cmd::REFRESH, &[])?;
        let done = self.wait_ready();
        self.idle_interval()?;
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
        let s = self.sequences();
        // A fast refresh compares with the frame shown: without it, every
        // pixel is driven; after grays, as a half refresh.
        let fast = refresh == Refresh::Fast && self.old_valid && !self.gray_shown;
        let refresh = if refresh == Refresh::Fast && self.gray_shown {
            Refresh::Half
        } else {
            refresh
        };
        self.gray_shown = false;
        // Kept, the frame goes whole into the new frame, and into the one
        // shown inverted for a half refresh (every pixel changes), white
        // for a full one. Without, the bands are there; the frame shown
        // is made white for a full refresh.
        let kept = self.kept();
        if kept {
            self.send(cmd::NEW, Plane::Frame, false)?;
        }
        match refresh {
            _ if fast => {}
            Refresh::Half if kept => self.send(cmd::OLD, Plane::Frame, true)?,
            Refresh::Half => {}
            _ => self.fill(cmd::OLD, 0xFF)?,
        }
        let interval = if fast {
            s.interval_fast
        } else {
            s.interval_full
        };
        self.command(cmd::VCOM_DATA_INTERVAL, interval)?;
        self.command(cmd::CASCADE, &[0x02])?;
        self.command(cmd::TEMPERATURE, &[if fast { 0x5A } else { 0x1E }])?;
        if !s.panel_after_power_on {
            self.command(cmd::PANEL_SETTING, &s.panel_otp)?;
        }
        if fast {
            self.command(cmd::POWER_OFF_SEQUENCE, &[0x20])?;
            self.command(cmd::GATE_SCAN, &[0x02])?;
        }
        self.power_on()?;
        if fast {
            self.command(cmd::PARTIAL_IN, &[])?;
            // The panel's lines whole: without the frame kept, the window
            // of the last band would be left.
            if s.fast_in_window || !kept {
                let window = partial_window(0, PANEL_WIDTH, s.first_line, PANEL_HEIGHT);
                self.command(cmd::PARTIAL_WINDOW, &window)?;
                self.wait_ready()?;
            }
        }
        if s.panel_after_power_on {
            self.command(cmd::PANEL_SETTING, &s.panel_otp)?;
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

    /// The border and data interval at rest, where the controller has one.
    fn idle_interval(&mut self) -> Result<(), Error> {
        match self.sequences().interval_idle {
            Some(interval) => self.command(cmd::VCOM_DATA_INTERVAL, interval),
            None => Ok(()),
        }
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
    /// then keeps the frame as the one shown (the controller copies it
    /// too, but the frame kept goes again, as the stock firmware sends
    /// it).
    pub fn wait(&mut self) -> Result<(), Error> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        let done = self.wait_ready();
        if pending.partial {
            self.command(cmd::PARTIAL_OUT, &[])?;
        }
        self.idle_interval()?;
        if let Err(e) = done {
            self.old_valid = false;
            return Err(e);
        }
        if self.kept() {
            self.send(cmd::OLD, Plane::Frame, false)?;
        }
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

    /// Sends `plane` into memory `command`, inverted if asked, its lines
    /// where the panel shows them (on a UC8179 last first, then the lines
    /// past the panel white; on a UC8279 those first, then in order).
    fn send(&mut self, command: u8, plane: Plane, invert: bool) -> Result<(), Error> {
        self.command(command, &[])?;
        self.dc.set_high().map_err(|_| Error::Pin)?;
        let s = self.sequences();
        let white = [0xFF; LINE];
        let blank = ADDRESSED_LINES - PANEL_HEIGHT;
        if s.lines_back {
            for _ in 0..blank {
                self.spi.write(&white).map_err(|_| Error::Spi)?;
            }
        }
        let plane = match plane {
            Plane::Frame => &self.frame,
            Plane::Old => &self.old,
            Plane::New => &self.new,
        };
        let mut line = [0u8; LINE];
        for i in 0..usize::from(PANEL_HEIGHT) {
            let index = if s.lines_back {
                i
            } else {
                usize::from(PANEL_HEIGHT) - 1 - i
            };
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
        if !s.lines_back {
            for _ in 0..blank {
                self.spi.write(&white).map_err(|_| Error::Spi)?;
            }
        }
        Ok(())
    }

    /// Fills memory `command` with `value`.
    fn fill(&mut self, command: u8, value: u8) -> Result<(), Error> {
        self.command(command, &[])?;
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

/// The window of the controller's memory (`PTL`) of columns `x..x +
/// width` (multiples of 8) and lines `y..y + height`, the lines outside
/// scanned too.
fn partial_window(x: u16, width: u16, y: u16, height: u16) -> [u8; 9] {
    let (last_x, last_y) = (x + width - 1, y + height - 1);
    [
        (x >> 8) as u8,
        (x & 0xF8) as u8,
        (last_x >> 8) as u8,
        (last_x & 0xF8) as u8 | 0x07,
        (y >> 8) as u8,
        y as u8,
        (last_y >> 8) as u8,
        last_y as u8,
        0x01,
    ]
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

/// The upright screen's pixel of the turned screen's `(x, y)`, with the
/// frame kept (the X4 Pro's turns).
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
/// touch on the upright screen lands on the screen PaperPaw draws.
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
    use paperpaw_platform::PixelFormat;

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
        /// Each command with its data, in order.
        log: Vec<(u8, Vec<u8>)>,
        /// The memories as addressed (600 lines of 100 bytes), written in
        /// the window while in partial mode.
        ram_new: Vec<u8>,
        ram_old: Vec<u8>,
        partial: bool,
        window: [u8; 9],
        /// Where the next byte goes: line, byte of the line.
        cursor: (usize, usize),
    }

    impl Controller {
        fn byte(&mut self, b: u8) {
            if !self.data {
                self.command = b;
                self.commands.push(b);
                self.log.push((b, Vec::new()));
                match b {
                    cmd::NEW => self.new.clear(),
                    cmd::OLD => self.old.clear(),
                    cmd::REFRESH => self.refreshed.push((self.old.clone(), self.new.clone())),
                    cmd::PARTIAL_IN => self.partial = true,
                    cmd::PARTIAL_OUT => self.partial = false,
                    _ => {}
                }
                if matches!(b, cmd::NEW | cmd::OLD) {
                    let (first, _, line, _) = self.area();
                    self.cursor = (line, first);
                }
                return;
            }
            if let Some((_, data)) = self.log.last_mut() {
                data.push(b);
            }
            match self.command {
                cmd::NEW | cmd::OLD => {
                    if self.command == cmd::NEW {
                        self.new.push(b);
                    } else {
                        self.old.push(b);
                    }
                    self.store(b);
                }
                cmd::TEMPERATURE => self.temperatures.push(b),
                cmd::PARTIAL_WINDOW => {
                    let at = self.log.last().map_or(0, |(_, d)| d.len() - 1);
                    self.window[at] = b;
                }
                _ => {}
            }
        }

        /// The bytes and lines written to: first and last byte, first and
        /// last line.
        fn area(&self) -> (usize, usize, usize, usize) {
            if !self.partial {
                return (0, LINE - 1, 0, usize::from(ADDRESSED_LINES) - 1);
            }
            let w = &self.window;
            let word = |high: u8, low: u8| usize::from(high) << 8 | usize::from(low);
            (
                word(w[0], w[1]) / 8,
                word(w[2], w[3]) / 8,
                word(w[4], w[5]),
                word(w[6], w[7]),
            )
        }

        fn store(&mut self, b: u8) {
            let (first, last, _, last_line) = self.area();
            let (line, at) = self.cursor;
            assert!(line <= last_line, "written past the window");
            if self.ram_new.is_empty() {
                let size = LINE * usize::from(ADDRESSED_LINES);
                self.ram_new = vec![0x55; size];
                self.ram_old = vec![0x55; size];
            }
            let ram = if self.command == cmd::NEW {
                &mut self.ram_new
            } else {
                &mut self.ram_old
            };
            ram[line * LINE + at] = b;
            self.cursor = if at == last {
                (line + 1, first)
            } else {
                (line, at + 1)
            };
        }

        /// The data of the last `command` sent.
        fn last(&self, command: u8) -> Option<&[u8]> {
            self.log
                .iter()
                .rev()
                .find(|(c, _)| *c == command)
                .map(|(_, d)| &d[..])
        }

        /// The commands from the last `from` on.
        fn since(&self, from: u8) -> Vec<u8> {
            let start = self.log.iter().rposition(|(c, _)| *c == from).unwrap_or(0);
            self.log[start..].iter().map(|(c, _)| *c).collect()
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
        display_of(Chip::Uc8179, Frame::Kept)
    }

    fn display_of(chip: Chip, frame: Frame) -> (Simulated, Rc<RefCell<Controller>>) {
        let c = Rc::new(RefCell::new(Controller::default()));
        let mut d = Display::new(chip, frame, Spi(c.clone()), Dc(c.clone()), Pin, Pin, Delay);
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

    /// The upright screen's pixel `(x, y)` in the controller's memory as
    /// addressed: its line and column. Without the frame kept, the screen
    /// turns as the X4's (see `bands`).
    fn addressed(chip: Chip, rotation: Rotation, x: u16, y: u16) -> (usize, u16) {
        // Where the SSD1677 has it: its row and column.
        let (column, row) = match rotation {
            Rotation::Deg0 => (y, x),
            Rotation::Deg90 => (PANEL_WIDTH - 1 - x, y),
            Rotation::Deg180 => (PANEL_WIDTH - 1 - y, PANEL_HEIGHT - 1 - x),
            Rotation::Deg270 => (x, PANEL_HEIGHT - 1 - y),
        };
        let line = match chip {
            Chip::Uc8179 => row,
            Chip::Uc8279 => ADDRESSED_LINES - 1 - row,
        };
        (usize::from(line), column)
    }

    fn white(ram: &[u8], (line, column): (usize, u16)) -> bool {
        ram[line * LINE + usize::from(column / 8)] & (0x80 >> (column % 8)) != 0
    }

    #[test]
    fn a_uc8279_shows_its_last_lines_in_order() {
        let (mut d, c) = display_of(Chip::Uc8279, Frame::Kept);
        // Screen pixel (5, 3) white: frame line 474, sent after the 120
        // lines the panel does not show.
        let band = band_with(5, 3);
        d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
        d.refresh(Refresh::Full, false).unwrap();
        let c = c.borrow();
        assert!(c.new[..120 * LINE].iter().all(|b| *b == 0xFF));
        let line = 120 + 474;
        assert_eq!(c.new[line * LINE] & 0x10, 0x10);
        assert_eq!(c.new[line * LINE] & 0x08, 0);
        // Where the bands go without a frame: the same line.
        assert_eq!(addressed(Chip::Uc8279, Rotation::Deg0, 5, 3), (594, 3));
        assert_eq!(addressed(Chip::Uc8179, Rotation::Deg0, 5, 3), (5, 3));
    }

    #[test]
    fn a_uc8279_is_set_again_after_its_power_comes_on() {
        let (mut d, c) = display_of(Chip::Uc8279, Frame::Kept);
        assert_eq!(c.borrow().log[0], (cmd::PANEL_SETTING, vec![0x37, 0x4D]));
        assert_eq!(c.borrow().last(cmd::PLL), Some(&[0x0E][..]));
        d.refresh(Refresh::Full, false).unwrap();
        assert_eq!(
            c.borrow().since(cmd::VCOM_DATA_INTERVAL),
            [
                cmd::VCOM_DATA_INTERVAL,
                cmd::CASCADE,
                cmd::TEMPERATURE,
                cmd::POWER_ON,
                cmd::PANEL_SETTING,
                cmd::REFRESH,
                cmd::OLD,
            ]
        );
        assert_eq!(c.borrow().last(cmd::VCOM_DATA_INTERVAL), Some(&[0x97][..]));
        // Fast: within a window of the lines the panel shows.
        d.refresh(Refresh::Fast, false).unwrap();
        let c = c.borrow();
        assert_eq!(c.temperatures, [0x1E, 0x5A]);
        assert_eq!(c.last(cmd::VCOM_DATA_INTERVAL), Some(&[0xD7][..]));
        assert_eq!(
            c.last(cmd::PARTIAL_WINDOW),
            Some(&[0x00, 0x00, 0x03, 0x1F, 0x00, 0x78, 0x02, 0x57, 0x01][..])
        );
        assert_eq!(c.last(cmd::PANEL_SETTING), Some(&[0x17, 0x4D][..]));
        let order = c.since(cmd::POWER_OFF_SEQUENCE);
        let at = |command| order.iter().position(|&c| c == command).unwrap();
        assert!(at(cmd::PARTIAL_IN) < at(cmd::PARTIAL_WINDOW));
        assert!(at(cmd::PARTIAL_WINDOW) < at(cmd::PANEL_SETTING));
        assert!(at(cmd::PANEL_SETTING) < at(cmd::REFRESH));
        assert!(at(cmd::REFRESH) < at(cmd::PARTIAL_OUT));
    }

    #[test]
    fn grays_without_their_waveforms_are_black_and_white() {
        for (chip, frame) in [
            (Chip::Uc8279, Frame::Kept),
            (Chip::Uc8179, Frame::InController),
        ] {
            let (mut d, c) = display_of(chip, frame);
            let band = gray_band();
            d.write_gray_rows(0, gray_view(&band)).unwrap();
            d.refresh_gray().unwrap();
            let c = c.borrow();
            // One refresh, every pixel driven, no waveforms of our own.
            assert_eq!(c.refreshed.len(), 1, "{chip:?} {frame:?}");
            assert_eq!(c.temperatures, [0x1E]);
            assert!(!c.commands.contains(&0x21));
            // Black and dark gray black, light gray and white white.
            let levels: Vec<bool> = (0..4)
                .map(|x| white(&c.ram_new, addressed(chip, Rotation::Deg0, x, 0)))
                .collect();
            assert_eq!(levels, [false, false, true, true], "{chip:?} {frame:?}");
        }
    }

    #[test]
    fn without_a_frame_bands_go_into_the_controller_through_windows() {
        let ink = |x: u16, y: u16| (x * 7 + y * 3).is_multiple_of(5) || x == y;
        for chip in [Chip::Uc8179, Chip::Uc8279] {
            for rotation in [
                Rotation::Deg0,
                Rotation::Deg90,
                Rotation::Deg180,
                Rotation::Deg270,
            ] {
                let (mut d, c) = display_of(chip, Frame::InController);
                d.set_rotation(rotation);
                let (width, height) = d.size();
                // Black and white into the new frame, gray into the one
                // shown.
                let stride = usize::from(width) / 8;
                let mut mono = vec![0xFFu8; stride * 16];
                let gray_stride = usize::from(width) / 4;
                let mut gray = vec![0xFFu8; gray_stride * 16];
                for y in 0..16 {
                    for x in 0..width {
                        if ink(x, y) {
                            let y = usize::from(y);
                            mono[y * stride + usize::from(x) / 8] &= !(0x80 >> (x % 8));
                            gray[y * gray_stride + usize::from(x) / 4] &=
                                !(0b11 << (6 - 2 * (x % 4)));
                        }
                    }
                }
                let view = |data, format, stride| FrameView {
                    width,
                    height: 16,
                    format,
                    stride,
                    data,
                };
                let mono = view(&mono, PixelFormat::Mono1, stride);
                d.write_rows(96, mono, Planes::NEW).unwrap();
                let gray = view(&gray, PixelFormat::Gray2, gray_stride);
                d.write_rows(96, gray, Planes::SHOWN).unwrap();
                let c = c.borrow();
                assert!(!c.partial, "partial mode left");
                for y in 96..112 {
                    for x in 0..width {
                        let at = addressed(chip, rotation, x, y);
                        let expected = !ink(x, y - 96);
                        let pixel = (chip, rotation, x, y);
                        assert_eq!(white(&c.ram_new, at), expected, "{pixel:?}");
                        assert_eq!(white(&c.ram_old, at), expected, "{pixel:?}");
                    }
                }
                // The rest stays white, as made at the start.
                for (x, y) in [(0, 95), (width - 1, 112), (width / 2, height - 1)] {
                    let at = addressed(chip, rotation, x, y);
                    assert!(white(&c.ram_new, at), "{chip:?} {rotation:?}");
                }
                drop(c);
                assert_eq!(
                    d.write_rows(4, mono, Planes::NEW),
                    Err(Error::Rows),
                    "not aligned"
                );
            }
        }
    }

    #[test]
    fn without_a_frame_refreshes_send_no_frame() {
        let (mut d, c) = display_of(Chip::Uc8179, Frame::InController);
        let band = band_with(0, 0);
        d.write_rows(0, view(&band, 8), Planes::BOTH).unwrap();
        // The first, full: the frame shown made white; nothing sent after.
        d.refresh(Refresh::Fast, false).unwrap();
        assert_eq!(
            c.borrow().since(cmd::REFRESH),
            [cmd::REFRESH, cmd::VCOM_DATA_INTERVAL]
        );
        assert!(c.borrow().refreshed[0].0.iter().all(|b| *b == 0xFF));
        // Fast, within the panel's lines (not the last band's window).
        d.write_rows(0, view(&band, 8), Planes::NEW).unwrap();
        d.refresh(Refresh::Fast, false).unwrap();
        let c = c.borrow();
        assert_eq!(c.temperatures, [0x1E, 0x5A]);
        assert_eq!(
            c.last(cmd::PARTIAL_WINDOW),
            Some(&[0x00, 0x00, 0x03, 0x1F, 0x00, 0x00, 0x01, 0xDF, 0x01][..])
        );
        let after = c.since(cmd::REFRESH);
        assert!(!after.contains(&cmd::OLD) && !after.contains(&cmd::NEW));
    }

    #[test]
    fn partial_windows_are_in_whole_bytes() {
        assert_eq!(
            partial_window(96, 16, 120, 480),
            [0x00, 0x60, 0x00, 0x6F, 0x00, 0x78, 0x02, 0x57, 0x01]
        );
    }
}
