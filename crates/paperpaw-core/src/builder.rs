//! Building a kernel: what the shell says of its device, the plugins, the
//! configuration, then the kernel made from them ([`KernelBuilder`]).

#[allow(unused_imports)]
use super::*;

/// Configures and creates a [`Kernel`].
pub struct KernelBuilder {
    device: DeviceInfo,
    plugins: Vec<Box<dyn Plugin>>,
    /// Plugins shipped switched off, until the user switches them on.
    off: Vec<String>,
    device_keymap: Option<Cow<'static, str>>,
    user_config: Option<String>,
    glyph_cache: usize,
    /// What the shell does; the device's touch, the firmware and the row
    /// of buttons come from their own settings at the end.
    caps: Capabilities,
    firmware: Option<Firmware>,
    code: Option<crate::code_plugin::CodePlugins>,
    safe_mode: Option<paperpaw_sdk::native::SafeMode>,
    /// Plugins the shell put into folders, to install before plugins load.
    staged: Vec<(Id, String)>,
    button_row: &'static [&'static str],
    frame_rows: Option<u16>,
    row_align: u16,
    storage: Option<Box<dyn Storage>>,
    clock: Option<fn() -> u64>,
    busy_mark: bool,
    setting_defaults: Vec<(String, String)>,
    plugin_dirs: Vec<String>,
    font_dirs: Vec<String>,
    catalog_device: Option<&'static str>,
    random: paperpaw_sdk::random::Random,
}

impl fmt::Debug for KernelBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KernelBuilder")
            .field("device", &self.device.id)
            .field("plugins", &self.plugins.len())
            .finish_non_exhaustive()
    }
}

