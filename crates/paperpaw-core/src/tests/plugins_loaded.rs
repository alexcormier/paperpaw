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
