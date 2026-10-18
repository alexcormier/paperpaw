use core::fmt;

use quireink_i18n::Translator;
use quireink_platform::{DeviceInfo, Instant, Storage, SystemStatus};

use crate::action::Actions;
use crate::fonts::Fonts;
use crate::formats::Formats;
use crate::hyphenation::Hyphenation;
use crate::menus::Menus;
use crate::plugins::Plugins;
use crate::settings::Settings;
use crate::status::StatusItems;

/// What the device and its shell can do: plugins offer only what works
/// here, and menu entries that need more are left out
/// ([`MenuNeed`](crate::MenuNeed)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Capabilities {
    /// A touch screen.
    pub touch: bool,
    /// The network can be reached: HTTP requests
    /// ([`Command::Http`](crate::Command::Http)), servers and connections.
    /// Not whether it is connected now: where QuireInk manages Wi-Fi
    /// ([`Env::wifi`]), work waits for a network a while.
    pub network: bool,
    /// QuireInk manages Wi-Fi itself ([`Env::wifi`]).
    pub wifi: bool,
    /// QuireInk is the device's firmware and updates it
    /// ([`Env::firmware`]).
    pub firmware: bool,
    /// Text is spoken aloud ([`Command::Speak`](crate::Command::Speak)).
    pub speech: bool,
    /// QuireInk shows a screen while the device sleeps
    /// ([`actions::SLEEP`](crate::builtin::actions::SLEEP)); where the
    /// system sleeps on its own (Android), it shows its own.
    pub sleep: bool,
    /// The screen can be turned
    /// ([`settings::ROTATION`](crate::builtin::settings::ROTATION)).
    pub rotation: bool,
    /// The screen is shown in the colours of
    /// [`settings::TEXT_COLOR`](crate::builtin::settings::TEXT_COLOR) and
    /// [`settings::PAPER_COLOR`](crate::builtin::settings::PAPER_COLOR).
    pub colors: bool,
    /// Buttons lie in a row below the screen, with a row of hints above
    /// them ([`settings::BUTTON_HINTS`](crate::builtin::settings::BUTTON_HINTS)).
    pub button_row: bool,
    /// The system has a clipboard of its own, shared with other apps.
    pub system_clipboard: bool,
    /// The panel shows black and white alone (no grays), e.g. the X4.
    pub black_and_white: bool,
}

/// Read-only view of the environment a plugin runs in.
///
/// Only the kernel makes one (fields are added in minor versions).
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct Env<'a> {
    pub device: &'a DeviceInfo,
    /// Time of the event being handled.
    pub now: Instant,
    pub metrics: Metrics,
    pub settings: &'a Settings,
    pub i18n: &'a Translator,
    pub fonts: &'a Fonts,
    pub formats: &'a Formats,
    /// Hyphenation patterns by language.
    pub hyphenation: &'a Hyphenation,
    pub menus: &'a Menus,
    /// Every action, e.g. to offer them in a menu editor.
    pub actions: &'a Actions,
    /// Every plugin with its state, for plugin managers.
    pub plugins: &'a Plugins,
    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](crate::time) for the local time).
    pub status: &'a SystemStatus,
    /// What the device did since QuireInk started: refreshes, input (see
    /// [`usage`](crate::usage)).
    pub usage: &'a crate::usage::Usage,
    /// Items for status lines, from every plugin.
    pub status_items: &'a StatusItems,
    /// Filters of book text before it is laid out, from every plugin.
    pub text_filters: &'a crate::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](crate::decorations)).
    pub decorators: &'a crate::decorations::Decorators,
    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// [`Registrar::add_stylesheet`](crate::Registrar::add_stylesheet)).
    pub stylesheets: &'a [alloc::borrow::Cow<'static, str>],
    /// Folders of what plugins keep for each book
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)).
    pub book_data: &'a [crate::library::BookData],
    /// Widgets for screens that show them, from every plugin.
    pub widgets: &'a crate::widgets::Widgets,
    /// The steps of the first-run setup, from every plugin (see
    /// [`setup`](crate::setup)).
    pub setup: &'a crate::setup::SetupSteps,
    /// What plugins offer each other (see [`services`](crate::services)).
    pub services: &'a crate::services::Services,
    /// What buttons, taps and swipes do (see [`keys`](crate::keys)).
    pub keys: &'a dyn crate::keys::KeyBindings,
    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub caps: Capabilities,
    /// Wi-Fi, where QuireInk manages it itself (see
    /// [`wifi`](crate::wifi)); `None` where the system does.
    pub wifi: Option<&'a crate::wifi::Wifi>,
    /// Firmware updates, where QuireInk is the device's firmware (see
    /// [`firmware`](crate::firmware)); `None` where it is an app.
    pub firmware: Option<&'a crate::firmware::Firmware>,
    /// Whether there is text on the clipboard, copied here
    /// ([`Command::SetClipboard`](crate::Command::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Command::ReadClipboard`](crate::Command::ReadClipboard).
    pub clipboard: bool,
    /// The slots some screen is bound to (see [`Env::has_slot`]).
    pub slots: &'a [crate::Id],
    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub screens: &'a [(crate::Id, alloc::borrow::Cow<'static, str>)],
    /// The folders reading fonts are found in at start-up, the first one
    /// QuireInk's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub font_dirs: &'a [alloc::string::String],
    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](crate::secrets)).
    pub secrets: &'a crate::secrets::Secrets,
    /// The device's storage. Plugins keep their own files below
    /// [`Env::plugin_dir`].
    pub storage: &'a dyn Storage,
}

