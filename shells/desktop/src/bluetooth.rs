//! A pretend Bluetooth radio for profiles of devices with one (the X4
//! Pro): it answers the kernel's Bluetooth calls as such a device would,
//! with made-up devices in reach, so that plugins and the Workshop can try
//! Bluetooth without hardware (see ADR 0027).
//!
//! The devices:
//! - `PaperPaw Remote`, a page-turner: an HID keyboard. Pairing asks only
//!   whether to pair. Once its keys go to the key maps, the script's
//!   `ble-key:<key>` presses one (`right`, `left`, `up`, `down`,
//!   `page-down`, `page-up`, `space`, `enter`, `escape`, `volume-up`,
//!   `volume-down`).
//! - `Pulse HR`, a heart-rate belt: the heart-rate service (0x180D), its
//!   measurement (0x2A37, notified each second while subscribed) and body
//!   sensor location (0x2A38, read: chest). Pairing compares the number
//!   123456.

use std::collections::VecDeque;

use paperpaw_core::sdk::bluetooth::{
    AttError, BtError, BtOp, GattCharacteristic, GattService, Handle, Properties, Security, Uuid,
    ValueKind,
};
use paperpaw_core::{BtCall, BtReport, Found, PairQuestion};
use paperpaw_platform::Instant;

/// How long the pretend radio takes to start, find a device and connect.
const START_MS: u64 = 10;
const FOUND_MS: u64 = 300;
const CONNECT_MS: u64 = 200;
const ANSWER_MS: u64 = 50;
/// How often the belt tells its heart rate.
const BEAT_MS: u64 = 1_000;

pub const REMOTE: &str = "fa:ke:00:00:00:01";
pub const BELT: &str = "fa:ke:00:00:00:02";

const MEASUREMENT: u16 = 0x0010;
const LOCATION: u16 = 0x0012;

fn belt_services() -> Vec<GattService> {
    vec![GattService::new(
        Uuid::sig(0x180D),
        vec![
            GattCharacteristic::new(
                Uuid::sig(0x2A37),
                Handle::new(MEASUREMENT),
                Properties::NOTIFY,
            ),
            GattCharacteristic::new(Uuid::sig(0x2A38), Handle::new(LOCATION), Properties::READ),
        ],
    )]
}

/// The boot keyboard's usage of key `name`.
fn usage(name: &str) -> Option<u8> {
    Some(match name {
        "right" => 0x4f,
        "left" => 0x50,
        "down" => 0x51,
        "up" => 0x52,
        "page-down" => 0x4e,
        "page-up" => 0x4b,
        "space" => 0x2c,
        "enter" => 0x28,
        "escape" => 0x29,
        "volume-up" => 0x80,
        "volume-down" => 0x81,
        _ => return None,
    })
}

