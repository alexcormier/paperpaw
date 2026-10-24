//! Every widget of the pack drawn where it has little room: in tiny
//! cells, narrow and flat ones, with the thickest lines and no margin, in
//! each of its styles. None may panic: in release a panic restarts the
//! device.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::widgets::categories;
use paperpaw_sdk::{PixelFormat, Rect, SettingKind, SettingValue};

use crate::books::tests::library;
use crate::harness::{DEVICE, Harness};

/// Cells with little room, and one at the far corner of the page.
const AREAS: [Rect; 7] = [
    Rect::new(5, 5, 1, 1),
    Rect::new(5, 5, 20, 10),
    Rect::new(5, 5, 40, 30),
    Rect::new(5, 5, 12, 120),
    Rect::new(5, 5, 300, 12),
    Rect::new(5, 5, 64, 64),
    Rect::new(440, 760, 40, 40),
];

/// Each widget's settings that change how it is drawn: a choice set to
/// each of its values, a switch to the other side.
fn variants(h: &Harness, id: &str) -> Vec<(String, SettingValue)> {
    let prefix = alloc::format!("{id}.");
    let mut variants = Vec::new();
    for spec in h.settings.specs() {
        let key = spec.id.as_str();
        if !key.starts_with(&prefix) {
            continue;
        }
        match &spec.kind {
            SettingKind::Choice(options) => {
                for value in options.iter() {
                    let value = SettingValue::Text(Cow::Owned(String::from(value)));
                    variants.push((String::from(key), value));
                }
            }
            SettingKind::Bool => {
                let on = h.settings.bool(key);
                variants.push((String::from(key), SettingValue::Bool(!on)));
            }
            _ => {}
        }
    }
    variants
}

/// Places, reads and draws `id` in each of [`AREAS`].
fn draw_everywhere(h: &Harness, fb: &mut Framebuffer, id: &str) {
    let env = h.env();
    for area in AREAS {
        let mut widget = env.widgets().make(id, &env).unwrap();
        let _ = widget.height(&env, area.width);
        let _ = widget.min_height(&env, area.width);
        widget.place(&env, area);
        let _ = widget.update(&env);
        widget.draw(&mut fb.canvas(), &env, area);
        let _ = widget.targets(&env, area);
    }
}

#[test]
fn every_widget_draws_in_little_room_with_thick_lines() {
    let mut h = Harness::new(library());
    h.set(core_settings::UI_LINES.as_str(), SettingValue::Int(10));
    h.set(core_settings::UI_MARGIN.as_str(), SettingValue::Int(0));
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    let ids: Vec<String> = h
        .widgets
        .iter()
        .map(|(id, _)| String::from(id.as_str()))
        .collect();
    assert!(ids.len() > 20, "{ids:?}");
    for id in &ids {
        draw_everywhere(&h, &mut fb, id);
        // Each setting's values, and each with each of another's: some
        // ways to draw need two (a ring without the percent in it).
        let variants = variants(&h, id);
        for (i, (key, value)) in variants.iter().enumerate() {
            let before = h.settings.get(key).cloned().unwrap();
            h.set(key, value.clone());
            draw_everywhere(&h, &mut fb, id);
            for (other, second) in variants.iter().skip(i + 1).filter(|(k, _)| k != key) {
                let was = h.settings.get(other).cloned().unwrap();
                h.set(other, second.clone());
                draw_everywhere(&h, &mut fb, id);
                h.set(other, was);
            }
            h.set(key, before);
        }
    }
}

