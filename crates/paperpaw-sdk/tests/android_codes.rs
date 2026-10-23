//! Android installs a version over another only if its version code is
//! higher, so the codes `shells/android/version-code.sh` gives are in the
//! order the update plugin puts versions in
//! (`firmware::compare_versions`), and versions they cannot order are
//! refused: installing one wrongly fails, and uninstalling to go on takes
//! the reader's data with it.

use std::cmp::Ordering;
use std::path::Path;
use std::process::Command;

use paperpaw_sdk::firmware::compare_versions;

/// Whether the script runs here: in the repository, with a bash.
fn runs() -> bool {
    code("1.2.3") == Some(Some(1_020_399))
}

/// The code of `version`; `None` where the script refuses it.
fn code(version: &str) -> Option<Option<u64>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shells/android/version-code.sh");
    if !script.exists() {
        return None;
    }
    // On Windows `bash` is found in the system's folder first, WSL's,
    // which does not take its paths; Git's `sh` is a bash.
    let shell = if cfg!(windows) { "sh" } else { "bash" };
    let out = Command::new(shell)
        .arg(&script)
        .arg(version)
        .output()
        .ok()?;
    Some(if out.status.success() {
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    } else {
        None
    })
}

#[test]
fn codes_order_versions_as_updates_do() {
    if !runs() {
        return;
    }
    let versions = [
        "0.4.0-dev",
        "0.4.0",
        "0.5.0-beta.1",
        "0.5.0-beta.2",
        "0.5.0-beta.10",
        "0.5.0-beta.49",
        "0.5.0-dev",
        "0.5.0-rc.1",
        "0.5.0-rc.2",
        "0.5.0-rc.48",
        "0.5.0",
        "0.5.1-beta.1",
        "0.5.1",
        "0.10.0",
        "0.99.99",
        "1.0.0-beta.1",
        "1.0.0",
        "99.99.99",
    ];
    let mut codes = Vec::new();
    for version in versions {
        let Some(code) = code(version) else {
            return;
        };
        codes.push(code.unwrap_or_else(|| panic!("{version} has no code")));
    }
    for (i, a) in versions.iter().enumerate() {
        for (j, b) in versions.iter().enumerate() {
            assert_eq!(
                codes[i].cmp(&codes[j]),
                compare_versions(a, b),
                "{a} ({}) and {b} ({})",
                codes[i],
                codes[j]
            );
        }
    }
    assert_eq!(
        compare_versions("0.5.0-dev", "0.5.0-beta.3"),
        Ordering::Greater
    );
}

#[test]
fn versions_the_codes_cannot_order_are_refused() {
    if !runs() {
        return;
    }
    for version in [
        "0.100.0",
        "0.5.100",
        "100.0.0",
        "0.05.0",
        "0.5.0-beta.0",
        "0.5.0-beta.50",
        "0.5.0-rc.49",
        "0.5.0-alpha.1",
        "0.5.0-dev.1",
        "0.5.0-",
        "0.5",
    ] {
        let Some(code) = code(version) else {
            return;
        };
        assert_eq!(code, None, "{version}");
    }
}
