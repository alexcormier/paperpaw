//! Declared work (ADR 0025): one queue for plugins' timers, deadlines,
//! natural moments and jobs.
//!
//! A work runs at its exact time, at one of its moments (waking, a book
//! opened or closed, going to sleep, the radio on anyway, charging, idle)
//! or when its deadline comes, whichever is first and allowed by the power
//! level. Work that needs the network and is only nice to have runs only
//! where that is cheap: with the radio on anyway, or where PaperPaw does
//! not manage Wi-Fi. A work with a step function ([`WorkSpec`]) is a job,
//! stepped in [`Kernel::tick`], one step a call.

use alloc::string::String;
use alloc::vec::Vec;
use core::mem;

use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::power::{Power, PowerLevel};
use paperpaw_sdk::work::{
    Importance, Moments, Progress, Reason, Refusal, Retry, Step, Why, Work, WorkRefused, WorkSpec,
    WorkState,
};
use paperpaw_sdk::{Command, Env, HttpReply, Id, Params, ServerNotice, SocketNotice, Uses};

use crate::Kernel;

/// Without input this long, the screen still on: the `IDLE` moment.
pub(crate) const IDLE_MS: u64 = 10_000;
/// A job's progress is told at most this often.
const PROGRESS_MS: u64 = 1_000;

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Job {
    /// A step to take.
    Ready,
    /// Nothing to do before then.
    Until(Instant),
    /// Nothing to do before this event.
    For(Id),
}

#[derive(Debug)]
struct Entry {
    work: Work,
    /// The plugin it is of: its id's namespace.
    owner: String,
    /// When it was declared: `after` and `within` count from here.
    declared: Instant,
    /// Failed attempts of a job, for its backoff.
    attempt: u32,
    /// A job running.
    job: Option<Job>,
    progress: Option<Progress>,
    /// When its progress was told last.
    told: Option<Instant>,
}

impl Entry {
    fn after(&self) -> Instant {
        self.declared.add_millis(millis(self.work.after))
    }

    /// When it is due at the latest, at this power: none for work that
    /// waits for a cheap moment.
    fn deadline(&self, power: &Power) -> Option<Instant> {
        let within = self.work.within?;
        let saving = power.level != PowerLevel::Normal;
        if self.work.importance == Importance::Optional && saving {
            return None;
        }
        let stretch = if saving { u64::from(power.stretch) } else { 1 };
        Some(
            self.after()
                .add_millis(millis(Some(within)).saturating_mul(stretch)),
        )
    }

    /// Held back by the power level.
    fn held(&self, power: &Power) -> bool {
        self.work.network && power.level == PowerLevel::Offline
    }
}

