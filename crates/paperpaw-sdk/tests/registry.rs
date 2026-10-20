//! What plugins register and how the registries answer: ids only in the
//! plugin's namespace and once per kind (also for settings and presets
//! kept in the program), typed event handlers, input and refresh filters
//! in their order, filter signatures, hyphenation by language with its
//! fallbacks, services by type, and formats by extension.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests panic on purpose when something is wrong.

mod common;

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::filters::{
    InputFilterSpec, InputFilters, InputVerdict, Refresh, RefreshFilterSpec, RefreshFilters,
    TextFilterSpec, TextFilters,
};
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::services::Services;
use paperpaw_sdk::{
    ActionSpec, ButtonState, Command, Contribution, Event, FormatSpec, Formats, Hyphenation,
    HyphenationSpec, Id, InputEvent, Manifest, Params, Priority, Rect, RefreshMode, RegisterError,
    Registrar, SettingKind, SettingSpec, SettingValue, SlotSpec, Version,
};

fn registrar() -> Registrar {
    Registrar::new(Id::new_static("demo"))
}

fn setting(id: &'static str) -> SettingSpec {
    SettingSpec::new(
        Id::new_static(id),
        "",
        SettingKind::Bool,
        SettingValue::Bool(false),
    )
}

#[test]
fn settings_kept_in_the_program_are_checked_like_others() {
    static OWN: [SettingSpec; 2] = [
        SettingSpec::fixed_switch("demo.a", "", false, groups::OTHER),
        SettingSpec::fixed_switch("demo.b", "", false, groups::OTHER),
    ];
    static FOREIGN: [SettingSpec; 1] = [SettingSpec::fixed_switch(
        "other.a",
        "",
        false,
        groups::OTHER,
    )];
    static TWICE: [SettingSpec; 2] = [
        SettingSpec::fixed_switch("demo.c", "", false, groups::OTHER),
        SettingSpec::fixed_switch("demo.c", "", true, groups::OTHER),
    ];
    static AGAIN: [SettingSpec; 1] = [SettingSpec::fixed_switch(
        "demo.a",
        "",
        false,
        groups::OTHER,
    )];
    static BUILT: [SettingSpec; 1] = [SettingSpec::fixed_switch(
        "demo.built",
        "",
        false,
        groups::OTHER,
    )];
    let mut r = registrar();
    r.add_settings(&OWN).unwrap();
    assert_eq!(
        r.add_settings(&FOREIGN),
        Err(RegisterError::Namespace {
            id: Id::new_static("other.a")
        })
    );
    assert!(matches!(
        r.add_settings(&TWICE),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(matches!(
        r.add_settings(&AGAIN),
        Err(RegisterError::Duplicate { .. })
    ));
    // One built at start with the id of one kept in the program, and the
    // other way round.
    assert!(matches!(
        r.add_setting(setting("demo.b")),
        Err(RegisterError::Duplicate { .. })
    ));
    r.add_setting(setting("demo.built")).unwrap();
    assert!(matches!(
        r.add_settings(&BUILT),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(matches!(
        r.add_setting(setting("other.x")),
        Err(RegisterError::Namespace { .. })
    ));
    // Refused lists are not added, not even in part.
    let kinds: Vec<&str> = r.contributions().iter().map(Contribution::kind).collect();
    assert_eq!(kinds, ["setting", "setting"]);
}

#[test]
fn presets_kept_in_the_program_are_checked_like_others() {
    static OWN: [Preset; 1] = [Preset::fixed("demo.calm", "home.layout", "", "", "")];
    static FOREIGN: [Preset; 1] = [Preset::fixed("home.calm", "home.layout", "", "", "")];
    static TWICE: [Preset; 2] = [
        Preset::fixed("demo.x", "home.layout", "", "", ""),
        Preset::fixed("demo.x", "home.layout", "", "", ""),
    ];
    let mut r = registrar();
    r.add_presets(&OWN).unwrap();
    assert!(matches!(
        r.add_presets(&FOREIGN),
        Err(RegisterError::Namespace { .. })
    ));
    assert!(matches!(
        r.add_presets(&TWICE),
        Err(RegisterError::Duplicate { .. })
    ));
    let made = |id: &'static str| {
        Preset::new(
            Id::new_static(id),
            Id::new_static("home.layout"),
            "",
            "",
            "",
        )
    };
    assert!(matches!(
        r.add_preset(made("demo.calm")),
        Err(RegisterError::Duplicate { .. })
    ));
    r.add_preset(made("demo.other")).unwrap();
    assert!(matches!(r.add_presets(&[]), Ok(())));
    static LATER: [Preset; 1] = [Preset::fixed("demo.other", "home.layout", "", "", "")];
    assert!(matches!(
        r.add_presets(&LATER),
        Err(RegisterError::Duplicate { .. })
    ));
    // A preset may be of another plugin's kind.
    assert_eq!(r.contributions().len(), 3);
}

#[test]
fn every_kind_of_contribution_says_what_it_is() {
    let mut r = registrar();
    r.add_slot(SlotSpec {
        id: Id::new_static("demo.slot"),
        title: Cow::Borrowed(""),
    })
    .unwrap();
    r.bind_slot(
        Id::new_static("core.reader"),
        Id::new_static("demo.screen"),
        Priority::PREFERRED,
    )
    .unwrap();
    r.add_keymap(Cow::Borrowed("")).unwrap();
    r.add_keymap_preset(Id::new_static("demo.keys"), Cow::Borrowed(""))
        .unwrap();
    r.add_stylesheet(Cow::Borrowed("p {}")).unwrap();
    r.add_book_data("cache", "demo-").unwrap();
    r.bind_font(
        Id::new_static("core.ui"),
        Id::new_static("x.font"),
        Priority::FALLBACK,
    )
    .unwrap();
    r.set_setting_default(Id::new_static("core.font-size"), Cow::Borrowed("12"))
        .unwrap();
    r.contribute(Id::new_static("other.point"), Cow::Borrowed("text"))
        .unwrap();
    r.add_translations(Cow::Borrowed("")).unwrap();
    r.add_service(Id::new_static("demo.count"), Box::new(1u8))
        .unwrap();
    r.subscribe(Id::new_static("core.book-opened"), Box::new(|_, _, _| {}))
        .unwrap();
    let kinds: Vec<&str> = r.contributions().iter().map(Contribution::kind).collect();
    assert_eq!(
        kinds,
        [
            "slot",
            "slot-binding",
            "keymap",
            "keymap-preset",
            "stylesheet",
            "book data",
            "font-binding",
            "setting-default",
            "to-point",
            "translations",
            "service",
            "subscription"
        ]
    );
    let ids: Vec<Option<&str>> = r
        .contributions()
        .iter()
        .map(|c| c.id().map(Id::as_str))
        .collect();
    // Bindings and the like name what others added: no id of their own.
    assert_eq!(ids[0], Some("demo.slot"));
    assert_eq!(ids[1], None);
    assert_eq!(ids[3], Some("demo.keys"));
    assert_eq!(ids[10], Some("demo.count"));
    assert!(format!("{:?}", r.contributions()[0]).contains("demo.slot"));
    // A service of another namespace is refused.
    assert!(matches!(
        r.add_service(Id::new_static("other.count"), Box::new(1u8)),
        Err(RegisterError::Namespace { .. })
    ));
    assert_eq!(r.plugin(), &Id::new_static("demo"));
    assert_eq!(r.into_contributions().len(), 12);
}

#[test]
fn typed_handlers_hear_only_what_they_can_read() {
    paperpaw_sdk::event_data! {
        pub struct Count {
            pub n: u32,
        }
    }
    const COUNTED: Event<Count> = Event::new("demo.counted");
    let heard = Rc::new(Cell::new(0));
    let mut r = registrar();
    let seen = heard.clone();
    r.on(COUNTED, move |_, count, out| {
        seen.set(seen.get() + count.n);
        out.push(Command::Restart);
    })
    .unwrap();
    let Some(Contribution::Subscription { event, handler }) = r.into_contributions().pop() else {
        panic!("a subscription");
    };
    assert_eq!(event.as_str(), "demo.counted");
    let h = common::Harness::new();
    let env = h.env();
    let mut out = Vec::new();
    handler(&env, &Params::new().with("n", "5"), &mut out);
    handler(&env, &Params::new().with("n", "five"), &mut out);
    handler(&env, &Params::new(), &mut out);
    assert_eq!(heard.get(), 5);
    assert_eq!(out, [Command::Restart]);
}

#[test]
fn input_filters_take_what_the_first_one_does_not_pass() {
    let mut filters = InputFilters::new();
    let button = |name: &'static str| InputEvent::Button {
        name,
        state: ButtonState::Pressed,
    };
    let make = |id: &'static str, verdict: fn(&InputEvent) -> InputVerdict| InputFilterSpec {
        id: Id::new_static(id),
        title: Cow::Borrowed(""),
        filter: Box::new(move |_, event| verdict(event)),
    };
    let h = common::Harness::new();
    let env = h.env();
    assert_eq!(filters.apply(&env, &button("a")), InputVerdict::Pass);
    filters.add(make("f.lock", |e| match e {
        InputEvent::Button { name: "lock", .. } => InputVerdict::Take,
        _ => InputVerdict::Pass,
    }));
    filters.add(make("f.run", |e| match e {
        InputEvent::Button {
            name: "lock" | "run",
            ..
        } => InputVerdict::Run(Id::new_static("f.action")),
        _ => InputVerdict::Pass,
    }));
    filters.shrink_to_fit();
    assert_eq!(filters.apply(&env, &button("lock")), InputVerdict::Take);
    assert_eq!(
        filters.apply(&env, &button("run")),
        InputVerdict::Run(Id::new_static("f.action"))
    );
    assert_eq!(filters.apply(&env, &button("other")), InputVerdict::Pass);
}

#[test]
fn refresh_filters_change_the_mode_in_turn_and_full_covers_the_screen() {
    let screen = Rect {
        x: 0,
        y: 0,
        width: 480,
        height: 800,
    };
    let part = Rect {
        x: 10,
        y: 20,
        width: 30,
        height: 40,
    };
    let mut filters = RefreshFilters::new();
    let h = common::Harness::new();
    let env = h.env();
    let refresh = Refresh {
        mode: RefreshMode::Partial,
        region: part,
        screen: Some(Id::new_static("core.reader")),
    };
    assert_eq!(filters.apply(&env, refresh.clone(), screen), refresh);
    filters.add(RefreshFilterSpec {
        id: Id::new_static("f.full"),
        title: Cow::Borrowed(""),
        filter: Box::new(|_, r| {
            (r.screen
                .as_ref()
                .is_some_and(|s| s.as_str() == "core.reader"))
            .then_some(RefreshMode::Full)
        }),
    });
    // A later filter sees the change and may take it back; the region
    // stays the whole screen.
    filters.add(RefreshFilterSpec {
        id: Id::new_static("f.fast"),
        title: Cow::Borrowed(""),
        filter: Box::new(|_, r| (r.mode == RefreshMode::Full).then_some(RefreshMode::Fast)),
    });
    let changed = filters.apply(&env, refresh.clone(), screen);
    assert_eq!((changed.mode, changed.region), (RefreshMode::Fast, screen));
    let elsewhere = Refresh {
        screen: None,
        ..refresh
    };
    assert_eq!(filters.apply(&env, elsewhere.clone(), screen), elsewhere);
}

#[test]
fn text_filter_signatures_name_those_that_do_something() {
    let mut filters = TextFilters::new();
    let h = common::Harness::new();
    let env = h.env();
    assert_eq!(filters.signature(&env), "");
    for (id, signature) in [("f.bold", "on"), ("f.idle", ""), ("f.size", "3")] {
        filters.add(TextFilterSpec {
            id: Id::new_static(id),
            title: Cow::Borrowed(""),
            filter: Box::new(|_, _| {}),
            signature: Box::new(move |_| signature.to_string()),
        });
    }
    filters.shrink_to_fit();
    assert_eq!(filters.signature(&env), "f.bold=on;f.size=3;");
    assert!(format!("{filters:?}").contains("f.bold"));
}

fn patterns(language: &str) -> &'static [u8] {
    // Patterns that differ by language: each breaks a word at another
    // place, so the one chosen is seen.
    let text = match language {
        "de" => "a1b",
        "de-AT" => "b1c",
        _ => "c1d",
    };
    Box::leak(paperpaw_sdk::hyphen::compile(text, "", 1, 1).into_boxed_slice())
}

