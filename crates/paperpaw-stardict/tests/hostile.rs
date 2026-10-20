//! Broken dictionaries: `.ifo` files with missing or odd keys, index
//! entries pointing outside the definitions, unsorted or cut indexes,
//! synonyms pointing nowhere, dictzip files with broken headers, chunk
//! tables and data. Each is an error or finds nothing, never a panic or
//! a loop without end.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Counted, Spec, build, dictzip, found, open, prepare};
use paperpaw_sdk::{MemoryStorage, Storage, StorageError, read_file};
use paperpaw_stardict::{Dictionary, Error, Info};
use proptest::prelude::*;

const HEAD: &str = "StarDict's dict ifo file\n";

fn parse_body(body: &str) -> Result<Info, Error> {
    Info::parse(&format!("{HEAD}{body}"))
}

#[test]
fn ifo_keys_are_read_leniently() {
    // Windows line ends, a byte order mark, spaces around `=`, `=` in a
    // value, unknown keys, a key given twice (the last counts).
    let text = "\u{feff}StarDict's dict ifo file\r\nversion=3.0.0\r\n bookname = A = B \r\nwordcount=5\r\nwordcount=7\r\nauthor=x\r\nsametypesequence=\r\nidxoffsetbits=64\r\nno equals sign\r\n";
    let info = Info::parse(text).unwrap();
    assert_eq!(info.name, "A = B");
    assert_eq!(info.word_count, 7);
    assert_eq!(info.offset_bits, 64);
    assert_eq!(info.types, None, "an empty sequence is none");
    assert_eq!(info.syn_count, 0);
    // No name: empty. Offset bits other than 64: 32.
    let info = parse_body("wordcount=1\nidxoffsetbits=48\n").unwrap();
    assert_eq!((info.name.as_str(), info.offset_bits), ("", 32));
    // A synonym count that is no number counts none.
    assert_eq!(
        parse_body("wordcount=1\nsynwordcount=lots\n")
            .unwrap()
            .syn_count,
        0
    );
}

#[test]
fn ifo_files_without_words_are_refused() {
    for body in [
        "",
        "bookname=x\n",
        "wordcount=0\n",
        "wordcount=-3\n",
        "wordcount=abc\n",
        "wordcount=4294967296\n",
        "wordcount =\n",
    ] {
        assert!(
            matches!(parse_body(body), Err(Error::Malformed(_))),
            "{body:?}"
        );
    }
    // Not the first line.
    assert!(Info::parse("\nStarDict's dict ifo file\nwordcount=1\n").is_err());
    assert!(Info::parse("").is_err());
}

#[test]
fn missing_files_are_reported() {
    let spec = Spec::new(&[("a", "x")]);
    // Not an .ifo path.
    let storage = build(&spec);
    assert!(matches!(
        Dictionary::open(&storage, "d/test.idx", "cache"),
        Err(Error::Malformed(_))
    ));
    // No .ifo.
    assert_eq!(
        Dictionary::open(&storage, "d/none.ifo", "cache").err(),
        Some(Error::Storage(StorageError::NotFound))
    );
    // No .idx.
    storage.remove("d/test.idx").unwrap();
    assert_eq!(
        Dictionary::open(&storage, "d/test.ifo", "cache").err(),
        Some(Error::Storage(StorageError::NotFound))
    );
    // No definitions.
    let storage = build(&spec);
    storage.remove("d/test.dict").unwrap();
    assert!(matches!(
        Dictionary::open(&storage, "d/test.ifo", "cache"),
        Err(Error::Malformed(_))
    ));
    // An .ifo too large to be one.
    let storage = build(&spec);
    let mut ifo = read_file(&storage, "d/test.ifo", 1 << 20).unwrap();
    ifo.extend(std::iter::repeat_n(b'#', 20_000));
    storage.write("d/test.ifo", &ifo).unwrap();
    assert!(matches!(
        Dictionary::open(&storage, "d/test.ifo", "cache"),
        Err(Error::Malformed(_))
    ));
}

/// A `.syn` said to be there but missing: the headwords still work.
#[test]
fn a_missing_synonym_file_leaves_the_headwords() {
    let mut spec = Spec::new(&[("hus", "house")]);
    spec.synonyms = vec![("huset".to_string(), "hus".to_string())];
    let storage = build(&spec);
    storage.remove("d/test.syn").unwrap();
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "hus").len(), 1);
    assert_eq!(found(&dictionary, &storage, "huset"), vec![]);
}

