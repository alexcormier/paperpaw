//! Contribution registries and plugin registration.

use alloc::borrow::Cow;
use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_i18n::Catalog;
use paperpaw_sdk::collections::VecMap;
use paperpaw_sdk::{
    ActionSpec, Actions, Contribution, EventHandler, FontSpec, FormatSpec, HyphenationSpec, Id,
    MenuEntry, Priority, RegisterError, ScreenSpec, SettingGroupSpec, SettingSpec, SlotSpec,
    StatusItemSpec, StatusLineSpec,
};

/// A candidate screen for a slot, proposed by a plugin.
#[derive(Debug)]
struct SlotCandidate {
    slot: Id,
    screen: Id,
    priority: Priority,
}

/// All active contributions except settings, key maps and translations,
/// which live in their own stores.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    pub(crate) screens: VecMap<Id, ScreenSpec>,
    pub(crate) slots: VecMap<Id, SlotSpec>,
    /// When screens show the row of hints, where they said.
    pub(crate) hints: VecMap<Id, paperpaw_sdk::Hints>,
    candidates: Vec<SlotCandidate>,
    /// Slot bindings from the user's configuration.
    user_bindings: VecMap<Id, Id>,
}

impl Registry {
    /// Releases unused capacity once registration is complete.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.screens.shrink_to_fit();
        self.slots.shrink_to_fit();
        self.hints.shrink_to_fit();
        self.candidates.shrink_to_fit();
        self.user_bindings.shrink_to_fit();
    }

    /// When `screen` shows the row of hints.
    pub(crate) fn hints_of(&self, screen: &Id) -> paperpaw_sdk::Hints {
        self.hints.get(screen).copied().unwrap_or_default()
    }

    /// Every slot some screen is bound to.
    pub(crate) fn bound_slots(&self) -> Vec<Id> {
        let mut slots: Vec<Id> = Vec::new();
        for slot in self
            .candidates
            .iter()
            .map(|c| &c.slot)
            .chain(self.user_bindings.iter().map(|(slot, _)| slot))
        {
            if !slots.contains(slot) && self.resolve_slot(slot.as_str()).is_some() {
                slots.push(slot.clone());
            }
        }
        slots
    }

    /// The screen bound to `slot`: the user's choice if that screen exists,
    /// otherwise the candidate with the highest priority (the later
    /// registration wins a tie).
    pub(crate) fn resolve_slot(&self, slot: &str) -> Option<&Id> {
        if let Some(screen) = self.user_bindings.get(slot)
            && self.screens.contains_key(screen)
        {
            return Some(screen);
        }
        self.candidates
            .iter()
            .filter(|c| c.slot.as_str() == slot && self.screens.contains_key(&c.screen))
            .max_by_key(|c| c.priority)
            .map(|c| &c.screen)
    }

    /// Every screen offered for `slot`.
    pub(crate) fn slot_screens<'a>(&'a self, slot: &'a Id) -> impl Iterator<Item = &'a Id> {
        self.candidates
            .iter()
            .filter(move |c| c.slot == *slot && self.screens.contains_key(&c.screen))
            .map(|c| &c.screen)
    }

    pub(crate) fn bind_user(&mut self, slot: Id, screen: Id) {
        self.user_bindings.insert(slot, screen);
    }

    /// Back to what the plugins bind `slot` to.
    pub(crate) fn unbind_user(&mut self, slot: &Id) {
        self.user_bindings.remove(slot);
    }

    pub(crate) fn user_bindings(&self) -> impl Iterator<Item = (&Id, &Id)> {
        self.user_bindings.iter()
    }
}

