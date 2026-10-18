//! Everything of the network for the main loop, one step at a time: the
//! kernel's Wi-Fi commands to the radio and what happened back, plugins'
//! HTTP requests (`http`) and their answers, their servers (`serve`) and
//! sockets (`sockets`), and the clock set over the network (`clock`).
//!
//! What is asked over USB (`wifi …`, `http get`) goes the same way; its
//! answers are printed once they come (see `remote`).

use alloc::string::String;
use alloc::vec::Vec;

use quireink_core::{
    Effects, HttpCall, Kernel, ServerCall, ServerEvent, SocketCall, SocketEvent, WifiCommand,
    WifiEvent, WifiState,
};

use crate::serve::Servers;
use crate::sockets::Sockets;
use quireink_platform::Instant;

use smoltcp::iface::SocketHandle;
use smoltcp::socket::dns;

use crate::clock;
use crate::http::Calls;
use crate::net::{Net, Resolve};
use crate::remote;
use crate::wifi::{Parts, Radio};

/// How often the clock is set again while online, and how long to wait
/// after a failed try.
const RESYNC_MS: u64 = 12 * 60 * 60 * 1000;
const RETRY_MS: u64 = 60_000;
/// How long an answer is waited for.
const ASK_MS: u64 = 5_000;

/// Setting the clock over the network (SNTP).
enum Sync {
    /// Nothing going on; the next try not before this time.
    Idle {
        not_before: u64,
    },
    Resolving {
        query: dns::QueryHandle,
        until: u64,
    },
    Asking {
        socket: SocketHandle,
        until: u64,
    },
}

/// Why plugins' servers and sockets fail without a network.
const NO_NETWORK: &str = "no network";
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
    /// Requests asked: their number and when.
    requests: Vec<(u32, u64)>,
}

pub struct Network {
    radio: Radio,
    calls: Calls,
    servers: Servers,
    sockets: Sockets,
    asked: Asked,
    sync: Sync,
    /// When the clock was last set over the network.
    synced: Option<u64>,
    /// A try to set the clock failed since the radio came online: secure
    /// requests no longer wait for it.
    sync_failed: bool,
}