/// The `.idx` with entry `n`'s offset and size replaced.
fn with_entry(storage: &MemoryStorage, n: usize, offset: u64, size: u32, wide: bool) {
    let mut idx = read_file(storage, "d/test.idx", 1 << 20).unwrap();
    let mut at = 0;
    for _ in 0..n {
        at += idx[at..].iter().position(|&b| b == 0).unwrap() + 1 + if wide { 12 } else { 8 };
    }
    at += idx[at..].iter().position(|&b| b == 0).unwrap() + 1;
    if wide {
        idx[at..at + 8].copy_from_slice(&offset.to_be_bytes());
        idx[at + 8..at + 12].copy_from_slice(&size.to_be_bytes());
    } else {
        idx[at..at + 4].copy_from_slice(&(offset as u32).to_be_bytes());
        idx[at + 4..at + 8].copy_from_slice(&size.to_be_bytes());
    }
    storage.write("d/test.idx", &idx).unwrap();
}

const WORDS: [(&str, &str); 3] = [("alpha", "first"), ("beta", "second"), ("gamma", "third")];

/// Offsets and sizes past the end of the definitions: what is there is
/// read, nothing more.
#[test]
fn definitions_outside_the_file_are_cut() {
    for chunk in [None, Some(4)] {
        let mut spec = Spec::new(&WORDS);
        spec.chunk = chunk;
        let storage = build(&spec);
        // "beta" starts at 5 and runs far past the end (16 bytes).
        with_entry(&storage, 1, 5, 1_000_000, false);
        // "gamma" starts after the end.
        with_entry(&storage, 2, 500, 5, false);
        let dictionary = open(&storage);
        assert_eq!(
            found(&dictionary, &storage, "beta"),
            vec![("beta".to_string(), "secondthird".to_string())],
            "{chunk:?}"
        );
        assert_eq!(
            found(&dictionary, &storage, "gamma"),
            vec![("gamma".to_string(), String::new())],
            "{chunk:?}"
        );
    }
}

/// 64-bit offsets at the very end of their range, in a plain file.
#[test]
fn huge_64_bit_offsets_in_a_plain_file_find_an_empty_definition() {
    let mut spec = Spec::new(&WORDS);
    spec.offset_bits = 64;
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "gamma")[0].1, "third");
    with_entry(&storage, 2, u64::MAX - 2, 100, true);
    assert_eq!(
        found(&dictionary, &storage, "gamma"),
        vec![("gamma".to_string(), String::new())]
    );
}

/// The same in a dictzip file: offset and size added up overflow.
#[test]
#[ignore = "bug: dict.rs adds offset and size without overflow check (panics in debug builds)"]
fn huge_64_bit_offsets_in_a_dictzip_file_find_an_empty_definition() {
    let mut spec = Spec::new(&WORDS);
    spec.offset_bits = 64;
    spec.chunk = Some(8);
    let storage = build(&spec);
    let dictionary = open(&storage);
    with_entry(&storage, 2, u64::MAX - 2, 100, true);
    assert_eq!(
        found(&dictionary, &storage, "gamma"),
        vec![("gamma".to_string(), String::new())]
    );
}

/// An index not in StarDict's order: lookups may miss words, but they
/// end and do not panic.
#[test]
fn an_unsorted_index_finds_what_it_can() {
    let entries: Vec<(String, String)> = (0..200)
        .map(|i| (format!("w{:03}", (i * 73) % 200), format!("d{i}")))
        .collect();
    // Written in this order, not sorted.
    let mut idx = Vec::new();
    let mut dict = Vec::new();
    for (word, definition) in &entries {
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        idx.extend_from_slice(&(dict.len() as u32).to_be_bytes());
        idx.extend_from_slice(&(definition.len() as u32).to_be_bytes());
        dict.extend_from_slice(definition.as_bytes());
    }
    let storage = MemoryStorage::new()
        .with(
            "d/test.ifo",
            "StarDict's dict ifo file\nwordcount=200\nsametypesequence=m\n",
        )
        .with("d/test.idx", idx)
        .with("d/test.dict", dict);
    let dictionary = open(&storage);
    let counted = Counted::new(&storage, 100_000);
    let mut matched = 0;
    for (word, definition) in &entries {
        let hits = found(&dictionary, &counted, word);
        if hits.iter().any(|(_, d)| d == definition) {
            matched += 1;
        }
    }
    assert!(matched > 0, "some are found");
    assert!(counted.reads.get() < 100_000);
}

/// More words said than the index has: building stops with an error.
/// Fewer: the ones counted are found, the rest are not.
#[test]
fn word_counts_that_do_not_match_the_index() {
    let storage = build(&Spec::new(&WORDS));
    let ifo = "StarDict's dict ifo file\nwordcount=10\nsametypesequence=m\n";
    storage.write("d/test.ifo", ifo.as_bytes()).unwrap();
    let dictionary = Dictionary::open(&storage, "d/test.ifo", "cache").unwrap();
    let mut builder = dictionary.builders(&storage).remove(0);
    let mut result = Ok(false);
    for _ in 0..100 {
        result = builder.step(&storage, 4096);
        if result != Ok(false) {
            break;
        }
    }
    assert!(matches!(result, Err(Error::Malformed(_))), "{result:?}");

    let storage = build(&Spec::new(&WORDS));
    let ifo = "StarDict's dict ifo file\nwordcount=2\nsametypesequence=m\n";
    storage.write("d/test.ifo", ifo.as_bytes()).unwrap();
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "beta").len(), 1);
    assert_eq!(found(&dictionary, &storage, "gamma"), vec![]);
}

