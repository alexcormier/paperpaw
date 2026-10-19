//! Settings: the user's configuration read and saved, a book's own
//! settings (the scope), and what the kernel does when one changes.

#[allow(unused_imports)]
use super::*;

impl Kernel {
    /// Writes a changed setting to the configuration file on the device,
    /// keeping the rest of the file as it is.
    /// Forgets a setting's value (see [`Command::ResetSetting`]): from the
    /// active scope, or from the configuration file.
    pub(crate) fn reset_setting(&mut self, id: &Id) {
        let scoped = self.shared.settings.goes_to_scope(id.as_str());
        let before = self.scoped_values();
        if let Err(error) = self.shared.settings.reset(id.as_str()) {
            return self.warn_runtime(format!("`{id}`: {error}"));
        }
        if scoped {
            self.save_scope();
            self.announce_scope_changes(before);
            return;
        }
        if let Some((section, key)) = id.as_str().split_once('.') {
            let storage = self.shared.storage.as_ref();
            if let Some(source) = read_config(storage) {
                let updated = quireink_config::remove(&source, section, key);
                if updated != source && storage.write(CONFIG_FILE, updated.as_bytes()).is_err() {
                    self.warn_runtime(format!("could not save `{id}` to {CONFIG_FILE}"));
                }
            }
        }
        self.setting_changed(id);
        self.output
            .commands
            .push(events::SETTING_CHANGED.emit(events::SettingChanged { id: id.clone() }));
    }

    pub(crate) fn persist_setting(&mut self, id: &Id) {
        let Some(value) = self.shared.settings.global(id.as_str()) else {
            return;
        };
        let Some((section, key)) = id.as_str().split_once('.') else {
            return;
        };
        let value = value.to_config_text();
        self.save_config(section, key, &value);
    }

    /// The effective values of the scoped settings, to see what a scope
    /// change changes.
    pub(crate) fn scoped_values(&self) -> Vec<(Id, Option<quireink_sdk::SettingValue>)> {
        let settings = &self.shared.settings;
        settings
            .specs()
            .filter(|spec| spec.scoped)
            .map(|spec| (spec.id.clone(), settings.get(spec.id.as_str()).cloned()))
            .collect()
    }

    /// Announces the scoped settings whose value differs from `before`.
    pub(crate) fn announce_scope_changes(
        &mut self,
        before: Vec<(Id, Option<quireink_sdk::SettingValue>)>,
    ) {
        for (id, old) in before {
            if self.shared.settings.get(id.as_str()) != old.as_ref() {
                self.setting_changed(&id);
                self.output
                    .commands
                    .push(events::SETTING_CHANGED.emit(events::SettingChanged { id: id.clone() }));
            }
        }
    }

    /// Activates the scope stored at `path` for the top screen, or ends
    /// the scope (see [`Command::SetScope`]).
    pub(crate) fn set_scope(&mut self, path: Option<String>) {
        if path == self.scope.as_ref().map(|(p, _)| p.clone()) {
            // Same scope (e.g. the reader shown again): keep it.
            if let Some((_, owner)) = &mut self.scope {
                *owner = self.stack.last().map(screen_key);
            }
            return;
        }
        let before = self.scoped_values();
        let values = path.as_deref().map(|path| {
            let storage = self.shared.storage.as_ref();
            let text = quireink_sdk::read_file(storage, path, 16 * 1024)
                .ok()
                .and_then(|data| String::from_utf8(data).ok())
                .unwrap_or_default();
            let mut values = quireink_sdk::collections::VecMap::new();
            for entry in quireink_config::parse(&text).filter_map(Result::ok) {
                let id = format!("{}.{}", entry.section, entry.key);
                let parsed = self
                    .shared
                    .settings
                    .specs()
                    .find(|spec| spec.id.as_str() == id)
                    .and_then(|spec| {
                        Some((spec.id.clone(), spec.kind.parse(&entry.value.as_str())?))
                    });
                if let Some((id, value)) = parsed {
                    values.insert(id, value);
                }
            }
            values
        });
        self.shared.settings.set_scope(values);
        self.scope = path.map(|path| (path, self.stack.last().map(screen_key)));
        self.announce_scope_changes(before);
    }

    /// Changes a setting in the active scope and saves the scope.
    pub(crate) fn set_in_scope(&mut self, id: &Id, value: quireink_sdk::SettingValue) {
        let before = self.scoped_values();
        if let Err(error) = self.shared.settings.set_in_scope(id.as_str(), value) {
            return self.warn_runtime(format!("`{id}`: {error}"));
        }
        self.save_scope();
        self.announce_scope_changes(before);
        // The switch itself is not a scoped value others see change.
        if *id == core_settings::SCOPE_ONLY {
            self.output
                .commands
                .push(events::SETTING_CHANGED.emit(events::SettingChanged { id: id.clone() }));
        }
    }

