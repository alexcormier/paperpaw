//! Every widget of the pack drawn where it has little room: in tiny
//! cells, narrow and flat ones, with the thickest lines and no margin, in
//! each of its styles. None may panic: in release a panic restarts the
//! device.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::builtin::settings as core_settings;
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
        let mut widget = env.widgets.make(id, &env).unwrap();
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
