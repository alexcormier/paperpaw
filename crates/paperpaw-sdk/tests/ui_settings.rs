//! Settings as rows: the value a row shows (with its unit, secrets as
//! dots, values for this book marked), the value one step on (numbers
//! within their range, choices round in both directions, whatever the
//! value held), which settings switch at once and which open a screen,
//! the room numbers above each other get, and notes that speak of
//! buttons where there is no touch screen.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::borrow::Cow;

use paperpaw_i18n::Catalog;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Choices, MenuEntry, SettingSpec, SettingValue, Text};
use proptest::prelude::*;

use support::{Device, Harness};

const SIZE: Setting<i64> = Setting::new("t.size");
const ON: Setting<bool> = Setting::new("t.on");
const LOOK: Setting<Text> = Setting::new("t.look");
const PAIR: Setting<Text> = Setting::new("t.pair");
const KEY: Setting<Text> = Setting::new("t.key");
const NAME: Setting<Text> = Setting::new("t.name");

const LOOKS: [&str; 3] = ["t.look.plain", "t.look.bold", "t.look.thin"];
const PAIRS: [&str; 2] = ["t.pair.a", "t.pair.b"];

const EN: &str = "[meta]
language = en
name = English
plural = one-other

[messages]
t.unit.px.one = {count} pixel
t.unit.px.other = {count} pixels
t.look.bold = Bold
t.tap = Tap a rule to pause it
t.tap.buttons = Select a rule to pause it
";

fn harness(device: Device) -> (Harness, [SettingSpec; 6]) {
    let mut h = Harness::new(device);
    h.i18n.add(Catalog::parse(EN).unwrap());
    h.i18n.set_language("en");
    let specs = [
        SettingSpec::number(&SIZE, "t.size", -5, 40, 12)
            .step(5)
            .unit("t.unit.px"),
        SettingSpec::switch(&ON, "t.on", true),
        SettingSpec::choice(&LOOK, "t.look", Choices::Fixed(&LOOKS), "t.look.plain"),
        SettingSpec::choice(&PAIR, "t.pair", Choices::Fixed(&PAIRS), "t.pair.a"),
        SettingSpec::text(&KEY, "t.key", "").secret(),
        SettingSpec::text(&NAME, "t.name", "Ann"),
    ];
    for spec in &specs {
        h.settings.define(spec.clone()).unwrap();
    }
    (h, specs)
}

