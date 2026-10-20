//! Snapshots: the plugin's screens and widgets drawn on emulated devices
//! and compared with the pictures in `tests/snapshots`, the cases listed
//! in `tests/snapshots/cases.ini`. A few pixels may differ between
//! computers; more fails, and `target/paperpaw-dev/test/report.html`
//! shows the expected picture, the actual one and where they differ.
//!
//! After a change meant to alter what is drawn, write the pictures anew
//! and look at them before committing them:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test --test snapshots
//! ```
//!
//! (In PowerShell: `$env:PAPERPAW_BLESS = 1; cargo test --test snapshots`.)

#[test]
fn screens_look_as_before() {
    // The plugin as the app gets it, as in examples/paperpaw-dev.rs.
    let checked =
        paperpaw_dev::snapshot::check_dir(|app| app.plugin(my_plugin::MyPlugin), "tests/snapshots");
    if let Err(problems) = checked {
        panic!("{problems}");
    }
}
