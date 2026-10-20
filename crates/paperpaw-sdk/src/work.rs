//! Declared work: what a plugin wants done, with what it needs and by
//! when, run by the kernel at the moment that costs least (ADR 0025).
//!
//! A plugin does not reach the network on its own account in the
//! background. It declares the work ([`Command::Work`]), and when it is
//! time the kernel emits the work's event with its parameters, as a
//! timer would; the plugin does the work then. The kernel runs together
//! everything that needs the network, so a reading session costs one
//! short connection when it starts and one when it ends:
//!
//! ```
//! use core::time::Duration;
//! use paperpaw_sdk::work::{Moments, Work};
//! use paperpaw_sdk::{Command, Id};
//!
//! const PUSH: Id = Id::new_static("kosync.push");
//! // The position: at the next natural moment, at the latest in 30 min.
//! let push = Work::network(PUSH)
//!     .expected()
//!     .within(Duration::from_secs(30 * 60))
//!     .at_moments(Moments::BOOK_CLOSED | Moments::SLEEP | Moments::ONLINE);
//! let command = Command::Work(push);
//! # let _ = command;
//! ```
//!
//! - **The id** is an event of the plugin. Declaring the same id again
//!   replaces the work; [`Command::CancelWork`] takes it back.
//! - **What it needs**: nothing, the network ([`Work::network`]), the
//!   device charging ([`Work::charging`]).
//! - **How much the reader counts on it**: [`expected`](Work::expected)
//!   work is done, at the latest by its deadline;
//!   [`optional`](Work::optional) work is done when it is cheap and left
//!   out when it is not.
//! - **When**: not before ([`after`](Work::after)), at the latest
//!   ([`within`](Work::within); without one, only when it is cheap), or
//!   exactly ([`Work::at`], a timer); [`every`](Work::every) declares it
//!   again after each run.
//! - **At which moments** it may run ([`Moments`]): waking, a book opened
//!   or closed, going to sleep, the radio on anyway, charging, idle.
//!
//! **A run** lasts while the requests made in handling the work's event,
//! and in handling their answers, are not answered; the kernel keeps the
//! radio on for them and knows when the work is done.
//!
//! **Nothing is refused silently, nothing nags.** Work held back by the
//! power mode stays queued and runs when allowed; work that can never run
//! (the reader switched the plugin's network off, the manifest does not
//! declare it) is taken out with [`events::WORK_REFUSED`] and a
//! [`Refusal`] saying why and when to try again. A request that does not
//! go out is answered with one too ([`HttpReply::refused`]). None of this
//! is ever shown over reading: background work cannot raise notices, its
//! [`Command::Notify`] becomes the plugin's note, shown on its own page,
//! the Power page and the battery view.
//!
//! [`Command::Work`]: crate::Command::Work
//! [`Command::CancelWork`]: crate::Command::CancelWork
//! [`Command::Notify`]: crate::Command::Notify
//! [`events::WORK_REFUSED`]: crate::builtin::events::WORK_REFUSED
//! [`HttpReply::refused`]: crate::HttpReply::refused

use alloc::string::{String, ToString};
use core::ops::BitOr;
use core::time::Duration;

use paperpaw_platform::Instant;

use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// Natural moments at which work may run, cheaply: the radio is on
/// anyway, or about to be for the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Moments(u8);

impl Moments {
    pub const NONE: Moments = Moments(0);
    /// The device started or woke up.
    pub const WAKE: Moments = Moments(1);
    /// A book was opened.
    pub const BOOK_OPENED: Moments = Moments(1 << 1);
    /// A book was closed.
    pub const BOOK_CLOSED: Moments = Moments(1 << 2);
    /// The device goes to sleep; it waits a little for work running then.
    pub const SLEEP: Moments = Moments(1 << 3);
    /// The radio is on anyway, for the reader or for other work.
    pub const ONLINE: Moments = Moments(1 << 4);
    /// The device was plugged in.
    pub const CHARGING: Moments = Moments(1 << 5);
    /// No input for a while, the screen still on.
    pub const IDLE: Moments = Moments(1 << 6);
    pub const ALL: Moments = Moments(0x7f);

