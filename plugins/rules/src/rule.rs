//! Rules: when something happens, do something, if a condition holds.
//! Kept in the plugin's folder, one rule a line:
//! `trigger<TAB>effect<TAB>target<TAB>on|off<TAB>condition` (no condition:
//! always).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::{Env, Id, Storage, StorageError};

use crate::PLUGIN;

/// Days of the week as bits, Sunday the lowest (as `LocalTime::weekday`).
pub(crate) const EVERY_DAY: u8 = 0b111_1111;
/// Monday to Friday.
pub(crate) const WEEKDAYS: u8 = 0b011_1110;
/// Saturday and Sunday.
pub(crate) const WEEKENDS: u8 = 0b100_0001;

/// When a rule applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// PaperPaw has started.
    Start,
    /// At this minute of the day, on the days set ([`EVERY_DAY`] …).
    Time {
        minute: u16,
        days: u8,
    },
    /// Every so many minutes, from the start.
    Every(u16),
    /// A book was opened, closed, read to its last page.
    BookOpened,
    BookClosed,
    BookFinished,
    /// The device woke up, or goes to sleep.
    Resume,
    Sleep,
    /// The battery's charge fell below this percentage.
    BatteryBelow(u8),
    /// The battery's charge rose to this percentage or above: once, not
    /// again until it fell clearly below ([`Levels`]).
    BatteryAtLeast(u8),
    /// Charging began, or ended.
    Charging,
    Unplugged,
    /// A switch (an on/off setting) was turned on, or off.
    SwitchOn(Id),
    SwitchOff(Id),
}

/// What a rule does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Runs an action, e.g. `opds.sync`.
    Action(Id),
    /// Turns a switch (an on/off setting) on or off.
    Switch(Id, bool),
    /// Sets a number (a setting with a range), e.g. the light to 20.
    Number(Id, i64),
    /// Shows a message.
    Message(String),
}

/// When a rule may apply at all, besides its trigger.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Condition {
    #[default]
    Always,
    /// Between two minutes of the day (over midnight when `from` is
    /// later than `to`).
    Between {
        from: u16,
        to: u16,
    },
    /// While a book is open, or while none is.
    Reading,
    NotReading,
    /// While the device runs on its battery, or while it charges.
    OnBattery,
    Charging,
    /// While a switch is on, or off.
    SwitchOn(Id),
    SwitchOff(Id),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rule {
    pub trigger: Trigger,
    pub effect: Effect,
    /// Rules can be paused without being removed.
    pub on: bool,
    pub condition: Condition,
}

impl Condition {
    /// As written in the file: `between 22:00 06:00`, `reading`,
    /// `not-reading`, `battery`, `charging`, `switch-on <id>`,
    /// `switch-off <id>`; empty for always.
    fn text(&self) -> String {
        match self {
            Condition::Always => String::new(),
            Condition::Between { from, to } => format!("between {} {}", clock(*from), clock(*to)),
            Condition::Reading => String::from("reading"),
            Condition::NotReading => String::from("not-reading"),
            Condition::OnBattery => String::from("battery"),
            Condition::Charging => String::from("charging"),
            Condition::SwitchOn(id) => format!("switch-on {id}"),
            Condition::SwitchOff(id) => format!("switch-off {id}"),
        }
    }

    fn parse(text: &str) -> Option<Condition> {
        let text = text.trim();
        let (word, rest) = text.split_once(' ').unwrap_or((text, ""));
        let rest = rest.trim();
        Some(match word {
            "" | "always" => Condition::Always,
            "between" => {
                let (from, to) = rest.split_once(' ')?;
                Condition::Between {
                    from: parse_time(from)?,
                    to: parse_time(to)?,
                }
            }
            "reading" => Condition::Reading,
            "not-reading" => Condition::NotReading,
            "battery" => Condition::OnBattery,
            "charging" => Condition::Charging,
            "switch-on" => Condition::SwitchOn(Id::parse(rest).ok()?),
            "switch-off" => Condition::SwitchOff(Id::parse(rest).ok()?),
            _ => return None,
        })
    }
}

