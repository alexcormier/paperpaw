//! The panel of an X4 or X4 Pro, whichever controller drives it: the
//! driver for the controller found at start ([`probe`](crate::probe)),
//! with the calls the firmware makes of any.

use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;
use paperpaw_platform::{FrameView, Rotation};

use crate::display::{self, Error, Planes, Refresh};
use crate::probe::Controller;
use crate::uc8179::{self, Chip, Frame};

/// Which device the panel is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// The X4: no memory for a frame of its own; turned clockwise on its
    /// side ([`Rotation::Deg90`], its buttons on the left).
    X4,
    /// The X4 Pro: a frame kept in its PSRAM; turned the other way on
    /// [`Rotation::Deg90`] (see [`uc8179::turned`]).
    X4Pro,
}

/// A panel, its controller's driver.
#[derive(Debug)]
pub enum Panel<SPI, DC, RST, BUSY, D> {
    Ssd1677 {
        display: display::Display<SPI, DC, RST, BUSY, D>,
        /// The X4 Pro's sides: [`Rotation::Deg90`] and
        /// [`Rotation::Deg270`] are the other way round.
        sides_swapped: bool,
    },
    UltraChip(uc8179::Display<SPI, DC, RST, BUSY, D>),
}

/// The same call to whichever driver.
macro_rules! each {
    ($panel:expr, $display:ident => $call:expr) => {
        match $panel {
            Panel::Ssd1677 {
                display: $display, ..
            } => $call,
            Panel::UltraChip($display) => $call,
        }
    };
}

impl<SPI, DC, RST, BUSY, D> Panel<SPI, DC, RST, BUSY, D>
where
    SPI: SpiDevice,
    DC: OutputPin,
    RST: OutputPin,
    BUSY: InputPin,
    D: DelayNs,
{
    /// The driver for `controller` in `device`, on the panel's SPI device
    /// and pins.
    pub fn new(
        device: Device,
        controller: Controller,
        spi: SPI,
        dc: DC,
        reset: RST,
        busy: BUSY,
        delay: D,
    ) -> Self {
        let chip = match controller {
            Controller::Ssd1677 => {
                return Panel::Ssd1677 {
                    display: display::Display::new(spi, dc, reset, busy, delay),
                    sides_swapped: device == Device::X4Pro,
                };
            }
            Controller::Uc8179 => Chip::Uc8179,
            Controller::Uc8279 => Chip::Uc8279,
        };
        let frame = match device {
            Device::X4 => Frame::InController,
            Device::X4Pro => Frame::Kept,
        };
        Panel::UltraChip(uc8179::Display::new(
            chip, frame, spi, dc, reset, busy, delay,
        ))
    }

    /// The controller driven.
    pub fn controller(&self) -> Controller {
        match self {
            Panel::Ssd1677 { .. } => Controller::Ssd1677,
            Panel::UltraChip(display) => match display.chip() {
                Chip::Uc8179 => Controller::Uc8179,
                Chip::Uc8279 => Controller::Uc8279,
            },
        }
    }

    /// How the screen PaperPaw draws is turned, as the device's setting
    /// says (see [`Device`]).
    pub fn set_rotation(&mut self, rotation: Rotation) {
        match self {
            Panel::Ssd1677 {
                display,
                sides_swapped,
            } => display.set_rotation(swapped(rotation, *sides_swapped)),
            Panel::UltraChip(display) => display.set_rotation(rotation),
        }
    }

    pub fn rotation(&self) -> Rotation {
        match self {
            Panel::Ssd1677 {
                display,
                sides_swapped,
            } => swapped(display.rotation(), *sides_swapped),
            Panel::UltraChip(display) => display.rotation(),
        }
    }

    /// The width and height of the screen PaperPaw draws, turned.
    pub fn size(&self) -> (u16, u16) {
        each!(self, d => d.size())
    }

    pub fn init(&mut self) -> Result<(), Error> {
        each!(self, d => d.init())
    }

    pub fn write_rows(
        &mut self,
        top: u16,
        rows: FrameView<'_>,
        planes: Planes,
    ) -> Result<(), Error> {
        each!(self, d => d.write_rows(top, rows, planes))
    }

    pub fn write_gray_rows(&mut self, top: u16, rows: FrameView<'_>) -> Result<(), Error> {
        each!(self, d => d.write_gray_rows(top, rows))
    }

    pub fn refresh_gray(&mut self) -> Result<(), Error> {
        each!(self, d => d.refresh_gray())
    }

    pub fn refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        each!(self, d => d.refresh(refresh, power_off))
    }

    pub fn start_refresh(&mut self, refresh: Refresh, power_off: bool) -> Result<(), Error> {
        each!(self, d => d.start_refresh(refresh, power_off))
    }

    pub fn wait(&mut self) -> Result<(), Error> {
        each!(self, d => d.wait())
    }

    pub fn sleep(&mut self) -> Result<(), Error> {
        each!(self, d => d.sleep())
    }
}

/// `rotation` with its sides the other way round, if `swap`.
fn swapped(rotation: Rotation, swap: bool) -> Rotation {
    match rotation {
        Rotation::Deg90 if swap => Rotation::Deg270,
        Rotation::Deg270 if swap => Rotation::Deg90,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::Infallible;
    use embedded_hal::spi::{ErrorType, Operation};

    struct Spi;
    struct Pin;
    struct Delay;

    impl ErrorType for Spi {
        type Error = Infallible;
    }
    impl SpiDevice for Spi {
        fn transaction(&mut self, _: &mut [Operation<'_, u8>]) -> Result<(), Infallible> {
            Ok(())
        }
    }
    impl embedded_hal::digital::ErrorType for Pin {
        type Error = Infallible;
    }
    impl OutputPin for Pin {
        fn set_low(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
        fn set_high(&mut self) -> Result<(), Infallible> {
            Ok(())
        }
    }
    impl InputPin for Pin {
        fn is_high(&mut self) -> Result<bool, Infallible> {
            Ok(true)
        }
        fn is_low(&mut self) -> Result<bool, Infallible> {
            Ok(false)
        }
    }
    impl DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }

    fn panel(device: Device, controller: Controller) -> Panel<Spi, Pin, Pin, Pin, Delay> {
        Panel::new(device, controller, Spi, Pin, Pin, Pin, Delay)
    }

    #[test]
    fn each_controller_its_driver() {
        for device in [Device::X4, Device::X4Pro] {
            for controller in [Controller::Ssd1677, Controller::Uc8179, Controller::Uc8279] {
                let panel = panel(device, controller);
                assert_eq!(panel.controller(), controller);
                let ultra = matches!(panel, Panel::UltraChip(_));
                assert_eq!(ultra, controller != Controller::Ssd1677);
            }
        }
    }

    #[test]
    fn an_x4_pro_turns_its_way_whichever_its_controller() {
        let mut ssd = panel(Device::X4Pro, Controller::Ssd1677);
        ssd.set_rotation(Rotation::Deg90);
        assert_eq!(ssd.rotation(), Rotation::Deg90);
        assert_eq!(ssd.size(), (800, 480));
        let Panel::Ssd1677 { display, .. } = &ssd else {
            unreachable!()
        };
        assert_eq!(display.rotation(), Rotation::Deg270);
        let mut x4 = panel(Device::X4, Controller::Ssd1677);
        x4.set_rotation(Rotation::Deg90);
        let Panel::Ssd1677 { display, .. } = &x4 else {
            unreachable!()
        };
        assert_eq!(display.rotation(), Rotation::Deg90);
    }
}
