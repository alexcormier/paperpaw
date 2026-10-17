//! Everything of the network for the main loop, one step at a time: the
//! kernel's Wi-Fi commands to the radio and what happened back, plugins'
//! HTTP requests (`http`) and their answers, and their servers and
//! sockets (not on the X4 yet: they fail at once, with the reason).
//!
//! What is asked over USB (`wifi …`, `http get`) goes the same way; its
//! answers are printed once they come (see `remote`).

use alloc::string::String;
use alloc::vec::Vec;

use quireink_core::{
    Effects, HttpCall, Kernel, ServerCall, ServerEvent, SocketCall, SocketEvent, WifiCommand,
    WifiEvent, WifiState,
};
use quireink_platform::Instant;

use crate::http::Calls;
use crate::remote;
use crate::wifi::{Parts, Radio};

/// Why plugins' servers and sockets fail here.
const NO_SOCKETS: &str = "not supported on this device yet";
/// The numbers of requests made over USB, apart from the kernel's.
const REMOTE_REQUESTS: u32 = 0xF000_0000;

/// What was asked over USB.
#[derive(Debug)]
pub enum Ask {
    /// Networks in reach (the radio switched on).
    Scan,
    /// Join a network, kept with its password once joined.
    Connect {
        ssid: String,
        password: String,
    },
    Status,
    Off,
    /// Fetch an address, into memory or the file `save_to`.
    Get {
        url: String,
        save_to: Option<String>,
    },
}

/// Answers still to give over USB.
#[derive(Debug, Default)]
struct Asked {
    scan: bool,
    /// The network asked to join, and its password, kept once joined.
    connect: Option<(String, String)>,
    next_request: u32,
}

pub struct Network {
    radio: Radio,
    calls: Calls,
    asked: Asked,
}

impl Network {
    pub fn new(parts: Parts) -> Self {
        Self {
            radio: Radio::new(parts),
            calls: Calls::default(),
            asked: Asked::default(),
        }
    }

    /// The radio is on: its tasks need the processor (no light sleep).
    pub fn radio_on(&self) -> bool {
        self.radio.is_on()
    }

    /// One step: what the kernel asked, what happened, the answers. The
    /// kernel's effects for the main loop to apply.
    pub fn step(&mut self, kernel: &mut Kernel, now: Instant) -> Vec<Effects> {
        let mut effects = Vec::new();
        for command in kernel.take_wifi_calls() {
            self.radio.apply(command);
        }
        for event in self.radio.poll() {
            self.answer_asked(kernel, &event);
            effects.push(kernel.wifi_event(now, event));
        }
        for call in kernel.take_requests() {
            self.calls.add(call);
        }
        for call in kernel.take_server_calls() {
            if let ServerCall::Listen { server, .. } = call {
                let failed = ServerEvent::Failed {
                    server,
                    reason: String::from(NO_SOCKETS),
                };
                effects.push(kernel.server_event(now, failed));
            }
        }
        for call in kernel.take_socket_calls() {
            let event = match call {
                SocketCall::Connect { socket, .. } => SocketEvent::Failed {
                    socket,
                    reason: String::from(NO_SOCKETS),
                },
                SocketCall::Broadcast { broadcast, .. } => SocketEvent::Done { broadcast },
                _ => continue,
            };
            effects.push(kernel.socket_event(now, event));
        }
        if self.calls.busy() {
            for answer in self.calls.poll(self.radio.net(), kernel.storage()) {
                if answer.id >= REMOTE_REQUESTS {
                    remote::http_answer(&answer);
                } else {
                    effects.push(kernel.http_response(now, answer));
                }
            }
        }
        effects
    }

    /// What was asked over USB.
    pub fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Scan => {
                self.asked.scan = true;
                self.radio.apply(WifiCommand::On);
                self.radio.apply(WifiCommand::Scan);
            }
            Ask::Connect { ssid, password } => {
                self.radio.apply(WifiCommand::On);
                self.radio.apply(WifiCommand::Join {
                    ssid: ssid.clone(),
                    password: password.clone(),
                });
                self.asked.connect = Some((ssid, password));
            }
            Ask::Status => remote::wifi_state(self.radio.state()),
            Ask::Off => {
                self.radio.apply(WifiCommand::Off);
                remote::ok();
            }
            Ask::Get { url, save_to } => {
                let id = REMOTE_REQUESTS + self.asked.next_request;
                self.asked.next_request = self.asked.next_request.wrapping_add(1) % 0x0FFF_FFFF;
                self.calls.add(HttpCall {
                    id,
                    method: String::from("GET"),
                    url,
                    headers: Vec::new(),
                    body: None,
                    body_from: None,
                    max_response: if save_to.is_some() {
                        quireink_core::MAX_FILE
                    } else {
                        64 * 1024
                    },
                    save_to,
                });
            }
        }
    }

    /// Answers what was asked over USB once it is known.
    fn answer_asked(&mut self, kernel: &mut Kernel, event: &WifiEvent) {
        match event {
            WifiEvent::Scanned(networks) if self.asked.scan => {
                self.asked.scan = false;
                remote::networks(networks);
            }
            WifiEvent::State(WifiState::Online { ssid, address }) => {
                if let Some((asked, password)) = self.asked.connect.take() {
                    if asked == *ssid {
                        kernel.set_secret(quireink_sdk::wifi::keep(&asked, &password));
                        remote::connected(ssid, address);
                    } else {
                        self.asked.connect = Some((asked, password));
                    }
                }
            }
            WifiEvent::Failed { ssid, reason }
                if self
                    .asked
                    .connect
                    .as_ref()
                    .is_some_and(|(asked, _)| asked == ssid) =>
            {
                self.asked.connect = None;
                remote::not_connected(ssid, *reason);
            }
            _ => {}
        }
    }
}
