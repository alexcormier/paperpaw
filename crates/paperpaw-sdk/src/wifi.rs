//! Wi-Fi, on devices where PaperPaw manages it itself (the X4; later
//! Linux readers): [`Env::wifi`](crate::Env) is `Some` there. Elsewhere
//! (Android, desktops) the system manages it and PaperPaw only uses the
//! network.
//!
//! The shell does what it is told ([`Command::Wifi`](crate::Command) with
//! a [`WifiCommand`]: radio on or off, find networks, join, leave) and
//! reports what happens; deciding when and which network is a plugin's
//! (the `wifi` plugin, which another can replace), as is the page to
//! choose networks. What it needs to know:
//!
//! - [`events::WIFI_CHANGED`](crate::builtin::events::WIFI_CHANGED): the
//!   [`WifiState`] changed;
//! - [`events::WIFI_SCANNED`](crate::builtin::events::WIFI_SCANNED): new
//!   [`Wifi::networks`];
//! - [`events::WIFI_FAILED`](crate::builtin::events::WIFI_FAILED): joining
//!   a network failed, and why;
//! - [`events::NETWORK_WANTED`](crate::builtin::events::NETWORK_WANTED):
//!   a plugin wants the network (a request, a server, a connection) while
//!   there is none; it waits a while in the kernel
//!   ([`NETWORK_WAIT_MS`]) and then fails;
//! - [`events::NETWORK_IDLE`](crate::builtin::events::NETWORK_IDLE):
//!   nothing uses the network any more, e.g. to switch the radio off a
//!   little later (it costs battery and memory).
//!
//! The networks joined before are kept with the passwords and keys
//! ([`secrets`](crate::secrets)) as `wifi.<name>` ([`known`], [`keep`],
//! [`forget`]), the same whichever plugin joins them.

use alloc::string::String;
use alloc::vec::Vec;

use crate::secrets::{SecretChange, Secrets};

/// How long a plugin's request, server or connection waits for the network
/// before it fails, on a device that manages Wi-Fi.
pub const NETWORK_WAIT_MS: u64 = 30_000;

/// What the shell is told to do with the radio. Printed for debugging
/// without the password.
#[derive(Clone, PartialEq, Eq)]
pub enum WifiCommand {
    /// Switches the radio on (not joining anything yet).
    On,
    /// Switches it off, leaving any network.
    Off,
    /// Looks for networks in reach; [`Wifi::networks`] has them after
    /// [`events::WIFI_SCANNED`](crate::builtin::events::WIFI_SCANNED).
    Scan,
    /// Joins a network: with WPA2 or newer if there is a password, else an
    /// open one.
    Join { ssid: String, password: String },
    /// Leaves the network, keeping the radio on.
    Leave,
}

impl core::fmt::Debug for WifiCommand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WifiCommand::On => f.write_str("On"),
            WifiCommand::Off => f.write_str("Off"),
            WifiCommand::Scan => f.write_str("Scan"),
            WifiCommand::Join { ssid, .. } => f
                .debug_struct("Join")
                .field("ssid", ssid)
                .field("password", &"…")
                .finish(),
            WifiCommand::Leave => f.write_str("Leave"),
        }
    }
}

/// Where the radio is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WifiState {
    #[default]
    Off,
    /// On, in no network.
    On,
    /// Joining a network.
    Joining { ssid: String },
    /// In a network, with an address: plugins' network work goes on.
    Online {
        ssid: String,
        /// The device's address, e.g. `192.168.1.20`.
        address: String,
    },
}

impl WifiState {
    pub fn is_on(&self) -> bool {
        *self != WifiState::Off
    }

    pub fn is_online(&self) -> bool {
        matches!(self, WifiState::Online { .. })
    }

    /// The network joined or being joined.
    pub fn ssid(&self) -> Option<&str> {
        match self {
            WifiState::Joining { ssid } | WifiState::Online { ssid, .. } => Some(ssid),
            _ => None,
        }
    }
}

/// A network in reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// Its signal, in dBm (e.g. -50 strong, -85 weak).
    pub signal: i8,
    /// It asks for a password.
    pub secured: bool,
}

