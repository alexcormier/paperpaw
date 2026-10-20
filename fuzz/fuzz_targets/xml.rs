//! Any bytes through the XML tokenizer, as XML or HTML and in chunks of a
//! size the input chooses; every start tag's attributes are read too.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_sdk::xml::{Token, Tokenizer, decode};

fuzz_target!(|data: &[u8]| {
    let Some((&size, data)) = data.split_first() else {
        return;
    };
    let mut tokenizer = if size & 0x80 != 0 {
        Tokenizer::html()
    } else {
        Tokenizer::new()
    };
    let mut emit = |token: Token<'_>| match token {
        Token::Text(text) => {
            let _ = decode(&text);
        }
        Token::Start { attrs, .. } => {
            for (name, _) in attrs.iter() {
                let _ = attrs.get(name);
            }
        }
        _ => {}
    };
    for chunk in data.chunks(usize::from(size & 0x7f).max(1)) {
        tokenizer.feed(chunk, &mut emit);
    }
    tokenizer.finish(&mut emit);
});
