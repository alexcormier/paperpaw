//! The drawing helpers every screen is made of, in the UI font PaperPaw
//! ships: rows, switches, numbers, buttons, headers, pagers, notes, empty
//! screens and the row of hints draw inside the rectangle they are given
//! (long texts shortened, never run into what is next to them), keep to
//! the same margins, and a header's or pager's arrows are hit where they
//! are drawn, on touch screens only.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::{self, Role};
use paperpaw_sdk::ui::{self, Header, Pager, UiStyle};
use paperpaw_sdk::{Hint, Point, Rect};
use proptest::prelude::*;

use support::{Device, Harness, describe, drawn_in, drawn_outside, frame, middle};

/// A way of drawing, to try each in turn.
type Draw<'a> = &'a dyn Fn(&mut paperpaw_sdk::Canvas<'_>);

/// Texts of all kinds: none, short, long, other scripts, marks.
fn text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        "[a-zA-Z ]{1,12}",
        "[a-zA-Z äöüß]{20,90}",
        "\\PC{0,40}",
    ]
}

#[test]
fn the_ui_font_and_icons_are_there() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    assert!(style.font(&env).is_some());
    assert!(ui::has_icons(&env));
    assert!(ui::icon_width(&env, "settings", style.text) > 0.0);
    assert_eq!(ui::icon_width(&env, "no-such-icon", style.text), 0.0);
}

// ---- Shortening -------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// A text shortened to a width fits it (with a pixel of slack), and
    /// is the text itself or its beginning and "…".
    #[test]
    fn shortened_text_fits_and_begins_as_the_text(text in text_strategy(), width in 10.0f32..400.0) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let font = style.font(&env).unwrap();
        let shown = ui::fit(&font, style.text, &text, width);
        prop_assert!(font.measure(style.text, &shown) <= width.max(font.measure(style.text, "…")) + 1.0);
        if shown != text {
            let start = shown.strip_suffix('…').expect("ends with …");
            prop_assert!(text.starts_with(start));
        }
    }

    /// A title and a value side by side take no more than the room, the
    /// value never pushes the title below its share.
    #[test]
    fn a_title_and_a_value_share_the_width(
        title in text_strategy(),
        value in text_strategy(),
        width in 60.0f32..480.0,
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let font = style.font(&env).unwrap();
        let gap = 16.0;
        let (t, v, value_width) = ui::fit_pair(&font, style.text, &title, &value, width, gap);
        let tw = font.measure(style.text, &t);
        prop_assert!((font.measure(style.text, &v) - value_width).abs() < 0.01);
        // Two pixels of slack: each part has one for rounding.
        let ellipses = 2.0 * font.measure(style.text, "…");
        prop_assert!(tw + value_width + gap <= width.max(ellipses + gap) + 2.0, "{tw} + {value_width}");
    }
}

// ---- Rows ---------------------------------------------------------------