#[test]
fn rows_show_values_with_units_and_secrets_as_dots() {
    let (mut h, [size, on, look, _, key, name]) = harness(Device::TOUCH);
    {
        let env = h.env();
        assert_eq!(ui::setting_value(&env, &size), "12 pixels");
        assert_eq!(ui::setting_value(&env, &on), "On");
        assert_eq!(
            ui::setting_value(&env, &look),
            "t.look.plain",
            "no translation: the key"
        );
        assert_eq!(
            ui::setting_value(&env, &key),
            "",
            "an empty secret shows nothing"
        );
        assert_eq!(ui::setting_value(&env, &name), "Ann");
    }
    h.settings.set("t.size", SettingValue::Int(1)).unwrap();
    h.settings
        .set("t.look", SettingValue::Text(Cow::Borrowed("t.look.bold")))
        .unwrap();
    // Kept with the secrets, as the kernel keeps a secret setting.
    h.secrets.apply(paperpaw_sdk::secrets::SecretChange::keep(
        "t.key", "Key", "s3cr3t",
    ));
    h.settings.set("t.on", SettingValue::Bool(false)).unwrap();
    let env = h.env();
    assert_eq!(ui::setting_value(&env, &size), "1 pixel");
    assert_eq!(ui::setting_value(&env, &look), "Bold");
    let shown = ui::setting_value(&env, &key);
    assert!(!shown.contains("s3cr3t"), "a secret never shows");
    assert!(shown.chars().all(|c| c == '•') && !shown.is_empty());
    assert_eq!(ui::setting_value(&env, &on), "Off");
    // A setting nobody defined shows nothing.
    let unknown = SettingSpec::text(&Setting::new("t.unknown"), "t.unknown", "x");
    assert_eq!(ui::setting_value(&env, &unknown), "");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// One step of a number moves it by its step and never out of its
    /// range, from any value it holds.
    #[test]
    fn number_steps_keep_to_the_range(start in -5i64..=40, steps in proptest::collection::vec(-3i64..=3, 1..20)) {
        let (mut h, [size, ..]) = harness(Device::TOUCH);
        h.settings.set("t.size", SettingValue::Int(start)).unwrap();
        for step in steps {
            let before = h.settings.int("t.size");
            let next = ui::next_value(&h.env(), &size, step).unwrap();
            let SettingValue::Int(next) = next else {
                return Err(TestCaseError::fail("not a number"));
            };
            prop_assert_eq!(next, (before + step * 5).clamp(-5, 40));
            h.settings.set("t.size", SettingValue::Int(next)).unwrap();
        }
    }

    /// A choice goes round its options both ways: as many steps on as
    /// there are options come back to where they began, a step on and a
    /// step back too.
    #[test]
    fn choices_go_round_both_ways(start in 0usize..3, step in -7i64..7) {
        let (mut h, [_, _, look, ..]) = harness(Device::TOUCH);
        h.settings
            .set("t.look", SettingValue::Text(Cow::Borrowed(LOOKS[start])))
            .unwrap();
        let next = ui::next_value(&h.env(), &look, step).unwrap();
        let expected = LOOKS[(start as i64 + step).rem_euclid(3) as usize];
        prop_assert_eq!(&next, &SettingValue::Text(Cow::Borrowed(expected)));
        h.settings.set("t.look", next).unwrap();
        let back = ui::next_value(&h.env(), &look, -step).unwrap();
        prop_assert_eq!(back, SettingValue::Text(Cow::Borrowed(LOOKS[start])));
    }
}

#[test]
fn switches_toggle_and_text_has_no_step() {
    let (h, [_, on, _, _, key, name]) = harness(Device::TOUCH);
    let env = h.env();
    assert_eq!(
        ui::next_value(&env, &on, 1),
        Some(SettingValue::Bool(false))
    );
    assert_eq!(
        ui::next_value(&env, &on, -1),
        Some(SettingValue::Bool(false))
    );
    assert_eq!(ui::next_value(&env, &key, 1), None);
    assert_eq!(ui::next_value(&env, &name, 1), None);
    let unknown = SettingSpec::switch(&Setting::new("t.unknown"), "t.unknown", true);
    assert_eq!(ui::next_value(&env, &unknown, 1), None);
}

/// A choice holding a value it does not offer (a theme or a plugin that
/// went away set it): one step goes to an offered one.
#[test]
fn a_choice_holding_a_value_it_does_not_offer_steps_to_one_it_does() {
    let (h, [_, _, look, ..]) = harness(Device::TOUCH);
    // Holding "t.look.plain", now offering others.
    let mut spec = look.clone();
    spec.kind = paperpaw_sdk::SettingKind::Choice(Choices::Fixed(&PAIRS));
    for step in [1, -1, 2] {
        let next = ui::next_value(&h.env(), &spec, step).unwrap();
        let SettingValue::Text(next) = next else {
            panic!("not text");
        };
        assert!(PAIRS.contains(&next.as_ref()), "{next}");
    }
}