/// The fields of an [`Env`], for the kernel to make one (plugins get it
/// made).
#[doc(hidden)]
#[allow(missing_debug_implementations)]
pub struct EnvParts<'a> {
    pub device: &'a DeviceInfo,
    /// Time of the event being handled.
    pub now: Instant,
    pub metrics: Metrics,
    pub settings: &'a Settings,
    pub i18n: &'a Translator,
    pub fonts: &'a Fonts,
    pub formats: &'a Formats,
    /// Hyphenation patterns by language.
    pub hyphenation: &'a Hyphenation,
    pub menus: &'a Menus,
    /// Every action, e.g. to offer them in a menu editor.
    pub actions: &'a Actions,
    /// Every plugin with its state, for plugin managers.
    pub plugins: &'a Plugins,
    /// The time of day and the battery, as far as the device knows them
    /// (see [`time`](crate::time) for the local time).
    pub status: &'a SystemStatus,
    /// What the device did since QuireInk started: refreshes, input (see
    /// [`usage`](crate::usage)).
    pub usage: &'a crate::usage::Usage,
    /// Items for status lines, from every plugin.
    pub status_items: &'a StatusItems,
    /// Filters of book text before it is laid out, from every plugin.
    pub text_filters: &'a crate::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin (see
    /// [`decorations`](crate::decorations)).
    pub decorators: &'a crate::decorations::Decorators,
    /// Plugins' stylesheets for books, in the order of the plugins (see
    /// [`Registrar::add_stylesheet`](crate::Registrar::add_stylesheet)).
    pub stylesheets: &'a [alloc::borrow::Cow<'static, str>],
    /// Folders of what plugins keep for each book
    /// ([`Registrar::add_book_data`](crate::Registrar::add_book_data)).
    pub book_data: &'a [crate::library::BookData],
    /// Widgets for screens that show them, from every plugin.
    pub widgets: &'a crate::widgets::Widgets,
    /// The steps of the first-run setup, from every plugin (see
    /// [`setup`](crate::setup)).
    pub setup: &'a crate::setup::SetupSteps,
    /// What plugins offer each other (see [`services`](crate::services)).
    pub services: &'a crate::services::Services,
    /// What buttons, taps and swipes do (see [`keys`](crate::keys)).
    pub keys: &'a dyn crate::keys::KeyBindings,
    /// What the device and its shell can do (network, speech, the sleep
    /// screen …).
    pub caps: Capabilities,
    /// Wi-Fi, where QuireInk manages it itself (see
    /// [`wifi`](crate::wifi)); `None` where the system does.
    pub wifi: Option<&'a crate::wifi::Wifi>,
    /// Firmware updates, where QuireInk is the device's firmware (see
    /// [`firmware`](crate::firmware)); `None` where it is an app.
    pub firmware: Option<&'a crate::firmware::Firmware>,
    /// Whether there is text on the clipboard, copied here
    /// ([`Command::SetClipboard`](crate::Command::SetClipboard)) or, where
    /// the system has a clipboard, in another app; read it with
    /// [`Command::ReadClipboard`](crate::Command::ReadClipboard).
    pub clipboard: bool,
    /// The slots some screen is bound to (see [`Env::has_slot`]).
    pub slots: &'a [crate::Id],
    /// Every screen: its id and the translation key of its name, e.g. for
    /// settings that differ per screen (what gestures do there).
    pub screens: &'a [(crate::Id, alloc::borrow::Cow<'static, str>)],
    /// The folders reading fonts are found in at start-up, the first one
    /// QuireInk's own (where new fonts go); empty where the device has
    /// only its built-in fonts.
    pub font_dirs: &'a [alloc::string::String],
    /// Passwords and keys, kept for every plugin in one place (see
    /// [`secrets`](crate::secrets)).
    pub secrets: &'a crate::secrets::Secrets,
    /// The device's storage. Plugins keep their own files below
    /// [`Env::plugin_dir`].
    pub storage: &'a dyn Storage,
}

