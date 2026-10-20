//! Searching the settings with words made up at random: whatever is
//! typed, the search opens (or, typed blank, does not) without a panic,
//! upper and lower case find the same, and each hit opens a page.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use proptest::prelude::*;

/// What a search for `query` lists, by the titles of its hits.
fn hits(app: &mut App, query: &str) -> Vec<String> {
    app.home();
    app.run(actions::SETTINGS);
    app.will_type(query);
    // The search is the first row of the main page.
    app.run(actions::CONFIRM);
    if app.screen() != "settings.search" {
        return Vec::new();
    }
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Failures become named tests, not files.
        failure_persistence: None,
        ..ProptestConfig::with_cases(64)
    })]

    #[test]
    fn any_words_search_without_a_panic_and_case_does_not_matter(
        query in "[a-zA-ZäöüÄÖÜéß 0-9.,%-]{0,24}",
        german in any::<bool>(),
    ) {
        let config = if german { "[core]\nlanguage = de\n" } else { "" };
        let mut app = App::new(X4, Shared::memory(config, &[]));
        let lower = hits(&mut app, &query.to_lowercase());
        let upper = hits(&mut app, &query.to_uppercase());
        prop_assert_eq!(&lower, &upper);
        if query.trim().is_empty() {
            prop_assert_eq!(app.screen(), "settings.list");
        } else {
            prop_assert_eq!(app.screen(), "settings.search");
            if !upper.is_empty() {
                // The first hit opens its page (or another plugin's).
                app.run(actions::CONFIRM);
                prop_assert_ne!(app.screen(), "settings.search");
            }
        }
    }
}
