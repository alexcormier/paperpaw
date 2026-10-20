//! KOReader's Lua settings files: what is read is written back the same
//! (property tests over arbitrary text and arbitrary values, as the fuzz
//! target `lua` checks), and hostile files (deep nesting, unterminated
//! strings and brackets, odd escapes, huge numbers) are refused or read
//! without a panic.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_plugin_koreader::lua::{self, Table, Value};
use proptest::prelude::*;

fn str_of(text: &str) -> Option<String> {
    match lua::parse(&format!("return {text}"))? {
        Value::Str(s) => Some(s),
        _ => None,
    }
}

fn nested(depth: usize) -> String {
    format!("return {}{}", "{".repeat(depth), "}".repeat(depth))
}

#[test]
fn nesting_is_read_up_to_its_limit() {
    assert!(lua::parse(&nested(64)).is_some());
    assert_eq!(lua::parse(&nested(65)), None);
    // Far deeper than any stack would take.
    assert_eq!(lua::parse(&nested(200_000)), None);
    // Deep through keys as well as values.
    let keys = format!("return {}1{}", "{[".repeat(100), "]=1}".repeat(100));
    assert_eq!(lua::parse(&keys), None);
    // Only opened.
    assert_eq!(lua::parse(&format!("return {}", "{".repeat(50))), None);
}

#[test]
fn unterminated_things_are_refused() {
    for text in [
        "return \"abc",
        "return 'abc",
        "return \"abc\\",
        "return \"a\nb\"",
        "return [[abc",
        "return [==[abc]=]",
        "return {[\"a\"] = }",
        "return {[\"a\" = 1}",
        "return {[\"a\"] 1}",
        "return {a = 1",
        "return {a = 1}}",
        "return {,,,",
        "return \"\\x4\"",
        "return \"\\xZZ\"",
        "return \"\\256\"",
        "return \"\\999\"",
        "return -",
        "return",
        "return {} trailing",
    ] {
        assert_eq!(lua::parse(text), None, "{text:?}");
    }
}

#[test]
fn comments_of_every_kind_are_skipped() {
    let text = "-- line\n--[[ long\ncomment ]]--[==[ another ]] still ]==]\n\
                return { --[[ inside ]] a = 1, -- after\n b = 2 }-- end";
    let Some(Value::Table(table)) = lua::parse(text) else {
        panic!("not read");
    };
    assert_eq!(table.get("a"), Some(&Value::Number(String::from("1"))));
    assert_eq!(table.get("b"), Some(&Value::Number(String::from("2"))));
    // An unclosed long comment is read as a line comment (Lua refuses
    // it; reading more than Lua does loses nothing).
    assert!(lua::parse("--[[ unclosed\nreturn 1").is_some());
}

