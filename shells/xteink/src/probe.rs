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
/// and busy lines, the reset held `reset_ms` (see
/// [`probe::RESET_MS`]). The lines are let go after.
#[expect(
    clippy::too_many_arguments,
    reason = "the panel's six lines, the delay and the reset's length"
)]
pub fn ask<'d>(
    clock: impl gpio::OutputPin + 'd,
    data: impl gpio::OutputPin + gpio::InputPin + 'd,
    select: impl gpio::OutputPin + 'd,
    dc: impl gpio::OutputPin + 'd,
    reset: impl gpio::OutputPin + 'd,
    busy: impl gpio::InputPin + 'd,
    mut delay: Delay,
    reset_ms: u32,
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
    let probe = probe::probe(&mut lines, &mut delay, reset_ms);
    // The waveform lab: what a UC8179's OTP holds, read while the lines are
    // still ours (a hundred milliseconds more at start, developer builds).
    #[cfg(feature = "waveform-lab")]
    if probe.controller() == Some(probe::Controller::Uc8179) {
        let mut otp = alloc::vec![0u8; paperpaw_x4::otp::READ_LEN];
        probe::read_otp(&mut lines, &mut delay, &mut otp);
        critical_section::with(|cs| OTP.borrow_ref_mut(cs).replace(otp));
    }
    probe
}

/// The UC8179's OTP, as far as it was read (see [`ask`]).
#[cfg(feature = "waveform-lab")]
static OTP: critical_section::Mutex<core::cell::RefCell<Option<alloc::vec::Vec<u8>>>> =
    critical_section::Mutex::new(core::cell::RefCell::new(None));

/// Tells the log what the OTP holds, if it was read: the vendor's own
/// waveforms, to compare with the lab's (see `paperpaw_x4::otp`).
#[cfg(feature = "waveform-lab")]
pub fn log_otp() {
    let otp = critical_section::with(|cs| OTP.borrow_ref_mut(cs).take());
    if let Some(otp) = otp {
        for line in paperpaw_x4::otp::describe(&otp) {
            log::info!("{line}");
        }
    }
}

/// The controller the factory wrote down for the panel, in the factory's
/// NVS (`hw_calib`, `screenType`; see `paperpaw_x4::nvs`), if it did:
/// only read. The X4 Classic's partition table keeps that NVS whole.
#[cfg(feature = "x4-classic")]
pub fn factory_controller(flash: &crate::ota::Flash) -> Option<probe::Controller> {
    use alloc::boxed::Box;
    use esp_bootloader_esp_idf::partitions::{
        self, DataPartitionSubType, PARTITION_TABLE_MAX_LEN, PartitionType,
    };
    use paperpaw_x4::nvs;

    let mut flash = flash.borrow_mut();
    let mut table = Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let (offset, len) = partitions::read_partition_table(&mut flash, &mut table[..])
        .ok()?
        .find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
        .ok()
        .flatten()
        .map(|p| (p.offset(), p.len()))?;
    let pages = len as usize / nvs::PAGE;
    let mut page = alloc::vec![0u8; nvs::PAGE];
    let mut read =
        |i: usize, page: &mut [u8]| flash.read(offset + (i * nvs::PAGE) as u32, page).is_ok();
    let index = (0..pages).find_map(|i| {
        read(i, &mut page)
            .then(|| nvs::namespace(&page, "hw_calib"))
            .flatten()
    })?;
    let screen_type = (0..pages).find_map(|i| {
        read(i, &mut page)
            .then(|| nvs::integer(&page, index, "screenType"))
            .flatten()
    })?;
    let controller = probe::factory_controller(screen_type);
    log::info!("display: the factory's screen type {screen_type} ({controller:?})");
    controller
}
