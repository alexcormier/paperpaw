//! Actions (SDK gap E2): run with params, handlers put in place of
//! others', what runs by itself, observers of actions and events, the
//! value a setting had.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{ObserverSpec, Params};

const APPLY: Id = Id::new_static("obs.apply");
const PLAIN: Id = Id::new_static("obs.plain");
const SCREEN_ONLY: Id = Id::new_static("obs.screen-only");
const WRAPPED: Id = Id::new_static("obs.wrapped");
const LOCK: Id = Id::new_static("g.lock");

/// Actions of its own, two handlers in place of one of them, and
/// observers of actions and of settings changed.
struct Observing(Log);

impl Plugin for Observing {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("obs"), "Observing", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_action(
            ActionSpec::new(APPLY, "obs.apply").handler_with(move |_, params, _| {
                let profile = params.get("profile").unwrap_or("none");
                log.borrow_mut().push(alloc::format!("apply {profile}"));
            }),
        )?;
        let log = self.0.clone();
        r.add_action(
            ActionSpec::new(PLAIN, "obs.plain")
                .describe("obs.plain.help")
                .keywords("obs.plain.words")
                .handler(move |_, _| log.borrow_mut().push(String::from("plain"))),
        )?;
        r.add_action(ActionSpec::new(SCREEN_ONLY, "obs.screen-only"))?;
        for (priority, name) in [(1, "low"), (5, "high")] {
            let log = self.0.clone();
            r.override_action(
                Id::new_static("obs.overridden"),
                move |_, _, _| log.borrow_mut().push(String::from(name)),
                Priority(priority),
            )?;
        }
        r.add_action(ActionSpec::new(
            Id::new_static("obs.overridden"),
            "obs.overridden",
        ))?;
        // An action of its own, wrapped by a handler in its place that
        // runs the original, tries to run itself and runs another.
        let log = self.0.clone();
        r.add_action(
            ActionSpec::new(WRAPPED, "obs.wrapped")
                .handler(move |_, _| log.borrow_mut().push(String::from("inner"))),
        )?;
        let log = self.0.clone();
        r.override_action(
            WRAPPED,
            move |_, params, out| {
                log.borrow_mut().push(String::from("outer"));
                out.push(Command::RunOriginal(WRAPPED, params.clone()));
                out.push(Command::RunAction(WRAPPED));
                out.push(Command::RunAction(PLAIN));
            },
            Priority::DEFAULT,
        )?;
        // In place of another plugin's action, which its screen takes.
        let log = self.0.clone();
        r.override_action(
            Id::new_static("a.special"),
            move |_, _, _| log.borrow_mut().push(String::from("obs special")),
            Priority::DEFAULT,
        )?;
        let log = self.0.clone();
        r.observe_actions(ObserverSpec::new(
            Id::new_static("obs.actions"),
            move |_, id, _, _| log.borrow_mut().push(alloc::format!("saw {id}")),
        ))?;
        let log = self.0.clone();
        r.observe_events(ObserverSpec::new(
            Id::new_static("obs.events"),
            move |_, id, params, _| {
                if *id == events::SETTING_CHANGED {
                    let old = params.get("old").unwrap_or("?");
                    log.borrow_mut().push(alloc::format!("changed from {old}"));
                }
            },
        ))
    }
}

fn observed() -> (Kernel, Log) {
    let (kernel, log, _) = observed_with_screen();
    (kernel, log)
}

/// A kernel with plugin `a`'s screen and its log, and the observing
/// plugin's log.
fn observed_with_screen() -> (Kernel, Log, Log) {
    let (log, screen) = (log(), log());
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &screen))
        .plugin(Observing(log.clone()))
        .plugin(Guarding(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    screen.borrow_mut().clear();
    (kernel, log, screen)
}

/// A lock that takes no action itself, a handler of its own plugin in
/// place of `obs.plain`'s, and an observer of actions.
struct Guarding(Log);

struct Lock;

impl Screen for Lock {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn guards(&self) -> bool {
        true
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Guarding {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("g"), "Guarding", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            LOCK,
            "lock",
            Box::new(|_, _| Box::new(Lock)),
        ))?;
        let log = self.0.clone();
        r.override_action(
            Id::new_static("obs.guarded"),
            move |_, _, _| log.borrow_mut().push(String::from("g guarded")),
            Priority::DEFAULT,
        )?;
        let log = self.0.clone();
        r.observe_actions(ObserverSpec::new(
            Id::new_static("g.actions"),
            move |_, id, _, _| log.borrow_mut().push(alloc::format!("g saw {id}")),
        ))
    }
}