/// From a value not among the options, the first step on is the first
/// option and the first step back the last (not the second and the
/// first).
#[test]
fn from_a_value_not_offered_the_steps_begin_at_the_ends() {
    let (h, [_, _, look, ..]) = harness(Device::TOUCH);
    // Holding "t.look.plain", now offering the pair alone.
    let mut spec = look.clone();
    spec.kind = paperpaw_sdk::SettingKind::Choice(Choices::Fixed(&PAIRS));
    let text = |v: Option<SettingValue>| match v {
        Some(SettingValue::Text(t)) => t.to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(text(ui::next_value(&h.env(), &spec, 1)), PAIRS[0]);
    assert_eq!(text(ui::next_value(&h.env(), &spec, -1)), PAIRS[1]);
}

/// A number at the end of the widest range, stepped by a large step:
/// no overflow, and within the range.
#[test]
fn numbers_at_the_ends_of_their_range_do_not_overflow() {
    let mut h = Harness::new(Device::TOUCH);
    let wide = Setting::<i64>::new("t.wide");
    let spec = SettingSpec::number(&wide, "t.wide", i64::MIN, i64::MAX, i64::MAX).step(i64::MAX);
    h.settings.define(spec.clone()).unwrap();
    assert_eq!(
        ui::next_value(&h.env(), &spec, 3),
        Some(SettingValue::Int(i64::MAX))
    );
    h.settings
        .set("t.wide", SettingValue::Int(i64::MIN))
        .unwrap();
    assert_eq!(
        ui::next_value(&h.env(), &spec, -3),
        Some(SettingValue::Int(i64::MIN))
    );
}

#[test]
fn two_options_switch_at_once_and_more_open_a_list() {
    let (mut h, [size, on, look, pair, _, name]) = harness(Device::TOUCH);
    {
        let env = h.env();
        assert!(ui::two_options(&env, &pair));
        assert!(!ui::opens_screen(&env, &pair), "switched at once");
        assert!(!ui::two_options(&env, &look));
        assert!(ui::opens_screen(&env, &look), "a list to choose from");
        for spec in [&size, &on, &name] {
            assert!(!ui::two_options(&env, spec));
            assert!(!ui::opens_screen(&env, spec));
        }
        let edited = name
            .clone()
            .editor(paperpaw_sdk::Id::new_static("t.editor"));
        assert!(ui::opens_screen(&env, &edited));
    }
    // More ways to choose listed with the options: a list after all.
    h.menus.add(MenuEntry::new(
        PAIR.id().clone(),
        paperpaw_sdk::Id::new_static("t.pair-more"),
    ));
    h.menus.set_title(
        paperpaw_sdk::Id::new_static("t.pair-more"),
        Cow::Borrowed("t.more"),
    );
    let env = h.env();
    assert!(!ui::two_options(&env, &pair));
    assert!(ui::opens_screen(&env, &pair));
}

#[test]
fn numbers_above_each_other_get_the_room_of_the_widest() {
    let (h, [size, on, ..]) = harness(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let narrow = SettingSpec::number(&Setting::new("t.narrow"), "t.narrow", 1, 9, 3);
    let room = ui::number_room(&env, &style, [&narrow, &size, &on]);
    // "-5 pixels" and "40 pixels": the wider of the two.
    let font = style.font(&env).unwrap();
    let widest = ["-5 pixels", "40 pixels"]
        .into_iter()
        .max_by(|a, b| {
            font.measure(style.text, a)
                .total_cmp(&font.measure(style.text, b))
        })
        .unwrap();
    assert_eq!(room, widest);
    assert_eq!(ui::number_room(&env, &style, [&on]), "");
    assert_eq!(ui::number_room(&env, &style, []), "");
}

#[test]
fn notes_speak_of_buttons_where_there_is_no_touch_screen() {
    let (touch, _) = harness(Device::TOUCH);
    let (buttons, _) = harness(Device::BUTTONS);
    assert_eq!(
        ui::tr_input(&touch.env(), "t.tap"),
        "Tap a rule to pause it"
    );
    assert_eq!(
        ui::tr_input(&buttons.env(), "t.tap"),
        "Select a rule to pause it"
    );
    // Without a version for buttons: the note itself.
    assert_eq!(ui::tr_input(&buttons.env(), "t.look.bold"), "Bold");
}
