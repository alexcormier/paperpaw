//! Plugins as they are loaded and installed: ids that may not be taken,
//! versions replaced, what is left of them on the storage.

use super::*;
use paperpaw_sdk::MemoryStorage;

/// A plugin, built in or a data plugin, may not take the name of a
/// section of `paperpaw.ini` that holds no plugin's settings: they would
/// be read as menus, slots or fonts there.
#[test]
fn plugins_may_not_take_the_names_of_config_sections() {
    let log = log();
    let storage = MemoryStorage::new().with("plugins/fonts/plugin.ini", "[plugin]\nname = Fonts\n");
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("menus", &log))
        .storage(storage)
        .build()
        .unwrap();
    for id in ["menus", "fonts"] {
        let info = kernel.plugins().get(id).unwrap();
        assert_eq!(info.state, paperpaw_sdk::PluginState::Failed, "{id}");
    }
    assert!(kernel.settings().get("menus.level").is_none());
    let warnings = kernel.take_warnings();
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.message.contains("names a section"))
            .count(),
        2,
        "{warnings:?}"
    );
}

/// An install cut short by the power (the old version set aside, the new
/// one not yet in its place) loses nothing: the old version is put back
/// at the next start. Where the new one got there, what was set aside
/// goes.
#[test]
fn a_plugin_set_aside_by_an_install_cut_short_is_put_back() {
    let manifest = "[plugin]\nname = Large print\n";
    let storage = MemoryStorage::new()
        .with("plugins/.large.old/plugin.ini", manifest)
        .with(
            "plugins/.small.old/plugin.ini",
            "[plugin]\nversion = 1.0.0\n",
        )
        .with("plugins/small/plugin.ini", "[plugin]\nversion = 2.0.0\n");
    let kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    let storage = kernel.storage();
    assert!(kernel.plugins().get("large").is_some());
    assert!(storage.exists("plugins/large/plugin.ini"));
    assert!(!storage.exists("plugins/.large.old/plugin.ini"));
    let small = kernel.plugins().get("small").unwrap();
    assert_eq!(small.manifest.version, Version::new(2, 0, 0));
    assert!(!storage.exists("plugins/.small.old/plugin.ini"));
}
