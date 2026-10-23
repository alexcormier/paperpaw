//! The plugin interface (`native::INTERFACE`, ADR 0026) holds as long as
//! what plugins see stays as `interface.txt` and the folder `interface/`
//! keep it:
//!
//! - the items and the shapes of the types of every crate that crosses
//!   (`interface/crates.txt`), as each crate's `api.txt` and `shape.txt`
//!   say (`tools/interface.py` writes them);
//! - the linker script plugins are linked with;
//! - the compilers of the release.
//!
//! What changed is raised by hand: by default every change raises the
//! interface. Only what cannot change what a plugin built before relies
//! on goes without: a new type, module or re-export, a new function or
//! constant outside a trait, a new `impl`, and the fields, variants and
//! items of a type that is itself new. A field, a variant or an item of a
//! trait that was there, any change of order or of a private field, and
//! anything gone raise it. Either way the files are written anew:
//!
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use paperpaw_sdk::native::{INTERFACE, TOOLCHAIN};

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

/// The crates whose types cross, as `interface/crates.txt` names them.
fn crates() -> Vec<String> {
    read("interface/crates.txt")
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

/// What plugins see besides the crates, as `interface.txt` writes it;
/// `None` outside the repository.
fn now() -> Option<String> {
    let linker = read("../paperpaw-native/paperpaw-plugin.x")?;
    let x4 = value(&read("../../shells/xteink/rust-toolchain.toml")?, "channel");
    let esp = value(
        &read("../../shells/xteink/versions.env")?,
        "ESP_RUST_VERSION",
    );
    let android = value(&read("../../shells/android/versions.env")?, "RUST_VERSION");
    Some(format!(
        "# What code plugins see at this plugin interface (ADR 0026), beside\n\
         # the crates of interface/: kept so that CI fails where it changes\n\
         # without native::INTERFACE raised. Written by `PAPERPAW_BLESS=1\n\
         # cargo test -p paperpaw-sdk --test interface`.\n\
         interface = {INTERFACE}\n\
         linker = {:016x}\n\
         x4 = rustc {x4}\n\
         x4-pro = esp {esp}\n\
         android = rustc {android}\n",
        fnv(&linker)
    ))
}

/// The path an item's line names: its first path, generics and all after
/// them left out.
fn path_of(text: &str) -> &str {
    let mut end = 0;
    let bytes = text.as_bytes();
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_alphanumeric() || b == b'_' {
            end += 1;
        } else if text[end..].starts_with("::") {
            end += 2;
        } else {
            break;
        }
    }
    &text[..end]
}

/// The path without its last part: the type, trait or module that holds it.
fn parent(path: &str) -> &str {
    path.rsplit_once("::").map_or("", |(parent, _)| parent)
}

/// The types of a crate's `shape.txt`: their path, and the line that
/// says their kind and what they hold.
fn types(shape: &str) -> BTreeMap<&str, &str> {
    shape
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|line| {
            let (_, rest) = line.split_once(' ')?;
            Some((path_of(rest), line))
        })
        .collect()
}

/// Whether an item's line, new in `api.txt`, leaves what a plugin built
/// before relies on as it was: a new item that is not part of a trait or
/// of a type that was there (`kept`, the types as they were).
fn is_free(line: &str, kept: &BTreeMap<&str, &str>) -> bool {
    let mut rest = line;
    while let Some(after) = rest.strip_prefix("#[") {
        rest = after.split_once("] ").map_or("", |(_, r)| r);
    }
    if rest.starts_with("impl") {
        return true;
    }
    let Some(rest) = rest.strip_prefix("pub ") else {
        return false;
    };
    let is_trait = |path: &str| kept.get(path).is_some_and(|l| l.starts_with("trait "));
    for keyword in [
        "struct ", "enum ", "union ", "trait ", "mod ", "use ", "macro ", "static ",
    ] {
        if rest.starts_with(keyword) {
            return true;
        }
    }
    let mut item = rest;
    for keyword in [
        "const ",
        "unsafe ",
        "async ",
        "extern \"C\" ",
        "fn ",
        "type ",
    ] {
        item = item.strip_prefix(keyword).unwrap_or(item);
    }
    if item.len() < rest.len() {
        // A function, constant or type alias: new outside a trait that
        // was there (a trait's would be a new item of its table).
        return !is_trait(parent(path_of(item)));
    }
    // A field or a variant: free only in a type that is itself new.
    let path = path_of(rest);
    let holder = parent(path);
    !kept.contains_key(holder) && !kept.contains_key(parent(holder))
}

