//! The plugin interface (`native::INTERFACE`, ADR 0028) holds as long as
//! what plugins see stays as `interface.txt` keeps it: the SDK's items
//! (`api.txt`: none gone or changed; new ones are free, a plugin built
//! before does not use them), the layouts of the types that cross
//! (`native::LAYOUT`), the linker script plugins are linked with, the
//! compilers of the release, and so the fingerprint. Where one changed,
//! the interface is raised by hand and the file written anew:
//!
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use paperpaw_sdk::native::{FINGERPRINT, INTERFACE, LAYOUT, TOOLCHAIN};

fn here(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// A file of the repository, line ends as on Linux; `None` outside it
/// (the SDK as published, without the repository around it).
fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(here(path))
        .ok()
        .map(|t| t.replace("\r\n", "\n"))
}

/// FNV-1a with 64 bits of `text`.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `key = value` of the toolchain files the release builds with.
fn value(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default()
}

/// What plugins see now, as `interface.txt` writes it; `None` outside the
/// repository.
fn now() -> Option<String> {
    let linker = read("../paperpaw-native/paperpaw-plugin.x")?;
    let x4 = value(&read("../../shells/xteink/rust-toolchain.toml")?, "channel");
    let esp = value(
        &read("../../shells/xteink/versions.env")?,
        "ESP_RUST_VERSION",
    );
    Some(format!(
        "# What code plugins see at this plugin interface (ADR 0028): kept so\n\
         # that CI fails where it changes without native::INTERFACE raised.\n\
         # Written by `PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test\n\
         # interface`, with interface-api.txt (the SDK's items, api.txt as\n\
         # it was), only once INTERFACE is raised.\n\
         interface = {INTERFACE}\n\
         features = {}\n\
         layout = {LAYOUT:016x}\n\
         fingerprint = {FINGERPRINT}\n\
         linker = {:016x}\n\
         x4 = rustc {x4}\n\
         x4-pro = esp {esp}\n",
        features(),
        fnv(&linker)
    ))
}

/// The features that change what crosses, as this test is built with:
/// `cargo test -p paperpaw-sdk` builds it so, a test of the whole
/// workspace may switch on more (colour, which the emulator draws).
fn features() -> String {
    format!(
        "outlines {}, colour {}",
        paperpaw_text::OUTLINES,
        paperpaw_graphics::COLOR
    )
}

#[test]
fn what_plugins_see_changes_only_with_the_interface() {
    let (Some(now), Some(api)) = (now(), read("api.txt")) else {
        return;
    };
    let kept = read("interface.txt").unwrap_or_default();
    // Built with other features (a test of the whole workspace): the
    // layouts and the fingerprint are another build's; CI checks this
    // one on its own (`cargo test -p paperpaw-sdk --test interface`).
    if value(&kept, "features") != features() && std::env::var_os("PAPERPAW_BLESS").is_none() {
        eprintln!(
            "interface: built with {}, not as interface.txt; not compared",
            features()
        );
        return;
    }
    let kept_api = read("interface-api.txt").unwrap_or_default();
    let kept_interface: u16 = value(&kept, "interface").parse().unwrap_or(0);
    let lines: BTreeSet<&str> = api.lines().collect();
    let gone: Vec<&str> = kept_api.lines().filter(|l| !lines.contains(l)).collect();
    let same = now == kept && gone.is_empty();
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        assert!(
            same || INTERFACE > kept_interface,
            "what plugins see changed: raise native::INTERFACE (now {INTERFACE}) first"
        );
        std::fs::write(here("interface.txt"), &now).unwrap();
        std::fs::write(here("interface-api.txt"), &api).unwrap();
        return;
    }
    assert_eq!(
        INTERFACE, kept_interface,
        "native::INTERFACE is {INTERFACE}, interface.txt says {kept_interface}: write it anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface)"
    );
    assert!(
        gone.is_empty(),
        "items of the SDK that plugins may use are gone or changed, so plugins built before \
         would not run: raise native::INTERFACE to {} and write interface.txt anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface). Gone or changed:\n{}",
        INTERFACE + 1,
        gone.join("\n")
    );
    assert_eq!(
        now,
        kept,
        "what plugins see changed (the layouts of the types that cross, the linker script or \
         the release's compilers): raise native::INTERFACE to {} and write interface.txt anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface)",
        INTERFACE + 1
    );
}

/// The compilers the SDK names for the chips are the release's.
#[test]
fn the_toolchains_named_are_the_releases() {
    let (Some(toolchain), Some(versions)) = (
        read("../../shells/xteink/rust-toolchain.toml"),
        read("../../shells/xteink/versions.env"),
    ) else {
        return;
    };
    let x4 = value(&toolchain, "channel");
    let esp = value(&versions, "ESP_RUST_VERSION");
    let src = read("src/native.rs").unwrap();
    assert!(
        src.contains(&format!("\"rustc {x4}, riscv32imc-unknown-none-elf\"")),
        "native::TOOLCHAIN names rustc {x4}"
    );
    assert!(
        src.contains(&format!("\"esp {esp}, xtensa-esp32s3-none-elf\"")),
        "native::TOOLCHAIN names esp {esp}"
    );
    assert!(!TOOLCHAIN.is_empty());
}

/// Every plugin project of the repository links with the format's own
/// linker script.
#[test]
fn plugin_projects_link_with_the_formats_script() {
    let Some(script) = read("../paperpaw-native/paperpaw-plugin.x") else {
        return;
    };
    for project in ["../../samples/hello-native", "../../tools/plugin-files"] {
        let copy = read(&format!("{project}/paperpaw-plugin.x"));
        assert_eq!(
            copy.as_deref(),
            Some(script.as_str()),
            "{project}/paperpaw-plugin.x"
        );
    }
}