#[test]
fn escapes_are_decoded_as_lua_does() {
    assert_eq!(
        str_of(r#""a\tb\nc\\d\"e\'f""#).as_deref(),
        Some("a\tb\nc\\d\"e'f")
    );
    assert_eq!(str_of(r#""\65\066\0671""#).as_deref(), Some("ABC1"));
    assert_eq!(str_of(r#""\x41\x6a""#).as_deref(), Some("Aj"));
    assert_eq!(str_of("\"a\\z   \n  b\"").as_deref(), Some("ab"));
    assert_eq!(str_of("\"a\\\nb\"").as_deref(), Some("a\nb"));
    assert_eq!(
        str_of(r#""\a\b\f\v\r""#).as_deref(),
        Some("\u{7}\u{8}\u{c}\u{b}\r")
    );
    // UTF-8 written as bytes.
    assert_eq!(str_of(r#""\xc3\xa9t\195\169""#).as_deref(), Some("été"));
    // Bytes that are no UTF-8: replaced, not a panic.
    assert_eq!(str_of(r#""\xff\xfe""#).as_deref(), Some("\u{fffd}\u{fffd}"));
    // Long strings: no escapes, a first line break dropped.
    assert_eq!(
        str_of("[==[\nraw \\n ]] text]==]").as_deref(),
        Some("raw \\n ]] text")
    );
    assert_eq!(str_of("'it''s'"), None);
}

#[test]
fn numbers_are_kept_as_written() {
    for number in [
        "0",
        "-0",
        "12",
        "0.25",
        "-3.5e-2",
        "1E+300",
        "0x1F",
        "0X1p4",
        "123456789012345678901234567890",
        "1e999999",
        ".5",
    ] {
        let Some(Value::Table(table)) = lua::parse(&format!("return {{ n = {number} }}")) else {
            panic!("{number}");
        };
        assert_eq!(
            table.get("n"),
            Some(&Value::Number(String::from(number))),
            "{number}"
        );
        // Written as it came.
        let again = lua::parse(&lua::write(&Value::Table(table.clone()))).unwrap();
        assert_eq!(again, Value::Table(table));
    }
}

#[test]
fn lists_go_by_their_numeric_keys() {
    let text = r#"return {
        [10] = "ten", [2] = "two", [1] = "one",
        [-1] = "minus", [1.5] = "half", ["3"] = "text",
        [99999999999999999999999] = "huge",
    }"#;
    let Some(Value::Table(table)) = lua::parse(text) else {
        panic!("not read");
    };
    let list: Vec<&Value> = table.list().collect();
    assert_eq!(
        list,
        [
            &Value::Str(String::from("one")),
            &Value::Str(String::from("two")),
            &Value::Str(String::from("ten")),
        ]
    );
    // Implicit keys count on past explicit ones and nils.
    let Some(Value::Table(table)) = lua::parse("return { 'a', nil, [7] = 'x', 'c' }") else {
        panic!("not read");
    };
    assert_eq!(table.fields.len(), 3);
    assert_eq!(table.fields[2].0, Value::Number(String::from("3")));
}

#[test]
fn tables_are_changed_in_place() {
    let mut table = Table::default();
    table.set("a", lua::string("1"));
    table.set("b", lua::string("2"));
    table.set("a", lua::string("3"));
    assert_eq!(table.fields.len(), 2);
    assert_eq!(table.str("a"), Some("3"));
    assert_eq!(table.fields[0].0, Value::Str(String::from("a")));
    table.set("a", Value::Nil);
    table.set("missing", Value::Nil);
    assert_eq!(table.fields.len(), 1);
    assert_eq!(table.str("b"), Some("2"));
    // Not a string: no text.
    table.set("n", Value::Number(String::from("1")));
    assert_eq!(table.str("n"), None);
}

#[test]
fn keys_of_any_kind_survive_a_round_trip() {
    let text = r#"return {
        [true] = 1, [false] = 2, [{ 1, 2 }] = "table key",
        ["with \"quotes\""] = 3, [ [[long key]] ] = 4,
        ["\n"] = 5, word_key = 6,
    }"#;
    let value = lua::parse(text).unwrap();
    assert_eq!(lua::parse(&lua::write(&value)), Some(value));
}

/// A Lua value KOReader could have written: no nil in tables (they leave
/// no field), numbers as Lua writes them.
fn values() -> impl Strategy<Value = Value> {
    let number = prop_oneof![
        any::<i64>().prop_map(|n| n.to_string()),
        any::<f64>()
            .prop_filter("finite", |f| f.is_finite())
            .prop_map(|f| format!("{f:e}")),
        Just(String::from("0x1F")),
    ];
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        number.prop_map(Value::Number),
        any::<String>().prop_map(Value::Str),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        proptest::collection::vec((inner.clone(), inner), 0..6)
            .prop_map(|fields| Value::Table(Table { fields }))
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// What is written is read back the same.
    #[test]
    fn values_round_trip(value in values()) {
        let written = lua::write(&value);
        prop_assert_eq!(lua::parse(&written), Some(value), "{}", written);
    }

    /// Any text that reads is written and read back the same (the fuzz
    /// target `lua`), and no text panics.
    #[test]
    fn text_that_reads_round_trips(text in r#"(return )?[{}\[\]=,;'"a-z0-9\\ \n.\-x]{0,80}"#) {
        if let Some(value) = lua::parse(&text) {
            prop_assert_eq!(lua::parse(&lua::write(&value)), Some(value));
        }
    }

    /// Arbitrary bytes never panic.
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = lua::parse(&String::from_utf8_lossy(&bytes));
    }
}