/// What changed in one crate from `kept` to `now` (its `api.txt` and
/// `shape.txt`): what breaks plugins built before, and what is only new.
#[derive(Default)]
struct Changes {
    breaking: Vec<String>,
    new: Vec<String>,
}

fn changes(crate_: &str, kept: (&str, &str), now: (&str, &str)) -> Changes {
    let mut out = Changes::default();
    let (kept_types, now_types) = (types(kept.1), types(now.1));
    for (path, line) in &kept_types {
        match now_types.get(path) {
            None => out.breaking.push(format!("{crate_}: gone: {line}")),
            Some(now) if now != line => {
                out.breaking
                    .push(format!("{crate_}: changed: {line}\n  now: {now}"));
            }
            Some(_) => {}
        }
    }
    for (path, line) in &now_types {
        if !kept_types.contains_key(path) {
            out.new.push(format!("{crate_}: {line}"));
        }
    }
    let kept_api: BTreeSet<&str> = kept.0.lines().collect();
    let now_api: BTreeSet<&str> = now.0.lines().collect();
    for line in kept_api.difference(&now_api) {
        out.breaking
            .push(format!("{crate_}: gone or changed: {line}"));
    }
    for line in now_api.difference(&kept_api) {
        if is_free(line, &kept_types) {
            out.new.push(format!("{crate_}: {line}"));
        } else {
            out.breaking
                .push(format!("{crate_}: added to what was there: {line}"));
        }
    }
    out
}

#[test]
fn what_plugins_see_changes_only_with_the_interface() {
    let Some(now) = now() else {
        return;
    };
    let bless = std::env::var_os("PAPERPAW_BLESS").is_some();
    let kept = read("interface.txt").unwrap_or_default();
    let kept_interface: u16 = value(&kept, "interface").parse().unwrap_or(0);
    let mut all = Changes::default();
    let mut files = Vec::new();
    for crate_ in crates() {
        let folder = format!("../{crate_}");
        let api = read(&format!("{folder}/api.txt")).expect("api.txt: python3 tools/interface.py");
        let shape =
            read(&format!("{folder}/shape.txt")).expect("shape.txt: python3 tools/interface.py");
        let kept_api = read(&format!("interface/{crate_}.api")).unwrap_or_default();
        let kept_shape = read(&format!("interface/{crate_}.shape")).unwrap_or_default();
        let found = changes(&crate_, (&kept_api, &kept_shape), (&api, &shape));
        all.breaking.extend(found.breaking);
        all.new.extend(found.new);
        files.push((format!("interface/{crate_}.api"), api));
        files.push((format!("interface/{crate_}.shape"), shape));
    }
    let rest_same = value(&now, "linker") == value(&kept, "linker")
        && ["x4", "x4-pro", "android"]
            .iter()
            .all(|key| value(&now, key) == value(&kept, key));
    let breaking = !all.breaking.is_empty() || !rest_same;
    if bless {
        assert!(
            !breaking || INTERFACE > kept_interface,
            "what plugins see changed: raise native::INTERFACE (now {INTERFACE}) first:\n{}",
            all.breaking.join("\n")
        );
        std::fs::write(here("interface.txt"), &now).unwrap();
        for (path, text) in files {
            std::fs::write(here(&path), text).unwrap();
        }
        return;
    }
    assert_eq!(
        INTERFACE, kept_interface,
        "native::INTERFACE is {INTERFACE}, interface.txt says {kept_interface}: write it anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface)"
    );
    assert!(
        !breaking,
        "what plugins built before rely on changed, so they would not run: raise \
         native::INTERFACE to {} and write the files anew (PAPERPAW_BLESS=1 cargo test -p \
         paperpaw-sdk --test interface). The linker script and the compilers {}. Changed:\n{}",
        INTERFACE + 1,
        if rest_same {
            "are as they were"
        } else {
            "changed"
        },
        all.breaking.join("\n")
    );
    assert!(
        all.new.is_empty(),
        "only new items, which plugins built before do not use: write the files anew \
         (PAPERPAW_BLESS=1 cargo test -p paperpaw-sdk --test interface), INTERFACE stays. \
         New:\n{}",
        all.new.join("\n")
    );
    assert_eq!(now, kept);
}

