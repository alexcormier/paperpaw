//! Versioned data formats (`paperpaw_sdk::data::Format`): the version a
//! file names in its first line, upgrades step by step, files of newer
//! versions left alone, and hostile first lines.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::data::{self, Error, Format, MARK};
use paperpaw_sdk::{MemoryStorage, Storage, StorageError};
use proptest::prelude::*;

/// Version 3: each step appends its number, so the steps run are seen.
const FORMAT: Format = Format::new(3, &[|v1| format!("{v1}+2"), |v2| format!("{v2}+3")]);
const PLAIN: Format = Format::new(1, &[]);

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// The version of any text is found without panic, and is 1 unless
    /// the first line says another.
    #[test]
    fn any_first_line_gives_a_version(text in "\\PC{0,30}(\n\\PC{0,10})?") {
        let version = Format::version_of(&text);
        if !text.trim_start().starts_with(MARK.trim_end()) {
            prop_assert_eq!(version, 1);
        }
    }

    /// A body written in this version reads back as it is.
    #[test]
    fn bodies_come_back(body in "\\PC{0,40}(\n\\PC{0,20}){0,3}") {
        // A body whose own first line looks like a mark is no body of
        // version 1.
        prop_assume!(!body.trim_start().starts_with("# format"));
        let storage = MemoryStorage::new();
        for (format, path) in [(FORMAT, "a.txt"), (PLAIN, "b.txt")] {
            format.save(&storage, path, &body).unwrap();
            prop_assert_eq!(format.load(&storage, path, 4096).unwrap(), Some(body.clone()));
        }
    }

    /// Files of any version up to this one are read in this one, each
    /// step run once; newer ones are not read.
    #[test]
    fn every_older_version_is_upgraded(version in 1u32..10, body in "[a-z ]{0,10}") {
        let text = if version == 1 {
            body.clone()
        } else {
            format!("{MARK}{version}\n{body}")
        };
        match FORMAT.read(&text) {
            Ok(read) => {
                prop_assert!(version <= 3);
                let steps: String = (version + 1..=3).map(|n| format!("+{n}")).collect();
                prop_assert_eq!(read.as_ref(), format!("{body}{steps}"));
            }
            Err(error) => {
                prop_assert!(version > 3);
                prop_assert_eq!(error, Error::Newer(version));
            }
        }
    }
}

#[test]
fn marks_with_spaces_and_line_ends_are_read() {
    assert_eq!(Format::version_of("# format 2\r\nx"), 2);
    assert_eq!(Format::version_of("  # format  7  \nx"), 7);
    assert_eq!(Format::version_of("# format 2"), 2);
    // Not a mark: version 1.
    for text in [
        "",
        "\n# format 2",
        "#format 2",
        "# format two",
        "# format -1",
        "# Format 2",
        "x\n# format 2",
    ] {
        assert_eq!(Format::version_of(text), 1, "{text:?}");
    }
    assert_eq!(Format::version_of("# format 4294967296"), 1);
}

#[test]
fn a_mark_without_a_body_is_an_empty_file() {
    assert_eq!(FORMAT.read("# format 3").unwrap(), "");
    assert_eq!(FORMAT.read("# format 2").unwrap(), "+3");
}

#[test]
fn newer_files_are_neither_read_nor_written_over() {
    let storage = MemoryStorage::new().with("a.txt", "# format 9\nnewer\n");
    assert_eq!(FORMAT.load(&storage, "a.txt", 1024), Err(Error::Newer(9)));
    assert_eq!(FORMAT.save(&storage, "a.txt", "x"), Err(Error::Newer(9)));
    assert_eq!(storage.get("a.txt").unwrap(), b"# format 9\nnewer\n");
    // A first line cut off (no line end in the first 32 bytes) does not
    // count: the file is written.
    let long = format!("# format 9{}", " ".repeat(40));
    let storage = MemoryStorage::new().with("b.txt", long);
    FORMAT.save(&storage, "b.txt", "x").unwrap();
    assert_eq!(storage.get("b.txt").unwrap(), b"# format 3\nx");
}

