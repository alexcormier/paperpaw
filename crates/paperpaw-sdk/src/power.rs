//! Power levels and modes (ADR 0025).
//!
//! PaperPaw saves by default; a **mode** trades features for battery on
//! top of that. Readers choose one (Normal, Saving, Offline, Automatic by
//! the battery, or one of their own), and every mode has a **level**,
//! one of three steps plugins understand:
//!
//! - [`PowerLevel::Normal`]: everything as configured.
//! - [`PowerLevel::Saving`]: every feature kept, but on demand: declared
//!   work waits longer ([`Power::stretch`]), optional work runs only
//!   when the radio is on anyway or the device charges.
//! - [`PowerLevel::Offline`]: reading only; no network work runs on its
//!   own, it waits for a connection the reader makes.
//!
//! Most plugins never read the level: work declared with its needs and a
//! deadline ([`work`](crate::work)) is limited by the kernel. The level
//! is for what declared work does not cover, e.g. how often a widget
//! changes ([`Power::every`]). The kernel emits
//! [`events::POWER_CHANGED`](crate::builtin::events::POWER_CHANGED) when
//! the level or the mode changes.
//!
//! A mode is a preset of the kind [`MODE_KIND`] (see
//! [`presets`](crate::presets)) with its level among its values
//! (`power.level = saving`), so plugins and data plugins can bring more.

use crate::id::Id;

/// How much energy to spend, as plugins see it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PowerLevel {
    /// Everything as configured.
    #[default]
    Normal,
    /// Every feature kept, but on demand.
    Saving,
    /// Reading only: no network on its own.
    Offline,
}

impl PowerLevel {
    /// Its name in settings and presets: `normal`, `saving`, `offline`.
    pub const fn as_str(self) -> &'static str {
        match self {
            PowerLevel::Normal => "normal",
            PowerLevel::Saving => "saving",
            PowerLevel::Offline => "offline",
        }
    }

    /// The level named `text` ([`as_str`](Self::as_str)).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "normal" => Some(PowerLevel::Normal),
            "saving" => Some(PowerLevel::Saving),
            "offline" => Some(PowerLevel::Offline),
            _ => None,
        }
    }
}

/// The kind of presets that are power modes.
pub const MODE_KIND: Id = Id::new_static("core.power-mode");
/// The setting in a mode's values that names its level.
pub const LEVEL: Id = Id::new_static("power.level");
/// The built-in modes.
pub const NORMAL: Id = Id::new_static("core.normal");
pub const SAVING: Id = Id::new_static("core.saving");
pub const OFFLINE: Id = Id::new_static("core.offline");
/// The value of the mode setting that lets the battery choose.
pub const AUTOMATIC: &str = "auto";

/// The power level and mode in use.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Power {
    pub level: PowerLevel,
    /// The mode worn: a built-in one ([`NORMAL`] …) or a preset's id.
    pub mode: Id,
    /// Chosen by Automatic, by the battery, not by the reader.
    pub automatic: bool,
    /// How many times longer declared work may wait than it says: 1 in
    /// Normal, 4 in Saving by default (a mode's value).
    pub stretch: u32,
}

impl Default for Power {
    fn default() -> Self {
        Self::new(PowerLevel::Normal, NORMAL, false, 1)
    }
}

impl Power {
    pub fn new(level: PowerLevel, mode: Id, automatic: bool, stretch: u32) -> Self {
        Self {
            level,
            mode,
            automatic,
            stretch: stretch.max(1),
        }
    }

    /// How often to do something done every `base` milliseconds in
    /// Normal: `stretch` times as seldom in Saving, never in Offline.
    pub fn every(&self, base: u64) -> Option<u64> {
        match self.level {
            PowerLevel::Normal => Some(base),
            PowerLevel::Saving => Some(base.saturating_mul(u64::from(self.stretch))),
            PowerLevel::Offline => None,
        }
    }

    /// Whether the network may be used for work of a plugin's own.
    pub fn online(&self) -> bool {
        self.level != PowerLevel::Offline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_named_both_ways() {
        for level in [PowerLevel::Normal, PowerLevel::Saving, PowerLevel::Offline] {
            assert_eq!(PowerLevel::parse(level.as_str()), Some(level));
        }
        assert_eq!(PowerLevel::parse(" saving "), Some(PowerLevel::Saving));
        assert_eq!(PowerLevel::parse("eco"), None);
    }

    #[test]
    fn intervals_follow_the_level() {
        let normal = Power::default();
        assert_eq!(normal.every(60_000), Some(60_000));
        let saving = Power::new(PowerLevel::Saving, SAVING, true, 4);
        assert_eq!(saving.every(60_000), Some(240_000));
        let offline = Power::new(PowerLevel::Offline, OFFLINE, false, 4);
        assert_eq!(offline.every(60_000), None);
        assert!(!offline.online());
    }

    #[test]
    fn a_stretch_is_at_least_one() {
        let power = Power::new(PowerLevel::Saving, SAVING, false, 0);
        assert_eq!(power.every(10), Some(10));
    }
}
