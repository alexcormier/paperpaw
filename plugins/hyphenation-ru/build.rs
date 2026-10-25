//! Compiles the hyphenation patterns into a table that is read in place.

use std::path::PathBuf;

/// Characters before and after a hyphen that the patterns were made for.
const LEFT: u8 = 2;
const RIGHT: u8 = 2;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=patterns");
    let out = PathBuf::from(
        std::env::var_os("OUT_DIR").unwrap_or_else(|| panic!("OUT_DIR is set by cargo")),
    );
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let exceptions: Option<&str> = Some("patterns/hyph-ru.hyp.txt");
    let table = paperpaw_hyphen::compile(
        &read("patterns/hyph-ru.pat.txt"),
        &exceptions.map(read).unwrap_or_default(),
        LEFT,
        RIGHT,
    );
    let path = out.join("ru.pphp");
    std::fs::write(&path, table).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
