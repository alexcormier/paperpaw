//! What plugins add to QuireInk: a [`Contribution`] each, collected by the
//! [`Registrar`] a plugin is given in [`Plugin::register`](crate::Plugin).
//!
//! The registrar only collects; the kernel applies the contributions once
//! the plugin is done (and parses key maps and translations then). So
//! anything can look at what a plugin adds without running QuireInk, e.g.
//! a plugin's own tests or a plugin library:
//!
//! ```
//! # use quireink_sdk::{Contribution, Id, Plugin, Registrar};
//! # fn check(plugin: &dyn Plugin) {
//! let mut registrar = Registrar::new(plugin.manifest().id);
//! plugin.register(&mut registrar).unwrap();
//! for contribution in registrar.contributions() {
//!     // e.g. list its screens and settings
//!     let _ = (contribution.kind(), contribution.id());
//! }
//! # }
//! ```

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

use crate::action::ActionSpec;
use crate::event::EventHandler;
use crate::filters::{InputFilterSpec, RefreshFilterSpec, TextFilterSpec};
use crate::fonts::FontSpec;
use crate::formats::FormatSpec;
use crate::hyphenation::HyphenationSpec;
use crate::id::Id;
use crate::menus::{MenuEntry, MenuNeed, MenuSection};
use crate::plugin::{Priority, RegisterError, SlotSpec};
use crate::screen::{Hints, ScreenSpec};
use crate::settings::{SettingGroupSpec, SettingSpec};
use crate::setup::SetupStepSpec;
use crate::status::{StatusItemSpec, StatusLineSpec};
use crate::widgets::WidgetSpec;

