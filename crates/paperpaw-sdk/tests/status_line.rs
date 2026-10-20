//! Status lines: the items listed in a setting, read however the list is
//! written; a line drawn inside its area whatever its items say, short
//! items (the page, the time) kept whole beside a long chapter title;
//! status items and whole lines found by their ids.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::borrow::Cow;

use paperpaw_sdk::status::{StatusLine, item_ids};
use paperpaw_sdk::{
    Color, Env, Id, Params, Rect, StatusFrame, StatusItemSpec, StatusLineSpec, StatusValue,
};
use proptest::prelude::*;

use support::{Device, Harness, drawn_outside, frame};

const PAGE: &str = "t.page";
const CHAPTER: &str = "t.chapter";
const LEVEL: &str = "t.battery";
const ICON: &str = "t.speaking";
const NOTHING: &str = "t.nothing";

/// Items whose values come from the context: the chapter's title, the
/// page, a battery at a level, an icon, and one that shows nothing.
fn harness() -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    let from = |key: &'static str| -> paperpaw_sdk::StatusFn {
        Box::new(move |_: &Env<'_>, context: &Params| {
            context.get(key).map(|v| StatusValue::Text(v.to_string()))
        })
    };
    h.status_items.add(StatusItemSpec {
        id: Id::new_static(PAGE),
        title: Cow::Borrowed("t.page.title"),
        value: from("page"),
    });
    h.status_items.add(StatusItemSpec {
        id: Id::new_static(CHAPTER),
        title: Cow::Borrowed("t.chapter.title"),
        value: from("chapter"),
    });
    h.status_items.add(StatusItemSpec {
        id: Id::new_static(LEVEL),
        title: Cow::Borrowed("t.battery.title"),
        value: Box::new(|_, context| {
            let percent = context.get("level")?.parse().ok()?;
            Some(StatusValue::Level {
                percent,
                charging: context.get("charging").is_some(),
            })
        }),
    });
    h.status_items.add(StatusItemSpec {
        id: Id::new_static(ICON),
        title: Cow::Borrowed("t.speaking.title"),
        value: Box::new(|_, _| Some(StatusValue::Icon(Cow::Borrowed("volume")))),
    });
    h.status_items.add(StatusItemSpec {
        id: Id::new_static(NOTHING),
        title: Cow::Borrowed("t.nothing.title"),
        value: Box::new(|_, _| None),
    });
    h
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// However a list is written (spaces, empty parts), its ids come out
    /// trimmed, none empty, in their order.
    #[test]
    fn listed_ids_come_out_trimmed_and_never_empty(list in "[a-z., \t]{0,60}") {
        let ids: Vec<&str> = item_ids(&list).collect();
        for id in &ids {
            prop_assert!(!id.is_empty());
            prop_assert_eq!(id.trim(), *id);
            prop_assert!(!id.contains(','));
        }
        let expected: Vec<&str> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        prop_assert_eq!(ids, expected);
    }
}

