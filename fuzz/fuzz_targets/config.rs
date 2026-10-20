//! Any text as an INI file: parse every entry and read its value; where
//! it parses cleanly, set a key the input names and check that the
//! result still parses cleanly.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_fuzz::split;

fn parses_cleanly(source: &str) -> bool {
    paperpaw_config::parse(source).all(|entry| match entry {
        Ok(entry) => {
            let _ = (entry.value.as_str(), entry.value.as_bool());
            let _ = entry.value.as_i64();
            true
        }
        Err(_) => false,
    })
}

fuzz_target!(|data: &[u8]| {
    let [section, key, value, source] = split::<4>(data).map(String::from_utf8_lossy);
    if parses_cleanly(&source) {
        let changed = paperpaw_config::set(&source, &section, &key, &value);
        assert!(parses_cleanly(&changed), "set wrote what parse rejects");
    }
});