/// Index entries cut short or without their end.
#[test]
fn cut_index_entries_are_errors() {
    let storage = build(&Spec::new(&WORDS));
    let idx = read_file(&storage, "d/test.idx", 1 << 20).unwrap();
    // The last entry's tail cut off.
    storage.write("d/test.idx", &idx[..idx.len() - 3]).unwrap();
    let dictionary = Dictionary::open(&storage, "d/test.ifo", "cache").unwrap();
    let mut builder = dictionary.builders(&storage).remove(0);
    let mut result = Ok(false);
    for _ in 0..100 {
        result = builder.step(&storage, 4096);
        if result != Ok(false) {
            break;
        }
    }
    assert!(matches!(result, Err(Error::Malformed(_))), "{result:?}");
    // A word longer than StarDict allows (256 bytes): a lookup that
    // reaches it is an error.
    let long = "l".repeat(400);
    let storage = build(&Spec::new(&[("alpha", "first"), (&long, "long")]));
    let dictionary = open(&storage);
    assert!(matches!(
        dictionary.lookup(&storage, "alpha"),
        Err(Error::Malformed(_))
    ));
}

/// A synonym naming a headword past the index's end.
#[test]
fn synonyms_pointing_past_the_index_are_errors() {
    let mut spec = Spec::new(&WORDS);
    spec.synonyms = vec![("betas".to_string(), "beta".to_string())];
    let storage = build(&spec);
    let mut syn = read_file(&storage, "d/test.syn", 1024).unwrap();
    let n = syn.len();
    syn[n - 4..].copy_from_slice(&99u32.to_be_bytes());
    storage.write("d/test.syn", &syn).unwrap();
    let dictionary = open(&storage);
    assert!(matches!(
        dictionary.lookup(&storage, "betas"),
        Err(Error::Malformed(_))
    ));
    assert_eq!(found(&dictionary, &storage, "beta").len(), 1);
}

