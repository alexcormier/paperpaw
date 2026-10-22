//! A plugin with code added to the standard app (`set_plugins`), as a
//! module built around a plugin being written adds it: it runs on the
//! device and `paperpaw-dev` checks it. A test binary of its own, as the
//! plugins added hold for every device started after.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_web::emulator::Emulator;

#[test]
fn a_plugin_with_code_is_added_and_checked() {
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(5_000);
    assert!(
        emulator.check(&["lint"], &["hello"]).is_err(),
        "nothing added yet"
    );

    // Added before the device starts, as a module's `pp_plugins` does
    // (one device at a time: the heap budget counts every kernel).
    drop(emulator);
    paperpaw_web::set_plugins(|app| app.plugin(paperpaw_sample_hello::Hello));
    let mut emulator = Emulator::new("xteink-x4", "en", false).unwrap();
    emulator.start().unwrap();
    emulator.advance(10_000);
    let json = emulator
        .check(&["lint", "reach", "perf"], &["hello"])
        .unwrap();
    // Each check ran, perf with the app's heap alone (this device's own
    // is no part of it).
    for ran in ["lint.pseudo", "reach.searched", "perf.calibration"] {
        assert!(json.contains(ran), "{ran}: {json}");
    }
    assert!(!json.contains("perf.heap\""), "{json}");
    assert!(!json.contains("dev.run"), "a check could not run: {json}");
    // Only plugins the app has, only checks of plugins.
    assert!(emulator.check(&["lint"], &["no-such-plugin"]).is_err());
    assert!(emulator.check(&["snapshot"], &["hello"]).is_err());
    // The device runs on after the check.
    assert!(emulator.button("right", true, 20_000));
    emulator.advance(25_000);
}