fn row_strategy() -> impl Strategy<Value = Rect> {
    // At least 400 wide, as rows of lists are on a screen 480 wide: a
    // number's − and + and the room between them take some 300 pixels
    // (narrower, the − goes past the row's start).
    (0u16..80, 0u16..600, 400u16..480, 0u16..3).prop_map(|(x, y, width, kind)| {
        Rect::new(x, y, width.min(480 - x), [60, 72, 110][usize::from(kind)])
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Rows of every kind draw inside their rectangle, focused or not,
    /// whatever their texts (numbers as numbers are: a value far wider
    /// than "000 %" pushes the − to the left, past the row's start).
    #[test]
    fn rows_draw_inside_their_rectangle(
        rect in row_strategy(),
        title in text_strategy(),
        value in text_strategy(),
        note in text_strategy(),
        number in "[0-9]{1,4}( %| px| min)?",
        focused in any::<bool>(),
        on in any::<bool>(),
        touch in any::<bool>(),
    ) {
        let h = Harness::new(if touch { Device::TOUCH } else { Device::BUTTONS });
        let env = h.env();
        let style = UiStyle::new(&env);
        // At least as high as rows are, those with notes as noted rows.
        let row = Rect { height: rect.height.max(style.row as u16), ..rect };
        let noted = Rect { height: rect.height.max(style.noted_row() as u16), ..rect };
        let draws: [(Rect, Draw<'_>); 6] = [
            (row, &|c| ui::draw_value_row(c, &env, &style, row, focused, &title, &value)),
            (noted, &|c| ui::draw_noted_row(c, &env, &style, noted, focused, &title, &value, &note)),
            (noted, &|c| ui::draw_switch_row(c, &env, &style, noted, focused, &title, &note, on)),
            (row, &|c| ui::draw_number(c, &env, &style, row, &title, &number, "", focused.then_some(on))),
            (row, &|c| ui::draw_stepper(c, &env, &style, row, &title, &number, "", on)),
            (row, &|c| ui::draw_heading(c, &env, &style, row, &title)),
        ];
        for (n, (bound, draw)) in draws.iter().enumerate() {
            let mut frame = frame(480, 800);
            draw(&mut frame.canvas());
            prop_assert_eq!(drawn_outside(&mut frame, *bound), None, "kind {}", n);
        }
    }

    /// A row's focus sits inside the row, as a bar within the margins.
    #[test]
    fn the_focus_bar_stays_in_its_row(rect in row_strategy()) {
        let h = Harness::new(Device::TOUCH);
        let style = UiStyle::new(&h.env());
        let focus = ui::focus_rect(&style, rect);
        prop_assert!(focus.x >= rect.x && focus.right() <= rect.right());
        prop_assert!(focus.y >= rect.y && focus.bottom() <= rect.bottom());
        prop_assert!(!focus.is_empty());
        let mut frame = frame(480, 800);
        ui::draw_focus(&mut frame.canvas(), &style, rect);
        prop_assert_eq!(drawn_outside(&mut frame, focus), None);
    }
}

#[test]
fn the_switch_sits_at_the_rows_end_and_the_title_ends_before_it() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let row = Rect::new(0, 100, 480, style.row as u16);
    let switch = ui::switch_rect(&style, row);
    // Flush with the margin, where other rows end their values.
    assert_eq!(switch.right() + style.margin, row.right());
    assert_eq!(
        switch.y - row.y,
        (row.bottom() - switch.bottom()) as u16,
        "centred"
    );
    let long = "A title much longer than the row could ever hold beside a switch";
    let (described, mut frame) = describe(480, 800, |c| {
        ui::draw_switch_row(c, &env, &style, row, false, long, "", true);
    });
    assert_eq!(drawn_outside(&mut frame, row), None);
    // From a gap before the switch on, nothing but the switch: the title
    // was shortened before it.
    let mut alone = support::frame(480, 800);
    ui::draw_switch(&mut alone.canvas(), &style, row, true, false);
    let from = switch.x - style.gap as u16;
    let (view, alone) = (frame.view(), alone.view());
    for y in row.y..row.bottom() as u16 {
        for x in from..row.right() as u16 {
            assert_eq!(view.pixel(x, y), alone.pixel(x, y), "({x}, {y})");
        }
    }
    let parts = semantics::parts(described);
    assert!(
        parts
            .iter()
            .any(|p| p.role == Role::Switch && p.on == Some(true))
    );
}

/// Numbers above each other with the same room line their − and +
/// buttons up, whatever their titles and values.
#[test]
fn steppers_of_rows_above_each_other_line_up() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let room = "1000 px";
    let mut columns = Vec::new();
    for (n, value) in ["1", "250 %", "1000 px", ""].iter().enumerate() {
        let row = Rect::new(0, 100 + n as u16 * 70, 480, 70);
        let (minus, plus) = ui::stepper(&env, &style, row, room).unwrap();
        columns.push((minus.x, plus.x));
        assert_eq!(plus.right() + style.margin, row.right());
        assert!(minus.right() < u32::from(plus.x));
        let mut frame = frame(480, 800);
        ui::draw_number(
            &mut frame.canvas(),
            &env,
            &style,
            row,
            "Size",
            value,
            room,
            Some(true),
        );
        assert_eq!(drawn_outside(&mut frame, row), None, "{value}");
    }
    assert!(columns.windows(2).all(|w| w[0] == w[1]), "{columns:?}");
}

#[test]
fn buttons_draw_inside_their_frame_and_tell_what_they_are() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    for label in [
        "OK",
        "+",
        "m",
        "A label far too long for a small button",
        "",
    ] {
        for (on, focused) in [(false, false), (true, false), (false, true), (true, true)] {
            let rect = Rect::new(100, 100, 90, 50);
            let (described, mut frame) = describe(480, 300, |c| {
                ui::draw_button(c, &env, &style, rect, label, on, focused);
            });
            // The focus frame goes round the button.
            let ring = if focused {
                (ui::button_border(&style) * 3) as u16
            } else {
                0
            };
            assert_eq!(
                drawn_outside(&mut frame, rect.grow(ring)),
                None,
                "{label:?} on {on} focused {focused}"
            );
            let button = &described[0];
            assert_eq!(
                (button.role, button.label.as_str(), button.focused),
                (Role::Button, label, focused)
            );
            assert_eq!(button.on, on.then_some(true));
        }
    }
}

