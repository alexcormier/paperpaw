//! `status-lines.txt`, the status lines chosen last (top, then bottom):
//! version 1 stays readable, what is saved is read back, and a file that
//! is damaged or missing yields a choice without a panic (an unknown line
//! falls back to the first of the rotation, see `current`).

use alloc::string::{String, ToString};

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;

#[test]
fn the_choice_of_version_1_is_read() {
    let storage = MemoryStorage::new().with(
        &choice_path(),
        include_str!("../../testdata/status-lines.txt.v1"),
    );
    assert_eq!(
        load_choice(&storage),
        [
            String::from("reader.status-line.empty"),
            String::from("reader.status-line.progress")
        ]
    );
}

#[test]
fn the_writer_still_writes_version_1() {
    let storage = MemoryStorage::new();
    let chosen = [
        String::from("reader.status-line.empty"),
        String::from("reader.status-line.progress"),
    ];
    save_choice(&storage, &chosen);
    let written = storage.get(&choice_path()).unwrap();
    let v1 = include_str!("../../testdata/status-lines.txt.v1").replace("\r\n", "\n");
    assert_eq!(written, v1.as_bytes());
}

#[test]
fn no_file_is_no_choice() {
    assert_eq!(
        load_choice(&MemoryStorage::new()),
        [String::new(), String::new()]
    );
}

#[test]
fn a_file_too_large_is_no_choice() {
    let storage = MemoryStorage::new().with(&choice_path(), "x".repeat(5000));
    assert_eq!(load_choice(&storage), [String::new(), String::new()]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn a_choice_saved_is_read_back(top in "[a-z.\\-]{0,40}", bottom in "[a-z.\\-]{0,40}") {
        let storage = MemoryStorage::new();
        save_choice(&storage, &[top.clone(), bottom.clone()]);
        prop_assert_eq!(load_choice(&storage), [top, bottom]);
    }

    #[test]
    fn any_bytes_are_read_without_a_panic(data in proptest::collection::vec(any::<u8>(), 0..1100)) {
        let storage = MemoryStorage::new();
        storage.write(&choice_path(), &data).unwrap();
        let [top, bottom] = load_choice(&storage);
        prop_assert!(!top.contains('\n') && !bottom.contains('\n'));
        prop_assert_eq!(top.trim(), top.to_string());
    }
}
