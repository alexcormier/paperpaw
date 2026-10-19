//! When the radio is on and which network is joined: the decisions, kept
//! apart from the page so they can be tried without a device.
//!
//! - The radio comes on when a plugin wants the network and the user
//!   lets it ([`ON_DEMAND`](crate::ON_DEMAND)); then the strongest network
//!   joined before is joined, the next one if that fails.
//! - A round that joins nothing is tried again a little later while the
//!   network is still wanted or charging keeps the radio on (e.g. while a
//!   router starts, or once a first try had no answer).
//! - It goes off a while after nothing uses the network
//!   ([`IDLE_OFF`](crate::IDLE_OFF)), unless the user switched it on on
//!   the page (it stays on until switched off there) or it stays on
//!   while charging ([`WHILE_CHARGING`](crate::WHILE_CHARGING)).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::wifi::{WifiCommand, WifiNetwork, WifiState};

/// What the radio is for now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Goal {
    /// Nothing to join.
    #[default]
    Nothing,
    /// The strongest network joined before, once networks are found.
    Known,
    /// The network the user chose, with the password given.
    Chosen { ssid: String, password: String },
}

/// What the plugin keeps between events.
#[derive(Debug, Default)]
pub struct Policy {
    pub goal: Goal,
    /// Networks tried this round (to try the next one).
    pub tried: Vec<String>,
    /// The user switched the radio on (on the page): it stays on.
    pub manual: bool,
    /// A network chosen with a password not kept yet: kept once joined.
    pub pending: Option<(String, String)>,
    /// Counts the times the radio was wanted; a timer to switch it off
    /// carries the count it was set at and does nothing once it changed.
    pub generation: u32,
    /// Charging kept the radio on, as last seen (to act on changes only).
    pub charging_kept: bool,
}

/// The strongest of `networks` joined before (`known`) and not tried yet.
pub fn best_known<'a>(
    networks: &[WifiNetwork],
    known: &'a [(String, String)],
    tried: &[String],
) -> Option<&'a (String, String)> {
    // Networks come strongest first.
    networks
        .iter()
        .filter(|n| !tried.contains(&n.ssid))
        .find_map(|n| known.iter().find(|(ssid, _)| *ssid == n.ssid))
}

impl Policy {
    /// The network is wanted: the radio on, or networks looked for.
    pub fn wanted(&mut self, state: &WifiState, known: &[(String, String)]) -> Vec<WifiCommand> {
        self.generation = self.generation.wrapping_add(1);
        if known.is_empty() {
            return Vec::new();
        }
        match state {
            WifiState::Off => {
                self.start_round(Goal::Known);
                alloc::vec![WifiCommand::On]
            }
            WifiState::On => {
                self.start_round(Goal::Known);
                alloc::vec![WifiCommand::Scan]
            }
            WifiState::Joining { .. } | WifiState::Online { .. } => Vec::new(),
        }
    }

    /// Tries the networks joined before again (after a round found none
    /// or failed): networks looked for anew.
    pub fn retry(&mut self) -> Vec<WifiCommand> {
        self.start_round(Goal::Known);
        alloc::vec![WifiCommand::Scan]
    }

    fn start_round(&mut self, goal: Goal) {
        self.goal = goal;
        self.tried.clear();
    }

    /// The user chose a network (with its password, or a kept one).
    pub fn choose(&mut self, state: &WifiState, ssid: &str, password: &str) -> Vec<WifiCommand> {
        self.start_round(Goal::Chosen {
            ssid: String::from(ssid),
            password: String::from(password),
        });
        if state.is_on() {
            alloc::vec![WifiCommand::Join {
                ssid: String::from(ssid),
                password: String::from(password),
            }]
        } else {
            self.manual = true;
            alloc::vec![WifiCommand::On]
        }
    }

    /// The radio's state changed.
    pub fn changed(&mut self, state: &WifiState) -> Vec<WifiCommand> {
        match state {
            WifiState::On => match &self.goal {
                Goal::Nothing => Vec::new(),
                Goal::Known => alloc::vec![WifiCommand::Scan],
                Goal::Chosen { ssid, password } => alloc::vec![WifiCommand::Join {
                    ssid: ssid.clone(),
                    password: password.clone(),
                }],
            },
            WifiState::Online { .. } => {
                self.goal = Goal::Nothing;
                self.tried.clear();
                Vec::new()
            }
            WifiState::Off => {
                self.goal = Goal::Nothing;
                self.manual = false;
                Vec::new()
            }
            WifiState::Joining { .. } => Vec::new(),
        }
    }