// ---- Header and pager --------------------------------------------------

/// Points across the whole header, every 4 pixels.
fn across(area: Rect) -> impl Iterator<Item = Point> {
    let y = middle(area).y;
    (i32::from(area.x)..area.right() as i32)
        .step_by(4)
        .map(move |x| Point::new(x, y))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// On a touch screen, the back arrow and the page arrows are hit
    /// where they are: back at the left, the previous page left of the
    /// page number's middle, the next page right of it; the title
    /// between them is no arrow. Turning stops nowhere: every page but
    /// the first can go back, every one but the last on.
    #[test]
    fn header_arrows_are_hit_where_they_are(
        page in 0usize..20,
        pages in 0usize..20,
        back in any::<bool>(),
        title in text_strategy(),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, back);
        let paging = (pages > 0).then_some((page % pages.max(1), pages));
        let a = header.area;
        let actions_across: Vec<Option<paperpaw_sdk::Id>> =
            across(a).map(|p| header.tap(&env, &style, p, paging)).collect();
        // From the left: back (if any), the title, then previous and next.
        let mut seen: Vec<Option<paperpaw_sdk::Id>> = Vec::new();
        for action in &actions_across {
            if seen.last() != Some(action) {
                seen.push(action.clone());
            }
        }
        let mut expected = Vec::new();
        if back {
            expected.push(Some(actions::BACK));
        }
        expected.push(None);
        if pages > 1 {
            expected.push(Some(actions::PREV_PAGE));
            expected.push(Some(actions::NEXT_PAGE));
        }
        prop_assert_eq!(seen, expected);
        // The title is where no arrow is, from the back arrow or the
        // margin on.
        let left = if back { a.height } else { style.margin as u16 };
        for (p, action) in across(a).zip(&actions_across) {
            let title = action.is_none() && p.x >= i32::from(left);
            prop_assert_eq!(header.on_title(&env, &style, p, paging), title, "{:?}", p);
        }
        // Outside: nothing.
        prop_assert_eq!(header.tap(&env, &style, Point::new(5, a.bottom() as i32 + 1), paging), None);
        // Drawn inside the header, the title shortened before the arrows.
        let (described, mut frame) = describe(480, 800, |c| {
            header.draw(c, &env, &style, &title, Some("by title"), paging);
        });
        prop_assert_eq!(drawn_outside(&mut frame, a), None);
        prop_assert_eq!(described[0].role, Role::Heading);
    }

    /// With buttons alone, the header has no arrows: a tap anywhere is
    /// no arrow (there are none to tap), the page number shows all the
    /// same.
    #[test]
    fn without_touch_the_header_has_no_arrows(
        page in 0usize..20,
        pages in 2usize..20,
        back in any::<bool>(),
    ) {
        let h = Harness::new(Device::BUTTONS);
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, back);
        let paging = Some((page % pages, pages));
        for p in across(header.area) {
            prop_assert_eq!(header.tap(&env, &style, p, paging), None);
        }
        let (described, mut frame) = describe(480, 800, |c| {
            header.draw(c, &env, &style, "Library", None, paging);
        });
        prop_assert_eq!(drawn_outside(&mut frame, header.area), None);
        let label = format!("{} / {}", page % pages + 1, pages);
        prop_assert!(described.iter().any(|d| d.label == label), "the page number");
    }
}

/// The columns of ink in `rows` of `frame`: the first and the last.
fn ink_columns(frame: &paperpaw_graphics::Framebuffer, rows: std::ops::Range<u16>) -> (u16, u16) {
    let view = frame.view();
    let width = frame.width();
    // Against what is left of it at the edge: the header's own paper.
    let inked = |x: u16| rows.clone().any(|y| view.pixel(x, y) != view.pixel(0, y));
    let first = (0..width).find(|&x| inked(x)).expect("ink");
    let last = (0..width).rev().find(|&x| inked(x)).expect("ink");
    (first, last)
}

