//! Actions (SDK gap E2): run with params, handlers put in place of
//! others', what runs by itself, observers of actions and events, the
//! value a setting had.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{ObserverSpec, Params};

const APPLY: Id = Id::new_static("obs.apply");
const PLAIN: Id = Id::new_static("obs.plain");
const SCREEN_ONLY: Id = Id::new_static("obs.screen-only");

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
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &super::log()))
        .plugin(Observing(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    (kernel, log)
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
            "apply night",
            "saw obs.apply",
            "apply none",
            "saw obs.plain",
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
    assert_eq!(*log.borrow(), ["saw obs.overridden", "high"]);
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
    assert_eq!(plain.description.as_deref(), Some("obs.plain.help"));
    assert_eq!(plain.keywords.as_deref(), Some("obs.plain.words"));
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