fn breaks(hyphenation: &Hyphenation, language: &str, word: &str) -> Option<Vec<usize>> {
    let patterns = hyphenation.get(language)?;
    let mut at = Vec::new();
    patterns.hyphenate(word, &mut |i| at.push(i));
    Some(at)
}

#[test]
fn hyphenation_falls_back_to_the_base_language_and_back() {
    let mut hyphenation = Hyphenation::new();
    let spec = |id: &'static str, language: &'static str| HyphenationSpec {
        id: Id::new_static(id),
        language: Cow::Borrowed(language),
        patterns: patterns(language),
    };
    hyphenation.add(spec("h.de", "de")).unwrap();
    hyphenation.add(spec("h.de-at", "de-AT")).unwrap();
    hyphenation.add(spec("h.en-us", "en-US")).unwrap();
    assert!(matches!(
        hyphenation.add(spec("h.de", "fr")),
        Err(RegisterError::Duplicate { .. })
    ));
    assert!(hyphenation.contains("h.de-at") && !hyphenation.contains("h.fr"));
    let de = breaks(&hyphenation, "de", "abcd");
    let at = breaks(&hyphenation, "de-AT", "abcd");
    let en = breaks(&hyphenation, "en-US", "abcd");
    assert_ne!(de, at);
    assert_ne!(de, en);
    // Case is ignored; a region not known takes the base language.
    assert_eq!(breaks(&hyphenation, "DE-at", "abcd"), at);
    assert_eq!(breaks(&hyphenation, "de-CH", "abcd"), de);
    assert_eq!(breaks(&hyphenation, "de_CH", "abcd"), de);
    // The base language takes a region's patterns where it has none.
    assert_eq!(breaks(&hyphenation, "en", "abcd"), en);
    assert_eq!(breaks(&hyphenation, "en-GB", "abcd"), en);
    assert_eq!(breaks(&hyphenation, "fr", "abcd"), None);
    assert_eq!(breaks(&hyphenation, "", "abcd"), None);
    // Patterns that are no patterns are refused.
    let broken = HyphenationSpec {
        id: Id::new_static("h.broken"),
        language: Cow::Borrowed("xx"),
        patterns: b"not compiled patterns",
    };
    assert!(matches!(
        hyphenation.add(broken),
        Err(RegisterError::Invalid { .. })
    ));
    assert!(!hyphenation.contains("h.broken"));
}

