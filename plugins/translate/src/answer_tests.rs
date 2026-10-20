//! DeepL's answers and the requests for them, at random: any body reads
//! without panic, every choice and language give a target DeepL knows,
//! and any text goes form-encoded, whole up to its limit.

extern crate std;

use alloc::string::String;

use paperpaw_sdk::web::percent_decode;
use proptest::prelude::*;

use super::{AUTO, LANGUAGES, MAX_CHARS, request, target, translation};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn any_body_reads_without_panic(body in ".{0,300}") {
        let _ = translation(&body);
        let wrapped = alloc::format!("{{\"translations\":[{{\"text\":{body}}}]}}");
        let _ = translation(&wrapped);
    }

    #[test]
    fn every_choice_gives_a_target_deepl_knows(
        to in prop_oneof![Just(String::from(AUTO)), prop::sample::select(LANGUAGES).prop_map(String::from)],
        ui in ".{0,10}",
    ) {
        let code = target(&to, &ui);
        prop_assert!(
            LANGUAGES
                .iter()
                .any(|l| l.strip_prefix("translate.to.").map(str::to_uppercase) == Some(code.clone())),
            "{} for {:?} {:?}", code, to, ui
        );
    }

    #[test]
    fn any_text_goes_whole_up_to_its_limit(text in ".{0,3500}", key in "[a-z0-9:]{1,12}") {
        let request = request(&key, &text, "DE");
        let body = request.body.unwrap();
        let (field, rest) = body.split_once("&target_lang=").unwrap();
        prop_assert_eq!(rest, "DE");
        let sent = field.strip_prefix("text=").unwrap();
        prop_assert!(!sent.contains(['&', '=', ' ']));
        let decoded = percent_decode(&sent.replace('+', " "));
        let expected: String = text.chars().take(MAX_CHARS).collect();
        prop_assert_eq!(decoded, expected);
    }
}
