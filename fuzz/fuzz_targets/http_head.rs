//! Any bytes as the head of an HTTP request, as the servers on the local
//! network (transfer, remote keyboard) receive it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_http::server::parse_request;

fuzz_target!(|data: &[u8]| {
    if let Ok(Some((_, used))) = parse_request(data) {
        assert!(used <= data.len());
    }
});
