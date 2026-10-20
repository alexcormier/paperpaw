//! The standard PaperPaw app: the first-party plugins every shell installs.
//!
//! The kernel itself has no product features; a shell decides which plugins
//! it ships. This crate keeps that list in one place, so the emulator, the
//! Android app and the firmware ship the same app. Shells can still add,
//! leave out or replace any plugin.

#![no_std]

use paperpaw_core::KernelBuilder;

/// How the default fonts are shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fonts {
    /// Scalable fonts rendered at runtime (needs the `outline` feature).
    #[cfg(feature = "outline")]
    Outline,
    /// Bitmap font packs read in place, for small devices.
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    Packs,
}

/// Adds the first-party plugins to `builder`.
pub fn install(builder: KernelBuilder, fonts: Fonts) -> KernelBuilder {
    let builder = match fonts {
        #[cfg(feature = "outline")]
        Fonts::Outline => builder.plugin(paperpaw_plugin_fonts_default::DefaultFonts),
        #[cfg(any(feature = "packs", not(feature = "outline")))]
        Fonts::Packs => builder.plugin(paperpaw_plugin_fonts_default::DefaultFontPacks),
    };
    builder
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_format_epub::EpubFormat)
        .plugin(paperpaw_plugin_format_epub::HtmlFormat)
        .plugin(paperpaw_plugin_format_epub::Fb2Format)
        .plugin(paperpaw_plugin_format_epub::MarkdownFormat)
        .plugin(paperpaw_plugin_format_epub::CbzFormat)
        .plugin(paperpaw_plugin_format_xtc::XtcFormat)
        .plugin(paperpaw_plugin_hyphenation::Hyphenation)
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
        .plugin(paperpaw_plugin_reader::Reader)
        .plugin(paperpaw_plugin_files::Files)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_menu::Menu)
        .plugin(paperpaw_plugin_menu_editor::MenuEditor)
        .plugin(paperpaw_plugin_kosync::Kosync)
        .plugin(paperpaw_plugin_koreader::KoReader)
        .plugin(paperpaw_plugin_calibre::Calibre)
        .plugin(paperpaw_plugin_stats::Stats)
        .plugin(paperpaw_plugin_battery::Battery)
        .plugin(paperpaw_plugin_wifi::Wifi)
        .plugin(paperpaw_plugin_file_manager::FileManager)
        .plugin(paperpaw_plugin_transfer::Transfer)
        .plugin(paperpaw_plugin_opds::Opds)
        .plugin(paperpaw_plugin_fonts_browser::FontsBrowser)
        .plugin(paperpaw_plugin_store::Store)
        .plugin(paperpaw_plugin_rules::Rules)
        .plugin(paperpaw_plugin_lock::Lock)
        .plugin(paperpaw_plugin_profiles::Profiles)
        .plugin(paperpaw_plugin_keys::Keys)
        .plugin(paperpaw_plugin_wikipedia::Wikipedia)
        .plugin(paperpaw_plugin_aids::Aids)
        .plugin(paperpaw_plugin_vocabulary::Vocabulary)
        .plugin(paperpaw_plugin_sleep::Sleep)
        .plugin(paperpaw_plugin_translate::Translate)
        .plugin(paperpaw_plugin_news::News)
        .plugin(paperpaw_plugin_weather::Weather)
        .plugin(paperpaw_plugin_widgets::Widgets)
        .plugin_off(paperpaw_plugin_browser::Browser)
        .plugin_off(paperpaw_plugin_readwise::Readwise)
        .plugin_off(paperpaw_plugin_joplin::Joplin)
        .plugin_off(paperpaw_plugin_wallabag::Wallabag)
        .plugin(paperpaw_plugin_keyboard::Keyboard)
        .plugin(paperpaw_plugin_settings::SettingsPlugin)
        .plugin(paperpaw_plugin_plugin_manager::PluginManager)
        .plugin(paperpaw_plugin_diagnostics::Diagnostics)
        .plugin(paperpaw_plugin_update::Update)
        .plugin(paperpaw_plugin_about::About)
        .plugin(paperpaw_plugin_setup::Setup)
}
