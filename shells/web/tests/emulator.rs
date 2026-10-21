//! The browser's emulator without a browser: each device starts the
//! standard app and shows it, takes its buttons and touch, takes a data
//! plugin's files and shows them after a restart, and checks them.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Mutex;

use paperpaw_platform::PointerPhase;
use paperpaw_web::emulator::{Emulator, PROFILES};

/// The heap budget counts every thread's kernel: one device at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn started(profile: &str) -> Emulator {
    let mut emulator = Emulator::new(profile, "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(5_000);
    emulator
}

/// The sample data plugin `name`'s files, as the page would hand them in.
fn sample(name: &str) -> Vec<(String, std::rc::Rc<[u8]>)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples/plugins")
        .join(name);
    paperpaw_dev::target::DataPlugin::read(&folder)
        .unwrap()
        .files
        .into_iter()
        .collect()
}

#[test]
fn every_device_shows_the_app() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    for profile in PROFILES {
        let mut emulator = started(profile);
        let (width, height) = emulator.size();
        assert_eq!(
            emulator.frame().len(),
            usize::from(width) * usize::from(height) * 4
        );
        let frame = emulator.shown();
        assert_eq!(frame.len(), usize::from(width) * usize::from(height) * 4);
        // Something drawn: not one colour all over.
        assert!(frame.chunks(4).any(|p| p != &frame[..4]), "{profile}");
        let device = emulator.device_json();
        assert!(
            device.contains(&format!("\"id\":\"{profile}\"")),
            "{device}"
        );
    }
}

#[test]
fn the_devices_are_as_their_profiles_say() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let x4 = Emulator::new("xteink-x4", "en", false).unwrap();
    let json = x4.device_json();
    assert!(json.contains("\"grays\":2"), "{json}");
    assert!(json.contains("\"touch\":false"), "{json}");
    assert!(json.contains("\"power\""), "{json}");
    let boox = Emulator::new("boox-go-7", "de", true).unwrap();
    let json = boox.device_json();
    assert!(json.contains("\"touch\":true"), "{json}");
    assert!(json.contains("\"dpi\":300"), "{json}");
    assert!(Emulator::new("no-such-device", "en", false).is_err());
    assert!(Emulator::new("xteink-x4", "../en", false).is_err());
}

#[test]
fn buttons_and_touch_are_taken() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut x4 = started("xteink-x4");
    let before = x4.shown();
    assert!(x4.has_button("right"));
    assert!(!x4.has_button("down"));
    assert!(!x4.button("no-such-button", true));
    // The home screen's focus moves.
    assert!(x4.button("right", true));
    assert!(x4.button("right", false));
    x4.advance(10_000);
    assert!(x4.shown() != before, "the press changed nothing");

    let mut boox = started("boox-go-7");
    let before = boox.shown();
    let (width, height) = boox.size();
    // A tap on the home screen's card (start reading) opens the library.
    let (x, y) = (i32::from(width) / 2, i32::from(height) * 34 / 100);
    boox.pointer(PointerPhase::Down, x, y);
    boox.advance(5_050);
    boox.pointer(PointerPhase::Up, x, y);
    boox.advance(8_000);
    assert!(boox.shown() != before, "the tap changed nothing");
}

#[test]
fn a_data_plugin_shows_after_a_restart() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut emulator = started("xteink-x4");
    let before = emulator.shown();
    for (path, bytes) in sample("large-print") {
        emulator.put_file("large-print", &path, &bytes).unwrap();
    }
    emulator.restart();
    emulator.advance(10_000);
    let large = emulator.shown();
    assert!(large != before, "no larger texts");
    // Taken off again: as it was.
    emulator.remove_plugin("large-print");
    emulator.restart();
    emulator.advance(15_000);
    assert!(emulator.shown() == before, "not as before");
    // Names and paths that would leave the plugin's folder are refused.
    assert!(emulator.put_file("../x", "plugin.ini", &[]).is_err());
    assert!(emulator.put_file("x", "../plugin.ini", &[]).is_err());
}

#[test]
fn the_plugin_is_checked_and_the_device_keeps_going() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut emulator = started("xteink-x4");
    assert!(emulator.check(&[]).is_err(), "nothing to check");
    for (path, bytes) in sample("theme-paw") {
        emulator.put_file("theme-paw", &path, &bytes).unwrap();
    }
    emulator.restart();
    let json = emulator.check(&["package", "lint"]).unwrap();
    assert!(json.contains("\"findings\""), "{json}");
    assert!(json.contains("package.size"), "{json}");
    assert!(emulator.check(&["perf"]).is_err());
    // The device runs on after the check.
    assert!(emulator.button("right", true));
    emulator.advance(20_000);
    assert!(!emulator.shown().is_empty());
}

/// A picture on the sleep screen of an X4 (256 KiB of heap): what the
/// kernel writes to the card in memory is no heap of the device's.
#[test]
fn a_sleep_picture_fits_the_x4s_heap() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let picture = include_bytes!("data/stripes-480x800-2bit.png");
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    let plugin = [
        (
            "plugin.ini",
            &b"[plugin]\nid = nights\nname = Nights\nversion = 1.0.0\n"[..],
        ),
        (
            "settings.ini",
            b"[sleep]\nshow = sleep.show.picture\nfolder = plugins/nights/sleep\n",
        ),
        ("sleep/stripes.png", picture),
    ];
    for (path, bytes) in plugin {
        emulator.put_file("nights", path, bytes).unwrap();
    }
    emulator.start().unwrap();
    emulator.advance(5_000);
    let before = paperpaw_emulator::budget::stats().failed;
    assert!(emulator.button("power", true));
    assert!(emulator.button("power", false));
    emulator.advance(10_000);
    assert_eq!(
        paperpaw_emulator::budget::stats().failed,
        before,
        "ran out of memory"
    );
}

/// Another device, night mode or the sleep screen, made apart: what is
/// shown stays as it is.
#[test]
fn snapshots_of_other_devices_leave_what_is_shown() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut x4 = started("xteink-x4");
    let shown = x4.shown();
    let (w, h, boox) = x4.snapshot(Some("boox-go-7"), None, false).unwrap();
    assert_eq!((w, h), (1264, 1680));
    assert_eq!(boox.len(), 1264 * 1680 * 4);
    let (_, _, dark) = x4.snapshot(None, Some(true), false).unwrap();
    let dark_pixels = dark.chunks(4).filter(|p| p[0] < 100).count();
    let light_pixels = shown.chunks(4).filter(|p| p[0] < 100).count();
    assert!(dark_pixels > light_pixels * 2, "night mode is dark");
    let (_, _, asleep) = x4.snapshot(None, None, true).unwrap();
    assert!(asleep != shown, "the sleep screen");
    // This one as it was, and still running.
    assert!(x4.shown() == shown);
    assert!(x4.button("right", true));
}

#[test]
fn the_frame_is_in_the_tones_of_e_ink() {
    use paperpaw_web::emulator::eink;
    // Paper stays; ink is e-ink's black, not a screen's; between, in order.
    assert_eq!(eink(255), 255);
    assert_eq!(eink(0x1e), 0x48);
    assert_eq!(eink(0), 0x48);
    assert!((0x1e..=255).map(eink).collect::<Vec<_>>().windows(2).all(|w| w[0] <= w[1]));
}