fn list_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        proptest::sample::select(vec![PAGE, CHAPTER, LEVEL, ICON, NOTHING, "t.unknown", ""]),
        0..4,
    )
    .prop_map(|ids| ids.join(", "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

    /// A status line draws inside its area, whatever its items show, at
    /// any size the user sets for status lines.
    #[test]
    fn a_status_line_draws_inside_its_area(
        lists in (list_strategy(), list_strategy(), list_strategy()),
        chapter in "\\PC{0,80}",
        page in "[0-9 /%]{0,12}",
        level in 0u16..300,
        charging in any::<bool>(),
        size in 10.0f32..28.0,
        width in 300u16..470,
        percent in any::<bool>(),
        top in any::<bool>(),
    ) {
        let h = harness();
        let env = h.env();
        let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
        let height = (size * 1.6) as u16;
        let area = Rect::new(475 - width, if top { 0 } else { 800 - height }, width, height);
        let line = StatusLine {
            area,
            lists: [&lists.0, &lists.1, &lists.2],
            size,
            color: Color::BLACK,
            percent,
        };
        let mut context = Params::new()
            .with("chapter", chapter)
            .with("page", page)
            .with("level", level.to_string());
        if charging {
            context = context.with("charging", "yes");
        }
        let mut frame = frame(480, 800);
        line.draw(&mut frame.canvas(), &env, &font, &context);
        // A pixel or two to the side for letters reaching past their
        // start (accents of Greek capitals do).
        let bound = Rect::new(area.x - 2, area.y, area.width + 4, area.height);
        prop_assert_eq!(drawn_outside(&mut frame, bound), None);
    }
}

/// A long chapter title on the left gives way: the page on the right is
/// drawn as it is without it.
#[test]
fn the_page_stays_whole_beside_a_long_chapter_title() {
    let h = harness();
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let area = Rect::new(0, 760, 480, 40);
    let context = Params::new()
        .with(
            "chapter",
            "Chapter Twelve, in which a very long title goes on and on and on",
        )
        .with("page", "123 / 456");
    let draw = |left: &str| {
        let mut frame = frame(480, 800);
        let line = StatusLine {
            area,
            lists: [left, "", PAGE],
            size: 22.0,
            color: Color::BLACK,
            percent: true,
        };
        line.draw(&mut frame.canvas(), &env, &font, &context);
        frame
    };
    let alone = draw("");
    let beside = draw(CHAPTER);
    // The right half: the page, the same pixels either way.
    let (a, b) = (alone.view(), beside.view());
    let width = font.measure(22.0, "123 / 456").ceil() as u16;
    for y in area.y..area.bottom() as u16 {
        for x in (480 - width)..480 {
            assert_eq!(a.pixel(x, y), b.pixel(x, y), "({x}, {y})");
        }
    }
    assert!(
        support::drawn_in(&beside, Rect::new(0, 760, 100, 40)),
        "the chapter shows"
    );
}

#[test]
fn an_empty_line_and_items_that_show_nothing_draw_nothing() {
    let h = harness();
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let area = Rect::new(0, 0, 480, 40);
    let empty = StatusLine {
        area,
        lists: [" , ", "", ","],
        size: 20.0,
        color: Color::BLACK,
        percent: false,
    };
    assert!(empty.is_empty());
    let nothing = StatusLine {
        lists: [NOTHING, "t.unknown", NOTHING],
        ..empty
    };
    assert!(!nothing.is_empty());
    for line in [empty, nothing] {
        let mut frame = frame(480, 100);
        line.draw(&mut frame.canvas(), &env, &font, &Params::new());
        assert_eq!(frame.take_dirty(), None);
    }
}

#[test]
fn items_and_lines_are_found_by_their_ids() {
    let mut h = harness();
    fn draw(canvas: &mut paperpaw_sdk::Canvas<'_>, _: &Env<'_>, frame: &StatusFrame<'_>) {
        canvas.fill(frame.area, Color::BLACK);
    }
    h.status_items.add_line(StatusLineSpec {
        id: Id::new_static("t.bar"),
        title: Cow::Borrowed("t.bar.title"),
        draw: Box::new(draw),
        signature: None,
    });
    h.status_items.add_line(StatusLineSpec {
        id: Id::new_static("t.clock"),
        title: Cow::Borrowed("t.clock.title"),
        draw: Box::new(draw),
        signature: Some(Box::new(|_, _| "10:41".to_string())),
    });
    let items = &h.status_items;
    let env = h.env();
    assert_eq!(items.title(PAGE), Some("t.page.title"));
    assert_eq!(items.title("t.bar"), Some("t.bar.title"), "lines too");
    assert_eq!(items.title("t.unknown"), None);
    let ids: Vec<&str> = items.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, [PAGE, CHAPTER, LEVEL, ICON, NOTHING]);
    let lines: Vec<&str> = items.lines().map(|(id, _)| id.as_str()).collect();
    assert_eq!(lines, ["t.bar", "t.clock"]);
    let context = Params::new().with("page", "3");
    assert_eq!(
        items.value(PAGE, &env, &context),
        Some(StatusValue::Text("3".into()))
    );
    assert_eq!(items.value("t.unknown", &env, &context), None);
    assert_eq!(items.line_signature("t.clock", &env, &context), "10:41");
    // Without a signature: drawn again when the context changes.
    let other = Params::new().with("page", "4");
    assert_ne!(
        items.line_signature("t.bar", &env, &context),
        items.line_signature("t.bar", &env, &other)
    );
    let status = StatusFrame {
        area: Rect::new(0, 0, 100, 20),
        top: true,
        size: 20.0,
        color: Color::BLACK,
        context: &context,
    };
    let mut frame = frame(100, 40);
    assert!(items.draw_line("t.bar", &mut frame.canvas(), &env, &status));
    assert!(!items.draw_line("t.unknown", &mut frame.canvas(), &env, &status));
    assert_eq!(drawn_outside(&mut frame, status.area), None);
}