    /// Writes the active scope's values to its file (removed when empty).
    pub(crate) fn save_scope(&mut self) {
        let (Some((path, _)), Some(values)) = (&self.scope, self.shared.settings.scope()) else {
            return;
        };
        let storage = self.shared.storage.as_ref();
        if values.is_empty() {
            let _ = storage.remove(path);
            return;
        }
        let mut text = String::new();
        for (id, value) in values.iter() {
            if let Some((section, key)) = id.as_str().split_once('.') {
                text = quireink_config::set(&text, section, key, &value.to_config_text());
            }
        }
        if storage.write(path, text.as_bytes()).is_err() {
            self.warn_runtime(format!("could not save settings to {path}"));
        }
    }

    /// Ends the scope when the screen that set it is gone.
    pub(crate) fn check_scope_owner(&mut self) {
        let Some((_, owner)) = &self.scope else {
            return;
        };
        let alive = self.stack.iter().any(|s| Some(screen_key(s)) == *owner);
        if !alive {
            self.set_scope(None);
        }
    }

    /// Shows settings without keeping them, for the screen on top (see
    /// [`Command::PreviewSettings`]); empty, ends that.
    pub(crate) fn preview_settings(
        &mut self,
        values: Vec<(Id, Option<quireink_sdk::SettingValue>)>,
    ) {
        let ending = values.is_empty();
        if ending && self.preview_owner.is_none() && !self.shared.settings.previewing() {
            return;
        }
        self.shared.settings.set_preview(values);
        self.preview_owner = if ending {
            None
        } else {
            self.stack.last().map(screen_key)
        };
        self.output.commands.push(events::PREVIEW_CHANGED.emit(()));
    }

    /// Ends the values shown without being kept when the screen that
    /// asked for them is gone.
    pub(crate) fn check_preview_owner(&mut self) {
        let Some(owner) = self.preview_owner else {
            return;
        };
        if !self.stack.iter().any(|s| screen_key(s) == owner) {
            self.preview_settings(Vec::new());
        }
    }

    /// Sets `key` in `section` of the configuration file on the device.
    pub(crate) fn save_config(&mut self, section: &str, key: &str, value: &str) {
        let storage = self.shared.storage.as_ref();
        let source = read_config(storage).unwrap_or_default();
        let updated = quireink_config::set(&source, section, key, value);
        if storage.write(CONFIG_FILE, updated.as_bytes()).is_err() {
            self.warn_runtime(format!("could not save `{section}.{key}` to {CONFIG_FILE}"));
        }
    }

