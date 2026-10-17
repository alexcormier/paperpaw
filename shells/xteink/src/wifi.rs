//! Wi-Fi, with esp-radio (Espressif's radio firmware with a Rust driver
//! around it, whose interface is still a beta before its 1.0: it is used
//! here alone, so a change of it changes this file). The radio needs a
//! scheduler that switches tasks (esp-rtos); both start the first time
//! Wi-Fi is wanted and run on until the X4 next starts.

use alloc::vec::Vec;
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use esp_hal::peripherals::{FROM_CPU_INTR0, WIFI};
use esp_hal::timer::timg::Timer;
use esp_radio::wifi::ap::AccessPointInfo;
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{Config, WifiController, WifiError};

/// The longest anything of the radio is waited for here.
const WAIT_MS: u64 = 15_000;

#[derive(Debug)]
pub enum Error {
    /// What the radio said (shown in the log).
    Radio(#[allow(dead_code)] WifiError),
    /// It took longer than [`WAIT_MS`].
    Timeout,
}

impl From<WifiError> for Error {
    fn from(e: WifiError) -> Self {
        Error::Radio(e)
    }
}

/// What the radio needs, until it starts.
pub struct Parts {
    pub timer: Timer<'static>,
    pub interrupt: FROM_CPU_INTR0<'static>,
    pub wifi: WIFI<'static>,
}

pub struct Wifi {
    controller: WifiController<'static>,
}

impl Wifi {
    /// Starts the scheduler and the radio, as a station (a client of
    /// networks).
    pub fn start(parts: Parts) -> Result<Self, WifiError> {
        use crate::power::{at, mark};
        mark(at::RADIO_SCHEDULER);
        esp_rtos::start(parts.timer, parts.interrupt);
        mark(at::RADIO_DRIVER);
        log::info!("wifi: driver");
        let mut controller = WifiController::new(parts.wifi, Default::default())?;
        mark(at::RADIO_STATION);
        log::info!("wifi: station");
        controller.set_config(&Config::Station(StationConfig::default()))?;
        mark(at::RADIO_STARTED);
        log::info!("wifi: started");
        Ok(Self { controller })
    }

    /// The networks in reach, at most `max`.
    pub fn scan(&mut self, max: usize) -> Result<Vec<AccessPointInfo>, Error> {
        crate::power::mark(crate::power::at::RADIO_SCAN);
        let config = ScanConfig::default().with_max(max);
        Ok(block_on(self.controller.scan_async(&config))??)
    }
}

/// The radio's firmware (libphy) locks the chip's internal I2C bus with
/// these while it calibrates; it brings empty ones, which an operating
/// system replaces. The bus is shared with the ADC, which reads the
/// buttons and the battery in an interrupt: that interrupt must not come
/// in between, so these hold all interrupts off (nested, by a count).
static mut PHY_I2C_LOCK: (u32, Option<critical_section::RestoreState>) = (0, None);

#[unsafe(no_mangle)]
extern "C" fn phy_i2c_enter_critical() {
    // SAFETY: the critical section is entered first; then nothing else
    // runs until it is left (one core).
    unsafe {
        let state = critical_section::acquire();
        let lock = &mut *core::ptr::addr_of_mut!(PHY_I2C_LOCK);
        if lock.0 == 0 {
            lock.1 = Some(state);
        }
        lock.0 += 1;
    }
}

#[unsafe(no_mangle)]
extern "C" fn phy_i2c_exit_critical() {
    // SAFETY: as in `phy_i2c_enter_critical`; still inside it.
    unsafe {
        let lock = &mut *core::ptr::addr_of_mut!(PHY_I2C_LOCK);
        if lock.0 == 0 {
            return;
        }
        lock.0 -= 1;
        if lock.0 == 0
            && let Some(state) = lock.1.take()
        {
            critical_section::release(state);
        }
    }
}

/// Waits for `future` here (QuireInk's firmware does not run async code),
/// at most [`WAIT_MS`]: the radio's tasks run meanwhile, switched to by the
/// scheduler.
fn block_on<F: Future>(future: F) -> Result<F::Output, Error> {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(WAIT_MS);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return Ok(value);
        }
        if esp_hal::time::Instant::now() > until {
            return Err(Error::Timeout);
        }
        crate::power::feed();
    }
}
