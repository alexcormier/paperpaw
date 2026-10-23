//! Tells the emulator the commit of PaperPaw's source it is built from,
//! and whether that commit is the release of its version (tagged
//! `v<version>`), for `--stdio`'s `ready`: tools pin `paperpaw-sdk` to
//! exactly that source. `PAPERPAW_COMMIT` and `PAPERPAW_RELEASE` (`true`
//! or `false`) say it where there is no Git (the release workflow sets
//! them); without either, neither is known.

use std::process::Command;

/// What `git` says in this folder, trimmed; `None` where it says nothing.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

fn main() {
    println!("cargo::rerun-if-env-changed=PAPERPAW_COMMIT");
    println!("cargo::rerun-if-env-changed=PAPERPAW_RELEASE");
    let tag = format!(
        "v{}",
        std::env::var("CARGO_PKG_VERSION").unwrap_or_default()
    );
    let commit = std::env::var("PAPERPAW_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]));
    let release = match std::env::var("PAPERPAW_RELEASE").ok().as_deref() {
        Some("true") => true,
        Some(_) => false,
        None => {
            git(&["tag", "--points-at", "HEAD"]).is_some_and(|tags| tags.lines().any(|t| t == tag))
        }
    };
    // Built again when the checkout moves on or is tagged; only what is
    // there (a path that is not makes Cargo build again every time).
    let mut watched = vec![
        String::from("HEAD"),
        String::from("packed-refs"),
        String::from("refs/tags"),
    ];
    watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name])
            && std::path::Path::new(&path).exists()
        {
            println!("cargo::rerun-if-changed={path}");
        }
    }
    println!(
        "cargo::rustc-env=PAPERPAW_SOURCE_COMMIT={}",
        commit.unwrap_or_default()
    );
    println!("cargo::rustc-env=PAPERPAW_SOURCE_RELEASE={release}");
}