    /// Switches a plugin for the next start (see [`Command::SetPluginEnabled`]).
    pub(crate) fn set_plugin_enabled(&mut self, id: &Id, enabled: bool) {
        let Some(info) = self.shared.plugins.get(id.as_str()) else {
            return self.warn_runtime(format!("unknown plugin `{id}`"));
        };
        if !enabled && !info.can_switch_off() {
            return self.warn_runtime(format!("plugin `{id}` cannot be switched off"));
        }
        if info.enabled == enabled {
            return;
        }
        self.save_config("plugins", id.as_str(), if enabled { "on" } else { "off" });
        if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
            info.enabled = enabled;
        }
    }

    pub(crate) fn setting_changed(&mut self, id: &Id) {
        if *id == core_settings::LANGUAGE {
            self.shared
                .translator
                .set_language(self.shared.settings.text(id.as_str()));
        } else if *id == core_settings::NIGHT_MODE {
            self.framebuffer
                .set_inverted(self.shared.settings.bool(id.as_str()));
        } else if *id == core_settings::NIGHT_PICTURES {
            self.framebuffer
                .set_pictures_kept(!self.shared.settings.bool(id.as_str()));
        } else if *id == core_settings::MONO_TEXT {
            self.framebuffer
                .set_mono_coverage(mono_coverage(&self.shared.settings));
        }
        if *id == core_settings::KEYMAP_PRESET {
            self.rebuild_keymap();
        }
        if *id == core_settings::STATUS_FONT {
            self.apply_status_font();
        }
        if *id == core_settings::READING_FONT {
            self.apply_reading_font();
        }
        // Its size follows the interface's.
        if *id == core_settings::BUTTON_HINTS
            || *id == core_settings::UI_SIZE
            || *id == core_settings::ROTATION
        {
            self.apply_hint_height();
        }
        if *id == core_settings::LANGUAGE
            || *id == core_settings::NIGHT_MODE
            || *id == core_settings::NIGHT_PICTURES
            || *id == core_settings::MONO_TEXT
            || *id == core_settings::TEXT_COLOR
            || *id == core_settings::PAPER_COLOR
            || *id == core_settings::TEXT_COLOR_OWN
            || *id == core_settings::PAPER_COLOR_OWN
        {
            let bounds = self.framebuffer.bounds();
            self.invalidate(bounds);
            self.output.request_refresh(RefreshMode::Full);
        }
    }

    pub(crate) fn apply_user_config(&mut self, source: &str) {
        for entry in quireink_config::parse(source) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.warn(
                        WarningSource::UserConfig,
                        Some(error.line),
                        error.to_string(),
                    );
                    continue;
                }
            };
            let line = Some(entry.line);
            let value = entry.value.as_str();
            match entry.section {
                "plugins" => {}
                section if section.starts_with("keymap.") => {}
                "fonts" => match (Id::parse(entry.key), Id::parse(&value)) {
                    (Ok(role), Ok(font)) if self.shared.fonts.contains(font.as_str()) => {
                        self.shared.fonts.bind_user(role, font);
                    }
                    (Ok(_), Ok(font)) => self.warn(
                        WarningSource::UserConfig,
                        line,
                        format!("unknown font `{font}`"),
                    ),
                    _ => self.warn(
                        WarningSource::UserConfig,
                        line,
                        "invalid font binding".into(),
                    ),
                },
                "menus" => {
                    let actions: Result<Vec<Id>, _> = value
                        .split(',')
                        .map(str::trim)
                        .filter(|a| !a.is_empty())
                        .map(Id::parse)
                        .collect();
                    match (Id::parse(entry.key), actions) {
                        (Ok(menu), Ok(actions)) => self.shared.menus.customize(menu, actions),
                        _ => self.warn(WarningSource::UserConfig, line, "invalid menu".into()),
                    }
                }
                "slots" => match (Id::parse(entry.key), Id::parse(&value)) {
                    (Ok(slot), Ok(screen)) => self.registry.bind_user(slot, screen),
                    _ => self.warn(
                        WarningSource::UserConfig,
                        line,
                        "invalid slot binding".into(),
                    ),
                },
                "" => self.warn(
                    WarningSource::UserConfig,
                    line,
                    "settings belong in a [<plugin-id>] section".into(),
                ),
                section => {
                    let id = format!("{section}.{}", entry.key);
                    if let Err(error) = self.shared.settings.set_from_text(&id, &value) {
                        self.warn(WarningSource::UserConfig, line, format!("`{id}`: {error}"));
                    }
                }
            }
        }
        self.keys
            .sources
            .push((Origin::User, Cow::Owned(source.to_string())));
        for problem in self.shared.keymap.load(source, Origin::User, false) {
            self.warn(
                WarningSource::UserConfig,
                Some(problem.line),
                problem.message.into_owned(),
            );
        }
    }

    /// Reports references to contributions that do not exist.
    pub(crate) fn validate(&mut self) {
        let mut messages = Vec::new();
        for (slot, screen) in self.registry.user_bindings() {
            if !self.registry.slots.contains_key(slot) {
                messages.push(format!("unknown slot `{slot}`"));
            } else if !self.registry.screens.contains_key(screen) {
                messages.push(format!("unknown screen `{screen}` for slot `{slot}`"));
            }
        }
        let preset_actions = self.preset_actions();
        let mut unknown: Vec<&Id> = self
            .shared
            .keymap
            .actions()
            .chain(self.shared.menus.referenced_items())
            .chain(preset_actions.iter())
            .filter(|id| {
                !self.shared.actions.contains(id.as_str())
                    && self.shared.settings.get(id.as_str()).is_none()
                    && !self.shared.menus.is_section(id.as_str())
            })
            .collect();
        unknown.sort();
        unknown.dedup();
        for action in unknown {
            messages.push(format!("unknown action `{action}` in a key map or menu"));
        }
        for message in messages {
            self.warn_runtime(message);
        }
    }
}

/// The configuration file on the device, if any.
pub(crate) fn read_config(storage: &dyn Storage) -> Option<String> {
    let data = quireink_sdk::read_file(storage, CONFIG_FILE, 64 * 1024).ok()?;
    String::from_utf8(data).ok()
}

/// Plugin ids switched on (`on`) or off in the `[plugins]` section of a
/// user config.
pub(crate) fn plugin_switches(source: &str, on: bool) -> Vec<String> {
    quireink_config::parse(source)
        .filter_map(Result::ok)
        .filter(|entry| entry.section == "plugins" && entry.value.as_bool() == Some(on))
        .map(|entry| String::from(entry.key))
        .collect()
}

/// From what coverage an edge pixel is drawn in black and white, by the
/// setting [`core_settings::MONO_TEXT`].
pub(crate) fn mono_coverage(settings: &Settings) -> u8 {
    let level = settings.text(core_settings::MONO_TEXT.as_str());
    match core_settings::MONO_TEXT_LEVELS
        .iter()
        .position(|l| *l == level)
    {
        Some(0) => 128,
        Some(2) => 64,
        _ => quireink_graphics::MONO_COVERAGE,
    }
}