impl Network {
    pub fn new(parts: Parts) -> Self {
        Self {
            radio: Radio::new(parts),
            calls: Calls::default(),
            servers: Servers::default(),
            sockets: Sockets::default(),
            asked: Asked::default(),
            sync: Sync::Idle { not_before: 0 },
            synced: None,
            sync_failed: false,
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
        self.serve(kernel, now, &mut effects);
        self.sync_clock(now);
        if self.calls.busy() {
            // Secure requests wait for the clock (their certificates' dates
            // are checked against it), unless it cannot be had.
            let clock_ready = clock::unix_ms(now).is_some() || self.sync_failed;
            for answer in self
                .calls
                .poll(self.radio.net(), kernel.storage(), clock_ready)
            {
                if answer.id >= REMOTE_REQUESTS {
                    let asked = self
                        .asked
                        .requests
                        .iter()
                        .position(|(id, _)| *id == answer.id);
                    let since = asked.map(|at| self.asked.requests.remove(at).1);
                    remote::http_answer(&answer, since.map_or(0, |at| now.0.saturating_sub(at)));
                } else {
                    effects.push(kernel.http_response(now, answer));
                }
            }
        }
        effects
    }

    /// Whether data is moving for plugins' requests, servers or sockets:
    /// the main loop comes by again at once.
    pub fn moving(&self) -> bool {
        self.calls.busy() || self.servers.moving() || self.sockets.moving()
    }

    /// Plugins' servers and sockets: what the kernel asked, then a step
    /// for each. Without a network they fail (what was open is gone).
    fn serve(&mut self, kernel: &mut Kernel, now: Instant, effects: &mut Vec<Effects>) {
        let Some(net) = self.radio.net() else {
            effects.extend(self.servers.lost(kernel, now));
            effects.extend(self.sockets.lost(kernel, now));
            for call in kernel.take_server_calls() {
                if let ServerCall::Listen { server, .. } = call {
                    let failed = ServerEvent::Failed {
                        server,
                        reason: String::from(NO_NETWORK),
                    };
                    effects.push(kernel.server_event(now, failed));
                }
            }
            for call in kernel.take_socket_calls() {
                let event = match call {
                    SocketCall::Connect { socket, .. } => SocketEvent::Failed {
                        socket,
                        reason: String::from(NO_NETWORK),
                    },
                    SocketCall::Broadcast { broadcast, .. } => SocketEvent::Done { broadcast },
                    _ => continue,
                };
                effects.push(kernel.socket_event(now, event));
            }
            return;
        };
        for call in kernel.take_server_calls() {
            effects.extend(self.servers.apply(call, net, kernel, now));
        }
        for call in kernel.take_socket_calls() {
            if let Some(event) = self.sockets.apply(call, net, now) {
                effects.push(kernel.socket_event(now, event));
            }
        }
        effects.extend(self.servers.poll(net, kernel, now));
        effects.extend(self.sockets.poll(net, kernel, now));
    }

    /// Sets the clock over the network when it is not known or old, once
    /// the radio is online; a step at a time, never waiting.
    fn sync_clock(&mut self, now: Instant) {
        let ms = now.0;
        let Some(net) = self.radio.net() else {
            // Offline: whatever went on is gone with the network.
            self.sync = Sync::Idle { not_before: 0 };
            self.sync_failed = false;
            return;
        };
        let due = clock::unix_ms(now).is_none()
            || self
                .synced
                .is_none_or(|at| ms.saturating_sub(at) >= RESYNC_MS);
        self.sync = match core::mem::replace(&mut self.sync, Sync::Idle { not_before: 0 }) {
            Sync::Idle { not_before } if !due || ms < not_before => Sync::Idle { not_before },
            Sync::Idle { .. } => match net.resolve(clock::SERVER) {
                Ok(Resolve::Done(address)) => ask(net, address, ms),
                Ok(Resolve::Asking(query)) => Sync::Resolving {
                    query,
                    until: ms + ASK_MS,
                },
                Err(_) => self.sync_failed(ms),
            },
            Sync::Resolving { query, until } => match net.resolved(query) {
                Some(Ok(address)) => ask(net, address, ms),
                Some(Err(_)) => self.sync_failed(ms),
                None if ms > until => {
                    net.forget(query);
                    self.sync_failed(ms)
                }
                None => Sync::Resolving { query, until },
            },
            Sync::Asking { socket, until } => {
                let udp = net.udp(socket);
                let mut time = None;
                while let Ok((data, _)) = udp.recv() {
                    time = time.or(clock::answer(data));
                }
                if let Some(unix_ms) = time {
                    net.release_udp(socket);
                    clock::set(now, unix_ms);
                    self.synced = Some(ms);
                    log::info!("clock: set over the network");
                    Sync::Idle { not_before: 0 }
                } else if ms > until {
                    net.release_udp(socket);
                    self.sync_failed(ms)
                } else {
                    Sync::Asking { socket, until }
                }
            }
        };
    }

    fn sync_failed(&mut self, ms: u64) -> Sync {
        log::info!("clock: not set over the network");
        self.sync_failed = true;
        Sync::Idle {
            not_before: ms + RETRY_MS,
        }
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
                self.asked.requests.push((id, crate::now().0));
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

/// Sends an SNTP request to `address`.
fn ask(net: &mut Net, address: smoltcp::wire::IpAddress, ms: u64) -> Sync {
    let Ok(socket) = net.open_udp(2, 2 * clock::MESSAGE) else {
        return Sync::Idle {
            not_before: ms + RETRY_MS,
        };
    };
    if net
        .udp(socket)
        .send_slice(&clock::request(), (address, clock::PORT))
        .is_err()
    {
        net.release_udp(socket);
        return Sync::Idle {
            not_before: ms + RETRY_MS,
        };
    }
    Sync::Asking {
        socket,
        until: ms + ASK_MS,
    }
}
