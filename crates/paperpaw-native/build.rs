#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! Puts the linker script for plugins (`paperpaw-plugin.x`) where the
//! linker finds it: a crate that depends on this one (through the SDK)
//! links with it by naming it in its rustflags.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=paperpaw-plugin.x");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy("paperpaw-plugin.x", out.join("paperpaw-plugin.x")).expect("paperpaw-plugin.x");
    println!("cargo::rustc-link-search=native={}", out.display());
}