#[test]
fn files_that_are_no_text_or_too_large_are_refused() {
    let storage = MemoryStorage::new()
        .with("bin.txt", b"\xff\xfe".to_vec())
        .with("big.txt", "x".repeat(100));
    assert_eq!(
        FORMAT.load(&storage, "bin.txt", 1024),
        Err(Error::Unreadable)
    );
    assert!(matches!(
        FORMAT.load(&storage, "big.txt", 99),
        Err(Error::Storage(_))
    ));
    assert_eq!(
        FORMAT
            .load(&storage, "big.txt", 100)
            .unwrap()
            .unwrap()
            .len(),
        100 + 4
    );
    assert_eq!(FORMAT.load(&storage, "none.txt", 100), Ok(None));
}

#[test]
fn writing_where_the_storage_refuses_is_an_error() {
    let storage = MemoryStorage::new();
    assert_eq!(
        FORMAT.save(&storage, "../outside.txt", "x"),
        Err(Error::Storage(StorageError::InvalidPath))
    );
}

#[test]
fn the_old_name_is_not_taken_over_where_the_new_one_is_used() {
    let storage = MemoryStorage::new()
        .with("quireink.ini", "[old]\n")
        .with("paperpaw.ini", "[new]\n");
    data::take_over_old_name(&storage);
    assert_eq!(storage.get("paperpaw.ini").unwrap(), b"[new]\n");
    assert!(storage.exists("quireink.ini"));
    // Nothing of the old name: nothing happens.
    let storage = MemoryStorage::new();
    data::take_over_old_name(&storage);
    assert!(!storage.exists(".paperpaw"));
}

/// A file written under the new name before the kernel starts (the X4
/// keeps a crash report or the flasher's Wi-Fi there first) does not keep
/// the rest from being taken over: entry by entry, what the new folder
/// has stays.
#[test]
fn the_old_folder_is_taken_over_beside_a_file_already_in_the_new_one() {
    let storage = MemoryStorage::new()
        .with(".quireink/secrets.txt", "s")
        .with(".quireink/library/books/a.ini", "a")
        .with(".quireink/wifi/known.txt", "old")
        .with(".quireink/crashes.txt", "old crash")
        .with(".paperpaw/crashes.txt", "new crash")
        .with(".paperpaw/wifi/wifi.ini", "flashed");
    data::take_over_old_name(&storage);
    assert_eq!(storage.get(".paperpaw/secrets.txt").unwrap(), b"s");
    assert_eq!(storage.get(".paperpaw/library/books/a.ini").unwrap(), b"a");
    assert_eq!(storage.get(".paperpaw/wifi/known.txt").unwrap(), b"old");
    assert_eq!(storage.get(".paperpaw/wifi/wifi.ini").unwrap(), b"flashed");
    assert_eq!(storage.get(".paperpaw/crashes.txt").unwrap(), b"new crash");
    // What the new folder had is not lost, and the old one keeps its own.
    assert_eq!(storage.get(".quireink/crashes.txt").unwrap(), b"old crash");
}

/// The guard against writing over a newer file reads the first line
/// however its bytes fall: a character cut at the end of what it reads, or
/// a byte order mark before the mark.
#[test]
fn newer_files_are_found_with_a_cut_character_or_a_byte_order_mark() {
    // 31 bytes, then a two-byte character across the 32nd.
    let cut = "# format 9\n[profile.1]\nname = N\u{e4}chte\ncore.x = 1\n";
    assert_eq!(&cut.as_bytes()[..31], b"# format 9\n[profile.1]\nname = N");
    let bom = "\u{feff}# format 9\nx = 1\n";
    let storage = MemoryStorage::new()
        .with("cut.ini", cut)
        .with("bom.ini", bom);
    for (path, text) in [("cut.ini", cut), ("bom.ini", bom)] {
        assert_eq!(FORMAT.load(&storage, path, 1024), Err(Error::Newer(9)));
        assert_eq!(FORMAT.save(&storage, path, "x"), Err(Error::Newer(9)));
        assert_eq!(storage.get(path).unwrap(), text.as_bytes());
    }
}

#[test]
#[should_panic(expected = "one upgrade per version after the first")]
fn a_format_without_its_upgrades_is_refused() {
    let steps: &'static [data::Upgrade] = &[];
    let _ = Format::new(2, steps);
}

/// A file whose first line says version 0 (`# format 0`, e.g. written by
/// hand or damaged) is read by no format, without a panic (its upgrades
/// are not indexed at `0 - 1`).
#[test]
fn a_file_of_version_0_does_not_panic() {
    assert_eq!(FORMAT.read("# format 0\nx"), Err(Error::Unreadable));
    assert_eq!(PLAIN.read("# format 0\nx"), Err(Error::Unreadable));
}