/// Whether `minute` of the day lies between `from` and `to` (`to` not
/// included; over midnight when `from` is later).
pub(crate) fn between(minute: u16, from: u16, to: u16) -> bool {
    if from <= to {
        (from..to).contains(&minute)
    } else {
        minute >= from || minute < to
    }
}

impl Trigger {
    /// As written in the file: `start`, `time 22:00`, `time 07:00 62`,
    /// `every 30`, `book`, `book-closed`, `book-finished`, `resume`,
    /// `sleep`, `battery-below 20`, `battery-at 80`, `charging`,
    /// `unplugged`, `switch-on <id>`, `switch-off <id>`.
    fn text(&self) -> String {
        match self {
            Trigger::Start => String::from("start"),
            Trigger::Time { minute, days } if *days == EVERY_DAY => {
                format!("time {}", clock(*minute))
            }
            Trigger::Time { minute, days } => format!("time {} {days}", clock(*minute)),
            Trigger::Every(minutes) => format!("every {minutes}"),
            Trigger::BookOpened => String::from("book"),
            Trigger::BookClosed => String::from("book-closed"),
            Trigger::BookFinished => String::from("book-finished"),
            Trigger::Resume => String::from("resume"),
            Trigger::Sleep => String::from("sleep"),
            Trigger::BatteryBelow(percent) => format!("battery-below {percent}"),
            Trigger::BatteryAtLeast(percent) => format!("battery-at {percent}"),
            Trigger::Charging => String::from("charging"),
            Trigger::Unplugged => String::from("unplugged"),
            Trigger::SwitchOn(id) => format!("switch-on {id}"),
            Trigger::SwitchOff(id) => format!("switch-off {id}"),
        }
    }

    fn parse(text: &str) -> Option<Trigger> {
        let text = text.trim();
        let (word, rest) = text.split_once(' ').unwrap_or((text, ""));
        let rest = rest.trim();
        Some(match word {
            "start" => Trigger::Start,
            "book" => Trigger::BookOpened,
            "book-closed" => Trigger::BookClosed,
            "book-finished" => Trigger::BookFinished,
            "resume" => Trigger::Resume,
            "sleep" => Trigger::Sleep,
            "charging" => Trigger::Charging,
            "unplugged" => Trigger::Unplugged,
            "time" => {
                let (time, days) = rest.split_once(' ').unwrap_or((rest, ""));
                let days = match days.trim() {
                    "" => EVERY_DAY,
                    days => days.parse::<u8>().ok().filter(|d| d & EVERY_DAY != 0)? & EVERY_DAY,
                };
                Trigger::Time {
                    minute: parse_time(time)?,
                    days,
                }
            }
            "every" => Trigger::Every(rest.parse::<u16>().ok().filter(|&m| m > 0)?),
            "battery-below" => Trigger::BatteryBelow(parse_percent(rest)?),
            "battery-at" => Trigger::BatteryAtLeast(parse_percent(rest)?),
            "switch-on" => Trigger::SwitchOn(Id::parse(rest).ok()?),
            "switch-off" => Trigger::SwitchOff(Id::parse(rest).ok()?),
            _ => return None,
        })
    }
}

/// A battery level from 1 to 100 %, as in `80` or `80 %`.
pub(crate) fn parse_percent(text: &str) -> Option<u8> {
    text.trim()
        .trim_end_matches('%')
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|p| (1..=100).contains(p))
}

/// How far the battery has to fall below a level reached before reaching
/// it counts again, in points: the X4 estimates its level from the
/// battery's voltage, which wavers by a point or two between readings.
pub(crate) const REARM: u8 = 2;

/// Battery levels (bit `n` for `n` %): those reached and not fallen
/// clearly below since, or those reached just now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Levels(u128);

