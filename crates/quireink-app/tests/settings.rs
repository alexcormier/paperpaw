//! Every setting of the settings pages says what it does: a help text
//! below its name, in every language shipped (E2 in docs/PLANNING.md).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use quireink_core::Kernel;
use quireink_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use quireink_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-boox",
    name: "Test Boox",
    display: DisplayInfo {
        width: 1264,
        height: 1680,
        dpi: 300,
        format: PixelFormat::Gray8,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &["back", "page-forward", "page-back"],
    },
    memory: MemoryClass::Large,
};

fn fonts() -> quireink_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return quireink_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return quireink_app::Fonts::Outline;
}

/// Groups whose settings are a widget's looks, chosen where the widget is
/// edited and named plainly enough there.
fn is_widget_group(kernel: &Kernel, group: &str) -> bool {
    kernel.widgets().has(group)
}

#[test]
fn every_setting_has_a_help_text() {
    let storage = MemoryStorage::new().with(".quireink/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = quireink_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    let settings = kernel.settings();
    let missing: Vec<String> = settings
        .specs()
        .filter(|spec| !is_widget_group(&kernel, settings.group_of(spec).as_str()))
        .filter(|spec| spec.description.is_none())
        .map(|spec| spec.id.to_string())
        .collect();
    assert!(
        missing.is_empty(),
        "settings without a help text: {missing:#?}"
    );
}
