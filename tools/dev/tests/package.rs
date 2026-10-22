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
    // As files.
    assert!(
        has(
            &report,
            "package.website",
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
    let mut context = context(
        "samples",
        &[root.join("airy-theme"), root.join("reading-nook")],
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