#[derive(Debug, Default)]
pub struct Bluetooth {
    on: bool,
    /// Links: number, address.
    links: Vec<(u32, &'static str)>,
    /// The remote's link whose keys are wanted.
    keys: Option<u32>,
    /// The belt's link whose heart rate is wanted, and the next beat.
    beats: Option<(u32, Instant)>,
    beat: u8,
    /// Pairing waiting for the reader's answer: link, address.
    pairing: Option<(u32, &'static str)>,
    /// What is reported when, in the order of time.
    due: VecDeque<(Instant, BtReport)>,
}

impl Bluetooth {
    /// Does what the kernel asked.
    pub fn apply(&mut self, call: BtCall, now: Instant) {
        let after = |ms| now.add_millis(ms);
        match call {
            BtCall::Radio { on } => {
                self.on = on;
                if !on {
                    self.due.clear();
                    self.links.clear();
                    self.keys = None;
                    self.beats = None;
                    self.pairing = None;
                }
                self.report(after(START_MS), BtReport::Radio { on });
            }
            BtCall::Scan { on: true, .. } if self.on => {
                for (address, name, services, appearance) in [
                    (REMOTE, "PaperPaw Remote", vec![Uuid::sig(0x1812)], 0x03C1),
                    (BELT, "Pulse HR", vec![Uuid::sig(0x180D)], 0x0341),
                ] {
                    let found = BtReport::Found(Found {
                        address: String::from(address),
                        name: Some(String::from(name)),
                        signal: Some(-55),
                        services,
                        appearance: Some(appearance),
                        ..Found::default()
                    });
                    self.report(after(FOUND_MS), found);
                }
            }
            // Watched for: the remote is always awake, and advertises.
            BtCall::Watch { addresses } if self.on && addresses.iter().any(|a| a == REMOTE) => {
                let found = BtReport::Found(Found {
                    address: String::from(REMOTE),
                    signal: Some(-55),
                    ..Found::default()
                });
                self.report(after(FOUND_MS), found);
            }
            BtCall::Connect { link, address, .. } => {
                let known = [REMOTE, BELT].into_iter().find(|a| *a == address);
                match known {
                    Some(address) if self.on => {
                        self.links.push((link, address));
                        let connected = BtReport::Connected {
                            link,
                            mtu: 23,
                            security: Security::None,
                        };
                        self.report(after(CONNECT_MS), connected);
                    }
                    _ => {
                        let failed = BtReport::Failed {
                            link,
                            op: BtOp::Connect,
                            handle: None,
                            error: BtError::Timeout,
                        };
                        self.report(after(CONNECT_MS), failed);
                    }
                }
            }
            BtCall::Disconnect { link } => {
                self.links.retain(|(l, _)| *l != link);
                if self.keys == Some(link) {
                    self.keys = None;
                }
                if self.beats.is_some_and(|(l, _)| l == link) {
                    self.beats = None;
                }
            }
            other => self.apply_on_link(other, now),
        }
    }

    /// [`Bluetooth::apply`] for what is done over a link.
    fn apply_on_link(&mut self, call: BtCall, now: Instant) {
        let after = |ms| now.add_millis(ms);
        let address = |links: &[(u32, &'static str)], link: u32| {
            links.iter().find(|(l, _)| *l == link).map(|(_, a)| *a)
        };
        match call {
            BtCall::Discover { link, .. } => {
                let services = match address(&self.links, link) {
                    Some(BELT) => belt_services(),
                    _ => Vec::new(),
                };
                self.report(after(ANSWER_MS), BtReport::Discovered { link, services });
            }
            BtCall::Read { link, handle } if address(&self.links, link) == Some(BELT) => {
                if handle == LOCATION {
                    let value = BtReport::Value {
                        link,
                        handle,
                        kind: ValueKind::Read,
                        bytes: vec![1],
                    };
                    self.report(after(ANSWER_MS), value);
                } else {
                    let failed = BtReport::Failed {
                        link,
                        op: BtOp::Read,
                        handle: Some(handle),
                        error: BtError::Att(AttError::INVALID_HANDLE),
                    };
                    self.report(after(ANSWER_MS), failed);
                }
            }
            BtCall::Write {
                link,
                handle,
                response,
                ..
            } => {
                let report = if response {
                    BtReport::Written { link, handle }
                } else {
                    BtReport::Sent { link, handle }
                };
                self.report(after(ANSWER_MS), report);
            }
            BtCall::Subscribe { link, handle, on } if handle == MEASUREMENT => {
                self.beats = on.then(|| (link, after(BEAT_MS)));
            }
            BtCall::Pair { link } => {
                let address = address(&self.links, link);
                if let Some(address) = address {
                    self.pairing = Some((link, address));
                    let question = if address == BELT {
                        PairQuestion::Compare(123_456)
                    } else {
                        PairQuestion::Confirm
                    };
                    self.report(after(ANSWER_MS), BtReport::Ask { link, question });
                }
            }
            BtCall::Answer { link, accept } => {
                if let Some((asked, address)) = self.pairing.take()
                    && asked == link
                {
                    let report = if accept {
                        BtReport::Paired {
                            link,
                            keys: format!("fake-{address}"),
                        }
                    } else {
                        BtReport::Failed {
                            link,
                            op: BtOp::Pair,
                            handle: None,
                            error: BtError::Declined,
                        }
                    };
                    self.report(after(ANSWER_MS), report);
                }
            }
            BtCall::Hid { link, on } if address(&self.links, link) == Some(REMOTE) => {
                self.keys = on.then_some(link);
                if on {
                    // No report map: the remote speaks the boot keyboard's
                    // reports.
                    let map = BtReport::HidMap {
                        link,
                        map: Vec::new(),
                    };
                    self.report(after(ANSWER_MS), map);
                }
            }
            BtCall::Advertise(advert) => {
                let on = advert.on;
                self.report(after(START_MS), BtReport::Advertising { on });
            }
            // Bonds, the server, notifying, answering, forgetting and the
            // rest: nothing to tell.
            _ => {}
        }
    }

    /// The remote's key `name` pressed and let go (the script's
    /// `ble-key`): `false` where its keys are not wanted, or there is no
    /// such key.
    pub fn press(&mut self, name: &str, now: Instant) -> bool {
        let (Some(link), Some(usage)) = (self.keys, usage(name)) else {
            return false;
        };
        for (at, key) in [(now, usage), (now.add_millis(80), 0)] {
            let report = BtReport::HidReport {
                link,
                report: None,
                bytes: vec![0, 0, key, 0, 0, 0, 0, 0],
            };
            self.report(at, report);
        }
        true
    }

    fn report(&mut self, at: Instant, report: BtReport) {
        let place = self
            .due
            .iter()
            .position(|(t, _)| *t > at)
            .unwrap_or(self.due.len());
        self.due.insert(place, (at, report));
    }

    /// Whether something it does settles by itself (not the belt's
    /// beats, which go on).
    pub fn settling(&self) -> bool {
        !self.due.is_empty()
    }

    /// When the next report is due.
    pub fn next_due(&self) -> Option<Instant> {
        let beat = self.beats.map(|(_, at)| at);
        self.due
            .front()
            .map(|(at, _)| *at)
            .into_iter()
            .chain(beat)
            .min()
    }

    /// The next report due by `now`.
    pub fn take_due(&mut self, now: Instant) -> Option<BtReport> {
        if let Some((link, at)) = self.beats
            && at <= now
            && self.due.front().is_none_or(|(t, _)| *t > at)
        {
            self.beats = Some((link, at.add_millis(BEAT_MS)));
            self.beat = (self.beat + 1) % 10;
            // Flags 0 (a one-byte rate), then the rate.
            return Some(BtReport::Value {
                link,
                handle: MEASUREMENT,
                kind: ValueKind::Notified,
                bytes: vec![0, 70 + self.beat],
            });
        }
        if self.due.front().is_some_and(|(at, _)| *at <= now) {
            return self.due.pop_front().map(|(_, report)| report);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_belt_beats_while_subscribed() {
        let mut bt = Bluetooth::default();
        bt.apply(BtCall::Radio { on: true }, Instant(0));
        bt.apply(
            BtCall::Connect {
                link: 1,
                address: String::from(BELT),
                power: paperpaw_core::sdk::bluetooth::LinkPower::Low,
            },
            Instant(0),
        );
        while bt.take_due(Instant(1_000)).is_some() {}
        bt.apply(
            BtCall::Subscribe {
                link: 1,
                handle: MEASUREMENT,
                on: true,
            },
            Instant(1_000),
        );
        assert_eq!(bt.next_due(), Some(Instant(2_000)));
        assert!(matches!(
            bt.take_due(Instant(2_000)),
            Some(BtReport::Value {
                handle: MEASUREMENT,
                ..
            })
        ));
    }

    #[test]
    fn the_remote_types_only_once_its_keys_are_wanted() {
        let mut bt = Bluetooth::default();
        assert!(!bt.press("right", Instant(0)));
        bt.apply(BtCall::Radio { on: true }, Instant(0));
        bt.apply(
            BtCall::Connect {
                link: 3,
                address: String::from(REMOTE),
                power: paperpaw_core::sdk::bluetooth::LinkPower::Low,
            },
            Instant(0),
        );
        bt.apply(BtCall::Hid { link: 3, on: true }, Instant(0));
        assert!(bt.press("right", Instant(500)));
        assert!(!bt.press("nonsense", Instant(500)));
    }
}