/// A plugin's setting before it is defined: built at start, or kept in
/// the program ([`SettingSpec::fixed`]).
pub(crate) enum Staged {
    Owned(alloc::boxed::Box<SettingSpec>),
    Fixed(&'static SettingSpec),
}

impl Staged {
    pub(crate) fn spec(&self) -> &SettingSpec {
        match self {
            Staged::Owned(spec) => spec,
            Staged::Fixed(spec) => spec,
        }
    }
}

/// Contributions of one plugin, collected during `register` and merged only
/// if the whole registration succeeds; a failing plugin leaves no traces.
#[derive(Default)]
pub(crate) struct Staging {
    actions: Vec<ActionSpec>,
    screens: Vec<ScreenSpec>,
    slots: Vec<SlotSpec>,
    candidates: Vec<SlotCandidate>,
    pub(crate) settings: Vec<Staged>,
    pub(crate) keymaps: Vec<Cow<'static, str>>,
    /// Key maps that apply while a switch (the id) is on.
    pub(crate) keymaps_when: Vec<(Id, Cow<'static, str>)>,
    pub(crate) catalogs: Vec<Catalog>,
    pub(crate) fonts: Vec<FontSpec>,
    pub(crate) font_files: Vec<(Id, alloc::string::String)>,
    pub(crate) font_extensions: Vec<(Id, alloc::string::String, Id)>,
    pub(crate) font_faces: Vec<paperpaw_sdk::FontFace>,
    pub(crate) font_roles: Vec<(Id, Id, Priority)>,
    pub(crate) formats: Vec<FormatSpec>,
    pub(crate) hyphenation: Vec<HyphenationSpec>,
    pub(crate) setting_groups: Vec<SettingGroupSpec>,
    /// Events, the plugins that hear them, and their handlers.
    pub(crate) subscriptions: Vec<(Id, Id, EventHandler)>,
    pub(crate) menu_entries: Vec<MenuEntry>,
    pub(crate) menu_sections: Vec<paperpaw_sdk::MenuSection>,
    pub(crate) icons: Vec<(Id, Cow<'static, str>)>,
    pub(crate) menu_needs: Vec<(Id, paperpaw_sdk::MenuNeed)>,
    pub(crate) screen_hints: Vec<(Id, paperpaw_sdk::Hints)>,
    pub(crate) keymap_presets: Vec<(Id, Cow<'static, str>)>,
    pub(crate) setting_defaults: Vec<(Id, Cow<'static, str>)>,
    pub(crate) status_items: Vec<StatusItemSpec>,
    pub(crate) status_lines: Vec<StatusLineSpec>,
    pub(crate) widgets: Vec<paperpaw_sdk::widgets::WidgetSpec>,
    pub(crate) setup_steps: Vec<paperpaw_sdk::setup::SetupStepSpec>,
    pub(crate) presets: Vec<paperpaw_sdk::presets::Preset>,
    pub(crate) fixed_presets: Vec<&'static [paperpaw_sdk::presets::Preset]>,
    pub(crate) text_filters: Vec<paperpaw_sdk::filters::TextFilterSpec>,
    pub(crate) decorators: Vec<paperpaw_sdk::decorations::DecoratorSpec>,
    pub(crate) input_filters: Vec<paperpaw_sdk::filters::InputFilterSpec>,
    pub(crate) refresh_filters: Vec<paperpaw_sdk::filters::RefreshFilterSpec>,
    /// Jobs: the steps of declared work (see [`paperpaw_sdk::work`]).
    pub(crate) works: Vec<paperpaw_sdk::work::WorkSpec>,
    pub(crate) stylesheets: Vec<Cow<'static, str>>,
    pub(crate) book_data: Vec<paperpaw_sdk::library::BookData>,
    pub(crate) services: Vec<(Id, paperpaw_sdk::services::Service)>,
    pub(crate) contributions: Vec<(Id, Cow<'static, str>)>,
}

impl Staging {
    /// Moves actions, screens, slots and bindings into `registry`.
    /// Settings, key maps and catalogs are left for the caller.
    pub(crate) fn merge_into(
        &mut self,
        registry: &mut Registry,
        actions: &mut Actions,
    ) -> Result<(), RegisterError> {
        let duplicate = self
            .actions
            .iter()
            .map(|a| &a.id)
            .find(|id| actions.contains(id.as_str()))
            .or_else(|| {
                self.screens
                    .iter()
                    .map(|s| &s.id)
                    .find(|id| registry.screens.contains_key(*id))
            })
            .or_else(|| {
                self.slots
                    .iter()
                    .map(|s| &s.id)
                    .find(|id| registry.slots.contains_key(*id))
            });
        if let Some(id) = duplicate {
            return Err(RegisterError::Duplicate { id: id.clone() });
        }
        for action in self.actions.drain(..) {
            actions.insert(action);
        }
        for screen in self.screens.drain(..) {
            registry.screens.insert(screen.id.clone(), screen);
        }
        for slot in self.slots.drain(..) {
            registry.slots.insert(slot.id.clone(), slot);
        }
        registry.candidates.append(&mut self.candidates);
        Ok(())
    }
}

/// Applies the contributions `plugin` made to `staging`, checking what its
/// registrar could not: key maps, translations, hyphenation patterns.
pub(crate) fn stage(
    plugin: &Id,
    contributions: Vec<Contribution>,
    staging: &mut Staging,
) -> Result<(), RegisterError> {
    for contribution in contributions {
        match contribution {
            Contribution::Action(action) => staging.actions.push(action),
            Contribution::Screen(screen) => staging.screens.push(screen),
            Contribution::Slot(slot) => staging.slots.push(slot),
            Contribution::SlotBinding {
                slot,
                screen,
                priority,
                ..
            } => staging.candidates.push(SlotCandidate {
                slot,
                screen,
                priority,
            }),
            Contribution::Setting(setting) => staging.settings.push(Staged::Owned(setting)),
            Contribution::FixedSettings(list) => {
                staging.settings.extend(list.iter().map(Staged::Fixed));
            }
            Contribution::SettingGroup(group) => staging.setting_groups.push(group),
            Contribution::SettingDefault { setting, value, .. } => {
                staging.setting_defaults.push((setting, value));
            }
            Contribution::Keymap(source) => {
                validate_keymap(&source)?;
                staging.keymaps.push(source);
            }
            Contribution::KeymapPreset { id, source, .. } => {
                validate_keymap(&source)?;
                staging.keymap_presets.push((id, source));
            }
            Contribution::KeymapWhen {
                setting, source, ..
            } => {
                validate_keymap(&source)?;
                staging.keymaps_when.push((setting, source));
            }
            Contribution::Stylesheet(css) => staging.stylesheets.push(css),
            Contribution::BookData(data) => staging.book_data.push(data),
            Contribution::Font(font) => staging.fonts.push(*font),
            Contribution::FontFile { id, path, .. } => staging.font_files.push((id, path)),
            Contribution::FontExtension {
                id, path, extends, ..
            } => {
                staging.font_extensions.push((id, path, extends));
            }
            Contribution::FontFace(face) => staging.font_faces.push(*face),
            Contribution::FontBinding {
                role,
                font,
                priority,
                ..
            } => staging.font_roles.push((role, font, priority)),
            Contribution::Format(format) => staging.formats.push(*format),
            Contribution::Hyphenation(spec) => {
                paperpaw_sdk::hyphen::Patterns::new(spec.patterns).map_err(|e| {
                    RegisterError::Invalid {
                        message: Cow::Owned(alloc::format!("{}: {e}", spec.id)),
                    }
                })?;
                staging.hyphenation.push(spec);
            }
            Contribution::MenuEntry(entry) => staging.menu_entries.push(entry),
            Contribution::MenuSection(section) => staging.menu_sections.push(section),
            Contribution::Icon { item, icon, .. } => staging.icons.push((item, icon)),
            Contribution::MenuNeed { item, need, .. } => staging.menu_needs.push((item, need)),
            Contribution::ScreenHints { screen, hints, .. } => {
                staging.screen_hints.push((screen, hints));
            }
            Contribution::Subscription { event, handler, .. } => {
                staging.subscriptions.push((event, plugin.clone(), handler));
            }
            Contribution::StatusItem(item) => staging.status_items.push(item),
            Contribution::StatusLine(line) => staging.status_lines.push(line),
            Contribution::TextFilter(filter) => staging.text_filters.push(filter),
            Contribution::Decorator(decorator) => staging.decorators.push(*decorator),
            Contribution::InputFilter(filter) => staging.input_filters.push(filter),
            Contribution::RefreshFilter(filter) => staging.refresh_filters.push(filter),
            Contribution::Work(spec) => staging.works.push(*spec),
            Contribution::Widget(widget) => staging.widgets.push(*widget),
            Contribution::SetupStep(step) => staging.setup_steps.push(*step),
            Contribution::Preset(preset) => staging.presets.push(*preset),
            Contribution::FixedPresets(list) => staging.fixed_presets.push(list),
            Contribution::Service { id, service, .. } => staging
                .services
                .push((id, paperpaw_sdk::services::Service::any(service))),
            Contribution::Published { id, service, .. } => staging.services.push((id, service)),
            Contribution::ToPoint { point, text, .. } => staging.contributions.push((point, text)),
            Contribution::Translations(source) => staging.catalogs.push(catalog(source)?),
            // A kind of a newer SDK than this kernel knows: said, not lost.
            other => {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(alloc::format!(
                        "a contribution of kind `{}` this PaperPaw does not know",
                        other.kind()
                    )),
                });
            }
        }
    }
    Ok(())
}

/// A translation catalog, its errors attributed to the plugin.
pub(crate) fn catalog(source: Cow<'static, str>) -> Result<Catalog, RegisterError> {
    Catalog::parse(source).map_err(|error| RegisterError::Parse {
        line: match &error {
            paperpaw_i18n::CatalogError::Syntax(e) => e.line,
            paperpaw_i18n::CatalogError::UnknownPluralRule { line }
            | paperpaw_i18n::CatalogError::UnexpectedSection { line } => *line,
            _ => 0,
        },
        message: Cow::Owned(error.to_string()),
    })
}

/// Checks a key map contribution so errors are attributed to the plugin.
fn validate_keymap(source: &str) -> Result<(), RegisterError> {
    let mut scratch = crate::keymap::Keymap::default();
    let problems = scratch.load(source, crate::keymap::Origin::Plugin, true);
    match problems.into_iter().next() {
        Some(problem) => Err(RegisterError::Parse {
            line: problem.line,
            message: problem.message,
        }),
        None => Ok(()),
    }
}