    const NAMES: [(Moments, &'static str); 7] = [
        (Moments::WAKE, "wake"),
        (Moments::BOOK_OPENED, "book-opened"),
        (Moments::BOOK_CLOSED, "book-closed"),
        (Moments::SLEEP, "sleep"),
        (Moments::ONLINE, "online"),
        (Moments::CHARGING, "charging"),
        (Moments::IDLE, "idle"),
    ];

    /// Whether every moment of `other` is among these.
    pub const fn contains(self, other: Moments) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any moment of `other` is among these.
    pub const fn intersects(self, other: Moments) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Moments {
        Moments(bits & Moments::ALL.0)
    }

    /// Their names, `,` between them, e.g. `book-closed,sleep`.
    pub fn names(self) -> String {
        let mut text = String::new();
        for (moment, name) in Self::NAMES {
            if self.contains(moment) {
                if !text.is_empty() {
                    text.push(',');
                }
                text.push_str(name);
            }
        }
        text
    }

    /// The moments named in `text` ([`names`](Self::names)); unknown
    /// names are passed over.
    pub fn parse(text: &str) -> Moments {
        text.split(',')
            .filter_map(|name| {
                Self::NAMES
                    .iter()
                    .find(|(_, n)| *n == name.trim())
                    .map(|(m, _)| *m)
            })
            .fold(Moments::NONE, |all, m| all | m)
    }
}

impl BitOr for Moments {
    type Output = Moments;

    fn bitor(self, other: Moments) -> Moments {
        Moments(self.0 | other.0)
    }
}

/// How much the reader counts on a work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Importance {
    /// The reader counts on it (a position, an export they sent): it is
    /// done, at the latest by its deadline (made longer by the mode).
    #[default]
    Expected,
    /// Nice to have (weather, headlines, a catalog refreshed): done when
    /// it is cheap; in Saving only with the radio on anyway or charging,
    /// in Offline not at all.
    Optional,
}

/// Something a plugin wants done (see the [module](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Work {
    /// The event emitted when it runs; the same id replaces it.
    pub id: Id,
    /// Passed with the event.
    pub params: Params,
    /// It needs the network.
    pub network: bool,
    /// It runs only while the device charges.
    pub charging: bool,
    pub importance: Importance,
    /// An exact time: a timer, run then whatever else holds.
    pub at: Option<Instant>,
    /// Not before this long from now.
    pub after: Option<Duration>,
    /// At the latest this long from now (made longer by the mode); none:
    /// only when it is cheap, never on its own account.
    pub within: Option<Duration>,
    /// Declared again this long after each run.
    pub every: Option<Duration>,
    /// The moments at which it may run.
    pub moments: Moments,
}

impl Work {
    /// Work needing nothing, at any moment (e.g. counting pages while
    /// idle: add [`at_moments`](Self::at_moments)).
    pub fn new(id: impl Into<Id>) -> Self {
        Self {
            id: id.into(),
            params: Params::new(),
            network: false,
            charging: false,
            importance: Importance::Expected,
            at: None,
            after: None,
            within: None,
            every: None,
            moments: Moments::ALL,
        }
    }

    /// Work needing the network.
    pub fn network(id: impl Into<Id>) -> Self {
        Self {
            network: true,
            ..Self::new(id)
        }
    }

    /// A timer: the event at `at`, exactly, whatever screen is open.
    pub fn at(id: impl Into<Id>, at: Instant) -> Self {
        Self {
            at: Some(at),
            moments: Moments::NONE,
            ..Self::new(id)
        }
    }

    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    pub fn expected(mut self) -> Self {
        self.importance = Importance::Expected;
        self
    }

    pub fn optional(mut self) -> Self {
        self.importance = Importance::Optional;
        self
    }

    /// Only while the device charges (e.g. a large download).
    pub fn charging(mut self) -> Self {
        self.charging = true;
        self
    }

    pub fn after(mut self, delay: Duration) -> Self {
        self.after = Some(delay);
        self
    }

    pub fn within(mut self, deadline: Duration) -> Self {
        self.within = Some(deadline);
        self
    }

    pub fn every(mut self, period: Duration) -> Self {
        self.every = Some(period);
        self
    }

    pub fn at_moments(mut self, moments: Moments) -> Self {
        self.moments = moments;
        self
    }

    /// Again after a failed attempt (the first is 1): not before 30 s,
    /// doubling up to 6 h, with up to a tenth more so that devices do not
    /// all return at once (`jitter`, 0 to 255, e.g. a random byte).
    pub fn retry(self, attempt: u32, jitter: u8) -> Self {
        self.after(backoff(attempt, jitter))
    }
}

/// The first wait after a failure.
pub const RETRY_FIRST: Duration = Duration::from_secs(30);
/// The longest wait between attempts.
pub const RETRY_MOST: Duration = Duration::from_secs(6 * 3600);

/// How long to wait before `attempt` (the first is 1): [`RETRY_FIRST`]
/// doubling up to [`RETRY_MOST`], plus up to a tenth by `jitter`.
pub fn backoff(attempt: u32, jitter: u8) -> Duration {
    let first = RETRY_FIRST.as_millis() as u64;
    let most = RETRY_MOST.as_millis() as u64;
    let doubled = first.saturating_mul(1u64 << attempt.saturating_sub(1).min(20));
    let base = doubled.min(most);
    let extra = base / 10 * u64::from(jitter) / 255;
    Duration::from_millis(base + extra)
}

