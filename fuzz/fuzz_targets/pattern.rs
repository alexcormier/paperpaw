//! Any bytes as a pattern and a text to search (split at the first zero
//! byte): made or refused, searched within bounds, matches inside the text
//! and at character boundaries.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_sdk::pattern::Pattern;

fuzz_target!(|data: &[u8]| {
    let split = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let source = String::from_utf8_lossy(&data[..split]);
    let text = String::from_utf8_lossy(data.get(split + 1..).unwrap_or_default());
    let Ok(pattern) = Pattern::new(&source) else {
        return;
    };
    let mut last = 0;
    for (start, end) in pattern.find_iter(&text) {
        assert!(last <= start && start < end && end <= text.len());
        assert!(text.is_char_boundary(start) && text.is_char_boundary(end));
        last = end;
    }
});
