//! The lint command on a real screen in the pseudo-language and on a
//! data plugin made to be found wanting.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_dev::cases;
use paperpaw_dev::cli::Context;
use paperpaw_dev::lint::{self, PSEUDO, pseudo};
use paperpaw_dev::report::{Level, Report};
use paperpaw_dev::session::Session;
use paperpaw_dev::target::{DataPlugin, Target};
use paperpaw_dev::variant::Variant;

fn out(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("paperpaw-dev-lint-{name}-{}", std::process::id()))
}

#[test]
fn the_settings_list_in_the_pseudo_language() {
    let context = Context::new(
        Target {
            plugins: vec!["settings".into()],
            ..Target::default()
        },
        out("pseudo"),
    );
    let files = lint::pseudo_plugin(&context).unwrap();
    assert!(files.iter().any(|(path, _)| path.ends_with("/plugin.ini")));
    assert!(files.len() > 2, "the catalogs are split: {}", files.len());
    let variant = Variant {
        language: PSEUDO.into(),
        ..Variant::x4()
    };
    let mut session = Session::new(&context, &variant).unwrap();
    assert_eq!(session.device.kernel.translator().language(), PSEUDO);
    let shown = cases::show_screen(&mut session, "settings.list", &[]).unwrap();
    assert!(shown.opened());
    let texts: Vec<&str> = shown
        .described
        .iter()
        .map(|d| d.label.as_str())
        .filter(|l| l.chars().filter(|c| c.is_alphabetic()).count() > 3)
        .collect();
    assert!(!texts.is_empty());
    // Every word drawn is the pseudo-language's: accented, longer.
    for text in &texts {
        assert!(
            text.chars().any(pseudo::is_accented),
            "not in the pseudo-language: {text:?} (of {texts:?})"
        );
    }
    let loaded = session
        .device
        .kernel
        .translator()
        .catalogs()
        .iter()
        .any(|c| c.language() == PSEUDO);
    assert!(loaded);
    drop(session);
    let _ = std::fs::remove_dir_all(&context.out);
}

/// The fixture data plugin, dashes put in (the repository has none).
fn wanting() -> DataPlugin {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lint/data-plugin");
    let mut plugin = DataPlugin::read(&from).unwrap();
    let text = String::from_utf8(plugin.files["plugin.ini"].to_vec()).unwrap();
    let text = text.replace("wanting;", "wanting \u{2014}");
    plugin
        .files
        .insert("plugin.ini".into(), text.into_bytes().into());
    // Handed in, as a community plugin is: not one of this repository's.
    plugin.folder = None;
    plugin
}

/// The lint command on [`wanting`], the libraries in folders or in
/// memory.
fn lint_wanting(dir: &Path, in_memory: bool) -> Report {
    let mut context = Context::new(
        Target {
            plugins: vec!["wanting".into()],
            data: vec![wanting()],
            ..Target::default()
        },
        dir.join("out"),
    );
    context.in_memory = in_memory;
    // One device with buttons, quick.
    context.profiles.truncate(1);
    context.languages = Some(vec!["en".into(), "de".into()]);
    let mut report = Report::default();
    lint::run(&context, &mut report).unwrap();
    report
}

#[test]
fn a_data_plugin_found_wanting() {
    let dir = out("data");
    let report = lint_wanting(&dir, false);
    let found = |check: &str, place: &str| {
        report
            .findings
            .iter()
            .find(|f| f.check == check && f.place.starts_with(place))
    };
    // Not in this repository: a community plugin's dash is a warning.
    let dash = found("lint.dash", "plugin.ini:3").expect("dash");
    assert_eq!(dash.level, Level::Warning);
    assert!(found("lint.unknown-setting", "settings.ini:3").is_some());
    assert!(found("lint.unknown-setting", "presets.ini").is_some());
    assert!(found("lint.untranslated", "wanting.wide").is_some());
    let outside = found("lint.id", "i18n/en.ini").expect("key outside");
    assert!(outside.message.contains("`elsewhere.title`"));
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.plugin.as_deref().is_none_or(|p| p == "wanting")),
        "{:#?}",
        report.findings
    );
    // In memory, as in a browser: the same findings, no library written.
    let _ = std::fs::remove_dir_all(&dir);
    let in_memory = lint_wanting(&dir, true);
    let places = |report: &Report| {
        let mut all: Vec<String> = report
            .findings
            .iter()
            .map(|f| format!("{} {}", f.check, f.place))
            .collect();
        all.sort();
        all
    };
    assert_eq!(places(&in_memory), places(&report));
    assert!(!dir.join("out/work").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
