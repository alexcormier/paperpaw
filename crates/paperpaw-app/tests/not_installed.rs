//! Plugins a shell did not install at the start (the web flasher's, ADR
//! 0024) are named in a notice, with why; one notice shows at a time, so
//! several share it, and safe mode's wins.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::{CodeLoader, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::native::{Native, NotInstalled, NotInstalledReason, SafeMode, SafeModeReason};
use paperpaw_sdk::{Id, MemoryStorage, Plugin, PluginProblem, Storage};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

/// Loads nothing: no plugin here has code.
struct NoCode;

impl CodeLoader for NoCode {
    fn load(&mut self, _: &dyn Storage, _: &str) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Unsupported)
    }
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The text shown right after a start with `not_installed`, and in safe
/// mode if `safe_mode`.
fn shown(not_installed: &[(&'static str, NotInstalledReason)], safe_mode: bool) -> String {
    let mut native = Native::new("code.ppn");
    native.not_installed = not_installed
        .iter()
        .map(|&(id, reason)| NotInstalled::new(Id::new_static(id), reason))
        .collect();
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[core]\nlanguage = en\n")
        .code_plugins(native, Box::new(NoCode));
    let builder = match safe_mode {
        true => builder.safe_mode(SafeMode::new(SafeModeReason::KeyHeld)),
        false => builder,
    };
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_plugin_by_another_author_is_named() {
    let shown = shown(&[("clock", NotInstalledReason::OtherAuthor)], false);
    assert!(
        shown.contains("clock was not installed: it is signed by another author"),
        "{shown}"
    );
}

#[test]
fn several_share_a_notice() {
    let shown = shown(
        &[
            ("clock", NotInstalledReason::OtherAuthor),
            ("weather", NotInstalledReason::Damaged),
        ],
        false,
    );
    // By the name of the version installed, if there is one.
    assert!(shown.contains("Not installed: clock, Weather."), "{shown}");
}

#[test]
fn safe_mode_says_its_own() {
    let shown = shown(&[("clock", NotInstalledReason::Damaged)], true);
    assert!(shown.contains("The key was held at the start"), "{shown}");
    assert!(!shown.contains("not installed"), "{shown}");
}

#[test]
fn nothing_is_said_when_all_were_installed() {
    let shown = shown(&[], false);
    assert!(!shown.contains("installed"), "{shown}");
}