/// Every crate of PaperPaw that a crate of `interface/crates.txt` depends
/// on is named there too: its types may cross as well. The SDK names the
/// same (`native::CROSSING_CRATES`, for services found by name).
#[test]
fn every_crate_below_the_sdk_is_kept() {
    let listed = crates();
    if listed.is_empty() {
        return;
    }
    let named: Vec<String> = paperpaw_sdk::native::CROSSING_CRATES
        .iter()
        .map(|c| c.replace('_', "-"))
        .collect();
    assert_eq!(
        named, listed,
        "native::CROSSING_CRATES and interface/crates.txt"
    );
    for crate_ in &listed {
        let manifest = read(&format!("../{crate_}/Cargo.toml")).unwrap();
        let dependencies = manifest
            .split("\n[dependencies]\n")
            .nth(1)
            .map(|rest| rest.split("\n[").next().unwrap_or_default())
            .unwrap_or_default();
        for line in dependencies.lines() {
            let name = line.split(['.', '=', ' ']).next().unwrap_or_default();
            if name.starts_with("paperpaw-") {
                assert!(
                    listed.iter().any(|l| l == name),
                    "{crate_} depends on {name}: add it to interface/crates.txt"
                );
            }
        }
    }
}

/// The compilers the SDK names for the chips are the release's, and every
/// plugin project of the repository builds with the X4's.
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
    for project in ["../../samples/hello-native", "../../tools/plugin-files"] {
        let pinned = value(
            &read(&format!("{project}/rust-toolchain.toml")).unwrap(),
            "channel",
        );
        assert_eq!(pinned, x4, "{project}/rust-toolchain.toml");
    }
    // The nightly that writes api.txt and shape.txt is CI's.
    let script = read("../../tools/interface.py").unwrap();
    let ci = read("../../.github/workflows/ci.yml").unwrap();
    let nightly = value(&script, "NIGHTLY");
    assert!(
        ci.contains(&format!("NIGHTLY: {nightly}")),
        "tools/interface.py and ci.yml name the same nightly"
    );
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

/// What the gate takes as new and what as changed.
#[test]
fn only_what_cannot_matter_to_a_plugin_is_free() {
    let kept_shape = "\
struct a::S { x: u8 }
enum a::E { One }
trait a::T { function f }
";
    let kept = types(kept_shape);
    for line in [
        "pub fn a::free() -> u8",
        "pub fn a::S::new() -> a::S",
        "pub const a::LIMIT: u8",
        "pub const fn a::S::make() -> a::S",
        "pub struct a::New",
        "pub a::New::y: u16",
        "pub a::Fresh::Variant",
        "#[non_exhaustive] pub enum a::Fresh",
        "impl core::fmt::Debug for a::S",
        "pub use a::S",
        "pub mod a::m",
    ] {
        assert!(is_free(line, &kept), "{line}");
    }
    for line in [
        "pub a::S::y: u16",
        "pub a::E::Two",
        "#[non_exhaustive] pub a::E::Two(u8)",
        "pub fn a::T::g(&self)",
        "pub const a::T::C: u8",
        "pub type a::T::Out",
        "something unknown",
    ] {
        assert!(!is_free(line, &kept), "{line}");
    }
    // A field moved, a private one added, the order of variants.
    let found = changes(
        "a",
        ("", kept_shape),
        (
            "",
            "struct a::S { y: u8, x: u8 }\nenum a::E { One }\ntrait a::T { function f }\n",
        ),
    );
    assert_eq!(found.breaking.len(), 1);
    let found = changes("a", ("pub fn a::gone()", kept_shape), ("", kept_shape));
    assert_eq!(found.breaking.len(), 1);
}