/// One thing a plugin adds. New kinds come in minor versions. The large
/// ones are boxed: a plugin's list stays small on small devices.
#[non_exhaustive]
pub enum Contribution {
    Action(ActionSpec),
    Screen(ScreenSpec),
    Slot(SlotSpec),
    /// A screen proposed for a slot (see [`Registrar::bind_slot`]).
    SlotBinding {
        slot: Id,
        screen: Id,
        priority: Priority,
    },
    Setting(Box<SettingSpec>),
    SettingGroup(SettingGroupSpec),
    /// Another default for any plugin's setting (see
    /// [`Registrar::set_setting_default`]).
    SettingDefault {
        setting: Id,
        value: Cow<'static, str>,
    },
    /// Default key bindings (see [`Registrar::add_keymap`]).
    Keymap(Cow<'static, str>),
    KeymapPreset {
        id: Id,
        source: Cow<'static, str>,
    },
    Stylesheet(Cow<'static, str>),
    Font(Box<FontSpec>),
    FontFile {
        id: Id,
        path: String,
    },
    FontExtension {
        id: Id,
        path: String,
        extends: Id,
    },
    FontBinding {
        role: Id,
        font: Id,
        priority: Priority,
    },
    Format(Box<FormatSpec>),
    Hyphenation(HyphenationSpec),
    MenuEntry(MenuEntry),
    MenuSection(MenuSection),
    Icon {
        item: Id,
        icon: Cow<'static, str>,
    },
    MenuNeed {
        item: Id,
        need: MenuNeed,
    },
    ScreenHints {
        screen: Id,
        hints: Hints,
    },
    Subscription {
        event: Id,
        handler: EventHandler,
    },
    StatusItem(StatusItemSpec),
    StatusLine(StatusLineSpec),
    TextFilter(TextFilterSpec),
    InputFilter(InputFilterSpec),
    RefreshFilter(RefreshFilterSpec),
    Widget(Box<WidgetSpec>),
    SetupStep(Box<SetupStepSpec>),
    Service {
        id: Id,
        service: Box<dyn Any>,
    },
    /// Text for a point another plugin defined (see
    /// [`services`](crate::services)).
    ToPoint {
        point: Id,
        text: Cow<'static, str>,
    },
    /// A translation catalog (see `quireink-i18n`).
    Translations(Cow<'static, str>),
}

impl Contribution {
    /// What kind it is, e.g. `action` or `setting`.
    pub fn kind(&self) -> &'static str {
        match self {
            Contribution::Action(_) => "action",
            Contribution::Screen(_) => "screen",
            Contribution::Slot(_) => "slot",
            Contribution::SlotBinding { .. } => "slot-binding",
            Contribution::Setting(_) => "setting",
            Contribution::SettingGroup(_) => "setting-group",
            Contribution::SettingDefault { .. } => "setting-default",
            Contribution::Keymap(_) => "keymap",
            Contribution::KeymapPreset { .. } => "keymap-preset",
            Contribution::Stylesheet(_) => "stylesheet",
            Contribution::Font(_) => "font",
            Contribution::FontFile { .. } => "font-file",
            Contribution::FontExtension { .. } => "font-extension",
            Contribution::FontBinding { .. } => "font-binding",
            Contribution::Format(_) => "format",
            Contribution::Hyphenation(_) => "hyphenation",
            Contribution::MenuEntry(_) => "menu-entry",
            Contribution::MenuSection(_) => "menu-section",
            Contribution::Icon { .. } => "icon",
            Contribution::MenuNeed { .. } => "menu-need",
            Contribution::ScreenHints { .. } => "screen-hints",
            Contribution::Subscription { .. } => "subscription",
            Contribution::StatusItem(_) => "status-item",
            Contribution::StatusLine(_) => "status-line",
            Contribution::TextFilter(_) => "text-filter",
            Contribution::InputFilter(_) => "input-filter",
            Contribution::RefreshFilter(_) => "refresh-filter",
            Contribution::Widget(_) => "widget",
            Contribution::SetupStep(_) => "setup-step",
            Contribution::Service { .. } => "service",
            Contribution::ToPoint { .. } => "to-point",
            Contribution::Translations(_) => "translations",
        }
    }

    /// The id of what it adds, for those that add something of their own
    /// (an action, a screen …): in the plugin's namespace and once per
    /// kind (fonts and font files share one). `None` for bindings, menu
    /// entries and the like, which may name what other plugins added.
    pub fn id(&self) -> Option<&Id> {
        Some(match self {
            Contribution::Action(a) => &a.id,
            Contribution::Screen(s) => &s.id,
            Contribution::Slot(s) => &s.id,
            Contribution::Setting(s) => &s.id,
            Contribution::SettingGroup(g) => &g.id,
            Contribution::KeymapPreset { id, .. } => id,
            Contribution::Font(f) => &f.id,
            Contribution::FontFile { id, .. } => id,
            Contribution::FontExtension { id, .. } => id,
            Contribution::Format(f) => &f.id,
            Contribution::Hyphenation(h) => &h.id,
            Contribution::MenuSection(s) => &s.id,
            Contribution::StatusItem(i) => &i.id,
            Contribution::StatusLine(l) => &l.id,
            Contribution::TextFilter(f) => &f.id,
            Contribution::InputFilter(f) => &f.id,
            Contribution::RefreshFilter(f) => &f.id,
            Contribution::Widget(w) => &w.id,
            Contribution::SetupStep(s) => &s.id,
            Contribution::Service { id, .. } => id,
            _ => return None,
        })
    }

    /// The kind its id is unique within.
    fn id_kind(&self) -> &'static str {
        match self {
            Contribution::FontFile { .. } => "font",
            other => other.kind(),
        }
    }
}

impl fmt::Debug for Contribution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = f.debug_struct("Contribution");
        out.field("kind", &self.kind());
        if let Some(id) = self.id() {
            out.field("id", id);
        }
        out.finish_non_exhaustive()
    }
}

/// Collects a plugin's contributions (see the [module documentation](self)).
///
/// Every id a plugin adds must be in its namespace (`<plugin>.…`), and
/// once per kind; bindings, menu entries and key maps may name what other
/// plugins added.
#[derive(Debug)]
pub struct Registrar {
    plugin: Id,
    contributions: Vec<Contribution>,
}

