//! Definitions as plain text: markup of any shape (unclosed tags, odd
//! entities) and the typed fields of StarDict definitions (text, HTML,
//! phonetics, sounds and pictures with their sizes, sizes larger than
//! what follows).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Spec, build, found, open};
use paperpaw_stardict::to_plain_text;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Whatever the markup, the text has no empty lines and its words
    /// are separated by single spaces.
    #[test]
    fn plain_text_is_tidy(markup in "(<[a-zA-Z/]{0,6}[^>]{0,5}>?|&[#a-zA-Z0-9]{0,10};?|[ \t\r\na-z\u{e4}\u{65e5}]{1,6}){0,30}") {
        let text = to_plain_text(&markup);
        for line in text.split('\n').filter(|_| !text.is_empty()) {
            prop_assert!(!line.is_empty(), "{text:?}");
            prop_assert!(!line.starts_with(' ') && !line.ends_with(' '), "{text:?}");
            prop_assert!(!line.contains("  "), "{text:?}");
        }
    }

    #[test]
    fn any_text_does_not_panic(markup in any::<String>()) {
        let _ = to_plain_text(&markup);
    }

    /// Text without markup keeps its words.
    #[test]
    fn plain_words_stay(words in prop::collection::vec("[a-zA-Z0-9\u{e4}\u{f8}\u{65e5}.,!?]{1,8}", 1..20)) {
        let text = to_plain_text(&words.join(" \t "));
        prop_assert_eq!(text, words.join(" "));
    }
}

#[test]
fn entities_of_every_kind() {
    assert_eq!(
        to_plain_text("&lt;b&gt; &quot;x&quot; &apos;y&apos;"),
        "<b> \"x\" 'y'"
    );
    assert_eq!(to_plain_text("&#65;&#x42;&#X43;"), "ABC");
    // Not characters: kept as written.
    assert_eq!(to_plain_text("&#xD800;"), "&#xD800;");
    assert_eq!(to_plain_text("&#1114112;"), "&#1114112;");
    assert_eq!(to_plain_text("&#xZZ;"), "&#xZZ;");
    assert_eq!(to_plain_text("&bogus;"), "&bogus;");
    // No `;`: the ampersand stays.
    assert_eq!(to_plain_text("salt & pepper"), "salt & pepper");
    assert_eq!(to_plain_text("AT&T"), "AT&T");
    // A long name is not an entity.
    assert_eq!(to_plain_text("&averyverylongname;"), "&averyverylongname;");
    // A no-break space is a space like others.
    assert_eq!(to_plain_text("a&nbsp;&nbsp;b"), "a b");
}

#[test]
fn tags_of_every_shape() {
    // Unclosed at the end: the rest is dropped.
    assert_eq!(to_plain_text("word <b"), "word");
    // Upper case, attributes, self-closing, closing.
    assert_eq!(
        to_plain_text("<P class=\"x\">one<BR/>two</p><div>three</div>"),
        "one\ntwo\nthree"
    );
    // List items in an unordered list.
    assert_eq!(to_plain_text("<ul><li>a</li><li>b</li></ul>"), "• a\n• b");
    // XDXF: `def` and `ex` start new lines, other tags vanish.
    assert_eq!(
        to_plain_text("<k>hus</k><def><abr>n</abr> house</def><ex>et hus</ex>"),
        "hus\nn house\net hus"
    );
    assert_eq!(to_plain_text(""), "");
    assert_eq!(to_plain_text("<br><br>  <p> </p>"), "");
}

/// Definitions whose fields carry their own type: text, HTML, a sound
/// with its size, phonetics, an unknown type.
#[test]
fn typed_fields_without_a_type_sequence() {
    let definition = concat!(
        "mplain text\0",
        "W\0\0\0\x03abc",
        "t/hʊs/\0",
        "h<b>bold</b> &amp; more\0",
        "zunknown\0",
        "yromaji\0",
    );
    let mut spec = Spec::new(&[("hus", definition)]);
    spec.types = None;
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(
        found(&dictionary, &storage, "hus")[0].1,
        "plain text\n[/hʊs/]\nbold & more\nromaji"
    );
}

/// A sound field whose size says more than follows: the definition ends
/// there.
#[test]
fn binary_fields_larger_than_the_definition_are_cut() {
    let mut spec = Spec::new(&[("a", "mfirst\0P\0\0\x10\0tiny"), ("b", "W\0\0")]);
    spec.types = None;
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "a")[0].1, "first");
    assert_eq!(found(&dictionary, &storage, "b")[0].1, "");
}

/// With a type sequence the last field takes the rest, a NUL in it or
/// not; earlier ones end at their NUL.
#[test]
fn typed_fields_with_a_type_sequence() {
    let mut spec = Spec::new(&[("a", "/ei/\0<i>letter</i>"), ("b", "/bi:/\0<b>b</b>")]);
    spec.types = Some("th".to_string());
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "a")[0].1, "[/ei/]\nletter");
    assert_eq!(found(&dictionary, &storage, "b")[0].1, "[/bi:/]\nb");
    // A last field that is binary: everything left, not shown.
    let mut spec = Spec::new(&[("c", "see\0RIFF....")]);
    spec.types = Some("mW".to_string());
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "c")[0].1, "see");
}

/// A definition that is only a short pronunciation: its text, with the
/// brackets around it, is longer than its data, and must still come
/// whole.
#[test]
fn a_definition_whose_text_is_longer_than_its_data_comes_whole() {
    let mut spec = Spec::new(&[("b", "/bi:/\0")]);
    spec.types = Some("th".to_string());
    let storage = build(&spec);
    let dictionary = open(&storage);
    assert_eq!(found(&dictionary, &storage, "b")[0].1, "[/bi:/]");
}
