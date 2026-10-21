#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! What native code plugins are built with (`native::FINGERPRINT`, ADR
//! 0024): plugin and firmware agree on the layout of every SDK type only
//! when the same compiler built them for the same target from the same
//! SDK with the same features. Hashed here:
//!
//! - the compiler: its release, commit and LLVM (not the computer it runs
//!   on: a plugin built on Linux fits a firmware built on Windows);
//! - the target;
//! - the SDK's version and its features, but those that only add
//!   functions and modules (`ADDING_ONLY`): a firmware built with them
//!   (the kernel's, the reader's) takes plugins built without;
//! - for a version still in work (`0.5.0-dev`, many states of the source
//!   share it), the source of the SDK and of the PaperPaw crates it is
//!   built from, line ends ignored.
//!
//! The features of crates below the SDK that change layouts are added in
//! `native.rs`, where they can be seen.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Features of the SDK that add functions and modules, and change no type
/// and no layout: `kernel`, what the kernel and the shells call (ADR
/// 0020), and `unstable`, the building blocks before 1.0 (`layout`, `zip`,
/// the reader's typesetting). Others are hashed, also those to come.
const ADDING_ONLY: &[&str] = &["kernel", "unstable"];

fn main() {
    let rustc = std::env::var("RUSTC").expect("RUSTC");
    let output = Command::new(&rustc).arg("-vV").output().expect("rustc -vV");
    let about = String::from_utf8_lossy(&output.stdout).into_owned();
    let field = |name: &str| {
        about
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .map_or("", str::trim)
            .to_string()
    };
    let target = std::env::var("TARGET").expect("TARGET");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut features: Vec<String> = std::env::vars()
        .filter_map(|(key, _)| key.strip_prefix("CARGO_FEATURE_").map(str::to_lowercase))
        .filter(|feature| !ADDING_ONLY.contains(&feature.as_str()))
        .collect();
    features.sort();

    let mut hash = Fnv::new();
    for part in [
        field("release:"),
        field("commit-hash:"),
        field("LLVM version:"),
        target.clone(),
        version.clone(),
        features.join(","),
    ] {
        hash.add(part.as_bytes());
        hash.add(&[0]);
    }
    if version.contains('-') {
        let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
        for krate in closure(&dir) {
            hash_crate(&krate, &mut hash);
        }
    }
    let toolchain = about.lines().next().unwrap_or("rustc").trim();
    println!("cargo::rustc-env=PAPERPAW_NATIVE_BUILD={:016x}", hash.0);
    println!("cargo::rustc-env=PAPERPAW_NATIVE_TOOLCHAIN={toolchain}, {target}");
    println!("cargo::rerun-if-env-changed=RUSTC");
}

/// FNV-1a with 64 bits (as `paperpaw_digest::fnv1a64`).
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn add(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// The SDK's folder and those of the PaperPaw crates it is built from,
/// found beside it as in the repository, by name.
fn closure(sdk: &Path) -> Vec<PathBuf> {
    let Some(parent) = sdk.parent() else {
        return vec![sdk.to_path_buf()];
    };
    let mut found = vec![sdk.to_path_buf()];
    let mut next = 0;
    while next < found.len() {
        for name in dependencies(&found[next]) {
            let dir = parent.join(&name);
            if dir.join("Cargo.toml").is_file() && !found.contains(&dir) {
                found.push(dir);
            }
        }
        next += 1;
    }
    found.sort();
    found
}

/// The `paperpaw-*` crates in a crate's `[dependencies]`.
fn dependencies(dir: &Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_dependencies = line == "[dependencies]";
        } else if in_dependencies && line.starts_with("paperpaw-") {
            let name: String = line
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            names.push(name);
        }
    }
    names
}

/// Adds a crate's manifest, build script, linker scripts and sources, in
/// a fixed order, and asks to be run again when they change.
fn hash_crate(dir: &Path, hash: &mut Fnv) {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_file() && (name == "Cargo.toml" || name == "build.rs" || name.ends_with(".x")) {
            files.push(path);
        }
    }
    sources(&dir.join("src"), &mut files);
    files.sort();
    println!("cargo::rerun-if-changed={}", dir.join("src").display());
    for file in files {
        println!("cargo::rerun-if-changed={}", file.display());
        let relative = file
            .strip_prefix(dir.parent().unwrap_or(dir))
            .unwrap_or(&file);
        let name = relative.to_string_lossy().replace('\\', "/");
        hash.add(name.as_bytes());
        hash.add(&[0]);
        let bytes = std::fs::read(&file).expect("a source file");
        let text: Vec<u8> = bytes.into_iter().filter(|&b| b != b'\r').collect();
        hash.add(&text);
        hash.add(&[0]);
    }
}

fn sources(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, files);
        } else {
            files.push(path);
        }
    }
}
