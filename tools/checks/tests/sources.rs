//! PaperPaw's Rust files: none longer than [`MAX_LINES`] but those listed
//! in `big-files.txt`, which may not grow and leave the list once they are
//! split; none with control characters other than tabs and line ends
//! (write `\0` and the like as escapes).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeMap;

use paperpaw_checks::{control_characters, lines, root, rust_files};

/// The most lines a Rust file has: split it where it gets longer.
const MAX_LINES: usize = 1500;

/// The files longer than [`MAX_LINES`] from before the limit, each with the
/// most lines it may have.
fn listed() -> BTreeMap<String, usize> {
    include_str!("../big-files.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (count, path) = line.split_once(' ').expect("`<lines> <path>`");
            (
                path.trim().to_string(),
                count.parse().expect("a number of lines"),
            )
        })
        .collect()
}

#[test]
fn rust_files_stay_short() {
    let listed = listed();
    let mut wrong = Vec::new();
    let mut seen = Vec::new();
    for (path, file) in rust_files(&root()) {
        let count = lines(&std::fs::read(&file).expect("readable"));
        match listed.get(&path) {
            Some(&most) if count > most => wrong.push(format!(
                "{path}: {count} lines, more than the {most} it had; split it rather"
            )),
            Some(_) if count <= MAX_LINES => wrong.push(format!(
                "{path}: {count} lines, within the limit now; take it out of tools/checks/big-files.txt"
            )),
            Some(_) => seen.push(path),
            None if count > MAX_LINES => wrong.push(format!(
                "{path}: {count} lines, more than {MAX_LINES}; split it into modules"
            )),
            None => {}
        }
    }
    for path in listed.keys().filter(|p| !seen.contains(p)) {
        if !wrong.iter().any(|w| w.starts_with(path.as_str())) {
            wrong.push(format!(
                "{path}: listed in tools/checks/big-files.txt but gone; take it out"
            ));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

#[test]
fn rust_files_have_no_control_characters() {
    let mut found = Vec::new();
    for (path, file) in rust_files(&root()) {
        for line in control_characters(&std::fs::read(&file).expect("readable")) {
            found.push(format!("{path}:{line}"));
        }
    }
    assert!(
        found.is_empty(),
        "control characters (write them as escapes, e.g. \\0):\n{}",
        found.join("\n")
    );
}
