//! A pretend radio for profiles of devices where PaperPaw manages Wi-Fi
//! (the X4): it answers the kernel's Wi-Fi calls as such a device would,
//! with made-up networks and delays, so that the `wifi` plugin and what
//! waits for a network can be tried and tested here. Once "online", the
//! computer's own network is used.
//!
//! The networks: `PaperPaw Test` (any password but `wrong`), `Cafe`
//! (open) and `Far Away` (out of reach once joined).

use std::collections::VecDeque;

use paperpaw_core::{JoinFailure, WifiCommand, WifiEvent, WifiNetwork, WifiState};
use paperpaw_platform::Instant;

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
        net("PaperPaw Test", -48, true),
        net("Cafe", -70, false),
        net("Far Away", -88, true),
    ]
}

#[derive(Debug, Default)]
pub struct Wifi {
    state: WifiState,
    /// What is reported when, in the order of time.
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
                self.report(after(SCAN_MS), WifiEvent::Scanned(networks()));
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
                        self.report(after(JOIN_MS), WifiEvent::Failed { ssid, reason });
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
            WifiCommand::Join { ssid, .. } => self.report(
                now,
                WifiEvent::Failed {
                    ssid,
                    reason: JoinFailure::Other,
                },
            ),
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
        self.report(at, WifiEvent::State(state));
    }

    /// Reports `event` at `at`, after what is due before or then: a state
    /// reported now does not wait for a scan still going on.
    fn report(&mut self, at: Instant, event: WifiEvent) {
        let place = self.due.partition_point(|(due, _)| *due <= at);
        self.due.insert(place, (at, event));
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
                ssid: String::from("PaperPaw Test"),
                password: String::from("wrong"),
            },
            Instant(1000),
        );
        let events = run(&mut wifi, 3000);
        assert!(events.contains(&WifiEvent::Failed {
            ssid: String::from("PaperPaw Test"),
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

    #[test]
    fn reports_come_in_the_order_of_time() {
        let mut wifi = Wifi::default();
        wifi.apply(WifiCommand::On, Instant(0));
        run(&mut wifi, 1000);
        // A scan takes 1.5 s; joining meanwhile says so at once.
        wifi.apply(WifiCommand::Scan, Instant(1000));
        wifi.apply(
            WifiCommand::Join {
                ssid: String::from("Cafe"),
                password: String::new(),
            },
            Instant(1100),
        );
        assert_eq!(
            run(&mut wifi, 1100),
            [WifiEvent::State(WifiState::Joining {
                ssid: String::from("Cafe")
            })]
        );
        // Joined after 1 s, before the scan ends.
        assert!(matches!(
            run(&mut wifi, 2100).as_slice(),
            [WifiEvent::State(WifiState::Online { .. })]
        ));
        assert!(matches!(
            run(&mut wifi, 2500).as_slice(),
            [WifiEvent::Scanned(_)]
        ));
    }
}
