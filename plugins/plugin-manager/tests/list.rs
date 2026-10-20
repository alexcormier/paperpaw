//! The plugin manager with the real kernel: its list follows plugins
//! installed while it is open (e.g. from a store opened from it).

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_plugin_manager::{OPEN, PluginManager};
use paperpaw_sdk::{
    ActionSpec, Command, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

/// Installs the data plugin downloaded into `plugins/.airy.part`, as a
/// store does.
const INSTALL: Id = Id::new_static("installer.install");

struct Installer;

impl Plugin for Installer {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("installer"),
            "Installer",
            Version::new(0, 1, 0),
        )
        .description("Installs a plugin.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(INSTALL, "installer.install").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::InstallPlugin(
                    Id::new_static("airy"),
                    String::from("plugins/.airy.part"),
                ));
            },
        ))
    }
}

/// What the screen shows, as one text.
fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn a_plugin_installed_while_the_list_is_open_is_listed() {
    let storage = MemoryStorage::new().with(
        "plugins/.airy.part/plugin.ini",
        "[plugin]\nid = airy\nname = Airy\ndescription = Roomy.\nversion = 1.0.0\n",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager)
        .plugin(Installer)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_action(Instant(100), OPEN);
    let shown_before = shown(&mut kernel);
    assert!(!shown_before.contains("Airy"), "{shown_before}");
    kernel.run_action(Instant(200), INSTALL);
    assert!(kernel.plugins().get("airy").is_some());
    let shown = shown(&mut kernel);
    assert!(shown.contains("Airy"), "{shown}");
    assert!(shown.contains("Roomy."), "{shown}");
}