    /// Networks were found: the best one joined before, if one is wanted.
    /// `None` when there is none to try.
    pub fn scanned(
        &mut self,
        networks: &[WifiNetwork],
        known: &[(String, String)],
    ) -> Option<Vec<WifiCommand>> {
        if self.goal != Goal::Known {
            return Some(Vec::new());
        }
        let (ssid, password) = best_known(networks, known, &self.tried)?;
        self.tried.push(ssid.clone());
        Some(alloc::vec![WifiCommand::Join {
            ssid: ssid.clone(),
            password: password.clone(),
        }])
    }

    /// Joining `ssid` failed: the next network joined before, if any is
    /// wanted. `None` when there is none to try.
    pub fn failed(
        &mut self,
        ssid: &str,
        networks: &[WifiNetwork],
        known: &[(String, String)],
    ) -> Option<Vec<WifiCommand>> {
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| pending == ssid)
        {
            self.pending = None;
        }
        match self.goal {
            Goal::Known => {
                if !self.tried.iter().any(|t| t == ssid) {
                    self.tried.push(String::from(ssid));
                }
                self.scanned(networks, known)
            }
            Goal::Chosen { .. } => {
                self.goal = Goal::Nothing;
                Some(Vec::new())
            }
            Goal::Nothing => Some(Vec::new()),
        }
    }

    /// Whether the radio may go off now that nothing uses the network.
    pub fn may_go_off(&self, charging_keeps_it: bool) -> bool {
        !self.manual && !charging_keeps_it && self.goal == Goal::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, signal: i8) -> WifiNetwork {
        WifiNetwork {
            ssid: String::from(ssid),
            signal,
            secured: true,
        }
    }

    fn known() -> Vec<(String, String)> {
        alloc::vec![
            (String::from("Home"), String::from("pw")),
            (String::from("Office"), String::from("pw2")),
        ]
    }

    fn join(ssid: &str, password: &str) -> WifiCommand {
        WifiCommand::Join {
            ssid: String::from(ssid),
            password: String::from(password),
        }
    }

    #[test]
    fn wanted_joins_the_strongest_known_then_the_next() {
        let mut policy = Policy::default();
        let known = known();
        assert_eq!(policy.wanted(&WifiState::Off, &known), [WifiCommand::On]);
        assert_eq!(policy.changed(&WifiState::On), [WifiCommand::Scan]);
        let networks = [net("Cafe", -40), net("Office", -50), net("Home", -60)];
        assert_eq!(
            policy.scanned(&networks, &known).unwrap(),
            [join("Office", "pw2")]
        );
        assert_eq!(
            policy.failed("Office", &networks, &known).unwrap(),
            [join("Home", "pw")]
        );
        assert_eq!(policy.failed("Home", &networks, &known), None);
        let online = WifiState::Online {
            ssid: String::from("Home"),
            address: String::from("10.0.0.2"),
        };
        assert!(policy.changed(&online).is_empty());
        assert!(policy.may_go_off(false));
        assert!(!policy.may_go_off(true));
    }

    #[test]
    fn nothing_known_nothing_done() {
        let mut policy = Policy::default();
        assert!(policy.wanted(&WifiState::Off, &[]).is_empty());
        assert_eq!(policy.scanned(&[net("Cafe", -40)], &known()).unwrap(), []);
    }

    #[test]
    fn a_chosen_network_switches_the_radio_on_and_keeps_it_on() {
        let mut policy = Policy::default();
        assert_eq!(
            policy.choose(&WifiState::Off, "Cafe", ""),
            [WifiCommand::On]
        );
        assert_eq!(policy.changed(&WifiState::On), [join("Cafe", "")]);
        assert!(policy.manual);
        policy.goal = Goal::Nothing;
        assert!(!policy.may_go_off(false));
        assert!(policy.changed(&WifiState::Off).is_empty());
        assert!(!policy.manual);
    }

    #[test]
    fn a_wrong_password_forgets_what_was_typed() {
        let mut policy = Policy {
            pending: Some((String::from("Cafe"), String::from("typo"))),
            ..Policy::default()
        };
        policy.choose(&WifiState::On, "Cafe", "typo");
        assert_eq!(policy.failed("Cafe", &[], &[]).unwrap(), []);
        assert_eq!(policy.pending, None);
        assert_eq!(policy.goal, Goal::Nothing);
    }
}