fn millis(duration: Option<core::time::Duration>) -> u64 {
    duration.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The work plugins declared, and the jobs' steps.
#[derive(Debug, Default)]
pub(crate) struct Queue {
    entries: Vec<Entry>,
    specs: Vec<WorkSpec>,
    /// Most works a plugin may have declared.
    per_plugin: usize,
    /// Moments seen since work last ran.
    moments: Moments,
    /// The radio was on (or the network there, where PaperPaw does not
    /// manage Wi-Fi) when work last ran.
    online: bool,
    charging: bool,
    /// The input `IDLE` was given for, so it comes once each time.
    idle_since: Option<Instant>,
    /// The call is a tick: times and deadlines come due, and jobs may
    /// take a step (never while input waits). Moments come at any call.
    ticking: bool,
}

impl Queue {
    pub(crate) fn new(per_plugin: usize) -> Self {
        Self {
            per_plugin,
            ..Self::default()
        }
    }

    /// A job's steps, from a plugin's registration.
    pub(crate) fn add_spec(&mut self, spec: WorkSpec) {
        self.specs.retain(|s| s.id != spec.id);
        self.specs.push(spec);
    }

    /// Declares `work`, in place of one with the same id and key.
    fn declare(&mut self, work: Work, now: Instant, out: &mut Vec<Command>) {
        let owner = String::from(work.id.namespace());
        let same = |e: &Entry| e.work.id == work.id && e.work.key == work.key;
        if let Some(entry) = self.entries.iter_mut().find(|e| same(e)) {
            *entry = Entry::new(work, owner, now);
            return;
        }
        if self.entries.iter().filter(|e| e.owner == owner).count() >= self.per_plugin {
            let refused = WorkRefused {
                work: work.id,
                refusal: Refusal::new(Reason::TooOften, Retry::WhenAllowed),
            };
            out.push(events::WORK_REFUSED.emit(refused));
            return;
        }
        self.entries.push(Entry::new(work, owner, now));
    }

    /// Takes back the work with this id and key (any key: `None`).
    fn cancel(&mut self, id: &Id, key: Option<&str>) {
        self.entries
            .retain(|e| !(e.work.id == *id && key.is_none_or(|k| e.work.key == k)));
    }

    /// An event was emitted: a moment, or what a job waited for.
    pub(crate) fn saw(&mut self, event: &Id) {
        let moment = match event {
            e if *e == events::STARTED || *e == events::RESUME => Moments::WAKE,
            e if *e == events::BOOK_OPENED => Moments::BOOK_OPENED,
            e if *e == events::BOOK_CLOSED => Moments::BOOK_CLOSED,
            e if *e == events::SUSPEND => Moments::SLEEP,
            _ => Moments::NONE,
        };
        self.moments = self.moments | moment;
        for entry in &mut self.entries {
            if entry.job.as_ref() == Some(&Job::For(event.clone())) {
                entry.job = Some(Job::Ready);
            }
        }
    }

    /// The call is a tick ([`Kernel::tick`]).
    pub(crate) fn ticking(&mut self) {
        self.ticking = true;
    }

    /// When the kernel must look at the work again, at the latest.
    pub(crate) fn next_wake(&self, power: &Power, last_activity: Instant) -> Option<Instant> {
        let mut next: Option<Instant> = None;
        let mut earliest = |at: Instant| next = Some(next.map_or(at, |n: Instant| n.min(at)));
        for entry in &self.entries {
            match &entry.job {
                Some(Job::Ready) => earliest(Instant(0)),
                Some(Job::Until(at)) => earliest(*at),
                Some(Job::For(_)) => {}
                None if entry.held(power) => {}
                None => {
                    if let Some(at) = entry.work.at {
                        earliest(at);
                    } else if let Some(due) = entry.deadline(power) {
                        earliest(due);
                    }
                    if entry.work.moments.contains(Moments::IDLE) && entry.job.is_none() {
                        let idle = last_activity.add_millis(IDLE_MS);
                        if self.idle_since != Some(last_activity) {
                            earliest(idle.max(entry.after()));
                        }
                    }
                }
            }
        }
        next
    }

    /// Where each work stands.
    fn states(&self, power: &Power) -> Vec<(Id, String, WorkState)> {
        self.entries
            .iter()
            .map(|e| {
                let state = if e.job.is_some() {
                    WorkState::Running {
                        progress: e.progress,
                    }
                } else if e.held(power) {
                    WorkState::Held(Refusal::new(Reason::Offline, Retry::WhenAllowed))
                } else {
                    WorkState::Waiting {
                        due: e.work.at.or_else(|| e.deadline(power)),
                    }
                };
                (e.work.id.clone(), e.work.key.clone(), state)
            })
            .collect()
    }

    /// The moments since the last run, with the radio, charging and idle
    /// looked at now.
    fn take_moments(&mut self, env: &Env<'_>, last_activity: Instant) -> Moments {
        let mut moments = mem::take(&mut self.moments);
        let online = env.wifi.map_or(env.caps.network, |w| w.state.is_online());
        if online && (!self.online || moments.contains(Moments::WAKE)) {
            moments = moments | Moments::ONLINE;
        }
        self.online = online;
        let charging = env.status.battery.is_some_and(|b| b.charging);
        if charging && !self.charging {
            moments = moments | Moments::CHARGING;
        }
        self.charging = charging;
        if env.now.since(last_activity) >= IDLE_MS && self.idle_since != Some(last_activity) {
            self.idle_since = Some(last_activity);
            moments = moments | Moments::IDLE;
        }
        moments
    }

    /// Why `entry` runs now, if it does.
    fn why(&self, entry: &Entry, moments: Moments, env: &Env<'_>) -> Option<Why> {
        if entry.job.is_some() || entry.held(env.power) {
            return None;
        }
        if let Some(at) = entry.work.at {
            return (self.ticking && at <= env.now).then_some(Why::Time);
        }
        if entry.after() > env.now || (entry.work.charging && !self.charging) {
            return None;
        }
        // Work only nice to have never brings the radio on.
        let cheap = !entry.work.network || self.online;
        let mut hit = Moments::from_bits(moments.bits() & entry.work.moments.bits());
        if entry.work.importance == Importance::Optional && !cheap {
            hit = Moments::NONE;
        }
        if !hit.is_empty() {
            return Some(Why::Moment(hit));
        }
        let optional_now = entry.work.importance == Importance::Optional && cheap;
        let due = self.ticking && entry.deadline(env.power).is_some_and(|due| due <= env.now);
        (due && (entry.work.importance == Importance::Expected || optional_now))
            .then_some(Why::Deadline)
    }
}

impl Entry {
    fn new(work: Work, owner: String, now: Instant) -> Self {
        Self {
            work,
            owner,
            declared: now,
            attempt: 0,
            job: None,
            progress: None,
            told: None,
        }
    }
}

impl Kernel {
    /// Declares or takes back work ([`Command::Work`],
    /// [`Command::CancelWork`], and timers of [`Command::Schedule`]).
    pub(crate) fn apply_work(&mut self, command: Command) {
        match command {
            Command::Work(work) => self
                .works
                .declare(work, self.now, &mut self.output.commands),
            #[expect(deprecated, reason = "timers of plugins not moved yet")]
            Command::Schedule(event, params, at) => {
                let work = Work::at(event, at).with(params);
                self.works
                    .declare(work, self.now, &mut self.output.commands);
            }
            Command::CancelWork(id, key) => self.works.cancel(&id, key.as_deref()),
            _ => {}
        }
    }

    /// Runs the work that is due, and a job's step where allowed.
    pub(crate) fn run_work(&mut self) {
        let env = self.shared.env(self.now);
        let moments = self.works.take_moments(&env, self.last_activity);
        let mut index = 0;
        while index < self.works.entries.len() {
            let Some(why) = self.works.why(&self.works.entries[index], moments, &env) else {
                index += 1;
                continue;
            };
            let entry = &mut self.works.entries[index];
            if self.works.specs.iter().any(|s| s.id == entry.work.id) {
                entry.job = Some(Job::Ready);
                entry.progress = None;
                index += 1;
                continue;
            }
            let mut params = entry.work.params.clone();
            // A timer's event comes as it was asked for.
            if why != Why::Time {
                why.write(&mut params);
            }
            self.output
                .commands
                .push(Command::Emit(entry.work.id.clone(), params));
            if again(entry, self.now) {
                index += 1;
            } else {
                self.works.entries.remove(index);
            }
        }
        let mut notes = Vec::new();
        if mem::take(&mut self.works.ticking) {
            let meter = &self.shared.meter;
            step_job(
                &mut self.works,
                &env,
                meter,
                &mut self.output.commands,
                &mut notes,
            );
        }
        let states = self.works.states(env.power);
        self.shared.work_states.set_works(states);
        for (owner, note) in notes {
            self.shared.work_states.set_note(owner, note);
        }
    }
}

impl Kernel {
    /// Network use of a plugin whose manifest does not declare it
    /// ([`Uses::NETWORK`]): refused and answered as such. The command
    /// back where it may go on. A request's plugin is its reply's (a
    /// server's, a socket's): their ids are in the plugin's namespace.
    pub(crate) fn declared_network(&mut self, command: Command) -> Option<Command> {
        let owner = match &command {
            Command::Http(request) => request.reply.namespace(),
            Command::Serve { server, .. } => server.namespace(),
            Command::Connect { socket, .. } => socket.namespace(),
            Command::Broadcast { id, .. } => id.namespace(),
            _ => return Some(command),
        };
        // The kernel's own, and what no plugin it knows asked for, go on.
        let declared = owner == crate::core_plugin::CORE.as_str()
            || self
                .shared
                .plugins
                .get(owner)
                .is_none_or(|p| p.manifest.uses.contains(Uses::NETWORK));
        if declared {
            return Some(command);
        }
        let owner = String::from(owner);
        let refusal = Refusal::new(Reason::NotDeclared, Retry::Never);
        let reason = String::from(Reason::NotDeclared.as_str());
        let (event, params) = match command {
            Command::Http(request) => {
                let reply = HttpReply::refused(refusal, request.params);
                (request.reply, reply.to_params())
            }
            Command::Serve { server, events, .. } => {
                (events, ServerNotice::Failed { server, reason }.to_params())
            }
            Command::Connect { socket, events, .. } => {
                (events, SocketNotice::Failed { socket, reason }.to_params())
            }
            Command::Broadcast { id, events, .. } => {
                let failed = SocketNotice::Failed { socket: id, reason };
                (events, failed.to_params())
            }
            _ => return None,
        };
        self.output.commands.push(Command::Emit(event, params));
        self.warn_runtime(alloc::format!(
            "{owner}: uses the network without declaring it (`Manifest::uses`)"
        ));
        None
    }
}

/// One step of the first job ready.
fn step_job(
    works: &mut Queue,
    env: &Env<'_>,
    meter: &alloc::rc::Rc<crate::meter::Meter>,
    out: &mut Vec<Command>,
    notes: &mut Vec<(Id, paperpaw_sdk::work::Note)>,
) {
    let now = env.now;
    for entry in &mut works.entries {
        if let Some(Job::Until(at)) = entry.job
            && at <= now
        {
            entry.job = Some(Job::Ready);
        }
    }
    let Some(index) = works.entries.iter().position(|e| e.job == Some(Job::Ready)) else {
        return;
    };
    let Some(spec) = works
        .specs
        .iter()
        .find(|s| s.id == works.entries[index].work.id)
    else {
        works.entries.remove(index);
        return;
    };
    let entry = &mut works.entries[index];
    let step = {
        let _running = meter.enter(&entry.owner);
        (spec.step)(env, &entry.work.params, out)
    };
    let tell = |entry: &mut Entry, out: &mut Vec<Command>| {
        entry.told = Some(now);
        let params = Params::new().with(params::ID, String::from(entry.work.id.as_str()));
        out.push(Command::Emit(events::WORK_CHANGED.id().clone(), params));
    };
    match step {
        Step::Continue => {}
        Step::Progress(progress) => {
            entry.progress = Some(progress);
            if entry.told.is_none_or(|t| now.since(t) >= PROGRESS_MS) {
                tell(entry, out);
            }
        }
        Step::Wait(at) => entry.job = Some(Job::Until(at)),
        Step::WaitFor(event) => entry.job = Some(Job::For(event)),
        Step::Done => {
            entry.job = None;
            entry.attempt = 0;
            tell(entry, out);
            if !again(entry, now) {
                works.entries.remove(index);
            }
        }
        Step::Failed(text) => {
            entry.job = None;
            entry.attempt = entry.attempt.saturating_add(1);
            entry.declared = now;
            let mut jitter = [0u8];
            env.random.fill(&mut jitter);
            let jitter = jitter[0];
            entry.work.after = Some(paperpaw_sdk::work::backoff(entry.attempt, jitter));
            entry.work.at = None;
            let owner = Id::parse(&entry.owner).unwrap_or_else(|_| entry.work.id.clone());
            tell(entry, out);
            notes.push((owner, paperpaw_sdk::work::Note { at: now, text }));
        }
    }
}

/// Declares `entry` again after its period, if it has one; whether it
/// stays.
fn again(entry: &mut Entry, now: Instant) -> bool {
    let Some(every) = entry.work.every else {
        return false;
    };
    entry.declared = now;
    entry.work.after = Some(every);
    entry.work.at = None;
    entry.job = None;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    fn power(level: PowerLevel) -> Power {
        Power::new(level, paperpaw_sdk::power::NORMAL, false, 4)
    }

    #[test]
    fn deadlines_stretch_in_saving_and_optional_ones_wait() {
        let work = Work::network(Id::new_static("a.b")).within(Duration::from_secs(60));
        let entry = Entry::new(work.clone(), String::from("a"), Instant(1000));
        assert_eq!(
            entry.deadline(&power(PowerLevel::Normal)),
            Some(Instant(61_000))
        );
        assert_eq!(
            entry.deadline(&power(PowerLevel::Saving)),
            Some(Instant(241_000))
        );
        let optional = Entry::new(work.optional(), String::from("a"), Instant(1000));
        assert_eq!(optional.deadline(&power(PowerLevel::Saving)), None);
        assert!(entry.held(&power(PowerLevel::Offline)));
        assert!(!entry.held(&power(PowerLevel::Saving)));
    }

    #[test]
    fn the_same_id_and_key_replace_and_a_plugin_has_a_limit() {
        let mut queue = Queue::new(2);
        let mut out = Vec::new();
        let id = Id::new_static("a.b");
        queue.declare(Work::new(id.clone()), Instant(0), &mut out);
        queue.declare(Work::new(id.clone()), Instant(5), &mut out);
        assert_eq!(queue.entries.len(), 1);
        queue.declare(Work::new(id.clone()).key("2"), Instant(5), &mut out);
        queue.declare(Work::new(id.clone()).key("3"), Instant(5), &mut out);
        assert_eq!(queue.entries.len(), 2);
        assert_eq!(out.len(), 1, "the third is refused");
        queue.cancel(&id, Some("2"));
        assert_eq!(queue.entries.len(), 1);
        queue.cancel(&id, None);
        assert!(queue.entries.is_empty());
    }

    #[test]
    fn moments_come_from_events() {
        let mut queue = Queue::new(8);
        queue.saw(&events::STARTED.id().clone());
        queue.saw(&events::BOOK_CLOSED.id().clone());
        assert_eq!(queue.moments, Moments::WAKE | Moments::BOOK_CLOSED);
    }
}