impl WifiNetwork {
    /// The signal in bars from 1 to 4.
    pub fn bars(&self) -> u8 {
        match self.signal {
            s if s >= -55 => 4,
            s if s >= -67 => 3,
            s if s >= -78 => 2,
            _ => 1,
        }
    }
}

/// Why joining a network failed (the parameter
/// [`params::REASON`](crate::builtin::params::REASON) of
/// [`events::WIFI_FAILED`](crate::builtin::events::WIFI_FAILED)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinFailure {
    /// Not in reach.
    NotFound,
    /// The password was not accepted.
    Password,
    /// No answer in time.
    Timeout,
    /// Anything else.
    Other,
}

impl crate::event::ParamValue for JoinFailure {
    fn to_param(&self) -> String {
        String::from(self.as_str())
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(Self::parse(text))
    }
}

impl JoinFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            JoinFailure::NotFound => "not-found",
            JoinFailure::Password => "password",
            JoinFailure::Timeout => "timeout",
            JoinFailure::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "not-found" => JoinFailure::NotFound,
            "password" => JoinFailure::Password,
            "timeout" => JoinFailure::Timeout,
            _ => JoinFailure::Other,
        }
    }
}

/// Wi-Fi as the kernel knows it ([`Env::wifi`](crate::Env)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wifi {
    pub state: WifiState,
    /// The networks found last, strongest first (each name once).
    pub networks: Vec<WifiNetwork>,
    /// Looking for networks now.
    pub scanning: bool,
    /// Plugins' network work waits for the network now.
    pub wanted: bool,
    /// Something uses the network now: a request, a server or a
    /// connection (not to switch the radio off then).
    pub busy: bool,
}

/// The prefix of the ids of the networks' passwords among the secrets.
pub const SECRET_PREFIX: &str = "wifi.";

/// The id of the secret keeping network `ssid`'s password.
pub fn secret_id(ssid: &str) -> String {
    alloc::format!("{SECRET_PREFIX}{ssid}")
}

/// The networks joined before, as `(name, password)` (an empty password
/// for an open network).
pub fn known(secrets: &Secrets) -> Vec<(String, String)> {
    secrets
        .iter()
        .filter_map(|secret| {
            let ssid = secret.id.strip_prefix(SECRET_PREFIX)?;
            Some((String::from(ssid), secret.value.clone()))
        })
        .collect()
}

/// Keeps network `ssid` with its password, to join it again.
pub fn keep(ssid: &str, password: &str) -> SecretChange {
    SecretChange::keep(&secret_id(ssid), &alloc::format!("Wi-Fi {ssid}"), password)
}

/// Forgets network `ssid`.
pub fn forget(ssid: &str) -> SecretChange {
    SecretChange::remove(&secret_id(ssid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_networks_are_secrets() {
        let mut secrets = Secrets::default();
        secrets.apply(keep("Home Net", "pw!"));
        secrets.apply(keep("Cafe", ""));
        secrets.apply(SecretChange::keep("translate.key", "Key", "k"));
        assert_eq!(
            known(&secrets),
            [
                (String::from("Home Net"), String::from("pw!")),
                (String::from("Cafe"), String::new())
            ]
        );
        secrets.apply(forget("Cafe"));
        assert_eq!(known(&secrets).len(), 1);
        // Kept and read back as named, tabs and all.
        secrets.apply(keep("Odd\tName", "pw"));
        let read = Secrets::decode(&secrets.encode());
        assert!(known(&read).contains(&(String::from("Odd\tName"), String::from("pw"))));
        let join = WifiCommand::Join {
            ssid: String::from("n"),
            password: String::from("s3cr3t"),
        };
        assert!(!alloc::format!("{join:?}").contains("s3cr3t"));
    }

    #[test]
    fn states_and_failures() {
        let online = WifiState::Online {
            ssid: String::from("n"),
            address: String::from("10.0.0.2"),
        };
        assert!(online.is_on() && online.is_online());
        assert_eq!(online.ssid(), Some("n"));
        assert!(!WifiState::Off.is_on());
        for failure in [
            JoinFailure::NotFound,
            JoinFailure::Password,
            JoinFailure::Timeout,
            JoinFailure::Other,
        ] {
            assert_eq!(JoinFailure::parse(failure.as_str()), failure);
        }
    }
}