#[test]
fn later_hyphenation_of_a_language_wins() {
    let mut hyphenation = Hyphenation::new();
    for (id, text) in [("h.one", "a1b"), ("h.two", "b1c")] {
        hyphenation
            .add(HyphenationSpec {
                id: Id::parse(id).unwrap(),
                language: Cow::Borrowed("de"),
                patterns: Box::leak(
                    paperpaw_sdk::hyphen::compile(text, "", 1, 1).into_boxed_slice(),
                ),
            })
            .unwrap();
    }
    let mut only_two = Hyphenation::new();
    only_two
        .add(HyphenationSpec {
            id: Id::new_static("h.two"),
            language: Cow::Borrowed("de"),
            patterns: Box::leak(paperpaw_sdk::hyphen::compile("b1c", "", 1, 1).into_boxed_slice()),
        })
        .unwrap();
    assert_eq!(
        breaks(&hyphenation, "de", "abcd"),
        breaks(&only_two, "de", "abcd")
    );
}

#[test]
fn services_are_found_by_id_and_type_and_the_first_wins() {
    let mut services = Services::new();
    services.add(Id::new_static("a.count"), Box::new(1u32));
    services.add(Id::new_static("a.count"), Box::new(2u32));
    services.add(Id::new_static("a.name"), Box::new(String::from("x")));
    assert_eq!(services.get::<u32>("a.count"), Some(&1));
    assert_eq!(
        services.get::<String>("a.name").map(String::as_str),
        Some("x")
    );
    assert_eq!(services.get::<&str>("a.name"), None);
    assert!(!services.has("a.none"));
    services.contribute(
        Id::new_static("a.point"),
        Id::new_static("b"),
        Cow::Borrowed("1"),
    );
    services.contribute(
        Id::new_static("a.point"),
        Id::new_static("c"),
        Cow::Borrowed("2"),
    );
    let from: Vec<(&str, &str)> = services
        .contributions("a.point")
        .map(|(plugin, text)| (plugin.as_str(), text))
        .collect();
    assert_eq!(from, [("b", "1"), ("c", "2")]);
    assert_eq!(services.contributions("a.none").count(), 0);
    services.shrink_to_fit();
    assert!(format!("{services:?}").contains("a.count"));
}

