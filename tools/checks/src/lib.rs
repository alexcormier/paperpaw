//! Checks of PaperPaw's own source, as tests (`cargo test -p
//! paperpaw-checks`, and with the workspace's): see `tests/sources.rs`.
//! What these checks find may only get less: a file over the limit that is
//! listed may not grow, and one that shrank below it leaves the list.

use std::path::{Path, PathBuf};

/// The repository's top folder.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Folders left out: built files, others' code (patched crates, fuzz
/// targets built on their own), tools' folders.
const SKIPPED: [&str; 6] = [
    ".git",
    ".claude",
    "target",
    "patches",
    "fuzz",
    "node_modules",
];

/// Every Rust file of the repository below `root`, by its path from there
/// with `/` between folders, sorted.
pub fn rust_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !SKIPPED.contains(&name.as_str()) && !name.starts_with("target") {
                    folders.push(path);
                }
            } else if name.ends_with(".rs") {
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((relative, path));
            }
        }
    }
    found.sort();
    found
}

/// The lines of `text`, a last one without its line end counted too.
pub fn lines(text: &[u8]) -> usize {
    let ends = text.iter().filter(|b| **b == b'\n').count();
    ends + usize::from(text.last().is_some_and(|b| *b != b'\n'))
}

/// Where `text` has a control character other than a tab or a line end:
/// line numbers from 1.
pub fn control_characters(text: &[u8]) -> Vec<usize> {
    text.split(|b| *b == b'\n')
        .enumerate()
        .filter(|(_, line)| {
            line.iter()
                .any(|b| (*b < 0x20 && *b != b'\t' && *b != b'\r') || *b == 0x7F)
        })
        .map(|(at, _)| at + 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_counted_with_or_without_a_last_line_end() {
        assert_eq!(lines(b""), 0);
        assert_eq!(lines(b"a\nb\n"), 2);
        assert_eq!(lines(b"a\nb"), 2);
    }

    #[test]
    fn tabs_and_line_ends_are_no_control_characters() {
        assert!(control_characters(b"a\tb\r\nc\n").is_empty());
        assert_eq!(control_characters(b"a\nb\0c\nd\x7f"), [2, 3]);
    }
}