/// Why a work or a request did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Reason {
    /// The mode is Offline: network work waits for the reader's next
    /// connection.
    Offline,
    /// The reader switched this plugin's network off.
    NetworkOff,
    /// The plugin's manifest does not say it uses the network
    /// ([`Uses::NETWORK`](crate::Uses::NETWORK)).
    NotDeclared,
    /// The mode defers it to a cheaper moment.
    Saving,
    /// No known network, or joining it failed.
    NoNetwork,
    /// The device switched off before it was done.
    Sleeping,
    /// The plugin asked again before it was told it might.
    TooOften,
}

impl Reason {
    const NAMES: [(Reason, &'static str); 7] = [
        (Reason::Offline, "offline"),
        (Reason::NetworkOff, "network-off"),
        (Reason::NotDeclared, "not-declared"),
        (Reason::Saving, "saving"),
        (Reason::NoNetwork, "no-network"),
        (Reason::Sleeping, "sleeping"),
        (Reason::TooOften, "too-often"),
    ];

    pub fn as_str(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(r, _)| *r == self)
            .map_or("", |(_, n)| n)
    }

    pub fn parse(text: &str) -> Option<Reason> {
        Self::NAMES
            .iter()
            .find(|(_, n)| *n == text)
            .map(|(r, _)| *r)
    }
}

/// When a refused work or request may be tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Retry {
    /// Not before then.
    After(Instant),
    /// At the next of these moments.
    At(Moments),
    /// When the mode or the reader allows it, told by
    /// [`events::WORK_ALLOWED`](crate::builtin::events::WORK_ALLOWED).
    WhenAllowed,
    /// Not without the reader changing something.
    Never,
}

impl Retry {
    fn write(self) -> String {
        match self {
            Retry::After(at) => alloc::format!("after:{}", at.millis()),
            Retry::At(moments) => alloc::format!("at:{}", moments.names()),
            Retry::WhenAllowed => String::from("allowed"),
            Retry::Never => String::from("never"),
        }
    }

    fn parse(text: &str) -> Option<Retry> {
        if let Some(millis) = text.strip_prefix("after:") {
            return millis.parse().ok().map(|m| Retry::After(Instant(m)));
        }
        if let Some(names) = text.strip_prefix("at:") {
            return Some(Retry::At(Moments::parse(names)));
        }
        match text {
            "allowed" => Some(Retry::WhenAllowed),
            "never" => Some(Retry::Never),
            _ => None,
        }
    }
}

/// Why a work or a request did not run, and when to try again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Refusal {
    pub reason: Reason,
    pub retry: Retry,
}

impl Refusal {
    pub const fn new(reason: Reason, retry: Retry) -> Self {
        Self { reason, retry }
    }

    /// Writes it into `params` ([`params::REFUSED`], [`params::RETRY`]).
    pub fn write(&self, params: &mut Params) {
        params.set(params::REFUSED, self.reason.as_str().to_string());
        params.set(params::RETRY, self.retry.write());
    }

    /// The refusal in `params`, if there is one.
    pub fn read(params: &Params) -> Option<Refusal> {
        let reason = Reason::parse(params.get(params::REFUSED)?)?;
        let retry = params
            .get(params::RETRY)
            .and_then(Retry::parse)
            .unwrap_or(Retry::Never);
        Some(Refusal { reason, retry })
    }
}

/// Parameters the kernel adds to work's events and refused replies.
pub mod params {
    /// Why a work runs now: `time`, `deadline` or a moment's name
    /// ([`Moments::names`](super::Moments::names)).
    pub const WHY: &str = "work-why";
    /// The work a refusal is about ([`WorkRefused`](super::WorkRefused)).
    pub const WORK: &str = "work";
    /// A refusal's reason ([`Reason::as_str`](super::Reason::as_str)).
    pub const REFUSED: &str = "refused";
    /// When to try again.
    pub const RETRY: &str = "retry";
}

/// Why a work runs now, from its event's parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Its exact time ([`Work::at`]).
    Time,
    /// Its deadline came.
    Deadline,
    /// One of its moments.
    Moment(Moments),
}

impl Why {
    /// Writes it into a work's event parameters.
    pub fn write(self, params: &mut Params) {
        let text = match self {
            Why::Time => String::from("time"),
            Why::Deadline => String::from("deadline"),
            Why::Moment(moments) => moments.names(),
        };
        params.set(params::WHY, text);
    }

