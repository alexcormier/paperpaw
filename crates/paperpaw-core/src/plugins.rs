//! Plugins registered: their contributions merged into the kernel, the
//! plugins switched off or required, menus, key maps and fonts.

#[allow(unused_imports)]
use super::*;

/// Sections of `paperpaw.ini` that are no plugin's settings: no plugin
/// may take their name as its id (its settings would be read as these).
pub(crate) const RESERVED_IDS: [&str; 4] = ["plugins", "fonts", "menus", "slots"];

/// The key maps the kernel's is built from.
#[derive(Debug, Default)]
pub(crate) struct KeySources {
    /// The plugins' key maps, by plugin, to rebuild the key map when the
    /// preset changes or a plugin goes.
    pub(crate) plugins: Vec<(Id, Cow<'static, str>)>,
    /// The device's own key map, under all others.
    pub(crate) device: Cow<'static, str>,
    pub(crate) presets: Vec<(Id, Cow<'static, str>)>,
    /// Plugins' key maps that apply while a switch (the id) is on.
    pub(crate) when: Vec<(Id, Cow<'static, str>)>,
    /// The user's bindings: `paperpaw.ini` as read at start and changed
    /// since ([`Command::Bind`]), then the shell's overrides (see
    /// [`KernelBuilder::user_config`]), which win.
    pub(crate) user: String,
    pub(crate) shell: String,
}

impl Kernel {
    /// Registers `plugin`, or only lists it if the user switched it off.
    pub(crate) fn add_plugin(&mut self, plugin: &dyn Plugin, disabled: &[String]) {
        let manifest = plugin.manifest();
        if manifest.id != CORE && disabled.iter().any(|id| id == manifest.id.as_str()) {
            self.register_off(plugin, manifest);
        } else {
            self.register_plugin(plugin, manifest);
        }
    }

    pub(crate) fn register_plugin(&mut self, plugin: &dyn Plugin, manifest: Manifest) {
        let id = manifest.id.clone();
        let source = || WarningSource::Plugin(id.clone());
        if id.as_str() != id.namespace() {
            return self.warn(source(), None, "plugin ids must not contain '.'".into());
        }
        if self.shared.plugins.get(id.as_str()).is_some() {
            return self.warn(
                source(),
                None,
                "a plugin with this id is already loaded".into(),
            );
        }
        if RESERVED_IDS.contains(&id.as_str()) {
            let message = format!("`{id}` names a section of {CONFIG_FILE}; not loaded");
            self.warn(source(), None, message);
            return self.add_plugin_info(manifest, PluginState::Failed, true);
        }
        if !SDK_VERSION.satisfies(&manifest.sdk) {
            let message = format!("requires SDK {}, this is {}", manifest.sdk, SDK_VERSION);
            self.warn(source(), None, message);
            return self.add_plugin_info(manifest, PluginState::Failed, true);
        }

        let mut staging = Staging::default();
        let mut registrar = Registrar::new(id.clone());
        let result = plugin
            .register(&mut registrar)
            .and_then(|()| registry::stage(&id, registrar.into_contributions(), &mut staging))
            .and_then(|()| self.merge(&id, staging));
        match result {
            Ok(()) => self.add_plugin_info(manifest, PluginState::Running, true),
            Err(error) => {
                let line = match error {
                    RegisterError::Parse { line, .. } => Some(line),
                    _ => None,
                };
                self.warn(source(), line, format!("not loaded: {error}"));
                self.add_plugin_info(manifest, PluginState::Failed, true);
            }
        }
    }

    /// Lists a plugin the user switched off. Only its translations are
    /// loaded, so plugin managers can show its name and description in the
    /// user's language.
    pub(crate) fn register_off(&mut self, plugin: &dyn Plugin, manifest: Manifest) {
        let id = manifest.id.clone();
        if id.as_str() != id.namespace() || self.shared.plugins.get(id.as_str()).is_some() {
            return;
        }
        if RESERVED_IDS.contains(&id.as_str()) {
            return self.register_plugin(plugin, manifest);
        }
        let mut registrar = Registrar::new(id);
        if SDK_VERSION.satisfies(&manifest.sdk) && plugin.register(&mut registrar).is_ok() {
            for contribution in registrar.into_contributions() {
                if let Contribution::Translations(source) = contribution
                    && let Ok(catalog) = registry::catalog(source)
                {
                    self.shared.translator.add(catalog);
                }
            }
        }
        self.add_plugin_info(manifest, PluginState::Off, false);
    }

    pub(crate) fn add_plugin_info(
        &mut self,
        manifest: Manifest,
        state: PluginState,
        enabled: bool,
    ) {
        self.shared.plugins.push(PluginInfo {
            manifest,
            state,
            enabled,
            required_for: RequiredFor::default(),
            kind: paperpaw_sdk::PluginKind::BuiltIn,
            problem: None,
            update: None,
        });
    }

    /// Gives the menus the labels of the actions and settings they list,
    /// and only those: every label costs memory.
    pub(crate) fn set_menu_titles(&mut self) {
        let referenced: Vec<Id> = self.shared.menus.referenced_items().cloned().collect();
        for id in referenced {
            let title = match self.shared.actions.get(id.as_str()) {
                Some(action) => Some(action.title.clone()),
                None => self
                    .shared
                    .settings
                    .specs()
                    .find(|spec| spec.id == id)
                    .map(|spec| spec.title.clone()),
            };
            if let Some(title) = title {
                self.shared.menus.set_title(id, title);
            }
        }
    }

    /// Marks plugins that alone fill a standard slot or provide all fonts:
    /// switching one of them off would leave the user without e.g.
    /// settings, a menu or text, and no way back.
    pub(crate) fn find_required_plugins(&mut self) {
        for slot in [
            slots::HOME,
            slots::READER,
            slots::LIBRARY,
            slots::MENU,
            slots::SETTINGS,
        ] {
            let sole = {
                let mut providers = self
                    .shared
                    .registry
                    .slot_screens(&slot)
                    .map(Id::namespace)
                    .filter(|plugin| *plugin != CORE.as_str());
                match providers.next() {
                    Some(first) if providers.all(|plugin| plugin == first) => first.to_string(),
                    _ => continue,
                }
            };
            if let Some(info) = self.shared.plugins.get_mut(&sole) {
                info.required_for.insert(&slot);
            }
        }
        // Any font can stand in for any role, but without fonts no text
        // is shown at all.
        let mut owners = self.shared.fonts.ids().map(Id::namespace);
        let sole = match owners.next() {
            Some(first) if owners.all(|plugin| plugin == first) => first.to_string(),
            _ => return,
        };
        if let Some(info) = self.shared.plugins.get_mut(&sole) {
            info.required_for.insert(&paperpaw_sdk::builtin::fonts::UI);
        }
    }

    /// Merges a plugin's contributions: all of them or none.
    pub(crate) fn merge(&mut self, plugin: &Id, mut staging: Staging) -> Result<(), RegisterError> {
        for setting in staging.settings.iter().map(registry::Staged::spec) {
            if self.shared.settings.get(setting.id.as_str()).is_some() {
                return Err(RegisterError::Duplicate {
                    id: setting.id.clone(),
                });
            }
            if !setting.kind.accepts(&setting.default) {
                return Err(RegisterError::Invalid {
                    message: Cow::Owned(format!("invalid default for `{}`", setting.id)),
                });
            }
        }
        if let Some(font) = staging
            .fonts
            .iter()
            .find(|f| self.shared.fonts.contains(f.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: font.id.clone(),
            });
        }
        if let Some(format) = staging
            .formats
            .iter()
            .find(|f| self.shared.formats.contains(f.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: format.id.clone(),
            });
        }
        if let Some(spec) = staging
            .hyphenation
            .iter()
            .find(|h| self.shared.hyphenation.contains(h.id.as_str()))
        {
            return Err(RegisterError::Duplicate {
                id: spec.id.clone(),
            });
        }
        staging.merge_into(&mut self.shared.registry, &mut self.shared.actions)?;
        for setting in staging.settings.drain(..) {
            // Cannot fail: checked above.
            let _ = match setting {
                registry::Staged::Owned(spec) => self.shared.settings.define(*spec),
                registry::Staged::Fixed(spec) => self.shared.settings.define_fixed(spec),
            };
        }
        for source in staging.keymaps.drain(..) {
            // Validated by the registrar.
            self.shared.keymap.load(&source, Origin::Plugin, true);
            self.keys.plugins.push((plugin.clone(), source));
        }
        self.keys.presets.append(&mut staging.keymap_presets);
        // Loaded once the settings are known (`rebuild_keymap`).
        self.keys.when.append(&mut staging.keymaps_when);
        for catalog in staging.catalogs.drain(..) {
            self.shared.translator.add(catalog);
        }
        for font in staging.fonts.drain(..) {
            // Cannot fail: checked above.
            let _ = self.shared.fonts.add(font);
        }
        for (id, path) in staging.font_files.drain(..) {
            self.add_font_file(plugin, id, path);
        }
        for (id, path, extends) in staging.font_extensions.drain(..) {
            if let Err(error) = self.shared.fonts.add_extension(id, path, extends) {
                self.warn(
                    WarningSource::Plugin(plugin.clone()),
                    None,
                    format!("{error}"),
                );
            }
        }
        for face in staging.font_faces.drain(..) {
            self.add_font_face(plugin, face);
        }
        for (role, font, priority) in staging.font_roles.drain(..) {
            self.shared.fonts.propose(role, font, priority);
        }
        for format in staging.formats.drain(..) {
            // Cannot fail: checked above.
            let _ = self.shared.formats.add(format);
        }
        for group in staging.setting_groups.drain(..) {
            if let Err(error) = self.shared.settings.add_group(group) {
                self.warn_runtime(format!("{error}"));
            }
        }
        for spec in staging.hyphenation.drain(..) {
            // Cannot fail: checked above and by the registrar.
            let _ = self.shared.hyphenation.add(spec);
        }
        self.subscriptions.append(&mut staging.subscriptions);
        for (action, priority, handler) in staging.overrides.drain(..) {
            // Not in place of a sealed action (one known by now; one that
            // comes later is never overridden either).
            if self
                .shared
                .actions
                .get(action.as_str())
                .is_some_and(paperpaw_sdk::ActionSpec::is_sealed)
            {
                self.warn_runtime(format!("{plugin}: `{action}` is sealed; not overridden"));
                continue;
            }
            self.shared
                .actions
                .add_override(action, priority, plugin.clone(), handler);
        }
        self.action_observers.append(&mut staging.action_observers);
        self.event_observers.append(&mut staging.event_observers);
        self.plugin_defaults.extend(
            staging
                .setting_defaults
                .drain(..)
                .map(|(id, value)| (plugin.clone(), id, value)),
        );
        for entry in staging.menu_entries.drain(..) {
            self.shared.menus.add(entry);
        }
        for section in staging.menu_sections.drain(..) {
            self.shared.menus.add_section(section);
        }
        for (menu, item, after) in staging.menu_placements.drain(..) {
            self.shared.menus.place(menu, item, after);
        }
        for (menu, item) in staging.menu_hidden.drain(..) {
            self.shared.menus.take_out(menu, item);
        }
        for (item, icon) in staging.icons.drain(..) {
            self.shared.menus.set_icon(item, icon);
        }
        for (item, need) in staging.menu_needs.drain(..) {
            self.shared.menus.set_need(item, need);
        }
        for (screen, hints) in staging.screen_hints.drain(..) {
            self.shared.registry.hints.insert(screen, hints);
        }
        for item in staging.status_items.drain(..) {
            self.shared.status_items.add(item);
        }
        for line in staging.status_lines.drain(..) {
            self.shared.status_items.add_line(line);
        }
        for step in staging.setup_steps.drain(..) {
            self.shared.setup.add(step);
        }
        for list in staging.fixed_presets.drain(..) {
            self.shared.settings.presets_mut().add_fixed(list);
        }
        for preset in staging.presets.drain(..) {
            self.shared.settings.presets_mut().add(preset);
        }
        for widget in staging.widgets.drain(..) {
            self.shared.widgets.add(widget);
        }
        self.stylesheet_owners
            .extend(staging.stylesheets.iter().map(|_| plugin.clone()));
        self.shared.stylesheets.append(&mut staging.stylesheets);
        self.shared.book_data.append(&mut staging.book_data);
        for (id, service) in staging.services.drain(..) {
            self.shared.services.insert(id, service);
        }
        for (point, contribution) in staging.contributions.drain(..) {
            self.shared
                .services
                .contribute(point, plugin.clone(), contribution);
        }
        for filter in staging.text_filters.drain(..) {
            self.shared.text_filters.add(filter);
        }
        for decorator in staging.decorators.drain(..) {
            self.shared.decorators.add(decorator);
        }
        for filter in staging.input_filters.drain(..) {
            self.shared.input_filters.add(filter);
        }
        for filter in staging.refresh_filters.drain(..) {
            self.shared.refresh_filters.add(filter);
        }
        for chrome in staging.chromes.drain(..) {
            self.add_chrome(chrome);
        }
        let caps = self.shared.caps;
        let inputs = staging.inputs.drain(..);
        let made = inputs.filter(|input| input.need.is_none_or(|need| need.met(caps)));
        self.shared.inputs.extend(made);
        // Recognizers are made once, their makers dropped; listed among
        // the inputs, to bind.
        for spec in staging.gestures.drain(..) {
            let recognizer = (spec.factory)();
            let mut listed = paperpaw_sdk::keys::InputSpec::new(spec.id.clone(), spec.title);
            listed.kind = paperpaw_sdk::keys::InputKind::Gesture;
            if caps.touch {
                self.shared.inputs.push(listed);
            }
            let id = spec.id;
            let fingers = Vec::new();
            let made = gestures::PluginRecognizer {
                id,
                recognizer,
                fingers,
            };
            self.input.recognizers.push(made);
        }
        for spec in staging.works.drain(..) {
            self.works.add_spec(spec);
        }
        Ok(())
    }

    /// Actions bound by any key map preset or key map for a switch, for
    /// validation.
    pub(crate) fn preset_actions(&self) -> Vec<Id> {
        let mut all = Keymap::default();
        for (_, source) in self.keys.presets.iter().chain(&self.keys.when) {
            all.load(source, Origin::Preset, true);
        }
        all.actions().cloned().collect()
    }

    /// Builds the key map from all sources and the selected preset.
    /// Problems were reported when the sources were first loaded.
    pub(crate) fn rebuild_keymap(&mut self) {
        let mut keymap = Keymap::default();
        let preset = self
            .shared
            .settings
            .text(core_settings::KEYMAP_PRESET.as_str());
        if let Some((_, source)) = self
            .keys
            .presets
            .iter()
            .find(|(id, _)| id.as_str() == preset)
        {
            keymap.load(source, Origin::Preset, true);
        }
        for (_, source) in &self.keys.plugins {
            keymap.load(source, Origin::Plugin, true);
        }
        keymap.load(&self.keys.device, Origin::Device, true);
        for source in [&self.keys.user, &self.keys.shell] {
            keymap.load(source, Origin::User, false);
        }
        // After the plugins' other bindings, which they then override.
        for (setting, source) in &self.keys.when {
            if self.shared.settings.bool(setting.as_str()) {
                keymap.load(source, Origin::Plugin, true);
            }
        }
        keymap.shrink_to_fit();
        self.shared.keymap = keymap;
    }

    /// Redefines or resets a menu (see [`Command::SetMenu`]) and saves it.
    pub(crate) fn set_menu(&mut self, menu: Id, entries: Option<Vec<Id>>) {
        let value = entries
            .as_ref()
            .map(|ids| ids.iter().map(Id::as_str).collect::<Vec<_>>().join(", "));
        match entries {
            Some(ids) => self.shared.menus.customize(menu.clone(), ids),
            None => self.shared.menus.reset(menu.as_str()),
        }
        // New entries need their labels.
        self.set_menu_titles();
        self.update_config(&format_args!("menu `{menu}`"), |source| match &value {
            Some(value) => paperpaw_config::set(source, "menus", menu.as_str(), value),
            None => paperpaw_config::remove(source, "menus", menu.as_str()),
        });
        self.output
            .commands
            .push(events::MENU_CHANGED.emit(events::MenuChanged::new(menu.clone())));
    }

    /// Adds a font file of `plugin`, described by its header.
    pub(crate) fn add_font_file(&mut self, plugin: &Id, id: Id, path: String) {
        let source = WarningSource::Plugin(plugin.clone());
        if !paperpaw_text_outlines() {
            return self.warn(
                source,
                None,
                format!("{path}: this device only reads fonts made for it (.ppfp)"),
            );
        }
        let storage = self.shared.storage.as_ref();
        let info = paperpaw_sdk::text::sfnt::read_info(&mut |offset, buf| {
            storage.read_at(&path, offset, buf).unwrap_or(0)
        });
        match info {
            Some(info) => {
                if let Err(error) = self.shared.fonts.add_file(id, path, info) {
                    self.warn(source, None, format!("{error}"));
                }
            }
            None => self.warn(source, None, format!("{path}: not a font")),
        }
    }

    /// Adds a face of a family `plugin` names in a file: packs everywhere,
    /// scalable fonts where they are drawn.
    pub(crate) fn add_font_face(&mut self, plugin: &Id, face: paperpaw_sdk::FontFace) {
        let source = WarningSource::Plugin(plugin.clone());
        if !face.is_pack() && !paperpaw_text_outlines() {
            let message = format!(
                "{}: this device only reads fonts made for it (.ppfp)",
                face.path
            );
            return self.warn(source, None, message);
        }
        if let Err(error) = self.shared.fonts.add_face(face) {
            self.warn(source, None, format!("{error}"));
        }
    }

    /// Chooses the reading font family from `core.reading-font` and loads
    /// its files.
    pub(crate) fn apply_reading_font(&mut self) {
        let chosen = self
            .shared
            .settings
            .text(core_settings::READING_FONT.as_str())
            .to_string();
        let family = (chosen != core_settings::DEFAULT_FONT
            && self.shared.fonts.families().contains(&chosen.as_str()))
        .then_some(chosen.as_str());
        self.shared.fonts.set_family(family);
        for path in self.shared.fonts.load(&self.shared.storage) {
            self.warn_runtime(format!("{path}: font could not be loaded"));
        }
    }

    /// Chooses what the status lines are in from `core.status-font` and
    /// loads its files.
    pub(crate) fn apply_status_font(&mut self) {
        let chosen = self
            .shared
            .settings
            .text(core_settings::STATUS_FONT.as_str())
            .to_string();
        self.shared.fonts.set_status(&chosen);
        for path in self.shared.fonts.load(&self.shared.storage) {
            self.warn_runtime(format!("{path}: font could not be loaded"));
        }
    }

    /// Loads the data plugin in `folder` while running (see
    /// [`Command::LoadPlugin`](paperpaw_sdk::Command::LoadPlugin)).
    pub(crate) fn load_plugin(&mut self, folder: &str) {
        if self.register_folder(folder) {
            self.plugins_changed();
        }
    }

    /// Registers the data plugin in `folder` (switched off where the
    /// configuration says so) and applies its defaults; `false`, with a
    /// warning, if it could not be.
    fn register_folder(&mut self, folder: &str) -> bool {
        let storage = self.shared.storage.clone();
        let plugin = match crate::data_plugin::load_folder(storage.as_ref(), folder) {
            Ok(plugin) => plugin,
            Err(problem) => {
                self.warn_runtime(problem);
                return false;
            }
        };
        let manifest = plugin.manifest();
        let id = manifest.id.clone();
        if self.shared.plugins.get(id.as_str()).is_some() {
            let problem = "a plugin with this id is already loaded".into();
            self.warn(WarningSource::Plugin(id), None, problem);
            return false;
        }
        let off = read_config(storage.as_ref())
            .ok()
            .flatten()
            .is_some_and(|c| crate::config::plugin_switches(&c, false).contains(&id.to_string()));
        if plugin.has_code() {
            self.add_code_plugin_later(&plugin, off);
        } else if off {
            plugin.keep_listing_only();
            self.register_off(&plugin, manifest);
        } else {
            self.register_plugin(&plugin, manifest);
            if let Some(info) = self.shared.plugins.get_mut(id.as_str()) {
                info.kind = paperpaw_sdk::PluginKind::Data;
            }
        }
        self.shared
            .data_folders
            .push((id, plugin.folder().to_string()));
        self.apply_defaults(&[]);
        true
    }

    /// Removes a data plugin while running (see
    /// [`Command::RemovePlugin`](paperpaw_sdk::Command::RemovePlugin)).
    pub(crate) fn remove_plugin(&mut self, id: &Id) {
        let Some(folder) = self.data_folder(id) else {
            return self.warn_runtime(format!("`{id}` is no data plugin; not removed"));
        };
        if let Err(error) = self.shared.storage.remove_dir_all(&folder) {
            return self.warn_runtime(format!("{folder}: not removed ({error:?})"));
        }
        self.unload_plugin(id);
        self.plugins_changed();
    }

    /// The folder of the data plugin `id` loaded (running or switched
    /// off).
    fn data_folder(&self, id: &Id) -> Option<String> {
        self.shared
            .data_folders
            .iter()
            .find(|(p, _)| p == id)
            .map(|(_, folder)| folder.clone())
    }

    /// Forgets the data plugin `id`: its folder, fonts and presets, and
    /// it in the list of plugins. What was chosen from it stays chosen
    /// until [`plugins_changed`](Self::plugins_changed) finds it no longer
    /// offered.
    fn unload_plugin(&mut self, id: &Id) {
        self.shared.data_folders.retain(|(p, _)| p != id);
        self.shared.fonts.remove_plugin(id.as_str());
        // Its key maps and stylesheets; the key map is built anew once the
        // plugins changed (`plugins_changed`).
        self.keys.plugins.retain(|(p, _)| p != id);
        self.keys
            .presets
            .retain(|(preset, _)| preset.namespace() != id.as_str());
        let mut owners = self.stylesheet_owners.iter();
        self.shared
            .stylesheets
            .retain(|_| owners.next().is_some_and(|owner| owner != id));
        self.stylesheet_owners.retain(|owner| owner != id);
        self.shared
            .settings
            .presets_mut()
            .remove_plugin(id.as_str());
        // A code plugin's code runs until the restart: it stays listed as
        // stopping then.
        match self.shared.plugins.get_mut(id.as_str()) {
            Some(info)
                if info.kind == paperpaw_sdk::PluginKind::Code
                    && info.state == PluginState::Running =>
            {
                info.enabled = false;
            }
            _ => {
                self.shared.plugins.remove(id.as_str());
            }
        }
    }

    /// A font chosen that is no longer offered gives way to the default.
    fn keep_offered_choices(&mut self) {
        for setting in [core_settings::READING_FONT, core_settings::STATUS_FONT] {
            let chosen = String::from(self.shared.settings.text(setting.as_str()));
            let value = paperpaw_sdk::SettingValue::Text(chosen.into());
            let offered = self
                .shared
                .settings
                .spec(setting.as_str())
                .is_some_and(|spec| spec.kind.accepts(&value));
            if !offered {
                self.reset_setting(setting.id());
            }
        }
    }

    /// Installs the data plugin `id` downloaded into `folder` (see
    /// [`Command::InstallPlugin`](paperpaw_sdk::Command::InstallPlugin)):
    /// the folder takes the place of every version of it on the storage,
    /// loaded or not, and is loaded at once.
    pub(crate) fn install_plugin(&mut self, id: &Id, folder: &str) {
        let dirs = self.shared.plugin_dirs.clone();
        // Loaded without a folder: built in, not to be replaced.
        let built_in =
            self.shared.plugins.get(id.as_str()).is_some() && self.data_folder(id).is_none();
        let Some(placed) = self.place_plugin(id, folder, &dirs, built_in) else {
            return;
        };
        // The new version in place: the old ones go, what was chosen from
        // them stays where the new one offers it too.
        placed.clear(self.shared.storage.as_ref());
        // A code plugin's code runs until the restart: the new version
        // takes over then.
        if let Some(info) = self.shared.plugins.get_mut(id.as_str())
            && info.kind == paperpaw_sdk::PluginKind::Code
            && info.state == PluginState::Running
        {
            info.update = Some(placed.version);
            info.enabled = true;
            self.shared.data_folders.retain(|(p, _)| p != id);
            self.shared.data_folders.push((id.clone(), placed.target));
            return self.plugins_changed();
        }
        if self.shared.plugins.get(id.as_str()).is_some() {
            self.unload_plugin(id);
        }
        // Changed even where the new one does not load: the old ones are
        // gone.
        self.register_folder(&placed.target);
        self.plugins_changed();
    }

    /// Installs the plugins the shell put into folders before the start
    /// (see [`KernelBuilder::install_plugin`](crate::KernelBuilder::install_plugin)),
    /// before the plugins in `dirs` are found; not over the plugins built
    /// in (`built_in`).
    pub(crate) fn install_staged(
        &mut self,
        staged: Vec<(Id, String)>,
        dirs: &[String],
        built_in: &[Id],
    ) {
        for (id, folder) in staged {
            let ours = built_in.contains(&id);
            if let Some(placed) = self.place_plugin(&id, &folder, dirs, ours) {
                placed.clear(self.shared.storage.as_ref());
            }
        }
    }

    /// Puts the plugin `id` in `folder` in the place of every version of
    /// it in `dirs` (and loaded); `None` when it is refused, said and its
    /// folder removed. A plugin `built_in` is not replaced, and one that
    /// would not register is refused before anything changes, so the
    /// version before stays.
    fn place_plugin(
        &mut self,
        id: &Id,
        folder: &str,
        dirs: &[String],
        built_in: bool,
    ) -> Option<Placed> {
        let storage = self.shared.storage.clone();
        let folder = folder.trim_end_matches('/');
        // The folder it is installed in: nothing to put in its place, and
        // nothing to remove.
        let dir = paperpaw_sdk::files::parent(folder);
        let target = paperpaw_sdk::files::join(dir, id.as_str());
        if paperpaw_sdk::files::same_path(storage.as_ref(), folder, &target) {
            let problem = format!("{folder}: is where `{id}` is installed; nothing to install");
            self.warn(WarningSource::Plugin(id.clone()), None, problem);
            return None;
        }
        let refuse = |kernel: &mut Kernel, problem: String| {
            let _ = storage.remove_dir_all(folder);
            kernel.warn(WarningSource::Plugin(id.clone()), None, problem);
            None
        };
        if built_in {
            return refuse(self, format!("`{id}` is built in; {folder} not installed"));
        }
        // What it says it is, before anything changes.
        let found = data_plugin::folder_plugin_id(storage.as_ref(), folder, id.as_str());
        if found.as_deref() != Some(id.as_str()) {
            return refuse(
                self,
                format!("{folder}: not the plugin `{id}`; not installed"),
            );
        }
        let plugin = match data_plugin::load_folder_as(storage.as_ref(), folder, id.as_str()) {
            Ok(plugin) => plugin,
            Err(problem) => return refuse(self, problem),
        };
        let version = plugin.manifest().version;
        // Registered aside first (its translations, key map, settings): one
        // that would fail leaves the version before in place.
        let mut registrar = Registrar::new(id.clone());
        let staged = plugin.register(&mut registrar).and_then(|()| {
            registry::stage(id, registrar.into_contributions(), &mut Staging::default())
        });
        drop(plugin);
        if let Err(error) = staged {
            return refuse(self, format!("{folder}: not installed: {error}"));
        }
        // Every version of it there is: loaded, switched off, or refused at
        // start.
        let mut old: Vec<String> = self.data_folder(id).into_iter().collect();
        for dir in dirs {
            for path in data_plugin::folders_of(storage.as_ref(), dir, id.as_str()) {
                if !old.contains(&path) {
                    old.push(path);
                }
            }
        }
        // Its own folder, named as it, in the plugin folder it was
        // downloaded into; one there of another plugin is left alone.
        let taken =
            storage.exists(&target) || paperpaw_sdk::files::is_dir(storage.as_ref(), &target);
        if taken && !old.contains(&target) {
            return refuse(
                self,
                format!("{target}: holds another plugin; `{id}` not installed"),
            );
        }
        // The version before steps aside until the new one is in place.
        let aside = paperpaw_sdk::files::join(dir, &format!(".{id}.old"));
        let _ = storage.remove_dir_all(&aside);
        let moved = old.contains(&target) && storage.rename(&target, &aside).is_ok();
        if old.contains(&target) && !moved {
            return refuse(self, format!("{target}: could not be replaced"));
        }
        if let Err(error) = storage.rename(folder, &target) {
            if moved {
                let _ = storage.rename(&aside, &target);
            }
            return refuse(self, format!("{target}: not installed ({error:?})"));
        }
        old.retain(|path| *path != target);
        Some(Placed {
            target,
            old,
            aside,
            version,
        })
    }

    /// After a plugin was loaded or removed while running: the choices of
    /// fonts and key map presets (a font chosen that is no longer offered
    /// giving way), menu labels, fonts and keys anew.
    fn plugins_changed(&mut self) {
        self.offer_choices();
        self.keep_offered_choices();
        self.set_menu_titles();
        self.apply_reading_font();
        self.apply_status_font();
        self.rebuild_keymap();
        self.output.commands.push(events::PLUGINS_CHANGED.emit(()));
    }

    /// Releases memory reserved while plugins registered: on small devices
    /// every kilobyte counts, and registries are not modified afterwards.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.shared.registry.shrink_to_fit();
        self.keys.plugins.shrink_to_fit();
        self.stylesheet_owners.shrink_to_fit();
        self.keys.presets.shrink_to_fit();
        self.keys.when.shrink_to_fit();
        self.keys.user.shrink_to_fit();
        self.keys.shell.shrink_to_fit();
        self.subscriptions.shrink_to_fit();
        self.action_observers.shrink_to_fit();
        self.event_observers.shrink_to_fit();
        self.shared.plugins.shrink_to_fit();
        self.warnings.shrink_to_fit();
        let shared = &mut self.shared;
        shared.settings.shrink_to_fit();
        shared.menus.shrink_to_fit();
        shared.actions.shrink_to_fit();
        shared.fonts.shrink_to_fit();
        shared.formats.shrink_to_fit();
        shared.hyphenation.shrink_to_fit();
        shared.status_items.shrink_to_fit();
        shared.widgets.shrink_to_fit();
        shared.setup.shrink_to_fit();
        shared.services.shrink_to_fit();
        shared.text_filters.shrink_to_fit();
        shared.decorators.shrink_to_fit();
        shared.input_filters.shrink_to_fit();
        shared.inputs.shrink_to_fit();
        shared.refresh_filters.shrink_to_fit();
        shared.stylesheets.shrink_to_fit();
        shared.translator.shrink_to_fit();
    }
}

/// A plugin put in its place, and what is left of the versions before.
struct Placed {
    /// Its folder.
    target: String,
    /// The folders of other versions of it.
    old: Vec<String>,
    /// Where the version before stepped aside.
    aside: String,
    /// Its version.
    version: paperpaw_sdk::Version,
}

impl Placed {
    /// Removes the versions before.
    fn clear(&self, storage: &dyn Storage) {
        for path in &self.old {
            let _ = storage.remove_dir_all(path);
        }
        let _ = storage.remove_dir_all(&self.aside);
    }
}