fn format(id: &'static str, extensions: &[&'static str]) -> FormatSpec {
    FormatSpec {
        id: Id::new_static(id),
        title: Cow::Borrowed(""),
        extensions: extensions.iter().map(|e| Cow::Borrowed(*e)).collect(),
        open: Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        prepared: None,
    }
}

#[test]
fn formats_are_found_by_the_longest_extension_and_later_wins() {
    let mut formats = Formats::new();
    formats.add(format("f.zip", &["zip"])).unwrap();
    formats.add(format("f.fb2", &["fb2", "fb2.zip"])).unwrap();
    formats.add(format("f.epub", &["epub"])).unwrap();
    formats.add(format("f.epub2", &["EPUB"])).unwrap();
    assert!(matches!(
        formats.add(format("f.zip", &["x"])),
        Err(RegisterError::Duplicate { .. })
    ));
    let id = |path: &str| formats.for_path(path).map(|f| f.id.as_str());
    assert_eq!(id("a/book.fb2.zip"), Some("f.fb2"));
    assert_eq!(id("a/book.zip"), Some("f.zip"));
    assert_eq!(id("Book.EpUb"), Some("f.epub2"));
    // The extension is the name's, after a dot, not the folder's.
    assert_eq!(id("a.epub/book"), None);
    assert_eq!(id("epub"), None);
    assert_eq!(id("bookepub"), None);
    assert_eq!(id("ünïcödé.epub"), Some("f.epub2"));
    assert_eq!(id(""), None);
    assert!(formats.supports("x.fb2") && !formats.supports("x.pdf"));
    assert!(formats.contains("f.zip") && !formats.contains("f.pdf"));
    // Opening what no format reads says so.
    let storage = paperpaw_sdk::MemoryStorage::new();
    assert!(matches!(
        formats.open(&storage, "x.pdf"),
        Err(DocumentError::UnsupportedFormat)
    ));
    assert!(formats.prepared(&storage, "x.pdf", &formats.options()));
}

