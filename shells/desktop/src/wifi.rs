//! A pretend radio for profiles of devices where QuireInk manages Wi-Fi
//! (the X4): it answers the kernel's Wi-Fi calls as such a device would,
//! with made-up networks and delays, so that the `wifi` plugin and what
//! waits for a network can be tried and tested here. Once "online", the
//! computer's own network is used.
//!
//! The networks: `QuireInk Test` (any password but `wrong`), `Cafe`
//! (open) and `Far Away` (out of reach once joined).

use std::collections::VecDeque;

use quireink_core::{JoinFailure, WifiCommand, WifiEvent, WifiNetwork, WifiState};
use quireink_platform::Instant;

/// How long the pretend radio takes to start, scan and join.
const START_MS: u64 = 200;
const SCAN_MS: u64 = 1500;
const JOIN_MS: u64 = 1000;

/// The address the emulated device gets.
const ADDRESS: &str = "192.168.1.42";

fn networks() -> Vec<WifiNetwork> {
    let net = |ssid: &str, signal, secured| WifiNetwork {
        ssid: String::from(ssid),
        signal,
        secured,
    };
    vec![
        net("QuireInk Test", -48, true),
        net("Cafe", -70, false),
        net("Far Away", -88, true),
    ]
}

#[derive(Debug, Default)]
pub struct Wifi {
    state: WifiState,
    /// What is reported when, in order.
    due: VecDeque<(Instant, WifiEvent)>,
}

impl Wifi {
    /// Does what the kernel asked.
    pub fn apply(&mut self, call: WifiCommand, now: Instant) {
        let after = |ms| now.add_millis(ms);
        match call {
            WifiCommand::On if self.state == WifiState::Off => {
                self.due.clear();
                self.set(after(START_MS), WifiState::On);
            }
            WifiCommand::On => {}
            WifiCommand::Off => {
                self.due.clear();
                self.set(now, WifiState::Off);
            }
            WifiCommand::Scan if self.state.is_on() => {
                self.due
                    .push_back((after(SCAN_MS), WifiEvent::Scanned(networks())));
            }
            WifiCommand::Scan => {}
            WifiCommand::Join { ssid, password } if self.state.is_on() => {
                self.due.retain(|(_, e)| !matches!(e, WifiEvent::State(_)));
                self.set(now, WifiState::Joining { ssid: ssid.clone() });
                let network = networks().into_iter().find(|n| n.ssid == ssid);
                let failure = match &network {
                    None => Some(JoinFailure::NotFound),
                    Some(n) if n.signal < -85 => Some(JoinFailure::Timeout),
                    Some(n) if n.secured && (password.is_empty() || password == "wrong") => {
                        Some(JoinFailure::Password)
                    }
                    Some(_) => None,
                };
                match failure {
                    Some(reason) => {
                        self.set(after(JOIN_MS), WifiState::On);
                        self.due
                            .push_back((after(JOIN_MS), WifiEvent::Failed { ssid, reason }));
                    }
                    None => self.set(
                        after(JOIN_MS),
                        WifiState::Online {
                            ssid,
                            address: String::from(ADDRESS),
                        },
                    ),
                }
            }
            WifiCommand::Join { ssid, .. } => self.due.push_back((
                now,
                WifiEvent::Failed {
                    ssid,
                    reason: JoinFailure::Other,
                },
            )),
            WifiCommand::Leave if self.state.is_on() => {
                self.due.clear();
                self.set(now, WifiState::On);
            }
            WifiCommand::Leave => {}
        }
    }

    /// Reports `state` at `at` (the state counts as it from then on).
    fn set(&mut self, at: Instant, state: WifiState) {
        self.state = state.clone();
        self.due.push_back((at, WifiEvent::State(state)));
    }

    /// When something is to be reported next.
    pub fn next_due(&self) -> Option<Instant> {
        self.due.front().map(|(at, _)| *at)
    }

    /// The next thing to report, if due.
    pub fn take_due(&mut self, now: Instant) -> Option<WifiEvent> {
        if self.due.front().is_some_and(|(at, _)| *at <= now) {
            self.due.pop_front().map(|(_, event)| event)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(wifi: &mut Wifi, until: u64) -> Vec<WifiEvent> {
        let mut events = Vec::new();
        while let Some(event) = wifi.take_due(Instant(until)) {
            events.push(event);
        }
        events
    }

    #[test]
    fn joins_and_fails_like_a_device() {
        let mut wifi = Wifi::default();
        wifi.apply(WifiCommand::On, Instant(0));
        assert_eq!(run(&mut wifi, 1000), [WifiEvent::State(WifiState::On)]);
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("QuireInk Test"),
                password: String::from("wrong"),
            },
            Instant(1000),
        );
        let events = run(&mut wifi, 3000);
        assert!(events.contains(&WifiEvent::Failed {
            ssid: String::from("QuireInk Test"),
            reason: JoinFailure::Password
        }));
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("Cafe"),
                password: String::new(),
            },
            Instant(3000),
        );
        let events = run(&mut wifi, 5000);
        assert!(matches!(
            events.last(),
            Some(WifiEvent::State(WifiState::Online { .. }))
        ));
        wifi.apply(WifiCommand::Off, Instant(5000));
        assert_eq!(run(&mut wifi, 5000), [WifiEvent::State(WifiState::Off)]);
    }
}