impl KernelBuilder {
    /// Adds a plugin. Plugins registered later win priority ties.
    pub fn plugin(mut self, plugin: impl Plugin + 'static) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Adds a plugin that is off until the user switches it on (e.g. in
    /// the plugin manager), for features not everyone wants.
    pub fn plugin_off(mut self, plugin: impl Plugin + 'static) -> Self {
        self.off.push(String::from(plugin.manifest().id.as_str()));
        self.plugin(plugin)
    }

    /// Key bindings specific to the device (e.g. its physical buttons).
    pub fn device_keymap(mut self, source: impl Into<Cow<'static, str>>) -> Self {
        self.device_keymap = Some(source.into());
        self
    }

    /// The user's configuration file (`paperpaw.ini`). It overrides
    /// settings, slot bindings and key maps, and can disable plugins.
    pub fn user_config(mut self, source: impl Into<String>) -> Self {
        self.user_config = Some(source.into());
        self
    }

    /// The device's storage (books, settings, plugin data). Without it the
    /// kernel runs with an empty, read-only storage.
    pub fn storage(mut self, storage: impl Storage + 'static) -> Self {
        self.storage = Some(Box::new(storage));
        self
    }

    /// Like [`storage`](Self::storage), for a storage chosen at run time
    /// (e.g. the SD card, or none without one).
    pub fn storage_boxed(mut self, storage: Box<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// A monotonic clock in microseconds, for measuring the time each
    /// plugin's code takes ([`paperpaw_sdk::usage::PluginUsage`]).
    /// Shows a mark at once when an input opens or closes a screen that
    /// took long the last time (or was not opened yet), before the work:
    /// the pressed button's hint inverted, or a sign in the corner. For
    /// shells whose panel refreshes on its own while the processor works
    /// (the X4's), which handle [`Effects::proceed`]; it needs the
    /// [`clock`](KernelBuilder::clock) to know what is slow.
    pub fn busy_mark(mut self, on: bool) -> Self {
        self.busy_mark = on;
        self
    }

    pub fn clock(mut self, clock: fn() -> u64) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Changes the default of a setting for this device, e.g. the
    /// two-button key map preset on a device with only two buttons. Users
    /// can still override it.
    pub fn setting_default<T: SettingType>(self, setting: &Setting<T>, value: T::Input) -> Self {
        let text = T::write(value).to_config_text();
        self.setting_default_text(setting.as_str(), &text)
    }

    /// As [`setting_default`](Self::setting_default), for a setting known only at run
    /// time (e.g. from a device profile's file), its value written as in
    /// `paperpaw.ini`.
    pub fn setting_default_text(mut self, id: &str, value: &str) -> Self {
        self.setting_defaults
            .push((String::from(id), String::from(value)));
        self
    }

    /// A folder on the storage with font files (OpenType/TrueType) to
    /// read with; they appear as one plugin, "font folder". Only for
    /// devices that render scalable fonts.
    pub fn font_dir(mut self, dir: impl Into<String>) -> Self {
        self.font_dirs.push(dir.into());
        self
    }

    /// A folder on the storage with data plugins (see `data_plugin`), one
    /// subfolder each. Without any, `plugins` is used.
    pub fn plugin_dir(mut self, dir: impl Into<String>) -> Self {
        self.plugin_dirs.push(dir.into());
        self
    }

    /// What plugin libraries call this kind of device, e.g. `x4`, `x4-pro`
    /// or `android` for paperpaw.org's (ADR 0023): its plugin store then
    /// offers what is made for it. Without, none is offered.
    pub fn catalog_device(mut self, name: &'static str) -> Self {
        self.catalog_device = Some(name);
        self
    }

    /// The platform's source of random bytes, for keys and tokens
    /// ([`paperpaw_sdk::random`]): `fill` fills a buffer and says whether
    /// it could. Without, nothing that needs one works (e.g. the transfer
    /// by browser).
    pub fn random(mut self, fill: fn(&mut [u8]) -> bool) -> Self {
        self.random = paperpaw_sdk::random::Random::new(fill);
        self
    }

    /// PaperPaw manages Wi-Fi itself on this device (the shell answers
    /// [`Io::Wifi`] and reports with [`Kernel::wifi_event`]);
    /// it implies [`network`](Self::network). See [`paperpaw_sdk::wifi`].
    pub fn wifi(mut self, managed: bool) -> Self {
        self.caps.wifi = managed;
        self.caps.network |= managed;
        self
    }

    /// PaperPaw loads native code plugins (see
    /// [`paperpaw_sdk::native`]): `loader` loads the code of each switched
    /// on at the start, files fit for `native` (which plugins see as
    /// [`Env::native`](paperpaw_sdk::Env)).
    pub fn code_plugins(mut self, native: Native, loader: Box<dyn CodeLoader>) -> Self {
        self.code = Some(crate::code_plugin::CodePlugins {
            native,
            loader,
            safe_mode: None,
        });
        self
    }

    /// This start goes without code plugins (safe mode): the shell found
    /// that the last ones did not get to their first frame, or the reader
    /// held the key for it. See [`paperpaw_sdk::native::SafeMode`]; the
    /// kernel fills in what it switched off.
    pub fn safe_mode(mut self, safe_mode: SafeMode) -> Self {
        self.safe_mode = Some(safe_mode);
        self
    }

    /// Installs the plugin `id` the shell put into `folder` (a folder of
    /// the plugin folder PaperPaw does not load, e.g. `plugins/.hello.part`:
    /// a plugin the web flasher wrote with the firmware) before plugins
    /// are found, so it loads at this start: in place of every version of
    /// it, as [`Command::InstallPlugin`](paperpaw_sdk::Command::InstallPlugin)
    /// installs one while PaperPaw runs. One that is not what it says is
    /// refused, with a warning, and its folder removed.
    pub fn install_plugin(mut self, id: Id, folder: impl Into<String>) -> Self {
        self.staged.push((id, folder.into()));
        self
    }

    /// PaperPaw is the device's firmware and updates it (the shell answers
    /// [`Io::Firmware`] and reports with
    /// [`Kernel::firmware_state`]). See [`paperpaw_sdk::firmware`].
    pub fn firmware(mut self, firmware: Firmware) -> Self {
        self.firmware = Some(firmware);
        self
    }

    /// The shell makes HTTP requests and serves ([`Io::Http`],
    /// [`Io::Server`], [`Io::Socket`]); without, plugins' requests fail
    /// at once.
    pub fn network(mut self, available: bool) -> Self {
        self.caps.network = available;
        self
    }

    /// The shell reads the system's clipboard when plugins ask for its
    /// text ([`Io::ReadClipboard`]); without, only text copied
    /// in PaperPaw is pasted.
    pub fn system_clipboard(mut self, available: bool) -> Self {
        self.caps.system_clipboard = available;
        self
    }

    /// The shell speaks text aloud ([`Io::Speech`]); without,
    /// plugins' speech fails at once.
    pub fn speech(mut self, available: bool) -> Self {
        self.caps.speech = available;
        self
    }

    /// The shell sleeps after showing PaperPaw's sleep screen (see
    /// [`Effect::Sleep`]); without, the system has its own (Android).
    pub fn sleep(mut self, available: bool) -> Self {
        self.caps.sleep = available;
        self
    }

    /// The shell turns the screen as the setting
    /// [`settings::ROTATION`](paperpaw_sdk::builtin::settings::ROTATION)
    /// says (see [`Kernel::rotation`]); without, it is not offered.
    pub fn rotation(mut self, available: bool) -> Self {
        self.caps.rotation = available;
        self
    }

    /// The shell shows black and white in the colours of the settings
    /// [`settings::TEXT_COLOR`](paperpaw_sdk::builtin::settings::TEXT_COLOR)
    /// and [`settings::PAPER_COLOR`](paperpaw_sdk::builtin::settings::PAPER_COLOR)
    /// (see [`Kernel::colors`]); without, they are not offered.
    /// The buttons that lie in a row below the screen, left to right, each
    /// under an equal share of its width (e.g. the X4's four): a row of
    /// hints above them shows what they do
    /// ([`settings::BUTTON_HINTS`](core_settings::BUTTON_HINTS)).
    pub fn button_row(mut self, buttons: &'static [&'static str]) -> Self {
        self.button_row = buttons;
        self
    }

    pub fn colors(mut self, available: bool) -> Self {
        self.caps.colors = available;
        self
    }

    /// Memory for glyphs rasterized at runtime (default 32 KiB). Bitmap font
    /// packs do not use it.
    pub fn glyph_cache(mut self, bytes: usize) -> Self {
        self.glyph_cache = bytes;
        self
    }

    /// Draws frames in bands of `rows` rows instead of holding the whole
    /// screen in RAM: the framebuffer shrinks to `rows / height` of its
    /// size, while each band costs one more pass over the visible screens.
    /// For devices whose panel controller keeps the frame (e.g. the X4).
    pub fn frame_rows(mut self, rows: u16) -> Self {
        self.frame_rows = Some(rows);
        self
    }

    /// Passes frames to [`Kernel::render`] in rows starting and ending at
    /// multiples of `rows`, e.g. 8 where the panel controller takes 8 of
    /// them in a byte (the X4's, turned). Frame bands should be a multiple
    /// of it.
    pub fn row_align(mut self, rows: u16) -> Self {
        self.row_align = rows.max(1);
        self
    }

    pub fn build(mut self) -> Result<Kernel, KernelError> {
        let mut kernel = self.new_kernel()?;
        paperpaw_sdk::data::take_over_old_name(kernel.shared.storage.as_ref());
        kernel.read_secrets();
        paperpaw_sdk::library::take_over_old(kernel.shared.storage.as_ref());
        // The configuration file on the device, then the shell's overrides.
        let stored = read_config(kernel.shared.storage.as_ref());
        let user_config = self.user_config.take();
        let configs: Vec<&str> = [stored.as_deref(), user_config.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        let dirs = if self.plugin_dirs.is_empty() {
            alloc::vec![String::from(paperpaw_sdk::builtin::PLUGIN_DIR)]
        } else {
            mem::take(&mut self.plugin_dirs)
        };
        kernel.install_staged(mem::take(&mut self.staged), &dirs);
        kernel.add_plugins(
            mem::take(&mut self.plugins),
            mem::take(&mut self.off),
            dirs,
            mem::take(&mut self.font_dirs),
            &configs,
            self.code
                .take()
                .map(|code| crate::code_plugin::CodePlugins {
                    safe_mode: self.safe_mode.take(),
                    ..code
                }),
        );
        if let Some(source) = self.device_keymap.take() {
            kernel.load_device_keymap(source);
        }
        kernel.offer_choices();
        kernel.apply_defaults(&self.setting_defaults);
        for source in &configs {
            kernel.apply_user_config(source);
        }
        kernel.set_menu_titles();
        kernel.apply_reading_font();
        kernel.apply_status_font();
        kernel.validate();
        kernel.rebuild_keymap();
        kernel.apply_display_settings();
        kernel.settle();
        Ok(kernel)
    }

    /// The kernel with nothing registered: its frame, its storage (metered,
    /// and what a call reads kept for the rest of it), what the shell said
    /// it does.
    fn new_kernel(&mut self) -> Result<Kernel, KernelError> {
        let display = self.device.display;
        let rows = self.frame_rows.unwrap_or(display.height);
        let framebuffer = Framebuffer::banded(display.width, display.height, rows, display.format)
            .map_err(KernelError::Graphics)?;
        let meter = meter::Meter::new(self.clock);
        // What a call reads, kept for the rest of it (more where memory is
        // plentiful).
        let recall = alloc::rc::Rc::new(recall::Recall::new(match self.device.memory {
            paperpaw_platform::MemoryClass::Small => 16 * 1024,
            paperpaw_platform::MemoryClass::Large => 256 * 1024,
        }));
        let storage: alloc::rc::Rc<dyn Storage> = alloc::rc::Rc::new(meter::Metered {
            inner: Box::new(recall::Recalled {
                inner: self.storage.take().unwrap_or_else(|| Box::new(NoStorage)),
                recall: recall.clone(),
            }),
            meter: meter.clone(),
        });
        // What the shell said it does, and what follows from the rest.
        let mut caps = self.caps;
        caps.touch = self.device.input.touch;
        caps.black_and_white = display.format == paperpaw_platform::PixelFormat::Mono1;
        caps.firmware = self.firmware.is_some();
        caps.button_row = !self.button_row.is_empty();
        Ok(Kernel {
            shared: Shared {
                meter,
                device: self.device,
                settings: Settings::new(),
                translator: Translator::new(),
                fonts: Fonts::new(self.glyph_cache),
                formats: Formats::new(),
                hyphenation: Hyphenation::new(),
                menus: Menus::new(),
                actions: Actions::new(),
                plugins: Plugins::new(),
                status: SystemStatus::default(),
                usage: paperpaw_sdk::usage::Usage::default(),
                status_items: StatusItems::new(),
                widgets: paperpaw_sdk::widgets::Widgets::new(),
                setup: paperpaw_sdk::setup::SetupSteps::new(),
                services: paperpaw_sdk::services::Services::new(),
                text_filters: paperpaw_sdk::filters::TextFilters::new(),
                decorators: paperpaw_sdk::decorations::Decorators::new(),
                input_filters: paperpaw_sdk::filters::InputFilters::new(),
                refresh_filters: paperpaw_sdk::filters::RefreshFilters::new(),
                stylesheets: Vec::new(),
                book_data: Vec::new(),
                caps,
                wifi: caps.wifi.then(paperpaw_sdk::wifi::Wifi::default),
                firmware: self.firmware.take(),
                native: None,
                clipboard: String::new(),
                system_clipboard: false,
                slots: Vec::new(),
                screens: Vec::new(),
                font_dirs: Vec::new(),
                data_folders: Vec::new(),
                plugin_dirs: Vec::new(),
                catalog_device: self.catalog_device,
                secrets: Default::default(),
                random: self.random,
                storage,
                recall,
                keymap: Keymap::default(),
            },
            frame: drawing::FrameState {
                pending: None,
                unrendered: None,
                rendered: None,
                row_align: self.row_align,
                band: self
                    .frame_rows
                    .map(|rows| u32::from(rows) * u32::from(display.width)),
                pictures_shown: false,
                screenshot: None,
                mark: None,
                refresh: RefreshPolicy::default(),
            },
            input: gestures::InputState {
                recognizer: Recognizer::default(),
                captured: Vec::new(),
                edge_touch: None,
                edge_pending: None,
                settling: false,
                stale_before: None,
                shown_at: [None; 2],
                dropped_buttons: Vec::new(),
                dropped_pointers: Vec::new(),
                swapped: Vec::new(),
            },
            io: network::IoState {
                http: io::http::Http::default(),
                servers: io::servers::Servers::default(),
                sockets: io::sockets::Sockets::default(),
                speech: io::speech::Speech::default(),
                clipboard: io::clipboard::Clipboard::new(caps.system_clipboard),
                wifi_calls: Vec::new(),
                firmware_calls: Vec::new(),
                network_waiting: None,
                network_busy: false,
            },
            keys: plugins::KeySources::default(),
            timing: drawing::Timing::default(),
            framebuffer,
            registry: Registry::default(),
            subscriptions: Vec::new(),
            screen_timer: None,
            show_pending: false,
            notice: None,
            scheduled: Vec::new(),
            stack: Vec::new(),
            output: Output::default(),
            warnings: Vec::new(),
            restart: false,
            sleep: false,
            hint: hints::HintRow::new(self.button_row),
            busy_mark: self.busy_mark,
            withheld: Vec::new(),
            last_activity: Instant(0),
            shown_since: Instant(0),
            scope: None,
            screens_changed: false,
            preview_owner: None,
            plugin_defaults: Vec::new(),
            now: Instant::default(),
        })
    }
}

/// The steps of [`KernelBuilder::build`].
impl Kernel {
    /// Passwords and keys, as kept on the storage.
    fn read_secrets(&mut self) {
        if let Ok(data) = paperpaw_sdk::read_file(
            self.shared.storage.as_ref(),
            paperpaw_sdk::secrets::FILE,
            64 * 1024,
        ) {
            let text = String::from_utf8_lossy(&data);
            self.shared.secrets = paperpaw_sdk::secrets::Secrets::decode(&text);
        }
    }

    /// The kernel's own plugin, the shell's, those of data on the storage
    /// and of fonts in its folders, but those switched off in `configs`
    /// or shipped off (`off`) and not switched on.
    fn add_plugins(
        &mut self,
        plugins: Vec<Box<dyn Plugin>>,
        off: Vec<String>,
        plugin_dirs: Vec<String>,
        font_dirs: Vec<String>,
        configs: &[&str],
        code: Option<crate::code_plugin::CodePlugins>,
    ) {
        self.shared.menus.set_capabilities(self.shared.caps);
        // Colour screens open documents with colour pictures (ADR 0015).
        let display = self.shared.device.display;
        self.shared.formats.set_color(display.format.is_color());
        // Pictures no larger than the screen shows them.
        let fit = paperpaw_sdk::image::Fit::screen(display.width.into(), display.height.into());
        self.shared.formats.set_image_fit(fit);
        let mut disabled: Vec<String> = configs
            .iter()
            .flat_map(|c| plugin_switches(c, false))
            .collect();
        // Shipped off, unless switched on.
        let on: Vec<String> = configs
            .iter()
            .flat_map(|c| plugin_switches(c, true))
            .collect();
        disabled.extend(off.into_iter().filter(|id| !on.contains(id)));
        let core: Box<dyn Plugin> = Box::new(CorePlugin);
        for plugin in core::iter::once(core).chain(plugins) {
            self.add_plugin(plugin.as_ref(), &disabled);
        }
        // Data plugins from the storage, after the built-in ones.
        let dirs = if plugin_dirs.is_empty() {
            alloc::vec![String::from(paperpaw_sdk::builtin::PLUGIN_DIR)]
        } else {
            plugin_dirs
        };
        let mut with_code = Vec::new();
        for dir in &dirs {
            let mut problems = Vec::new();
            let found = data_plugin::discover(self.shared.storage.as_ref(), dir, &mut |problem| {
                problems.push(problem);
            });
            for problem in problems {
                self.warn_runtime(problem);
            }
            for plugin in found {
                self.shared
                    .data_folders
                    .push((plugin.manifest().id, plugin.folder().to_string()));
                if plugin.has_code() {
                    with_code.push(plugin);
                    continue;
                }
                self.add_plugin(&plugin, &disabled);
                if let Some(info) = self.shared.plugins.get_mut(plugin.manifest().id.as_str()) {
                    info.kind = paperpaw_sdk::PluginKind::Data;
                }
            }
        }
        self.add_code_plugins(with_code, &disabled, code);
        self.shared.plugin_dirs = dirs;
        if !font_dirs.is_empty()
            && let Some(plugin) = data_plugin::font_folder(self.shared.storage.as_ref(), &font_dirs)
        {
            self.add_plugin(&plugin, &disabled);
        }
        self.shared.font_dirs = font_dirs;
        self.find_required_plugins();
    }

    /// The device's own key map, under all others.
    fn load_device_keymap(&mut self, source: Cow<'static, str>) {
        for problem in self.shared.keymap.load(&source, Origin::Device, true) {
            self.warn(
                WarningSource::DeviceKeymap,
                Some(problem.line),
                problem.message.into_owned(),
            );
        }
        self.keys.sources.push((Origin::Device, source));
    }

    /// The choices of settings that plugins add to: key map presets and
    /// reading fonts.
    pub(crate) fn offer_choices(&mut self) {
        let mut presets: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(keymap_presets::STANDARD_NAME)];
        presets.extend(
            self.keys
                .presets
                .iter()
                .map(|(id, _)| Cow::Owned(id.as_str().to_string())),
        );
        self.shared
            .settings
            .set_choices(core_settings::KEYMAP_PRESET.as_str(), presets);
        let mut families: Vec<Cow<'static, str>> =
            alloc::vec![Cow::Borrowed(core_settings::DEFAULT_FONT)];
        families.extend(
            self.shared
                .fonts
                .families()
                .into_iter()
                .map(|f| Cow::Owned(f.to_string())),
        );
        let mut status: Vec<Cow<'static, str>> = alloc::vec![
            Cow::Borrowed(core_settings::STATUS_FONT_UI),
            Cow::Borrowed(core_settings::STATUS_FONT_BOOK),
        ];
        status.extend(families.iter().skip(1).cloned());
        self.shared
            .settings
            .set_choices(core_settings::READING_FONT.as_str(), families);
        self.shared
            .settings
            .set_choices(core_settings::STATUS_FONT.as_str(), status);
    }

