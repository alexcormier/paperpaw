//! The screen an update on trial shows until the device starts again to
//! load its code plugins: nothing closes it, nothing shows above it, it
//! takes no input, and its bar fills in steps until the trial ends.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{BASE, BUTTONS, Harness, pointer, press, render};
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::{actions, screens};
use paperpaw_core::sdk::{Command, Params};
use paperpaw_platform::{Instant, PointerPhase, PointerTool};

/// A kernel showing the screen for a trial from 1 to 31 seconds; when it
/// wakes next.
fn opened(harness: &Harness) -> (Kernel, Option<Instant>) {
    let mut kernel = harness.kernel();
    let params = Params::new().with("from", "1000").with("until", "31000");
    let effects = kernel.open_screen(Instant(1000), screens::FINISHING_UPDATE, params);
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&screens::FINISHING_UPDATE));
    (kernel, effects.wake_at())
}

fn finishing(harness: &Harness) -> Kernel {
    opened(harness).0
}

#[test]
fn nothing_closes_it_or_shows_above_it() {
    let harness = Harness::new();
    let mut kernel = finishing(&harness);
    kernel.run_action(Instant(2000), actions::BACK);
    kernel.run_action(Instant(2100), actions::HOME);
    kernel.open_screen(Instant(2200), BASE, Params::new());
    harness.run(&mut kernel, 2300, [Command::Back, Command::Back]);
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&screens::FINISHING_UPDATE));
}

#[test]
fn it_takes_no_input() {
    let harness = Harness::new();
    let mut kernel = finishing(&harness);
    for (i, name) in BUTTONS.iter().enumerate() {
        press(&mut kernel, 3000 + 100 * i as u64, name);
    }
    for (i, phase) in [PointerPhase::Down, PointerPhase::Up]
        .into_iter()
        .enumerate()
    {
        kernel.handle(
            Instant(5000 + i as u64),
            pointer(0, phase, 60, 80, PointerTool::Finger),
        );
    }
    // A plugin's action does not run either.
    harness.run(&mut kernel, 6000, [Command::Back]);
    render(&mut kernel);
    assert_eq!(kernel.active_screen(), Some(&screens::FINISHING_UPDATE));
}

#[test]
fn its_bar_fills_in_steps_until_the_trial_ends() {
    let harness = Harness::new();
    let (mut kernel, wake) = opened(&harness);
    assert_eq!(wake, Some(Instant(4000)));
    let effects = kernel.tick(Instant(4000));
    assert!(effects.refresh().is_some(), "the bar is drawn anew");
    assert_eq!(effects.wake_at(), Some(Instant(7000)));
    let effects = kernel.tick(Instant(31_000));
    assert_eq!(effects.wake_at(), None, "full at the end: no more wakes");
}
