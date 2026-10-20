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

/// A data plugin switched off keeps of its translations only its name
/// and description, to be listed: the rest (a language pack's whole
/// catalogs) would take memory for nothing.
#[test]
fn a_data_plugin_switched_off_keeps_only_what_lists_it() {
    let catalog = "[meta]\nlanguage = en\nname = English\n\n[messages]\nbig.plugin.name = Big print\nbig.other = Taken\n";
    let storage = MemoryStorage::new()
        .with("plugins/big/plugin.ini", "[plugin]\nname = Big\n")
        .with("plugins/big/i18n/en.ini", catalog);
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[plugins]\nbig = off\n")
        .build()
        .unwrap();
    let info = kernel.plugins().get("big").unwrap();
    assert_eq!(info.state, paperpaw_sdk::PluginState::Off);
    assert_eq!(
        kernel.translator().get("big.plugin.name"),
        Some("Big print")
    );
    assert_eq!(kernel.translator().get("big.other"), None);
}

/// A kernel with the data plugin `big` installed, version 1.
fn installed_kernel(storage: MemoryStorage, log: &Log) -> Kernel {
    let storage = storage
        .with("plugins/big/plugin.ini", "[plugin]\nversion = 1.0.0\n")
        .with(
            "plugins/big/i18n/en.ini",
            "[meta]\nlanguage = en\nname = English\n\n[messages]\nbig.plugin.name = Big\n",
        );
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", log))
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.take_warnings();
    kernel
}

fn install(kernel: &mut Kernel, id: &'static str, folder: &str) {
    kernel.apply(Command::InstallPlugin(
        Id::new_static(id),
        String::from(folder),
    ));
    kernel.finish();
}

/// An update that would not register (a translation with a syntax error)
/// is refused before anything changes: the version before stays.
#[test]
fn an_update_that_would_not_register_leaves_the_version_before() {
    let log = log();
    let storage = MemoryStorage::new()
        .with(
            "plugins/.big.part/plugin.ini",
            "[plugin]\nid = big\nversion = 2.0.0\n",
        )
        .with("plugins/.big.part/i18n/de.ini", "[meta\nlanguage = de\n");
    let mut kernel = installed_kernel(storage, &log);
    install(&mut kernel, "big", "plugins/.big.part");
    let big = kernel.plugins().get("big").unwrap();
    assert_eq!(big.manifest.version, Version::new(1, 0, 0));
    assert_eq!(big.state, paperpaw_sdk::PluginState::Running);
    assert!(kernel.storage().exists("plugins/big/plugin.ini"));
    assert!(!kernel.storage().exists("plugins/.big.part/plugin.ini"));
    assert!(!kernel.take_warnings().is_empty());
}

/// A data plugin with the id of a plugin built in is refused: it would
/// unload that plugin's fonts, translations and listing.
#[test]
fn a_data_plugin_may_not_take_the_place_of_one_built_in() {
    let log = log();
    let storage = MemoryStorage::new().with(
        "plugins/.a.part/plugin.ini",
        "[plugin]\nid = a\nname = Fake\n",
    );
    let mut kernel = installed_kernel(storage, &log);
    install(&mut kernel, "a", "plugins/.a.part");
    let a = kernel.plugins().get("a").unwrap();
    assert_eq!(a.state, paperpaw_sdk::PluginState::Running);
    assert_eq!(a.manifest.name, "Test");
    assert!(!kernel.storage().exists("plugins/a/plugin.ini"));
    assert!(!kernel.storage().exists("plugins/.a.part/plugin.ini"));
}

/// Installing a plugin from the folder it is installed in changes
/// nothing: it is not removed.
#[test]
fn installing_a_plugin_from_its_own_folder_keeps_it() {
    let log = log();
    let mut kernel = installed_kernel(MemoryStorage::new(), &log);
    install(&mut kernel, "big", "plugins/big");
    assert!(kernel.storage().exists("plugins/big/plugin.ini"));
    assert!(kernel.plugins().get("big").is_some());
    assert!(!kernel.take_warnings().is_empty());
}
