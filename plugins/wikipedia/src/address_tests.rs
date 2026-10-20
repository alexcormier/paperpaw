//! The address of a summary for any word and language: always on a
//! wiki of letters at `wikipedia.org`, the word one encoded path segment.

extern crate std;

use proptest::prelude::*;

use super::{summary_url, wiki};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn any_word_stays_one_segment_on_a_wiki(language in ".{0,20}", word in ".{0,40}") {
        let url = summary_url(&language, &word);
        let rest = url.strip_prefix("https://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        let code = host.strip_suffix(".wikipedia.org").unwrap();
        prop_assert!(!code.is_empty());
        prop_assert!(code.chars().all(|c| c.is_ascii_lowercase()), "{}", code);
        prop_assert_eq!(code, wiki(&language));
        let title = path.strip_prefix("api/rest_v1/page/summary/").unwrap();
        prop_assert!(!title.contains(['/', '?', '#', ' ', '\n', '\t']), "{}", title);
        prop_assert!(title.is_ascii(), "{}", title);
    }
}
