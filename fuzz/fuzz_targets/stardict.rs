//! Any bytes as a StarDict dictionary (`.ifo`, `.idx`, `.syn` and a plain
//! or dictzip `.dict`): open it, build its indexes and look up a word.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::split;
use paperpaw_sdk::MemoryStorage;
use paperpaw_stardict::{Dictionary, to_plain_text};

/// Steps given to the index builders at most, and bytes read per step.
const MAX_STEPS: usize = 1000;
const STEP_BUDGET: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, data)) = data.split_first() else {
        return;
    };
    let [ifo, idx, syn, word, dict] = split::<5>(data);
    let dict_name = if flags & 1 != 0 {
        "d/words.dict.dz"
    } else {
        "d/words.dict"
    };
    let storage = MemoryStorage::new()
        .with("d/words.ifo", ifo.to_vec())
        .with("d/words.idx", idx.to_vec())
        .with("d/words.syn", syn.to_vec())
        .with(dict_name, dict.to_vec());
    let Ok(dictionary) = Dictionary::open(&storage, "d/words.ifo", "cache") else {
        return;
    };
    let mut steps = 0;
    for mut builder in dictionary.builders(&storage) {
        while steps < MAX_STEPS && matches!(builder.step(&storage, STEP_BUDGET), Ok(false)) {
            steps += 1;
        }
    }
    let word = String::from_utf8_lossy(word);
    if let Ok(definitions) = dictionary.lookup(&storage, &word) {
        for definition in &definitions {
            let _ = to_plain_text(&definition.text);
        }
    }
});
