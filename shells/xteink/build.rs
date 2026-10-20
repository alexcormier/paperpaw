#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! Links with esp-hal's linker script. Here rather than in
//! `.cargo/config.toml`: `RUSTFLAGS` (as CI sets it) replaces the
//! configuration's flags, not this.
//!
//! The X4's memory map is PaperPaw's own (`memory-x4.x`, copied as
//! `memory.x`; not named so here, where the linker would find it for the
//! X4 Pro too): esp-hal's maps 4 MB
//! of flash for code and read-only data together, and the fonts take much
//! of it. The X4 Pro and X4 Classic (ESP32-S3) keep esp-hal's, whose code and data each
//! have their own 32 MB window; its linker (GCC) takes the script through
//! `-Wl`.
//!
//! Also gives the firmware PaperPaw's version (`PAPERPAW_VERSION`), the
//! workspace's: the firmware is outside it; and the list of the plugin
//! crates built in (`plugins.rs`, see `paperpaw_update::Plugins`): those
//! of the app as built here (`paperpaw-app`'s, by `links`), with their
//! versions from the firmware's lockfile, which names optional crates
//! whether they are built or not.

use std::path::PathBuf;

fn main() {
    let workspace = std::fs::read_to_string("../../Cargo.toml").expect("../../Cargo.toml");
    let version = workspace
        .split("[workspace.package]")
        .nth(1)
        .and_then(|part| part.lines().find_map(|l| l.strip_prefix("version = ")))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("the workspace's version");
    println!("cargo::rustc-env=PAPERPAW_VERSION={version}");
    println!("cargo::rerun-if-changed=../../Cargo.toml");
    println!("cargo::rerun-if-changed=memory-x4.x");
    plugins();
    if std::env::var_os("CARGO_FEATURE_ESP32S3").is_some() {
        println!("cargo::rustc-link-arg=-nostartfiles");
        println!("cargo::rustc-link-arg=-Wl,-Tlinkall.x");
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::copy("memory-x4.x", out.join("memory.x")).expect("memory-x4.x");
    println!("cargo::rustc-link-search={}", out.display());
    println!("cargo::rustc-link-arg=-Tlinkall.x");
}

/// Writes `plugins.rs`: the list of the plugin crates built in, the
/// bytes the firmware keeps right after its application descriptor.
fn plugins() {
    println!("cargo::rerun-if-changed=Cargo.lock");
    let built: Vec<String> = std::env::var("DEP_PAPERPAW_APP_CRATES")
        .expect("paperpaw-app names its plugin crates")
        .split(',')
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect();
    let lock = std::fs::read_to_string("Cargo.lock").expect("Cargo.lock");
    let mut crates: Vec<(String, String)> = Vec::new();
    for package in lock.split("[[package]]") {
        let field = |key: &str| {
            package.lines().find_map(|l| {
                l.strip_prefix(key)
                    .map(|v| v.trim().trim_matches('"').to_string())
            })
        };
        if let (Some(name), Some(version)) = (field("name = "), field("version = "))
            && built.contains(&name)
        {
            crates.push((name, version));
        }
    }
    crates.sort();
    let bytes = paperpaw_update::Plugins::record(
        crates
            .iter()
            .map(|(name, version)| paperpaw_update::PluginCrate { name, version }),
    )
    .expect("the list of plugins is longer than updates and the web flasher read");
    let code = format!(
        "/// The plugin crates built in (`paperpaw_update::Plugins`), right after
         /// the application descriptor, where updates and the web flasher read them.
         #[unsafe(link_section = \".flash.appdesc.plugins\")]
         #[used]
         pub static PLUGINS: [u8; {}] = {:?};
",
        bytes.len(),
        bytes
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("plugins.rs"), code).expect("plugins.rs");
}
