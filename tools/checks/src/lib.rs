//! Checks of PaperPaw's own source, as tests (`cargo test -p
//! paperpaw-checks`, and with the workspace's): see `tests/sources.rs`
//! and, for the SDK's design rules, `tests/sdk.rs`.
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

/// Methods whose result may be assigned through: they lend the value
/// itself (`&mut`), not a copy of it.
const LENDING: [&str; 5] = ["borrow_mut", "unwrap", "expect", "lock", "extra"];

/// Where `text` assigns to a field of what a call returned
/// (`list.area().y = 3`), with the method's name: line numbers from 1. A
/// method that returns a copy makes the assignment change nothing, and
/// neither the compiler nor clippy says so. Calls of methods that lend the
/// value itself are left out: those ending in `_mut` and those of
/// `LENDING`.
pub fn assigned_through_calls(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        let bytes = code.as_bytes();
        for (i, _) in code.match_indices(").") {
            // Not inside a string (an odd number of quotes before).
            if code[..i].replace("\\\"", "").matches('"').count() % 2 == 1 {
                continue;
            }
            // The field, then an assignment (not a comparison).
            let rest = &code[i + 2..];
            let field_len = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            if field_len == 0 || rest.as_bytes()[0].is_ascii_digit() {
                continue;
            }
            let after = rest[field_len..].trim_start();
            if after.starts_with('(') || after.starts_with('.') {
                continue;
            }
            let assigns = ["=", "+=", "-=", "*=", "/=", "|=", "&="]
                .iter()
                .any(|op| after.starts_with(op) && !after[op.len()..].starts_with('='))
                && !after.starts_with("==")
                && !after.starts_with("=>");
            if !assigns {
                continue;
            }
            // The call's own name, before its parenthesis.
            let mut depth = 0usize;
            let mut open = None;
            for j in (0..=i).rev() {
                match bytes[j] {
                    b')' => depth += 1,
                    b'(' => {
                        depth -= 1;
                        if depth == 0 {
                            open = Some(j);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(open) = open else {
                continue;
            };
            let name: String = code[..open]
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if name.is_empty() || name.ends_with("_mut") || LENDING.contains(&name.as_str()) {
                continue;
            }
            found.push((at + 1, name));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_of_a_returned_copy_assigned_is_found() {
        let found = |text: &str| assigned_through_calls(text);
        assert_eq!(
            found("    self.list.area().y = y as u16 + gap;"),
            [(1, "area".to_string())]
        );
        assert_eq!(
            found("a.rect(1, (2)).height += 3;"),
            [(1, "rect".to_string())]
        );
        // Lent values, comparisons, calls, tuples and comments pass.
        assert!(found("s.borrow_mut().server = Some(x);").is_empty());
        assert!(found("self.options_mut().timeout = t;").is_empty());
        assert!(found("p.get_mut(\"a\").unwrap().enabled = false;").is_empty());
        assert!(found("if a.area().y == 3 {}").is_empty());
        assert!(found("let y = a.area().y;").is_empty());
        assert!(found("a.area().y.max(3);").is_empty());
        assert!(found("let x = (a, b).0 = 1;").is_empty());
        assert!(found("// a.area().y = 3").is_empty());
        assert!(found("let s = \"a.area().y = 3\";").is_empty());
        assert!(found("match a.kind() { x => 1 }").is_empty());
    }

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