/// Before the clock is set (a plain X4 until Wi-Fi sets it), every time
/// widget says so, in each of its styles, rather than drawing an empty
/// frame, a face without hands or a wrong hint.
#[test]
fn time_widgets_say_when_the_time_is_not_known() {
    let mut h = Harness::new(library());
    h.status.clock = None;
    let mut fb = Framebuffer::new(
        DEVICE.display.width,
        DEVICE.display.height,
        PixelFormat::Gray4,
    )
    .unwrap();
    let area = Rect::new(10, 10, 460, 300);
    let ids: Vec<String> = h
        .widgets
        .iter()
        .filter(|(id, _)| h.widgets.category(id.as_str()) == Some(categories::TIME))
        .map(|(id, _)| String::from(id.as_str()))
        .collect();
    assert!(ids.len() >= 9, "{ids:?}");
    let message = String::from(h.i18n.get("widgets.time.unknown").unwrap());
    let says = |h: &Harness, fb: &mut Framebuffer, id: &str| {
        let env = h.env();
        let mut widget = env.widgets().make(id, &env).unwrap();
        widget.place(&env, area);
        let mut described = Vec::new();
        widget.draw(&mut fb.canvas().describing(&mut described), &env, area);
        let said: Vec<&str> = described.iter().map(|d| d.label.as_str()).collect();
        said.join(" ").contains(&message)
    };
    for id in &ids {
        assert!(says(&h, &mut fb, id), "{id}");
        for (key, value) in variants(&h, id) {
            let before = h.settings.get(&key).cloned().unwrap();
            h.set(&key, value.clone());
            assert!(says(&h, &mut fb, id), "{id}: {key} = {value:?}");
            h.set(&key, before);
        }
    }
}

/// The time and words widgets draw inside the place they are given, in
/// cells as small as the X4's and as narrow as a column, also with a
/// note, to-dos and a quote to show: nothing reaches past its edges.
#[test]
fn time_and_words_widgets_keep_inside_their_cell() {
    let mut h = Harness::new(library());
    h.set(
        "widgets.note.text",
        SettingValue::Text(Cow::Borrowed(
            "Remember: the library closes early on Fridays.",
        )),
    );
    let ids = [
        "widgets.calendar",
        "widgets.year-progress",
        "widgets.countdown",
        "widgets.moon",
        "widgets.day",
        "widgets.greeting",
        "widgets.logo",
        "widgets.note",
        "widgets.picture",
        "widgets.quote",
        "widgets.todo",
    ];
    let areas = [
        Rect::new(40, 40, 90, 67),
        Rect::new(40, 40, 197, 67),
        Rect::new(40, 40, 90, 151),
        Rect::new(40, 40, 197, 151),
        Rect::new(40, 40, 411, 67),
        Rect::new(40, 40, 30, 30),
    ];
    let env = h.env();
    for id in ids {
        for area in areas {
            let mut fb = Framebuffer::new(
                DEVICE.display.width,
                DEVICE.display.height,
                PixelFormat::Gray4,
            )
            .unwrap();
            let mut widget = env.widgets().make(id, &env).unwrap();
            widget.place(&env, area);
            let _ = widget.update(&env);
            {
                let mut canvas = fb.canvas();
                canvas.clear(paperpaw_sdk::Color::WHITE);
                widget.draw(&mut canvas, &env, area);
            }
            let view = fb.view();
            for y in 0..DEVICE.display.height {
                for x in 0..DEVICE.display.width {
                    let inside = x >= area.x
                        && x < area.x + area.width
                        && y >= area.y
                        && y < area.y + area.height;
                    if !inside {
                        assert_eq!(view.pixel(x, y).luma(), 255, "{id} in {area:?} at {x},{y}");
                    }
                }
            }
        }
    }
}

/// Text is broken between words only: the lines together say the text,
/// word for word, and a word wider than a line is told, not split.
#[test]
fn lines_break_between_words_only() {
    let h = Harness::new(library());
    let env = h.env();
    let font = paperpaw_sdk::ui::UiStyle::new(&env).font(&env).unwrap();
    let text = "Remember: the library closes early on Fridays.";
    for width in [60.0, 120.0, 400.0] {
        let (lines, _) = crate::time::break_words(&font, 20.0, text, width);
        assert_eq!(lines.join(" "), text, "{width}");
    }
    let (lines, too_wide) = crate::time::break_words(&font, 20.0, text, 30.0);
    assert!(too_wide);
    assert!(lines.contains(&String::from("Remember:")), "{lines:?}");
}
