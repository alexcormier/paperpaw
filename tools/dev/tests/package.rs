//! Packaging checks on data plugins: the samples pass but for what is
//! known, a fixture wrong in every file is caught, by the files and on
//! the device; releases and `.ppn` files.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::{Path, PathBuf};

use paperpaw_dev::cli::Context;
use paperpaw_dev::report::{Level, Report};
use paperpaw_dev::target::Target;

fn context(name: &str, paths: &[PathBuf]) -> Context {
    let out = std::env::temp_dir().join(format!(
        "paperpaw-dev-package-{name}-{}",
        std::process::id()
    ));
    let mut target = Target::default();
    for path in paths {
        target.take_path(path).unwrap();
    }
    let mut context = Context::new(target, out);
    // One device is enough here.
    context.profiles = vec![paperpaw_emulator::profiles::find("xteink-x4").unwrap()];
    context
}

fn run(context: &Context) -> Report {
    let mut report = Report::default();
    paperpaw_dev::package::run(context, &mut report).unwrap();
    let _ = std::fs::remove_dir_all(&context.out);
    report
}

fn has(report: &Report, check: &str, place: &str, level: Level) -> bool {
    report
        .findings
        .iter()
        .any(|f| f.check == check && f.place == place && f.level == level)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/package")
        .join(name)
}

#[test]
fn a_plugin_wrong_everywhere_is_caught() {
    let report = run(&context("broken", &[fixture("broken-theme")]));
    let text = report.text(Level::Info);
    // As files: a font pack in fonts/ is taken, but this one does not
    // load on the device.
    assert!(
        !has(
            &report,
            "package.website",
            "fonts/cyrillic.ppfp",
            Level::Error
        ),
        "{text}"
    );
    assert!(
        has(
            &report,
            "package.device",
            "fonts/cyrillic.ppfp",
            Level::Error
        ),
        "{text}"
    );
    assert!(
        has(&report, "package.files", "style.css", Level::Warning),
        "{text}"
    );
    // On the device.
    assert!(
        has(&report, "package.settings", "settings.ini:2", Level::Error),
        "{text}"
    );
    assert!(
        has(&report, "package.settings", "settings.ini:5", Level::Error),
        "{text}"
    );
    // A number out of its range, a choice not by its option's whole id,
    // and one place of a widget: each said with what it takes.
    let says = |place: &str, part: &str| {
        report
            .findings
            .iter()
            .any(|f| f.check == "package.settings" && f.place == place && f.message.contains(part))
    };
    assert!(says("settings.ini:8", "a whole number from"), "{text}");
    assert!(says("settings.ini:11", "files.view.covers"), "{text}");
    assert!(says("settings.ini:14", "`home.clock.date`"), "{text}");
    assert!(
        has(&report, "package.keymap", "keymap.ini:2", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.presets", "presets.ini:2", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.presets", "presets.ini:6", Level::Warning),
        "{text}"
    );
    assert!(
        has(&report, "package.fonts", "plugin.ini:9", Level::Error),
        "{text}"
    );
    assert!(
        has(
            &report,
            "package.device",
            "fonts/cyrillic.ppfp",
            Level::Error
        ),
        "{text}"
    );
    // A preset's name that is a key no catalog has.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "package.presets" && f.message.contains("broken-theme.x")),
        "{text}"
    );
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.plugin.as_deref() == Some("broken-theme"))
    );
}

#[test]
fn the_samples_load_and_a_release_is_compared() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/plugins");
    // A theme as a preset beside them, which PaperPaw 0.3 could not read.
    let themes = std::env::temp_dir().join(format!("paperpaw-dev-themes-{}", std::process::id()));
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("plugin.ini"),
        "[plugin]\nid = themes\nname = Themes\ndescription = A theme.\nversion = 1.0.0\nformats = 1\n",
    )
    .unwrap();
    std::fs::write(
        themes.join("presets.ini"),
        "[tall]\nkind = core.theme\nname = Tall\ndescription = Taller rows.\ncore.ui-row = 180\n",
    )
    .unwrap();
    let mut context = context(
        "samples",
        &[
            root.join("airy-theme"),
            root.join("reading-nook"),
            themes.clone(),
        ],
    );
    context.release = Some("0.3.0".into());
    let report = run(&context);
    let text = report.text(Level::Info);
    assert!(
        !has(&report, "package.device", "plugin.ini", Level::Error),
        "{text}"
    );
    // 0.3 had no presets in data plugins.
    assert!(
        has(&report, "package.release", "presets.ini", Level::Error),
        "{text}"
    );
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.level == Level::Error && f.check != "package.release")
        .collect();
    assert!(errors.is_empty(), "{text}");
    let _ = std::fs::remove_dir_all(&themes);
}

#[test]
fn ppn_files_are_checked_and_releases_must_be_versions() {
    let dir = std::env::temp_dir().join(format!("paperpaw-dev-ppn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hello-x4.ppn");
    std::fs::write(&file, b"PPNP").unwrap();
    let place = file.display().to_string();
    let mut context = context("ppn", &[file]);
    context.release = Some("not-a-version".into());
    let report = run(&context);
    // Cut short: the device refuses it.
    assert!(has(&report, "package.ppn", &place, Level::Error));
    assert!(has(
        &report,
        "package.release",
        "not-a-version",
        Level::Error
    ));
    assert_eq!(report.findings.len(), 2, "{}", report.text(Level::Info));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A code plugin's folder and its code name the same version: the
/// device loads no code of another.
#[test]
fn a_code_plugins_folder_names_its_codes_version() {
    let version = paperpaw_sdk::package_version!();
    let folder = |name: &str, version: &str| {
        let ini = format!(
            "[plugin]\nid = weather\nname = Weather\ndescription = The weather.\nversion = {version}\n"
        );
        let mut context = context(name, &[]);
        context.target.plugins = vec!["weather".into()];
        context.target.data = vec![paperpaw_dev::target::DataPlugin::new(
            "weather",
            [(String::from("plugin.ini"), ini.into_bytes())],
        )];
        run(&context)
    };
    // As the release writes it: a beta's plugin.ini names its numbers only.
    let release = version.to_string();
    let release = release.split('-').next().unwrap_or_default();
    let report = folder("version-same", release);
    assert!(!has(&report, "package.version", "plugin.ini", Level::Error));
    let report = folder("version-other", "9.9.9");
    assert!(
        has(&report, "package.version", "plugin.ini", Level::Error),
        "{}",
        report.text(Level::Info)
    );
}