#[test]
fn register_errors_say_what_is_wrong() {
    let id = Id::new_static("a.b");
    assert_eq!(
        RegisterError::Namespace { id: id.clone() }.to_string(),
        "`a.b` is outside the plugin's namespace"
    );
    assert_eq!(
        RegisterError::Duplicate { id }.to_string(),
        "`a.b` is already registered"
    );
    assert_eq!(
        RegisterError::Parse {
            line: 3,
            message: Cow::Borrowed("bad")
        }
        .to_string(),
        "line 3: bad"
    );
    assert_eq!(
        RegisterError::Invalid {
            message: Cow::Borrowed("why")
        }
        .to_string(),
        "why"
    );
    assert!(Priority::FALLBACK < Priority::DEFAULT && Priority::DEFAULT < Priority::PREFERRED);
    let manifest = Manifest {
        id: Id::new_static("demo"),
        name: Cow::Borrowed("Demo"),
        description: Cow::Borrowed(""),
        version: Version::new(1, 0, 0),
        sdk: paperpaw_sdk::SDK_VERSION,
        uses: paperpaw_sdk::Uses::NOTHING,
    };
    assert_eq!(manifest.clone(), manifest);
}

#[test]
fn actions_of_another_namespace_or_twice_are_refused() {
    let mut r = registrar();
    r.add_action(ActionSpec::new(Id::new_static("demo.go"), "x"))
        .unwrap();
    // A placed id is in another namespace.
    assert!(matches!(
        r.add_action(ActionSpec::new(Id::parse("demo@2.go").unwrap(), "x")),
        Err(RegisterError::Namespace { .. })
    ));
    // The plugin's own id is its namespace.
    r.add_action(ActionSpec::new(Id::new_static("demo"), "x"))
        .unwrap();
    assert!(matches!(
        r.add_action(ActionSpec::new(Id::new_static("demo"), "x")),
        Err(RegisterError::Duplicate { .. })
    ));
}
