//! What the kernel asks of the shell, for plugins and for itself, and
//! what the shell reports back: HTTP requests ([`http`]), plugins'
//! servers ([`servers`]) and sockets ([`sockets`]), speech ([`speech`]),
//! the system's clipboard ([`clipboard`]), the radio and the firmware.
//! Each part keeps its own state; the shell takes all of it at once as
//! [`Io`] (`Kernel::take_io`).

pub(crate) mod clipboard;
pub(crate) mod http;
pub(crate) mod servers;
pub(crate) mod sockets;
pub(crate) mod speech;

use alloc::string::String;
use alloc::vec::Vec;

use quireink_sdk::firmware::FirmwareCommand;
use quireink_sdk::wifi::{JoinFailure, WifiCommand, WifiNetwork, WifiState};

pub use http::{HttpCall, HttpResponse, MAX_FILE, MAX_RESPONSE};
pub use servers::{ServedFile, ServerCall, ServerEvent};
pub use sockets::{SOCKET_PART, SocketCall, SocketEvent};
pub use speech::SpeechCall;

/// Something the shell is to do (`Kernel::take_io`, after every kernel
/// call). Every kind is for each shell to carry out, or to answer as
/// it says where the shell cannot: matched in full, without a catch-all,
/// the compiler names a kind a shell forgot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Io {
    /// Make an HTTP request; answer with `Kernel::http_response` (status
    /// 0 and why, where it cannot be made).
    Http(HttpCall),
    /// For a plugin's server; report with `Kernel::server_event`
    /// (`Failed` where the shell cannot serve).
    Server(ServerCall),
    /// For a plugin's socket or broadcast; report with
    /// `Kernel::socket_event` (`Failed`, or `Done` for a broadcast, where
    /// the shell has none).
    Socket(SocketCall),
    /// Speak or stop; report each text's end with `Kernel::speech_done`
    /// (not spoken, where the shell cannot speak).
    Speech(SpeechCall),
    /// Put text copied on the system's clipboard; nothing to report
    /// (nothing to do without one).
    Copy(String),
    /// Read the system's clipboard; answer with `Kernel::clipboard_text`
    /// (`None` where it cannot be read).
    ReadClipboard,
    /// For the radio, where QuireInk manages Wi-Fi; report with
    /// `Kernel::wifi_event`.
    Wifi(WifiCommand),
    /// For the firmware, where QuireInk is it; report with
    /// `Kernel::firmware_state`.
    Firmware(FirmwareCommand),
}

/// What a shell tells about Wi-Fi, where QuireInk manages it (see
/// `Kernel::wifi_event` and `quireink_sdk::wifi`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiEvent {
    /// Where the radio is now.
    State(WifiState),
    /// The networks a scan found, in any order.
    Scanned(Vec<WifiNetwork>),
    /// Joining network `ssid` failed (the state says where the radio is
    /// now).
    Failed { ssid: String, reason: JoinFailure },
}