    /// Why the work whose event brought `params` runs.
    pub fn read(params: &Params) -> Option<Why> {
        match params.get(params::WHY)? {
            "time" => Some(Why::Time),
            "deadline" => Some(Why::Deadline),
            names => Some(Why::Moment(Moments::parse(names))).filter(|w| match w {
                Why::Moment(m) => !m.is_empty(),
                _ => true,
            }),
        }
    }
}

/// A declared work taken out because it can never run
/// ([`events::WORK_REFUSED`](crate::builtin::events::WORK_REFUSED)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkRefused {
    pub work: Id,
    pub refusal: Refusal,
}

impl EventData for WorkRefused {
    fn to_params(&self) -> Params {
        let mut params = Params::new().with(params::WORK, self.work.as_str().to_string());
        self.refusal.write(&mut params);
        params
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(Self {
            work: Id::parse(params.get(params::WORK)?).ok()?,
            refusal: Refusal::read(params)?,
        })
    }
}

/// Where a declared work stands, for a plugin's page ("Waiting for the
/// next connection", "Held: Offline mode").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkState {
    /// Queued; at the latest at `due` (none: only when it is cheap).
    Waiting { due: Option<Instant> },
    /// Queued, but held back, e.g. by the mode.
    Held(Refusal),
    /// Its event was emitted and its requests are not all answered.
    Running,
}

/// What a plugin's background work last said, kept by the kernel in
/// place of a notice: background work never shows one over reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub at: Instant,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_are_named_both_ways() {
        let m = Moments::BOOK_CLOSED | Moments::SLEEP | Moments::ONLINE;
        assert_eq!(m.names(), "book-closed,sleep,online");
        assert_eq!(Moments::parse(&m.names()), m);
        assert_eq!(Moments::parse("sleep, nonsense"), Moments::SLEEP);
        assert!(m.contains(Moments::SLEEP));
        assert!(!m.contains(Moments::SLEEP | Moments::WAKE));
        assert!(m.intersects(Moments::SLEEP | Moments::WAKE));
        assert_eq!(Moments::from_bits(0xff), Moments::ALL);
    }

    #[test]
    fn builders_say_what_they_set() {
        let w = Work::network(Id::new_static("kosync.push"))
            .optional()
            .within(Duration::from_secs(60))
            .every(Duration::from_secs(3600))
            .at_moments(Moments::SLEEP);
        assert!(w.network && !w.charging);
        assert_eq!(w.importance, Importance::Optional);
        assert_eq!(w.within, Some(Duration::from_secs(60)));
        assert_eq!(w.every, Some(Duration::from_secs(3600)));
        assert_eq!(w.moments, Moments::SLEEP);
        let timer = Work::at(Id::new_static("rules.time"), Instant(5000));
        assert_eq!(timer.at, Some(Instant(5000)));
        assert!(!timer.network);
        assert_eq!(timer.moments, Moments::NONE);
    }

    #[test]
    fn backoff_doubles_up_to_six_hours() {
        assert_eq!(backoff(1, 0), Duration::from_secs(30));
        assert_eq!(backoff(2, 0), Duration::from_secs(60));
        assert_eq!(backoff(3, 0), Duration::from_secs(120));
        assert_eq!(backoff(40, 0), RETRY_MOST);
        assert_eq!(backoff(1, 255), Duration::from_secs(33));
        assert_eq!(backoff(0, 0), Duration::from_secs(30));
    }

    #[test]
    fn refusals_survive_parameters() {
        for retry in [
            Retry::After(Instant(1234)),
            Retry::At(Moments::WAKE | Moments::ONLINE),
            Retry::WhenAllowed,
            Retry::Never,
        ] {
            let refusal = Refusal::new(Reason::Saving, retry);
            let mut params = Params::new();
            refusal.write(&mut params);
            assert_eq!(Refusal::read(&params), Some(refusal));
        }
        assert_eq!(Refusal::read(&Params::new()), None);
        for (reason, _) in Reason::NAMES {
            assert_eq!(Reason::parse(reason.as_str()), Some(reason));
        }
    }

    #[test]
    fn refused_work_is_an_event() {
        let refused = WorkRefused {
            work: Id::new_static("stats.sync"),
            refusal: Refusal::new(Reason::NetworkOff, Retry::Never),
        };
        assert_eq!(
            WorkRefused::from_params(&refused.to_params()),
            Some(refused)
        );
    }

    #[test]
    fn why_a_work_runs() {
        for why in [Why::Time, Why::Deadline, Why::Moment(Moments::SLEEP)] {
            let mut params = Params::new();
            why.write(&mut params);
            assert_eq!(Why::read(&params), Some(why));
        }
        assert_eq!(Why::read(&Params::new().with(params::WHY, "x")), None);
    }
}
