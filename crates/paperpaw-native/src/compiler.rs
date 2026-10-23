//! The compiler a plugin was built with, as its file names it
//! ([`Header::compiler`](crate::Header)): until PaperPaw 1.0 Rust types
//! cross between plugin and firmware as they are, and another compiler
//! may lay them out or call them otherwise in ways no size shows, so a
//! plugin runs only on firmware built by the same compiler (ADR 0026).
//!
//! Rust writes its release into every object it makes, as
//! `rustc version 1.99.0 (b940084d7 2026-09-28)`, kept in the ELF section
//! `.comment`; `rustc -vV` says the same as `rustc 1.99.0 (…)`. Both
//! come down to the release and the commit in brackets ([`release`]),
//! and a file keeps a hash of that ([`id`]).

/// What Rust writes into the objects it makes, before its release.
pub const MARK: &str = "rustc version ";

/// The release and commit of a compiler's own words, `rustc version 1.99.0
/// (b940084d7 2026-09-28)` in an object or `rustc 1.99.0 (b940084d7
/// 2026-09-28)` from `rustc -V`: `1.99.0 (b940084d7 2026-09-28)`, to the
/// first closing bracket (espup's toolchain adds another).
pub fn release(words: &str) -> Option<&str> {
    let words = words.trim();
    let rest = words
        .strip_prefix(MARK)
        .or_else(|| words.strip_prefix("rustc "))?;
    let end = rest.find(')')?;
    let release = &rest[..=end];
    (release.contains(" (") && !release.starts_with(' ')).then_some(release)
}

/// The hash a plugin file keeps of a compiler's [`release`]: FNV-1a with
/// 32 bits, never 0 (which no file has).
pub const fn id(release: &str) -> u32 {
    let bytes = release.as_bytes();
    let mut hash: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    if hash == 0 { 1 } else { hash }
}

/// The releases of the compilers named in `bytes` (an ELF section
/// `.comment`, or a whole library), each once, in the order found.
pub fn named_in(bytes: &[u8]) -> alloc::vec::Vec<&str> {
    let mark = MARK.as_bytes();
    let mut found = alloc::vec::Vec::new();
    let mut at = 0;
    while let Some(start) = find(&bytes[at..], mark) {
        let from = at + start;
        let end = bytes[from..]
            .iter()
            .position(|&b| b == 0 || b == b'\n')
            .map_or(bytes.len(), |n| from + n);
        if let Some(release) = core::str::from_utf8(&bytes[from..end])
            .ok()
            .and_then(release)
            && !found.contains(&release)
        {
            found.push(release);
        }
        at = from + mark.len();
    }
    found
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_and_rustc_name_the_same_release() {
        let object = "rustc version 1.99.0 (b940084d7 2026-09-28)";
        let asked = "rustc 1.99.0 (b940084d7 2026-09-28)\n";
        assert_eq!(release(object), Some("1.99.0 (b940084d7 2026-09-28)"));
        assert_eq!(release(object), release(asked));
        // espup's adds its own version after it.
        assert_eq!(
            release("rustc 1.97.0-nightly (8ea53bcd7 2026-07-08) (1.97.0.0)"),
            Some("1.97.0-nightly (8ea53bcd7 2026-07-08)")
        );
        for words in [
            "",
            "rustc",
            "rustc 1.99.0",
            "gcc 14.2",
            "rustc version  (x)",
        ] {
            assert_eq!(release(words), None, "{words:?}");
        }
        assert_ne!(
            id("1.99.0 (b940084d7 2026-09-28)"),
            id("1.99.1 (b940084d7 2026-09-28)")
        );
        assert_ne!(id(""), 0);
    }

    #[test]
    fn the_releases_named_in_a_section() {
        let section = b"Linker: LLD 21.1.0\0rustc version 1.99.0 (b940084d7 2026-09-28)\0\
            rustc version 1.99.0 (b940084d7 2026-09-28)\0GCC: (crosstool-NG) 14.2.0\0\
            rustc version 1.98.0 (aaaaaaaaa 2026-08-01)\0";
        assert_eq!(
            named_in(section),
            [
                "1.99.0 (b940084d7 2026-09-28)",
                "1.98.0 (aaaaaaaaa 2026-08-01)"
            ]
        );
        assert!(named_in(b"nothing of the kind").is_empty());
    }
}
