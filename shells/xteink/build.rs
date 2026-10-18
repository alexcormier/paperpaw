//! Links with esp-hal's linker script. Here rather than in
//! `.cargo/config.toml`: `RUSTFLAGS` (as CI sets it) replaces the
//! configuration's flags, not this.
//!
//! The memory map is QuireInk's own (`memory.x`): esp-hal's maps 4 MB of
//! flash for code and read-only data together, and the fonts take much of it.
//!
//! Also gives the firmware QuireInk's version (`QUIREINK_VERSION`), the
//! workspace's: the firmware is outside it.

use std::path::PathBuf;

fn main() {
    let workspace = std::fs::read_to_string("../../Cargo.toml").expect("../../Cargo.toml");
    let version = workspace
        .split("[workspace.package]")
        .nth(1)
        .and_then(|part| part.lines().find_map(|l| l.strip_prefix("version = ")))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("the workspace's version");
    println!("cargo::rustc-env=QUIREINK_VERSION={version}");
    println!("cargo::rerun-if-changed=../../Cargo.toml");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy("memory.x", out.join("memory.x")).expect("memory.x");
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rerun-if-changed=memory.x");
    println!("cargo::rustc-link-arg=-Tlinkall.x");
}