/// While a screen guards the device, handlers another plugin put in
/// place of an action's own do not run (the action's own does), the
/// guard's plugin's do; only its observers see the actions.
#[test]
fn a_guard_keeps_others_overrides_and_observers_out() {
    let (mut kernel, log, _) = observed_with_screen();
    kernel.open_screen(Instant(10), LOCK, Params::new());
    log.borrow_mut().clear();
    kernel.apply(Command::RunAction(Id::new_static("obs.overridden")));
    kernel.apply(Command::RunAction(WRAPPED));
    kernel.apply(Command::RunAction(Id::new_static("obs.guarded")));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "g saw obs.overridden",
            "g saw obs.wrapped",
            "inner",
            "g saw obs.guarded",
            "g guarded"
        ]
    );
}

/// A screen that takes an action comes before a handler in its place;
/// one run with params reaches the screen too.
#[test]
fn a_screen_comes_before_an_override() {
    let (mut kernel, log, screen) = observed_with_screen();
    let special = Id::new_static("a.special");
    kernel.apply(Command::RunActionWith(
        special.clone(),
        Params::new().with("n", "1"),
    ));
    kernel.finish();
    assert_eq!(*screen.borrow(), ["a:a.special"]);
    assert!(
        !log.borrow().contains(&String::from("obs special")),
        "{:?}",
        log.borrow()
    );
}

/// A handler in place of an action's own runs the original; what runs
/// the action itself again is dropped, with a warning, and nothing else.
#[test]
fn an_override_runs_the_original_and_never_itself() {
    let (mut kernel, log) = observed();
    kernel.apply(Command::RunAction(WRAPPED));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "saw obs.wrapped",
            "g saw obs.wrapped",
            "outer",
            "inner",
            "saw obs.plain",
            "g saw obs.plain",
            "plain"
        ]
    );
    let warnings = kernel.take_warnings();
    assert!(
        warnings.iter().any(|w| w.message.contains("runs itself")),
        "{warnings:?}"
    );
}

/// A handler made for params gets those it is run with, none when run
/// plainly; observers see each action first.
#[test]
fn actions_run_with_params() {
    let (mut kernel, log) = observed();
    let params = Params::new().with("profile", "night");
    kernel.apply(Command::RunActionWith(APPLY, params));
    kernel.apply(Command::RunAction(APPLY));
    kernel.apply(Command::RunAction(PLAIN));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        [
            "saw obs.apply",
            "g saw obs.apply",
            "apply night",
            "saw obs.apply",
            "g saw obs.apply",
            "apply none",
            "saw obs.plain",
            "g saw obs.plain",
            "plain"
        ]
    );
}

/// Of the handlers put in place of an action's own, the one of the
/// highest priority runs.
#[test]
fn the_override_of_the_highest_priority_runs() {
    let (mut kernel, log) = observed();
    kernel.apply(Command::RunAction(Id::new_static("obs.overridden")));
    kernel.finish();
    assert_eq!(
        *log.borrow(),
        ["saw obs.overridden", "g saw obs.overridden", "high"]
    );
}

/// What runs by itself is told from screen actions; descriptions and
/// keywords are kept.
#[test]
fn runnable_actions_are_told_from_screen_actions() {
    let (kernel, _) = observed();
    let actions = &kernel.shared.actions;
    assert!(actions.runnable(PLAIN.as_str()));
    assert!(actions.runnable(APPLY.as_str()));
    assert!(actions.runnable("obs.overridden"), "by its override");
    assert!(!actions.runnable(SCREEN_ONLY.as_str()));
    assert!(!actions.runnable("obs.nothing"));
    let plain = actions.get(PLAIN.as_str()).unwrap();
    assert_eq!(plain.description(), Some("obs.plain.help"));
    assert_eq!(plain.search_keywords(), Some("obs.plain.words"));
}

/// An observer of events hears a setting change with the value it had.
#[test]
fn a_setting_changed_says_what_it_was() {
    let (mut kernel, log) = observed();
    let level = Id::new_static("a.level");
    kernel.apply(Command::SetSetting(level.clone(), SettingValue::Int(4)));
    kernel.finish();
    kernel.apply(Command::ResetSetting(level));
    kernel.finish();
    assert_eq!(*log.borrow(), ["changed from 2", "changed from 4"]);
}