impl Levels {
    /// The levels up to `percent`, as reached by a battery at `percent`.
    pub(crate) fn up_to(percent: u8) -> Self {
        Levels(u128::MAX >> (127 - u32::from(percent.min(127))))
    }

    pub(crate) fn contains(self, percent: u8) -> bool {
        percent < 128 && self.0 & (1 << percent) != 0
    }

    /// The battery is at `percent` now: the levels it reached just now.
    /// Those it fell more than [`REARM`] points below count as not
    /// reached any more.
    pub(crate) fn reach(&mut self, percent: u8) -> Levels {
        let now = Levels::up_to(percent).0;
        let new = now & !self.0;
        self.0 = (self.0 | now) & Levels::up_to(percent.saturating_add(REARM)).0;
        Levels(new)
    }
}

/// `22:00` for minute 1320 of the day.
pub(crate) fn clock(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// The minute of the day for `22:00` (also `22.00`, `7:5`, `22`).
pub(crate) fn parse_time(text: &str) -> Option<u16> {
    let text = text.trim();
    let (hours, minutes) = text.split_once([':', '.', 'h']).unwrap_or((text, "0"));
    let hours: u16 = hours.trim().parse().ok()?;
    let minutes: u16 = minutes.trim().parse().ok()?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

fn path() -> String {
    format!("{}/rules.txt", Env::plugin_dir(PLUGIN.as_str()))
}

/// The rules the card keeps: none where there is no file yet, an error
/// where it cannot be read (then nothing may be saved over it).
pub(crate) fn load(storage: &dyn Storage) -> Result<Vec<Rule>, StorageError> {
    let data = match paperpaw_sdk::read_file(storage, &path(), 64 * 1024) {
        Ok(data) => data,
        Err(StorageError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(parse(&String::from_utf8_lossy(&data)))
}

fn parse(text: &str) -> Vec<Rule> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let trigger = Trigger::parse(fields.next()?)?;
            let kind = fields.next()?;
            let target = fields.next()?;
            let id = || Id::parse(target.trim()).ok();
            let effect = match kind {
                "action" => Effect::Action(id()?),
                "switch-on" => Effect::Switch(id()?, true),
                "switch-off" => Effect::Switch(id()?, false),
                "set" => {
                    let (id, value) = target.split_once('=')?;
                    Effect::Number(Id::parse(id.trim()).ok()?, value.trim().parse().ok()?)
                }
                "message" => Effect::Message(target.to_string()),
                _ => return None,
            };
            let on = fields.next().is_none_or(|f| f.trim() != "off");
            let condition = Condition::parse(fields.next().unwrap_or_default())?;
            Some(Rule {
                trigger,
                effect,
                on,
                condition,
            })
        })
        .collect()
}

pub(crate) fn save(storage: &dyn Storage, rules: &[Rule]) -> Result<(), StorageError> {
    let mut text = String::new();
    for rule in rules {
        let (kind, target) = match &rule.effect {
            Effect::Action(id) => ("action", id.to_string()),
            Effect::Switch(id, true) => ("switch-on", id.to_string()),
            Effect::Switch(id, false) => ("switch-off", id.to_string()),
            Effect::Number(id, value) => ("set", format!("{id}={value}")),
            // One line, no fields within it.
            Effect::Message(text) => ("message", text.replace(['\t', '\n', '\r'], " ")),
        };
        let mut line = format!(
            "{}\t{kind}\t{target}\t{}",
            rule.trigger.text(),
            if rule.on { "on" } else { "off" }
        );
        if rule.condition != Condition::Always {
            line.push('\t');
            line.push_str(&rule.condition.text());
        }
        text.push_str(&line);
        text.push('\n');
    }
    storage.write(&path(), text.as_bytes())
}

/// The rules, read once and kept: every event looks at them, and only
/// the list of rules changes them.
#[derive(Debug, Default)]
pub(crate) struct Store {
    rules: RefCell<Option<Vec<Rule>>>,
}

impl Store {
    /// Runs `f` with the rules; none while they cannot be read (they are
    /// tried again next time).
    pub(crate) fn with<R>(&self, storage: &dyn Storage, f: impl FnOnce(&[Rule]) -> R) -> R {
        if self.rules.borrow().is_none()
            && let Ok(rules) = load(storage)
        {
            *self.rules.borrow_mut() = Some(rules);
        }
        f(self.rules.borrow().as_deref().unwrap_or_default())
    }

    /// The rules to change, if they can be read.
    pub(crate) fn get(&self, storage: &dyn Storage) -> Option<Vec<Rule>> {
        self.with(storage, |_| ());
        self.rules.borrow().clone()
    }

    /// Keeps `rules`, on the card and here.
    pub(crate) fn save(&self, storage: &dyn Storage, rules: Vec<Rule>) -> Result<(), StorageError> {
        let saved = save(storage, &rules);
        // Not saved, what the card has is read again next time.
        *self.rules.borrow_mut() = saved.is_ok().then_some(rules);
        saved
    }
}

/// Minutes from `now` (minute of the day) until `at`: the next time the
/// clock shows `at`, today or tomorrow; never 0.
pub(crate) fn minutes_until(now: u16, at: u16) -> u16 {
    let ahead = (at + 24 * 60 - now) % (24 * 60);
    if ahead == 0 { 24 * 60 } else { ahead }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn rules_are_kept() {
        let storage = MemoryStorage::new();
        assert!(load(&storage).unwrap().is_empty());
        let night = Id::new_static("core.night-mode");
        let rules: Vec<Rule> = [
            Trigger::Time {
                minute: 22 * 60,
                days: EVERY_DAY,
            },
            Trigger::Time {
                minute: 7 * 60,
                days: WEEKDAYS,
            },
            Trigger::Start,
            Trigger::Every(30),
            Trigger::BookOpened,
            Trigger::BookClosed,
            Trigger::BookFinished,
            Trigger::Resume,
            Trigger::Sleep,
            Trigger::BatteryBelow(20),
            Trigger::BatteryAtLeast(80),
            Trigger::Charging,
            Trigger::Unplugged,
            Trigger::SwitchOn(night.clone()),
            Trigger::SwitchOff(night.clone()),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, trigger)| Rule {
            trigger,
            effect: match i % 4 {
                0 => Effect::Switch(night.clone(), true),
                1 => Effect::Action(Id::new_static("opds.sync")),
                2 => Effect::Number(Id::new_static("core.frontlight"), -3),
                _ => Effect::Message(String::from("Time to sleep")),
            },
            on: i % 3 != 0,
            condition: match i % 7 {
                0 => Condition::Always,
                1 => Condition::Between {
                    from: 22 * 60,
                    to: 6 * 60,
                },
                2 => Condition::Reading,
                3 => Condition::NotReading,
                4 => Condition::OnBattery,
                5 => Condition::Charging,
                _ => Condition::SwitchOff(night.clone()),
            },
        })
        .collect();
        save(&storage, &rules).unwrap();
        assert_eq!(load(&storage).unwrap(), rules);
        // Written before days existed: every day.
        assert_eq!(
            Trigger::parse("time 22:00"),
            Some(Trigger::Time {
                minute: 1320,
                days: EVERY_DAY
            })
        );
        assert_eq!(Trigger::parse("battery-below 0"), None);
        assert_eq!(Trigger::parse("battery-at 101"), None);
        assert_eq!(
            Trigger::parse("battery-at 80 %"),
            Some(Trigger::BatteryAtLeast(80))
        );
        assert_eq!(Trigger::parse("every 0"), None);
        assert_eq!(Trigger::parse("time 22:00 0"), None);
        // Written before conditions and messages: always; tabs in a
        // message do not split the line.
        save(
            &storage,
            &[Rule {
                trigger: Trigger::Start,
                effect: Effect::Message(String::from("a\tb")),
                on: true,
                condition: Condition::SwitchOn(night.clone()),
            }],
        )
        .unwrap();
        assert_eq!(
            load(&storage).unwrap()[0].effect,
            Effect::Message(String::from("a b"))
        );
        assert_eq!(Condition::parse(""), Some(Condition::Always));
        assert_eq!(Condition::parse("between 22:00"), None);
    }

    #[test]
    fn times_between() {
        assert!(between(23 * 60, 22 * 60, 6 * 60));
        assert!(between(60, 22 * 60, 6 * 60));
        assert!(!between(12 * 60, 22 * 60, 6 * 60));
        assert!(between(9 * 60, 8 * 60, 17 * 60));
        assert!(!between(17 * 60, 8 * 60, 17 * 60));
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("22:00"), Some(1320));
        assert_eq!(parse_time(" 7.05 "), Some(425));
        assert_eq!(parse_time("7"), Some(420));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("abc"), None);
        assert_eq!(clock(425), "07:05");
        assert_eq!(minutes_until(1300, 1320), 20);
        assert_eq!(minutes_until(1330, 1320), 24 * 60 - 10);
        assert_eq!(minutes_until(1320, 1320), 24 * 60, "just done: tomorrow");
    }

    #[test]
    fn levels_reached() {
        let mut levels = Levels::up_to(75);
        assert!(levels.contains(75) && !levels.contains(76));
        // Rising: each level once.
        let new = levels.reach(81);
        assert!((76..=81).all(|p| new.contains(p)));
        assert!(!new.contains(75) && !new.contains(82));
        assert_eq!(levels.reach(81), Levels::default());
        // Wavering a little: not again.
        assert_eq!(levels.reach(79), Levels::default());
        assert_eq!(levels.reach(81), Levels::default());
        // Clearly below, then up again: once more.
        assert_eq!(levels.reach(77), Levels::default());
        assert!(levels.reach(80).contains(80));
        assert!(Levels::up_to(100).contains(100));
        assert!(Levels::up_to(0).contains(0) && !Levels::up_to(0).contains(1));
        assert_eq!(parse_percent(" 80 % "), Some(80));
        assert_eq!(parse_percent("0"), None);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn rules_of_version_1_are_read() {
        let storage = MemoryStorage::new().with(&path(), include_str!("../testdata/rules.txt.v1"));
        let rules = load(&storage).unwrap();
        let night = Id::new_static("core.night-mode");
        assert_eq!(rules.len(), 5);
        assert_eq!(
            rules[0].trigger,
            Trigger::Time {
                minute: 22 * 60,
                days: EVERY_DAY
            }
        );
        assert_eq!(rules[0].effect, Effect::Switch(night.clone(), true));
        assert_eq!(rules[0].condition, Condition::Always);
        assert_eq!(
            rules[1].trigger,
            Trigger::Time {
                minute: 7 * 60,
                days: WEEKDAYS
            }
        );
        assert_eq!(rules[1].effect, Effect::Switch(night, false));
        assert_eq!(
            rules[2].effect,
            Effect::Message(String::from("Battery low"))
        );
        assert!(!rules[2].on);
        assert_eq!(rules[2].condition, Condition::NotReading);
        assert_eq!(
            rules[3].effect,
            Effect::Number(Id::new_static("reader.font-size"), 12)
        );
        assert_eq!(
            rules[3].condition,
            Condition::Between {
                from: 8 * 60,
                to: 17 * 60
            }
        );
        assert_eq!(rules[4].trigger, Trigger::BatteryAtLeast(80));
        assert_eq!(
            rules[4].effect,
            Effect::Message(String::from("Unplug now to spare the battery"))
        );
        assert!(!rules[4].on);
        assert_eq!(rules[4].condition, Condition::Charging);
    }
}