    /// Defaults from plugins (e.g. themes), then the device's.
    pub(crate) fn apply_defaults(&mut self, device: &[(String, String)]) {
        for (plugin, id, value) in mem::take(&mut self.plugin_defaults) {
            if let Err(error) = self
                .shared
                .settings
                .set_default_from_text(id.as_str(), &value)
            {
                self.warn(
                    WarningSource::Plugin(plugin),
                    None,
                    format!("default for `{id}`: {error}"),
                );
            }
        }
        for (id, value) in device {
            if let Err(error) = self.shared.settings.set_default_from_text(id, value) {
                self.warn_runtime(format!("device default `{id}`: {error}"));
            }
        }
    }

    /// The language, night mode and colours the settings say.
    fn apply_display_settings(&mut self) {
        let shared = &mut self.shared;
        shared
            .translator
            .set_language(shared.settings.read(&core_settings::LANGUAGE));
        let settings = &self.shared.settings;
        self.framebuffer
            .set_inverted(settings.bool(core_settings::NIGHT_MODE.as_str()));
        self.framebuffer
            .set_pictures_kept(!settings.bool(core_settings::NIGHT_PICTURES.as_str()));
        self.framebuffer.set_mono_coverage(mono_coverage(settings));
        if let Some(panel) = self.shared.device.display.color {
            self.framebuffer.set_saturation(panel.boost_percent());
        }
    }

    /// Done registering: memory given back, the slots and screens known,
    /// the row of hints as tall as the first screen is made for.
    fn settle(&mut self) {
        self.shrink_to_fit();
        self.shared.slots = self.registry.bound_slots();
        self.shared.screens = self
            .registry
            .screens
            .iter()
            .map(|(id, spec)| (id.clone(), spec.title.clone()))
            .collect();
        self.hint.height = self.wanted_hint_height();
    }
}

impl Kernel {
    pub fn builder(device: DeviceInfo) -> KernelBuilder {
        KernelBuilder {
            device,
            plugins: Vec::new(),
            off: Vec::new(),
            device_keymap: None,
            user_config: None,
            glyph_cache: DEFAULT_GLYPH_CACHE,
            caps: Capabilities::default(),
            firmware: None,
            code: None,
            safe_mode: None,
            staged: Vec::new(),
            button_row: &[],
            frame_rows: None,
            row_align: 1,
            storage: None,
            clock: None,
            busy_mark: false,
            setting_defaults: Vec::new(),
            plugin_dirs: Vec::new(),
            font_dirs: Vec::new(),
            catalog_device: None,
            random: paperpaw_sdk::random::Random::NONE,
        }
    }
}