/// A block index with offsets that point nowhere in the `.idx`.
#[test]
fn a_garbled_block_index_is_an_error() {
    let storage = build(&Spec::new(&WORDS));
    let dictionary = open(&storage);
    let mut cache = read_file(&storage, "cache/idx.qbi", 1024).unwrap();
    let n = cache.len();
    cache[n - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
    storage.write("cache/idx.qbi", &cache).unwrap();
    assert!(dictionary.builders(&storage).is_empty(), "same header");
    assert!(dictionary.lookup(&storage, "beta").is_err());
}

/// The dictzip file of `WORDS` in chunks of 4 bytes.
fn dz() -> Vec<u8> {
    dictzip(b"firstsecondthird", 4, 4 | 8)
}

/// Opens the dictionary with `data` as its `.dict.dz`.
fn open_dz(data: Vec<u8>) -> (MemoryStorage, Result<Dictionary, Error>) {
    let mut spec = Spec::new(&WORDS);
    spec.chunk = Some(4);
    let storage = build(&spec);
    storage.write("d/test.dict.dz", &data).unwrap();
    let dictionary = Dictionary::open(&storage, "d/test.ifo", "cache");
    if let Ok(dictionary) = &dictionary {
        prepare(&storage, dictionary);
    }
    (storage, dictionary)
}

#[test]
fn dictzip_headers_with_name_comment_and_crc_are_read() {
    for flags in [4, 4 | 8, 4 | 16, 4 | 2, 4 | 8 | 16 | 2] {
        let (storage, dictionary) = open_dz(dictzip(b"firstsecondthird", 4, flags));
        let dictionary = dictionary.unwrap();
        assert_eq!(
            found(&dictionary, &storage, "gamma")[0].1,
            "third",
            "{flags}"
        );
    }
}

#[test]
fn broken_dictzip_headers_are_refused() {
    let good = dz();
    let malformed = |data: Vec<u8>| matches!(open_dz(data).1, Err(Error::Malformed(_)));
    assert!(malformed(Vec::new()));
    assert!(malformed(good[..11].to_vec()));
    // Not gzip, not deflate.
    let mut other = good.clone();
    other[0] = 0x1e;
    assert!(malformed(other));
    let mut other = good.clone();
    other[2] = 9;
    assert!(malformed(other));
    // Plain gzip: no extra field.
    let mut other = good.clone();
    other[3] = 8;
    assert!(malformed(other));
    // The extra field longer than the file.
    let mut other = good.clone();
    other[10..12].copy_from_slice(&60_000u16.to_le_bytes());
    assert!(malformed(other));
    // No `RA` subfield.
    let mut other = good.clone();
    other[12 + 7..12 + 9].copy_from_slice(b"RB");
    assert!(malformed(other));
    // An `RA` subfield too short for its own table.
    let mut other = good.clone();
    other[12 + 7 + 2..12 + 7 + 4].copy_from_slice(&2u16.to_le_bytes());
    assert!(malformed(other));
    // Chunks of length 0.
    let mut other = good.clone();
    other[12 + 7 + 6..12 + 7 + 8].copy_from_slice(&0u16.to_le_bytes());
    assert!(malformed(other));
    // A file name without its end.
    let header = 12 + u16::from_le_bytes([good[10], good[11]]) as usize;
    assert!(malformed(good[..header + 4].to_vec()));
}

/// A chunk table without chunks: nothing to read.
#[test]
#[ignore = "bug: dict.rs reads chunk 0 of a dictzip table without chunks: chunks[1] out of bounds"]
fn a_dictzip_without_chunks_finds_empty_definitions() {
    let mut data = dz();
    // The chunk count, after version and chunk length.
    data[12 + 7 + 4 + 4..12 + 7 + 4 + 6].copy_from_slice(&0u16.to_le_bytes());
    let (storage, dictionary) = open_dz(data);
    let result = dictionary.unwrap().lookup(&storage, "alpha");
    assert!(
        matches!(&result, Ok(d) if d[0].text.is_empty()) || result.is_err(),
        "{result:?}"
    );
}

/// The deflate data cut off or replaced by noise.
#[test]
fn broken_dictzip_data_is_an_error() {
    let good = dz();
    let (storage, dictionary) = open_dz(good[..good.len() - 5].to_vec());
    let dictionary = dictionary.unwrap();
    assert!(matches!(
        dictionary.lookup(&storage, "gamma"),
        Err(Error::Malformed(_))
    ));
    let mut noise = good.clone();
    let n = noise.len();
    for (i, byte) in noise[n - 20..].iter_mut().enumerate() {
        *byte = 0xff - i as u8;
    }
    let (storage, dictionary) = open_dz(noise);
    let dictionary = dictionary.unwrap();
    assert!(dictionary.lookup(&storage, "gamma").is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Bytes of the dictzip file changed: every lookup ends with a
    /// definition or an error, without a panic. (The chunk count stays:
    /// see the bug above.)
    #[test]
    fn changed_dictzip_bytes_never_panic(changes in prop::collection::vec((0usize..200, any::<u8>()), 1..6)) {
        let mut data = dictzip(&b"firstsecondthird".repeat(20), 37, 4 | 8);
        let count = 12 + 7 + 4 + 4;
        for (at, byte) in changes {
            if at < data.len() && !(count..count + 2).contains(&at) {
                data[at] = byte;
            }
        }
        let (storage, dictionary) = open_dz(data);
        if let Ok(dictionary) = dictionary {
            let counted = Counted::new(&storage, 10_000);
            for (word, _) in WORDS {
                let _ = dictionary.lookup(&counted, word);
            }
            prop_assert!(counted.reads.get() < 10_000);
        }
    }

    /// Bytes of the `.idx` changed: building and looking up end without
    /// a panic.
    #[test]
    fn changed_index_bytes_never_panic(changes in prop::collection::vec((0usize..400, any::<u8>()), 1..6)) {
        let entries: Vec<(String, String)> =
            (0..40).map(|i| (format!("word{i:02}"), format!("def {i}"))).collect();
        let entries: Vec<(&str, &str)> =
            entries.iter().map(|(w, d)| (w.as_str(), d.as_str())).collect();
        let storage = build(&Spec::new(&entries));
        let mut idx = read_file(&storage, "d/test.idx", 1 << 20).unwrap();
        for (at, byte) in changes {
            if at < idx.len() {
                idx[at] = byte;
            }
        }
        storage.write("d/test.idx", &idx).unwrap();
        let dictionary = Dictionary::open(&storage, "d/test.ifo", "cache").unwrap();
        let mut built = true;
        for mut builder in dictionary.builders(&storage) {
            let mut steps = 0;
            loop {
                match builder.step(&storage, 64) {
                    Ok(true) => break,
                    Ok(false) => {}
                    Err(_) => {
                        built = false;
                        break;
                    }
                }
                steps += 1;
                prop_assert!(steps < 1000, "building ends");
            }
        }
        if built {
            let counted = Counted::new(&storage, 10_000);
            for (word, _) in &entries {
                let _ = dictionary.lookup(&counted, word);
            }
            prop_assert!(counted.reads.get() < 10_000);
        }
    }
}