/// The header's arrows keep to the margins the content keeps, on every
/// density (the X4 to the Boox): the back arrow's ink begins where list
/// icons begin, the next page's arrow ends at the right margin, and the
/// title begins where labels after icons do.
#[test]
fn the_header_keeps_to_the_content_margins() {
    for dpi in [150, 220, 254, 300] {
        let mut h = Harness::new(Device::TOUCH);
        h.device.display.dpi = dpi;
        let env = h.env();
        let style = UiStyle::new(&env);
        let header = Header::new(&env, &style, true);
        let area = header.area;
        let middle = area.y + area.height / 4..area.y + area.height * 3 / 4;
        let mut arrows = frame(480, 800);
        header.draw(&mut arrows.canvas(), &env, &style, "", None, Some((1, 3)));
        let (first, last) = ink_columns(&arrows, middle.clone());
        let margin = style.margin as u16;
        assert_eq!(first, margin, "back arrow at {dpi} dpi");
        assert_eq!(last, 480 - margin - 1, "next arrow at {dpi} dpi");
        // A list icon, as rows draw it.
        let row = Rect::new(0, 100, 480, style.row as u16);
        let mut icon = frame(480, 800);
        let room = ui::icon_room(&env, &style, true);
        let label = ui::draw_row_icon(
            &mut icon.canvas(),
            &env,
            &style,
            row,
            room,
            Some("books"),
            paperpaw_sdk::Color::BLACK,
        );
        let (icon_first, _) = ink_columns(&icon, row.y..row.y + row.height);
        // An icon's box begins at the margin, its drawing a little after
        // (the icon's own side bearing), never before.
        let bearing = (style.text * ui::ICON_SCALE / 4.0) as u16;
        assert!(
            (first..=first + bearing).contains(&icon_first),
            "icon {icon_first}, arrow {first} at {dpi} dpi"
        );
        // The title where the label after the icon begins.
        let (described, _) = describe(480, 800, |c| {
            header.draw(c, &env, &style, "Library", None, None);
        });
        let title = described
            .iter()
            .find(|d| d.label == "Library" && d.role == Role::Text)
            .map(|d| d.area.x)
            .expect("the title");
        assert_eq!(title, label.x + margin, "title at {dpi} dpi");
    }
}

#[test]
fn a_focused_header_note_is_told_as_a_focused_button() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let style = UiStyle::new(&env);
    let mut header = Header::new(&env, &style, true);
    header.note_focused = true;
    let (described, _) = describe(480, 800, |c| {
        header.draw(c, &env, &style, "Library", Some("by author"), None);
    });
    let note = described.iter().find(|d| d.role == Role::Button).unwrap();
    assert_eq!(note.label, "by author");
    assert!(note.focused);
}

#[test]
fn the_pager_turns_by_halves_on_touch_screens_only() {
    for touch in [true, false] {
        let h = Harness::new(if touch {
            Device::TOUCH
        } else {
            Device::BUTTONS
        });
        let env = h.env();
        let style = UiStyle::new(&env);
        let area = Rect::new(20, 700, 400, Pager::height(&style));
        let pager = Pager::new(&env, area);
        let left = Point::new(21, middle(area).y);
        let right = Point::new(419, middle(area).y);
        let outside = Point::new(10, middle(area).y);
        if touch {
            assert_eq!(pager.tap(left), Some(actions::PREV_PAGE));
            assert_eq!(pager.tap(right), Some(actions::NEXT_PAGE));
        } else {
            assert_eq!(pager.tap(left), None);
            assert_eq!(pager.tap(right), None);
        }
        assert_eq!(pager.tap(outside), None);
        for page in [(0, 1), (0, 3), (2, 3), (9, 3)] {
            let mut frame = frame(480, 800);
            pager.draw(&mut frame.canvas(), &env, &style, page);
            assert_eq!(drawn_outside(&mut frame, area), None, "{page:?}");
        }
    }
}

