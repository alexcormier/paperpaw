//! Wi-Fi, with esp-radio (Espressif's radio firmware with a Rust driver
//! around it, whose interface is still a beta before its 1.0: it is used
//! here alone, so a change of it changes this file). The radio needs a
//! scheduler that switches tasks (esp-rtos); both start the first time
//! Wi-Fi is wanted and run on until the X4 next starts.
//!
//! Joining a network takes seconds: it goes on in the background, polled
//! by the main loop ([`Wifi::poll`]), while the X4 stays usable.
//!
//! The networks joined are kept with QuireInk's passwords and keys (see
//! `quireink_sdk::secrets`), as `wifi.<name>`: the settings list them, to
//! change or remove.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::{Pin, pin};
use core::task::{Context, Poll, Waker};

use esp_hal::peripherals::{FROM_CPU_INTR0, WIFI};
use esp_hal::timer::timg::Timer;
use esp_radio::wifi::ap::AccessPointInfo;
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::{ConnectedInfo, StationConfig};
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config, ConnectionError, DisconnectReason, WifiController,
    WifiError,
};

/// The longest anything of the radio is waited for here.
const WAIT_MS: u64 = 15_000;
/// The longest a try to join a network is waited for to stop.
const STOP_MS: u64 = 2_000;

/// `future`'s outcome, or `None` if it takes longer than `ms`.
async fn within<F: Future>(ms: u64, future: F) -> Option<F::Output> {
    let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(ms);
    let mut future = pin!(future);
    core::future::poll_fn(|cx| match future.as_mut().poll(cx) {
        Poll::Ready(value) => Poll::Ready(Some(value)),
        // Polled again and again (from the main loop): no waking needed.
        Poll::Pending if esp_hal::time::Instant::now() > until => Poll::Ready(None),
        Poll::Pending => Poll::Pending,
    })
    .await
}

#[derive(Debug)]
pub enum Error {
    /// What the radio said (shown in the log).
    Radio(#[allow(dead_code)] WifiError),
    /// It took longer than [`WAIT_MS`].
    Timeout,
    /// The network did not let the X4 in, and why (e.g. a wrong password).
    Refused(#[allow(dead_code)] DisconnectReason),
    /// Joining a network is going on.
    Busy,
}

impl From<ConnectionError> for Error {
    fn from(e: ConnectionError) -> Self {
        match e {
            ConnectionError::Failed(info) => Error::Refused(info.reason),
            ConnectionError::WifiError(e) => Error::Radio(e),
            _ => Error::Radio(WifiError::Unsupported),
        }
    }
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

/// The prefix of the ids of networks' passwords among the secrets.
pub const SECRET_PREFIX: &str = "wifi.";

/// Joining a network, which holds the controller until it is done.
type Joining = Pin<Box<dyn Future<Output = (Result<ConnectedInfo, Error>, Controller)> + 'static>>;
type Controller = &'static mut WifiController<'static>;

enum State {
    Idle(Controller),
    Joining(Joining),
    /// Only while changing between the two.
    Changing,
}

pub struct Wifi {
    state: State,
    /// The network over it.
    pub net: crate::net::Net,
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
        let net = crate::net::Net::new(esp_radio::wifi::Interface::station());
        // It runs until the X4 starts again.
        let controller = Box::leak(Box::new(controller));
        Ok(Self {
            state: State::Idle(controller),
            net,
        })
    }

    fn controller(&mut self) -> Result<&mut WifiController<'static>, Error> {
        match &mut self.state {
            State::Idle(controller) => Ok(controller),
            _ => Err(Error::Busy),
        }
    }

    /// The networks in reach, at most `max`.
    pub fn scan(&mut self, max: usize) -> Result<Vec<AccessPointInfo>, Error> {
        crate::power::mark(crate::power::at::RADIO_SCAN);
        let config = ScanConfig::default().with_max(max);
        let controller = self.controller()?;
        Ok(block_on(controller.scan_async(&config))??)
    }

    /// Starts joining the network `ssid`, with WPA2 or newer if there is a
    /// password, else an open one; [`Wifi::poll`] tells when it is done.
    pub fn join(&mut self, ssid: &str, password: &str) -> Result<(), Error> {
        crate::power::mark(crate::power::at::RADIO_CONNECT);
        let authentication = if password.is_empty() {
            AuthenticationMethodConfig::Open
        } else {
            AuthenticationMethodConfig::Wpa2Personal(password.try_into()?)
        };
        let config = StationConfig::default()
            .with_ssid(ssid.try_into()?)
            .with_authentication(authentication);
        self.controller()?.set_config(&Config::Station(config))?;
        let State::Idle(controller) = core::mem::replace(&mut self.state, State::Changing) else {
            return Err(Error::Busy);
        };
        self.state = State::Joining(Box::pin(async move {
            // Bounded: once, after flashing, no answer ever came.
            let result = match within(WAIT_MS, controller.connect_async()).await {
                Some(result) => result.map_err(Error::from),
                None => {
                    // Stops the try (bounded too: it may not answer either).
                    let _ = within(STOP_MS, controller.disconnect_async()).await;
                    Err(Error::Timeout)
                }
            };
            (result, controller)
        }));
        Ok(())
    }

    /// Joining a network is going on.
    pub fn joining(&self) -> bool {
        matches!(self.state, State::Joining(_))
    }

    /// Moves the network on; the outcome of joining one, once it is known.
    pub fn poll(&mut self) -> Option<Result<ConnectedInfo, Error>> {
        self.net.poll();
        let State::Joining(joining) = &mut self.state else {
            return None;
        };
        let mut cx = Context::from_waker(Waker::noop());
        let Poll::Ready((result, controller)) = joining.as_mut().poll(&mut cx) else {
            return None;
        };
        self.state = State::Idle(controller);
        if result.is_ok() {
            self.net.joined();
        }
        Some(result)
    }

    /// Joins the network `ssid` and waits for it, at most [`WAIT_MS`].
    pub fn connect(&mut self, ssid: &str, password: &str) -> Result<ConnectedInfo, Error> {
        self.join(ssid, password)?;
        let until = esp_hal::time::Instant::now() + esp_hal::time::Duration::from_millis(WAIT_MS);
        loop {
            if let Some(result) = self.poll() {
                return result;
            }
            if esp_hal::time::Instant::now() > until {
                return Err(Error::Timeout);
            }
            crate::power::feed();
        }
    }
}

/// The networks joined before, as `(name, password)`.
pub fn known(secrets: &quireink_sdk::secrets::Secrets) -> Vec<(String, String)> {
    secrets
        .iter()
        .filter_map(|secret| {
            let name = secret.id.strip_prefix(SECRET_PREFIX)?;
            Some((String::from(name), secret.value.clone()))
        })
        .collect()
}

/// The secret keeping the password of network `ssid`.
pub fn secret(ssid: &str, password: &str) -> quireink_sdk::secrets::SecretChange {
    quireink_sdk::secrets::SecretChange::keep(
        &alloc::format!("{SECRET_PREFIX}{ssid}"),
        &alloc::format!("Wi-Fi {ssid}"),
        password,
    )
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
