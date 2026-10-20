//! Asks the panel which controller it has (`paperpaw_x4::probe`), on its
//! lines driven by hand before the SPI bus takes them. Every board asks,
//! with its own pins.

use embedded_hal::digital::{ErrorType, InputPin, OutputPin};
use esp_hal::delay::Delay;
use esp_hal::gpio::{self, Flex, Input, InputConfig, Level, Output, OutputConfig, Pull};
use paperpaw_x4::probe::{self, DataLine, Lines, Probe};

/// The panel's data line, which the controller answers on.
struct Data<'d>(Flex<'d>);

impl ErrorType for Data<'_> {
    type Error = core::convert::Infallible;
}

impl OutputPin for Data<'_> {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.set_low();
        Ok(())
    }
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.set_high();
        Ok(())
    }
}

impl InputPin for Data<'_> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.is_high())
    }
    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.is_low())
    }
}

impl DataLine for Data<'_> {
    fn listen(&mut self) {
        self.0.set_output_enable(false);
        self.0
            .apply_input_config(&InputConfig::default().with_pull(Pull::Up));
        self.0.set_input_enable(true);
    }
    fn drive(&mut self) {
        self.0.set_input_enable(false);
        self.0.apply_output_config(&OutputConfig::default());
        self.0.set_output_enable(true);
    }
}

/// The panel's answers, on its clock, data, select, data/command, reset
/// and busy lines. The lines are let go after.
pub fn ask<'d>(
    clock: impl gpio::OutputPin + 'd,
    data: impl gpio::OutputPin + gpio::InputPin + 'd,
    select: impl gpio::OutputPin + 'd,
    dc: impl gpio::OutputPin + 'd,
    reset: impl gpio::OutputPin + 'd,
    busy: impl gpio::InputPin + 'd,
    mut delay: Delay,
) -> Probe {
    let mut data = Data(Flex::new(data));
    data.drive();
    let mut lines = Lines {
        clock: Output::new(clock, Level::Low, OutputConfig::default()),
        data,
        select: Output::new(select, Level::High, OutputConfig::default()),
        dc: Output::new(dc, Level::Low, OutputConfig::default()),
        reset: Output::new(reset, Level::High, OutputConfig::default()),
        busy: Input::new(busy, InputConfig::default()),
    };
    probe::probe(&mut lines, &mut delay)
}
