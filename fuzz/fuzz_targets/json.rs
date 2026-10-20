//! Any bytes as JSON: the whole-text helpers, and the streaming parser
//! fed in chunks of a size the input chooses.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_json::stream::{Event, MAX_STRING, Parser};

fuzz_target!(|data: &[u8]| {
    let Some((&size, data)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(data);
    let members = paperpaw_json::object(&text);
    let _ = paperpaw_json::text(&members, "title");
    let _ = paperpaw_json::raw_member(&text, "items");
    let _ = paperpaw_json::string(&text);
    let _ = paperpaw_json::unquote(&text);
    let _ = paperpaw_json::string_array(&text);

    let mut parser = Parser::default();
    let mut emit = |event: Event<'_>| {
        if let Event::Value(_, value) = event {
            // A cut may end inside a character: allow its bytes.
            assert!(value.len() <= MAX_STRING + 3, "string longer than the cap");
        }
    };
    for chunk in data.chunks(usize::from(size).max(1)) {
        parser.feed(chunk, &mut emit);
    }
});
