#![expect(
    clippy::expect_used,
    reason = "a build script that cannot go on fails at once"
)]

//! Tells the shells which plugin crates this build of the app has
//! (`DEP_PAPERPAW_APP_CRATES` in their build scripts, by `links`): every
//! `paperpaw-plugin-*` dependency that is not optional, and the optional
//! ones of the features switched on. The X4's firmware writes that list
//! into its image (ADR 0022); its `Cargo.lock` cannot say it, as a lock
//! file names optional dependencies whether they are built or not.

fn main() {
    println!("cargo::rerun-if-changed=Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    // As checked out on Windows, too.
    let manifest = manifest.replace("\r\n", "\n");
    let section = |name: &str| {
        manifest
            .split(&format!("\n[{name}]\n"))
            .nth(1)
            .map_or("", |rest| rest.split("\n[").next().unwrap_or(rest))
    };
    let mut crates: Vec<String> = section("dependencies")
        .lines()
        .filter(|line| !line.contains("optional = true"))
        .filter_map(|line| line.split(['.', ' ', '=']).next())
        .filter(|name| name.starts_with("paperpaw-plugin-"))
        .map(String::from)
        .collect();
    // `feature = ["dep:…", …]`, perhaps over several lines.
    let mut on = false;
    for line in section("features").lines() {
        if let Some((name, _)) = line.split_once(" = [") {
            let variable = format!(
                "CARGO_FEATURE_{}",
                name.trim().to_uppercase().replace('-', "_")
            );
            on = std::env::var_os(variable).is_some();
        }
        if on {
            for part in line.split("\"dep:").skip(1) {
                if let Some(name) = part.split('"').next()
                    && name.starts_with("paperpaw-plugin-")
                {
                    crates.push(String::from(name));
                }
            }
        }
    }
    crates.sort_unstable();
    crates.dedup();
    let list = crates.join(",");
    println!("cargo::metadata=crates={list}");
    println!("cargo::rustc-env=PAPERPAW_APP_CRATES={list}");
}
