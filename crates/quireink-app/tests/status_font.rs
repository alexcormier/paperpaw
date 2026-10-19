//! The status lines' font (`core.status-font`): the menus' at first, the
//! book's or a family of its own once chosen, for every reader (role
//! `core.font-status`).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use quireink_core::Kernel;
use quireink_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use quireink_sdk::MemoryStorage;
use quireink_sdk::builtin::{fonts, settings};

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

fn fonts() -> quireink_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return quireink_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return quireink_app::Fonts::Outline;
}

fn status(kernel: &Kernel) -> String {
    let font = kernel.fonts().role(fonts::STATUS.as_str()).unwrap();
    font.id().to_string()
}

fn role(kernel: &Kernel, role: &quireink_sdk::Id) -> String {
    kernel.fonts().role(role.as_str()).unwrap().id().to_string()
}

#[test]
fn the_status_lines_follow_their_setting() {
    let storage = MemoryStorage::new().with(".quireink/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage);
    let mut kernel = quireink_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    // As the menus at first.
    assert_eq!(status(&kernel), role(&kernel, &fonts::UI));
    assert_ne!(role(&kernel, &fonts::UI), role(&kernel, &fonts::READING));
    // As the book.
    let _ = kernel
        .set_setting(
            Instant(10),
            settings::STATUS_FONT.as_str(),
            settings::STATUS_FONT_BOOK,
        )
        .unwrap();
    assert_eq!(status(&kernel), role(&kernel, &fonts::READING));
    // A family of its own: every family installed is offered.
    let spec = kernel
        .settings()
        .spec(settings::STATUS_FONT.as_str())
        .unwrap();
    let quireink_sdk::SettingKind::Choice(choices) = &spec.kind else {
        panic!("a choice");
    };
    let families = kernel.fonts().families();
    assert!(families.iter().all(|f| choices.contains(f)), "{families:?}");
    let ui_family = families
        .iter()
        .find(|f| f.contains("Atkinson"))
        .unwrap()
        .to_string();
    let _ = kernel
        .set_setting(Instant(20), settings::STATUS_FONT.as_str(), &ui_family)
        .unwrap();
    assert_eq!(status(&kernel), role(&kernel, &fonts::UI));
}