impl Registrar {
    /// A registrar for plugin `plugin`.
    pub fn new(plugin: Id) -> Self {
        Self {
            plugin,
            contributions: Vec::new(),
        }
    }

    /// The id of the plugin being registered.
    pub fn plugin(&self) -> &Id {
        &self.plugin
    }

    /// What was added so far, in order.
    pub fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }

    /// What was added, for the kernel to apply.
    pub fn into_contributions(self) -> Vec<Contribution> {
        self.contributions
    }

    /// Adds `contribution`; every method below is a shorthand for one.
    pub fn add(&mut self, contribution: Contribution) -> Result<(), RegisterError> {
        if let Some(id) = contribution.id() {
            if !id.is_owned_by(&self.plugin) {
                return Err(RegisterError::Namespace { id: id.clone() });
            }
            let kind = contribution.id_kind();
            if self
                .contributions
                .iter()
                .any(|c| c.id_kind() == kind && c.id() == Some(id))
            {
                return Err(RegisterError::Duplicate { id: id.clone() });
            }
        }
        self.contributions.push(contribution);
        Ok(())
    }

    pub fn add_action(&mut self, action: ActionSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Action(action))
    }

    pub fn add_screen(&mut self, screen: ScreenSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Screen(screen))
    }

    pub fn add_slot(&mut self, slot: SlotSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Slot(slot))
    }

    /// Proposes `screen` for `slot`. The binding with the highest priority
    /// wins unless the user configured the slot explicitly.
    pub fn bind_slot(
        &mut self,
        slot: Id,
        screen: Id,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::SlotBinding {
            slot,
            screen,
            priority,
        })
    }

    pub fn add_setting(&mut self, setting: SettingSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Setting(Box::new(setting)))
    }

    /// Adds default key bindings in the QuireInk config format:
    ///
    /// ```ini
    /// [keymap.global]
    /// button.page-forward = core.next-page
    /// long.back = core.home
    /// tap.center = core.menu
    /// ```
    pub fn add_keymap(&mut self, source: Cow<'static, str>) -> Result<(), RegisterError> {
        self.add(Contribution::Keymap(source))
    }

    /// A stylesheet for books, applied after a book's own (where the
    /// format reads CSS, e.g. EPUB), e.g. for a theme that sets books
    /// apart; see [`Env::stylesheets`](crate::Env).
    pub fn add_stylesheet(&mut self, css: Cow<'static, str>) -> Result<(), RegisterError> {
        self.add(Contribution::Stylesheet(css))
    }

    /// Adds a key map preset (same format as [`Registrar::add_keymap`]) that
    /// users can select with the `core.keymap-preset` setting. Its title
    /// is the translation of its id.
    pub fn add_keymap_preset(
        &mut self,
        id: Id,
        source: Cow<'static, str>,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::KeymapPreset { id, source })
    }

    /// Adds a font. Bitmap packs work everywhere; scalable fonts need the
    /// `outline` feature.
    pub fn add_font(&mut self, font: FontSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Font(Box::new(font)))
    }

    /// Adds a font file on the storage (OpenType/TrueType): its family and
    /// style are read from its header, and it is loaded only when its
    /// family is chosen for reading. Needs scalable font support.
    pub fn add_font_file(&mut self, id: Id, path: String) -> Result<(), RegisterError> {
        self.add(Contribution::FontFile { id, path })
    }

    /// Adds a font pack on the storage (`.qifp`) with characters font
    /// `extends` lacks, e.g. Cyrillic for a font made for Latin; read a
    /// glyph at a time (see [`Fonts::add_extension`](crate::fonts::Fonts::add_extension)).
    pub fn add_font_extension(
        &mut self,
        id: Id,
        path: String,
        extends: Id,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::FontExtension { id, path, extends })
    }

    /// Proposes a font for a role such as
    /// [`builtin::fonts::UI`](crate::builtin::fonts::UI).
    pub fn bind_font(
        &mut self,
        role: Id,
        font: Id,
        priority: Priority,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::FontBinding {
            role,
            font,
            priority,
        })
    }

    /// Puts an action into a menu. Menus can be extended by any plugin and
    /// redefined by the user.
    pub fn add_menu_entry(&mut self, entry: MenuEntry) -> Result<(), RegisterError> {
        self.add(Contribution::MenuEntry(entry))
    }

    /// Adds a heading to a menu; entries ordered after it, up to the next
    /// heading, belong to it.
    pub fn add_menu_section(&mut self, section: MenuSection) -> Result<(), RegisterError> {
        self.add(Contribution::MenuSection(section))
    }

    /// Gives an action or setting an icon by name (see
    /// [`icons`](crate::icons)), shown with it in menus. Any item may get
    /// one, e.g. by an icon theme; the last registered wins.
    pub fn add_icon(
        &mut self,
        item: impl Into<Id>,
        icon: Cow<'static, str>,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::Icon {
            item: item.into(),
            icon,
        })
    }

    /// Offers an action or setting in menus only on devices that meet
    /// `need`, e.g. reading aloud only where the device can speak.
    pub fn set_menu_need(
        &mut self,
        item: impl Into<Id>,
        need: MenuNeed,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::MenuNeed {
            item: item.into(),
            need,
        })
    }

    /// When `screen` shows the row of hints for the buttons below it
    /// ([`Hints::Shown`](crate::Hints) unless set), and so the size it is
    /// made for.
    pub fn set_screen_hints(&mut self, screen: Id, hints: Hints) -> Result<(), RegisterError> {
        self.add(Contribution::ScreenHints { screen, hints })
    }

    /// Adds a group for the settings screen.
    pub fn add_setting_group(&mut self, group: SettingGroupSpec) -> Result<(), RegisterError> {
        self.add(Contribution::SettingGroup(group))
    }

    /// Adds a file format.
    pub fn add_format(&mut self, format: FormatSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Format(Box::new(format)))
    }

    /// Adds hyphenation patterns for a language.
    pub fn add_hyphenation(&mut self, spec: HyphenationSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Hyphenation(spec))
    }

    /// Subscribes to an event by its id, with what it carries as named
    /// strings, e.g. for events known only at run time; [`Registrar::on`]
    /// hears an [`Event`](crate::event::Event) with its data.
    pub fn subscribe(
        &mut self,
        event: impl Into<Id>,
        handler: EventHandler,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::Subscription {
            event: event.into(),
            handler,
        })
    }

    /// Hears `event` with what it carries (see [`event`](crate::event)):
    /// `handler` is not called for an emit of it that lacks what it needs.
    pub fn on<P: crate::event::EventData>(
        &mut self,
        event: crate::event::Event<P>,
        handler: impl Fn(&crate::Env<'_>, &P, &mut Vec<crate::Command>) + 'static,
    ) -> Result<(), RegisterError> {
        self.subscribe(
            event.id().clone(),
            Box::new(move |env, params, out| {
                if let Some(data) = P::from_params(params) {
                    handler(env, &data, out);
                }
            }),
        )
    }

    /// Adds an item that status lines can show, e.g. a clock or the
    /// reading time left.
    pub fn add_status_item(&mut self, item: StatusItemSpec) -> Result<(), RegisterError> {
        self.add(Contribution::StatusItem(item))
    }

    /// Adds a filter of book text before it is laid out (see
    /// [`filters`](crate::filters)).
    pub fn add_text_filter(&mut self, filter: TextFilterSpec) -> Result<(), RegisterError> {
        self.add(Contribution::TextFilter(filter))
    }

    /// Adds a filter of every input, before screens and key maps see it
    /// (see [`filters`](crate::filters)).
    pub fn add_input_filter(&mut self, filter: InputFilterSpec) -> Result<(), RegisterError> {
        self.add(Contribution::InputFilter(filter))
    }

    /// Adds a filter of how the panel is refreshed (see
    /// [`filters`](crate::filters)).
    pub fn add_refresh_filter(&mut self, filter: RefreshFilterSpec) -> Result<(), RegisterError> {
        self.add(Contribution::RefreshFilter(filter))
    }

    /// Adds a widget, e.g. for the home screen (see
    /// [`widgets`](crate::widgets)).
    pub fn add_widget(&mut self, widget: WidgetSpec) -> Result<(), RegisterError> {
        self.add(Contribution::Widget(Box::new(widget)))
    }

    /// Adds a step to the first-run setup (see [`setup`](crate::setup)).
    pub fn add_setup_step(&mut self, step: SetupStepSpec) -> Result<(), RegisterError> {
        self.add(Contribution::SetupStep(Box::new(step)))
    }

    /// Adds a whole status line, drawn by the plugin (see
    /// [`status`](crate::status)).
    pub fn add_status_line(&mut self, line: StatusLineSpec) -> Result<(), RegisterError> {
        self.add(Contribution::StatusLine(line))
    }

    /// Publishes a service for other plugins as `id` (see
    /// [`services`](crate::services)).
    pub fn add_service(&mut self, id: Id, service: Box<dyn Any>) -> Result<(), RegisterError> {
        self.add(Contribution::Service { id, service })
    }

    /// Adds `contribution` to `point`, a point another plugin defined
    /// (see [`services`](crate::services)).
    pub fn contribute(
        &mut self,
        point: Id,
        contribution: Cow<'static, str>,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::ToPoint {
            point,
            text: contribution,
        })
    }

    /// Adds a translation catalog (see `quireink-i18n`).
    pub fn add_translations(&mut self, source: Cow<'static, str>) -> Result<(), RegisterError> {
        self.add(Contribution::Translations(source))
    }

    /// Changes the default of any plugin's setting, written as in
    /// `quireink.ini` (e.g. a theme's text size). Device defaults and the
    /// user's choice still win; unknown settings are reported once all
    /// plugins are registered.
    pub fn set_setting_default(
        &mut self,
        setting: Id,
        value: Cow<'static, str>,
    ) -> Result<(), RegisterError> {
        self.add(Contribution::SettingDefault { setting, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &'static str) -> ActionSpec {
        ActionSpec {
            id: Id::new_static(id),
            title: Cow::Borrowed("x"),
            handler: None,
        }
    }

    #[test]
    fn ids_in_the_namespace_and_once() {
        let mut r = Registrar::new(Id::new_static("demo"));
        r.add_action(action("demo.go")).unwrap();
        assert_eq!(
            r.add_action(action("other.go")),
            Err(RegisterError::Namespace {
                id: Id::new_static("other.go")
            })
        );
        assert_eq!(
            r.add_action(action("demo.go")),
            Err(RegisterError::Duplicate {
                id: Id::new_static("demo.go")
            })
        );
        // Another kind may use the same id; bindings name others' ids.
        r.add_menu_section(MenuSection {
            menu: Id::new_static("core.main"),
            id: Id::new_static("demo.go"),
            title: Cow::Borrowed("x"),
            order: 0,
        })
        .unwrap();
        r.add_icon(Id::new_static("other.go"), Cow::Borrowed("book"))
            .unwrap();
        r.add_font_file(Id::new_static("demo.serif"), "a.ttf".into())
            .unwrap();
        assert!(matches!(
            r.add_font_extension(
                Id::new_static("demo.serif"),
                "b.qifp".into(),
                Id::new_static("x.y")
            ),
            Ok(())
        ));
        let kinds: Vec<&str> = r.contributions().iter().map(Contribution::kind).collect();
        // Kept small: a plugin's list is held while it registers.
        assert!(core::mem::size_of::<Contribution>() <= 96);
        assert_eq!(
            kinds,
            [
                "action",
                "menu-section",
                "icon",
                "font-file",
                "font-extension"
            ]
        );
    }
}