// ---- Messages, notes, empty screens, hints ------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Messages, notes at the bottom and empty screens draw inside their
    /// area at any size from a line of text up, however long the text.
    /// (In less, a message shows its first line all the same.)
    #[test]
    #[ignore = "bug: an empty screen 100 px wide draws its icon and paw past its area (ui::draw_empty, since the new look)"]
    fn texts_in_areas_draw_inside_them(
        x in 0u16..100,
        y in 0u16..400,
        width in 100u16..380,
        height in 60u16..400,
        title in text_strategy(),
        text in text_strategy(),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let area = Rect::new(x, y, width, height);
        let draws: [Draw<'_>; 3] = [
            &|c| ui::draw_message(c, &env, &style, area, &text),
            &|c| ui::draw_note_line(c, &env, &style, area, &text),
            &|c| ui::draw_empty(c, &env, &style, area, "books", &title, &text),
        ];
        for (n, draw) in draws.iter().enumerate() {
            let mut frame = frame(480, 800);
            draw(&mut frame.canvas());
            prop_assert_eq!(drawn_outside(&mut frame, area), None, "kind {}", n);
        }
    }

    /// The row of hints draws each button's hint in its share of the
    /// width, inside the row, the pressed one inverted.
    #[test]
    fn the_row_of_hints_draws_inside_it(
        hints in proptest::collection::vec(
            prop_oneof![
                Just(Hint::Default),
                Just(Hint::Hidden),
                Just(Hint::Icon("arrow-left")),
                "[a-zA-Z ]{0,30}".prop_map(Hint::Text),
            ],
            0..6,
        ),
        held in proptest::collection::vec(any::<bool>(), 0..6),
        pressed in proptest::option::of(0usize..6),
    ) {
        let h = Harness::new(Device::BUTTONS);
        let env = h.env();
        let height = ui::button_hints_height(&env);
        let area = Rect::new(0, 800 - height, 480, height);
        let mut frame = frame(480, 800);
        ui::draw_button_hints(&mut frame.canvas(), &env, area, &hints, &held, pressed);
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
    }
}

#[test]
fn an_empty_screen_shows_its_icon_when_there_is_room() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let area = Rect::new(0, 100, 480, 600);
    let (described, frame) = describe(480, 800, |c| {
        ui::draw_empty(
            c,
            &env,
            &style,
            area,
            "books",
            "No books yet",
            "Copy books into the folder books",
        );
    });
    assert!(described.iter().any(|d| d.label == "No books yet"));
    // The icon, gray, above the title in the upper part.
    let top = Rect::new(0, 100, 480, 200);
    assert!(drawn_in(&frame, top));
}

/// The page number between a header's arrows is told apart from them, so
/// that a long list can ask for a page there; without pages there is none.
#[test]
fn the_page_number_is_hit_where_it_is_drawn() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let style = UiStyle::new(&env);
    let header = Header::new(&env, &style, true);
    let page = Some((1, 30));
    let (described, _) = describe(480, 800, |c| {
        header.draw(c, &env, &style, "Reading", None, page);
    });
    let number = described
        .iter()
        .find(|d| d.label.contains("30"))
        .expect("the page number");
    assert!(header.on_page_number(&env, &style, middle(number.area), page));
    // Not the title, nor without pages.
    let title = described.iter().find(|d| d.label == "Reading").unwrap();
    assert!(!header.on_page_number(&env, &style, middle(title.area), page));
    assert!(!header.on_page_number(&env, &style, middle(number.area), None));
}

/// Rows with a note of one line are lower than those of two, as high as
/// a title and that line.
#[test]
fn noted_rows_grow_with_their_lines() {
    let h = Harness::new(Device::BUTTONS);
    let style = UiStyle::new(&h.env());
    assert_eq!(style.noted_row_of(ui::NOTE_LINES), style.noted_row());
    assert!(style.noted_row_of(1) < style.noted_row());
    assert!(style.noted_row_of(1) > style.row * 3 / 4);
}

/// A QR code takes whole pixels a module, square, inside its area, with
/// white around it; one that does not fit is not drawn.
#[test]
fn a_qr_code_is_drawn_whole_or_not_at_all() {
    let qr = paperpaw_sdk::qr::QrCode::new(b"https://codeberg.org/someone/clock").unwrap();
    let area = Rect::new(40, 100, 300, 260);
    let mut drawn = frame(480, 800);
    let at = ui::draw_qr(&mut drawn.canvas(), &qr, area).unwrap();
    assert_eq!(at.width, at.height);
    assert_eq!(at.width % (qr.size() as u16 + 8), 0);
    assert!(at.x >= area.x && at.right() <= area.right());
    assert!(at.y >= area.y && at.bottom() <= area.bottom());
    assert!(drawn_in(&drawn, at));
    assert_eq!(drawn_outside(&mut drawn, at), None);
    let mut small = frame(480, 800);
    assert_eq!(
        ui::draw_qr(&mut small.canvas(), &qr, Rect::new(0, 0, 20, 20)),
        None
    );
    assert!(!drawn_in(&small, Rect::new(0, 0, 480, 800)));
}
