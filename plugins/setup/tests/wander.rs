//! The first-run setup under random presses and taps: whatever is
//! pressed (and wherever tapped), nothing panics, every screen the guide
//! opens leads back to it, and from wherever it is the page buttons
//! finish it at the home screen with the setup noted as done.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use proptest::prelude::*;

/// What is done: an action as a button would, or a tap.
#[derive(Debug, Clone)]
enum Input {
    Action(Id),
    Tap(i32, i32),
}

fn input() -> impl Strategy<Value = Input> {
    prop_oneof![
        prop::sample::select(vec![
            actions::UP,
            actions::DOWN,
            actions::LEFT,
            actions::RIGHT,
            actions::CONFIRM,
            actions::NEXT_PAGE,
            actions::PREV_PAGE,
        ])
        .prop_map(Input::Action),
        (0i32..480, 0i32..800).prop_map(|(x, y)| Input::Tap(x, y)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48))
    })]

    #[test]
    fn random_presses_never_break_the_guide(
        inputs in prop::collection::vec(input(), 0..40),
        touch in any::<bool>(),
    ) {
        let device = if touch { TOUCH } else { X4 };
        let mut app = App::new(device, Shared::bare("", &[]));
        prop_assert_eq!(app.screen(), "setup.guide");
        for input in inputs {
            if app.screen() != "setup.guide" {
                break;
            }
            match input {
                Input::Action(action) => app.run(action),
                Input::Tap(x, y) if touch => app.tap(Point::new(x, y)),
                Input::Tap(..) => {}
            }
            // Lists and screens it opened lead back to it.
            for _ in 0..4 {
                let screen = app.screen();
                if screen == "setup.guide" || screen == "home.dashboard" {
                    break;
                }
                app.run(actions::BACK);
            }
        }
        // Finished with the page buttons, wherever it is.
        for _ in 0..12 {
            if app.screen() != "setup.guide" {
                break;
            }
            app.run(actions::NEXT_PAGE);
        }
        prop_assert_eq!(app.screen(), "home.dashboard");
        prop_assert!(app.storage.exists(SETUP_DONE));
        // Whatever was chosen is read back the same.
        let language = app.kernel.settings().text("core.language").to_string();
        let size = app.kernel.settings().int("core.font-size");
        let app = app.restart();
        prop_assert_eq!(app.screen(), "home.dashboard");
        prop_assert_eq!(app.kernel.settings().text("core.language"), language.as_str());
        prop_assert_eq!(app.kernel.settings().int("core.font-size"), size);
    }
}
