//! Lookups in a real dictionary, which is not part of the repository:
//! `PAPERPAW_STARDICT=/path/to/name.ifo cargo test -p paperpaw-stardict -- --ignored`
//! prints the definitions of the words in `PAPERPAW_STARDICT_WORDS`
//! (comma-separated) and how long preparing and looking up took.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Instant;

use paperpaw_fs::DirStorage;
use paperpaw_stardict::Dictionary;

#[test]
#[ignore = "needs a dictionary: set PAPERPAW_STARDICT"]
fn real_dictionary() {
    let ifo = std::env::var("PAPERPAW_STARDICT").expect("PAPERPAW_STARDICT");
    let ifo = std::path::Path::new(&ifo);
    let storage = DirStorage::new(ifo.parent().unwrap()).unwrap();
    let name = ifo.file_name().unwrap().to_str().unwrap();
    let dictionary = Dictionary::open(&storage, name, ".paperpaw-test-cache").unwrap();
    println!(
        "{}: {} words",
        dictionary.info.name, dictionary.info.word_count
    );
    let start = Instant::now();
    for mut builder in dictionary.builders(&storage) {
        while !builder.step(&storage, 64 * 1024).unwrap() {}
    }
    println!("prepared in {:?}", start.elapsed());
    let words = std::env::var("PAPERPAW_STARDICT_WORDS").unwrap_or_else(|_| "hus".into());
    for word in words.split(',') {
        let start = Instant::now();
        let found = dictionary.lookup(&storage, word).unwrap();
        println!("--- {word} ({:?}): {} found", start.elapsed(), found.len());
        for definition in found {
            println!("[{}]\n{}", definition.word, definition.text);
        }
    }
}