impl<'a> From<EnvParts<'a>> for Env<'a> {
    fn from(p: EnvParts<'a>) -> Self {
        Self {
            device: p.device,
            now: p.now,
            metrics: p.metrics,
            settings: p.settings,
            i18n: p.i18n,
            fonts: p.fonts,
            formats: p.formats,
            hyphenation: p.hyphenation,
            menus: p.menus,
            actions: p.actions,
            plugins: p.plugins,
            status: p.status,
            usage: p.usage,
            status_items: p.status_items,
            text_filters: p.text_filters,
            decorators: p.decorators,
            stylesheets: p.stylesheets,
            book_data: p.book_data,
            widgets: p.widgets,
            setup: p.setup,
            services: p.services,
            keys: p.keys,
            caps: p.caps,
            wifi: p.wifi,
            firmware: p.firmware,
            clipboard: p.clipboard,
            slots: p.slots,
            screens: p.screens,
            font_dirs: p.font_dirs,
            secrets: p.secrets,
            storage: p.storage,
        }
    }
}

impl fmt::Debug for Env<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Env")
            .field("device", &self.device.id)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

impl Env<'_> {
    /// Whether a screen is bound to `slot`, e.g. whether a browser is
    /// there to open links to the web
    /// ([`slots::BROWSER`](crate::builtin::slots::BROWSER)).
    pub fn has_slot(&self, slot: &crate::Id) -> bool {
        self.slots.contains(slot)
    }

    /// The directory where plugin `plugin` keeps its data, e.g.
    /// `.quireink/plugins/reader` (in QuireInk's own folder,
    /// [`data::DIR`](crate::data::DIR)).
    pub fn plugin_dir(plugin: &str) -> alloc::string::String {
        alloc::format!("{}/plugins/{plugin}", crate::data::DIR)
    }
}

/// Converts physical lengths to pixels for the current screen, so layouts
/// look the same on low- and high-density displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    pub dpi: u16,
}

impl Metrics {
    pub const fn new(dpi: u16) -> Self {
        Self { dpi }
    }

    /// Pixels for `mm` millimetres (rounded, at least 1 for non-zero input).
    pub const fn mm(&self, mm: u32) -> u32 {
        self.tenth_mm(mm * 10)
    }

    /// Pixels for a font size in typographic points (1/72 inch).
    pub fn pt(&self, points: f32) -> f32 {
        points * f32::from(self.dpi) / 72.0
    }

    /// Pixels for `tenths` tenths of a millimetre.
    pub const fn tenth_mm(&self, tenths: u32) -> u32 {
        let px = (tenths * self.dpi as u32 + 127) / 254;
        if px == 0 && tenths > 0 { 1 } else { px }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_millimetres() {
        let metrics = Metrics::new(254);
        assert_eq!(metrics.mm(1), 10);
        assert_eq!(metrics.tenth_mm(5), 5);
        assert_eq!(Metrics::new(100).tenth_mm(1), 1);
        assert_eq!(Metrics::new(100).mm(0), 0);
    }
}
