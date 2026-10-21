//! The end of every call: commands carried out, what changed drawn and
//! how the panel refreshes.

#[allow(unused_imports)]
use super::*;

impl Kernel {
    pub(crate) fn finish(&mut self) -> Effects {
        self.run_commands();
        // The time since, to the screen on top.
        let top = self.stack.last().map_or(meter::CORE, |s| s.id.namespace());
        let shown = self.now.since(self.shown_since);
        self.shared.meter.shown(top, shown);
        self.shown_since = self.now;
        // A screen that set a scope may have closed.
        self.check_scope_owner();
        self.check_preview_owner();
        self.release_held_notice();
        // Work due now runs before the network is looked at, so that its
        // requests want it in this call; what it brings due runs too.
        for _ in 0..work::MAX_ROUNDS {
            if !self.run_work() {
                break;
            }
        }
        self.check_network();
        self.run_commands();
        if let Some(row) = self.hint_row() {
            let (hints, held) = (self.current_hints(), self.current_holds());
            if hints != self.hint.shown || held != self.hint.held {
                self.hint.shown = hints;
                self.hint.held = held;
                self.invalidate(row);
            }
        }
        if mem::take(&mut self.output.redraw)
            && let Some(top) = self.stack.len().checked_sub(1)
        {
            let region = self.region_of(top);
            self.invalidate(region);
        }
        if let Some(area) = self.output.redraw_area.take()
            && let Some(area) = area.intersection(&self.framebuffer.bounds())
        {
            self.invalidate(area);
        }
        // Set exactly, it wins over what was requested.
        let set = self.output.refresh_set.take();
        let requested = set.or(self.output.refresh.take());
        self.output.refresh.take();
        let told = self.output.refresh_cause.take();
        // The screen on top, nothing over it (or all of the screen its
        // own, what is below showing through), best in grays.
        let bounds = self.framebuffer.bounds();
        let grays = self.stack.last().is_some_and(|top| {
            let whole = match top.screen.region(&self.shared.env(self.now)) {
                None => true,
                Some(region) => {
                    top.screen.shows_through() != ShowsThrough::Nothing
                        && region.intersection(&bounds) == Some(bounds)
                }
            };
            whole && top.screen.grays()
        });
        let mut effects = Effects {
            restart: mem::take(&mut self.restart),
            sleep: mem::take(&mut self.sleep),
            ..Effects::default()
        };
        if let Some(dirty) = self.frame.pending.take() {
            self.frame.unrendered = Some(self.frame.unrendered.map_or(dirty, |u| u.union(&dirty)));
            // What a filter deferred goes with this refresh.
            let dirty = self
                .frame
                .deferred
                .take()
                .map_or(dirty, |d| d.union(&dirty));
            let interval = self
                .shared
                .settings
                .int(core_settings::FULL_REFRESH_INTERVAL.as_str())
                .clamp(0, i64::from(u32::MAX)) as u32;
            let screen = self.framebuffer.bounds();
            let mut requested = requested.unwrap_or(RefreshMode::Partial);
            // Leaving large pictures: a fast refresh would keep their ghost
            // (not where the mode was set exactly).
            let area = |r: Rect| u32::from(r.width) * u32::from(r.height);
            if set.is_none() && self.frame.pictures_shown && area(dirty) * 2 >= area(screen) {
                requested = RefreshMode::Full;
            }
            let (mode, region) = self
                .frame
                .refresh
                .decide(requested, dirty, screen, interval);
            let top = self.stack.last();
            let mut refresh =
                paperpaw_sdk::filters::Refresh::new(mode, region, top.map(|t| t.id.clone()));
            refresh.slot = top.and_then(|t| t.slot.clone());
            refresh.cause = told.unwrap_or(if self.screens_changed {
                paperpaw_sdk::filters::RefreshCause::Screens
            } else {
                paperpaw_sdk::filters::RefreshCause::Other
            });
            refresh.since_full = self.frame.refresh.since_full();
            refresh.periodic = mode != requested;
            refresh.grays = grays;
            refresh.pictures = self.frame.pictures_shown;
            // Never a screen opened or closed, nor what shows while a screen
            // guards the device, kept off the panel; nor too often.
            let may_defer = !self.screens_changed
                && self.guard().is_none()
                && self.frame.defers < drawing::MAX_DEFERS;
            // While a screen guards the device, only its plugin's filters
            // see refreshes: others could count what is typed into it.
            let guard = self.guard().map(|guard| self.stack[guard].id.namespace());
            let env = self.shared.env(self.now);
            match self
                .shared
                .refresh_filters
                .apply_limited(&env, refresh, screen, may_defer, guard)
            {
                Some(refresh) => {
                    self.frame.defers = 0;
                    // Counted as done, after the filters.
                    self.frame.refresh.done(refresh.mode);
                    effects.refresh = Some((refresh.region, refresh.mode));
                    effects.grays = refresh.grays;
                    self.shared.usage.refreshed(refresh.mode, refresh.region);
                    let top = self.stack.last().map_or(meter::CORE, |s| s.id.namespace());
                    self.shared
                        .meter
                        .refreshed(top, refresh.mode, refresh.region);
                }
                None => {
                    self.frame.deferred = Some(dirty);
                    self.frame.defers += 1;
                }
            }
        }
        let input = [
            self.input.recognizer.next_deadline(&self.thresholds()),
            self.input.pending.as_ref().map(|(_, until, _)| *until),
            self.recognizers_due(),
        ]
        .into_iter()
        .flatten()
        .min();
        let notice = self.notice.as_ref().map(|(_, until)| *until);
        let work = self.works.next_wake(&self.base_power(), self.last_activity);
        // After a screenshot is drawn, a message says where it went.
        let shot = self
            .frame
            .screenshot
            .is_some()
            .then(|| self.now.add_millis(300));
        // Only the screen on top runs: a covered one's time waits. Bars
        // shown run too.
        let timer = [
            self.stack.last().and_then(|top| top.timer),
            self.chromes_due(),
        ]
        .into_iter()
        .flatten()
        .min();
        effects.wake_at = [
            self.hint.until,
            input,
            timer,
            notice,
            work,
            self.sleep_due(),
            shot,
            self.network_deadline(),
        ]
        .into_iter()
        .flatten()
        .min();
        // A screen came or went: presses until it shows are dropped.
        if mem::take(&mut self.screens_changed) {
            self.input.settling = true;
        }
        // What was read is kept for the frame the call drew, if any, then
        // forgotten (see `render`).
        if self.frame.unrendered.is_none() {
            self.shared.recall.clear();
        }
        self.timed();
        // What background work said in this call, kept as notes.
        self.keep_notes();
        // The next call is the reader's only if it says so.
        self.shared.reader = false;
        effects
    }

    pub(crate) fn run_commands(&mut self) {
        for _ in 0..MAX_COMMAND_ROUNDS {
            if self.output.commands.is_empty() {
                // All done: the screen on top now is told it shows, if it
                // changed; what that asks for runs too.
                if !mem::take(&mut self.show_pending) {
                    return;
                }
                self.with_top(|screen, cx| {
                    screen.on_show(cx);
                    Handled::Yes
                });
                if let Some(top) = self.stack.last() {
                    let shown = events::ScreenShown::new(top.id.clone()).slot(top.slot.clone());
                    self.output.commands.push(events::SCREEN_SHOWN.emit(shown));
                }
                continue;
            }
            let trusted = mem::take(&mut self.trusted);
            let own = mem::take(&mut self.own_trusted);
            for (i, command) in mem::take(&mut self.output.commands).into_iter().enumerate() {
                self.from_guard = trusted.contains(&i);
                self.from_guard_own = own.contains(&i);
                self.apply(command);
            }
            self.from_guard = false;
            self.from_guard_own = false;
        }
        if !self.output.commands.is_empty() {
            self.output.commands.clear();
            self.warn_runtime("too many chained commands; the rest was dropped");
        }
    }

    /// Carries out a command, by its kind.
    pub(crate) fn apply(&mut self, command: Command) {
        // Network use the plugin's manifest does not declare: refused.
        let Some(command) = self.declared_network(command) else {
            return;
        };
        match command {
            // While a screen guards the device, nobody but it changes how
            // the device is set up: a setting could switch the lock off.
            command if self.held_by_guard() && changes_setup(&command) => {
                self.warn_runtime(format!(
                    "{} not while a screen guards the device",
                    command_name(&command)
                ));
            }
            Command::OpenSlot(slot, params) => self.open_slot_command(slot, None, params),
            Command::OpenInSlot(slot, screen, params) => {
                self.open_slot_command(slot, Some(screen), params);
            }
            Command::Close(key) => self.close_screen(key),
            Command::Replace(screen, params) => self.replace(screen, &params),
            Command::BackTo(id) => self.back_to(&id),
            Command::BindSlot(slot, screen) => self.bind_slot(slot, screen),
            Command::SwitchTask(task) => self.switch_task(task),
            // Not past a screen that guards the device, but from it: what
            // is typed into it is the reader's alone.
            Command::Input(_) if self.held_by_guard() => {}
            Command::Input(name) => match InputKey::parse(&name) {
                Some(key) => self.route_key(key),
                None => self.warn_runtime(format!("unknown input `{name}`")),
            },
            // Laid by the screen itself as it asks (see `with_screen`):
            // here it came from elsewhere.
            Command::SetKeyLayer(..) => {
                self.warn_runtime("a key map layer is laid by its screen alone; not laid");
            }
            Command::OpenScreen(screen, params) => self.open(screen, None, &params),
            // Not past a screen that guards the device, but from it: a
            // screen above it closes itself (the sleep screen on waking).
            Command::Back
                if self.held_by_guard() && self.guard() == self.stack.len().checked_sub(1) => {}
            Command::Back => self.close_top(paperpaw_sdk::CloseCause::Back),
            Command::Notify(text) | Command::Announce(text) => self.notify(text),
            Command::Emit(event, params) => self.emit(&event, &params),
            Command::RunAction(action) => self.dispatch(action),
            Command::RunActionWith(action, params) => self.dispatch_with(action, &params),
            // Run in its place where a handler in place of the action's own
            // asked for it (see `run_handler`): here it came from elsewhere.
            Command::RunOriginal(action, _) => {
                self.warn_runtime(format!(
                    "`{action}`'s own handler runs only from a handler in its place"
                ));
            }
            command @ (Command::Work(_)
            | Command::CancelWork(..)
            | Command::CancelHttp(_)
            | Command::Wear(_)
            | Command::GoOnline) => self.apply_work(command),
            Command::Restart => self.restart = true,
            Command::LoadPlugin(folder) => self.load_plugin(&folder),
            Command::RemovePlugin(id) => self.remove_plugin(&id),
            Command::InstallPlugin(id, folder) => self.install_plugin(&id, &folder),
            Command::Sleep => self.sleep = self.shared.caps.sleep,
            command @ (Command::SetSetting(..)
            | Command::ResetSetting(_)
            | Command::PreviewSettings(_)
            | Command::SetScope(_)
            | Command::SetPluginEnabled(..)
            | Command::SetSecret(_)
            | Command::SetMenu(..)
            | Command::Bind { .. }) => self.apply_setting(command),
            command @ (Command::Redraw(_) | Command::Screenshot(_) | Command::ShowHints) => {
                self.apply_drawing(command);
            }
            command => self.apply_io(command),
        }
    }

    /// Opens the screen bound to `slot`, or shows it again where it is
    /// open for the same (closing what is above it).
    /// Opens the screen bound to `slot`, or `screen` for it
    /// ([`Command::OpenInSlot`]), or shows it again where the slot is open
    /// with it and the same params (closing what is above it).
    pub(crate) fn open_slot_command(&mut self, slot: Id, screen: Option<Id>, params: Params) {
        let open = self.stack.iter().position(|s| {
            s.slot.as_ref() == Some(&slot) && screen.as_ref().is_none_or(|screen| s.id == *screen)
        });
        // Below a screen that guards the device, it stays as it is; one
        // for something else waits below the guard too.
        let below_guard = |index: usize| self.held_by_guard() && self.guard() > Some(index);
        if let Some(index) = open.filter(|&index| below_guard(index)) {
            if params.is_empty() || self.stack[index].params == params {
                return;
            }
        } else if let Some(index) = open {
            if params.is_empty() || self.stack[index].params == params {
                if index + 1 < self.stack.len()
                    && self.may_close_above(index, paperpaw_sdk::CloseCause::Slot)
                {
                    self.close_above(index);
                }
                return;
            }
            if !self.may_close_from(index, paperpaw_sdk::CloseCause::Slot) {
                return;
            }
            // The slot for something else (e.g. another book): opened anew
            // in place of the old screen. What the closed ones covered is
            // drawn again, also where the new one does not cover it.
            let closed = (index..self.stack.len())
                .map(|i| self.changed_region(i))
                .reduce(|a, b| a.union(&b));
            self.close_from(index);
            self.stop_for_closed_screens();
            if let Some(region) = closed {
                self.invalidate(region);
            }
        }
        match screen.or_else(|| self.shared.registry.resolve_slot(slot.as_str()).cloned()) {
            Some(screen) => self.open(screen, Some(slot), &params),
            None => self.warn_runtime(format!("no screen for slot `{slot}`")),
        }
    }

    /// Binds `slot` to `screen` for the user, or back to the plugins'
    /// (see [`Command::BindSlot`]): at once, and kept in `paperpaw.ini`.
    fn bind_slot(&mut self, slot: Id, screen: Option<Id>) {
        match &screen {
            Some(screen) if !self.shared.registry.screens.contains_key(screen) => {
                return self.warn_runtime(format!("unknown screen `{screen}` for slot `{slot}`"));
            }
            Some(screen) => self.shared.registry.bind_user(slot.clone(), screen.clone()),
            None => self.shared.registry.unbind_user(&slot),
        }
        self.update_config(&format_args!("slot `{slot}`"), |source| match &screen {
            Some(screen) => paperpaw_config::set(source, "slots", slot.as_str(), screen.as_str()),
            None => paperpaw_config::remove(source, "slots", slot.as_str()),
        });
        self.shared.slots = self.shared.registry.bound_slots();
    }

    /// An event to its subscribers, then to every open screen. One that a
    /// screen guarding the device gave, or one it opened (the password
    /// typed on the keyboard), goes to those screens and their plugins
    /// alone.
    pub(crate) fn emit(&mut self, event: &Id, params: &Params) {
        // A page turned, also by itself, is use.
        if *event == events::PAGE_TURNED {
            self.last_activity = self.now;
        }
        let guarded = self.guarded_screens();
        if let Some(screens) = guarded {
            return self.emit_to_guard(event, params, &screens);
        }
        // Moments for declared work, and jobs waiting for this event.
        self.works.saw(event, params);
        let env = self.shared.env(self.now);
        let mut refused_secret = false;
        for (_, plugin, handler) in self.subscriptions.iter().filter(|(id, ..)| id == event) {
            let _running = self.shared.meter.enter(plugin.as_str());
            let from = self.output.commands.len();
            handler(&env, params, &mut self.output.commands);
            let reserved = Reserved::of(&self.shared.settings, &self.shared.registry);
            let out = &mut self.output.commands;
            refused_secret |= vet_reserved(out, from, plugin.as_str(), &reserved);
            // Background work says nothing over reading.
            let top = self.stack.last().map(|s| s.id.namespace());
            work::quiet(
                self.shared.reader,
                top,
                self.now,
                from,
                plugin.as_str(),
                &mut self.output.commands,
                &mut self.works,
            );
        }
        if refused_secret {
            self.warn_runtime("a plugin asked what only the settings editor may; not done");
        }
        // A setting may change which bars show and their size, and how
        // input is recognized.
        if *event == events::SETTING_CHANGED {
            self.input.thresholds.set(None);
            self.sync_chromes();
        }
        self.observe(event, params, false);
        self.chromes_hear(event, params);
        // Screens learn that they show from `on_show`.
        if *event == events::SCREEN_SHOWN {
            return;
        }
        for index in 0..self.stack.len() {
            self.with_screen(index, |screen, cx| {
                screen.on_event(event, params, cx);
                Handled::Yes
            });
        }
    }

    /// Where the event being given goes when a screen guarding the device
    /// or one it opened gave it: the places of the guard and of the
    /// screens it opened. `None` for any other event.
    fn guarded_screens(&self) -> Option<Vec<usize>> {
        let guard = self.guard().filter(|_| self.from_guard_own)?;
        Some(
            (guard..self.stack.len())
                .filter(|&i| i == guard || self.stack[i].by_guard)
                .collect(),
        )
    }

    /// [`Self::emit`] for an event of a screen guarding the device: to the
    /// plugins of `screens` that subscribed to it, then to `screens`. No
    /// other plugin, screen, bar or rule hears it.
    fn emit_to_guard(&mut self, event: &Id, params: &Params, screens: &[usize]) {
        let owners: Vec<String> = screens
            .iter()
            .map(|&i| self.stack[i].id.namespace().to_string())
            .collect();
        let env = self.shared.env(self.now);
        for (_, plugin, handler) in self
            .subscriptions
            .iter()
            .filter(|(id, plugin, _)| id == event && owners.iter().any(|o| o == plugin.as_str()))
        {
            let _running = self.shared.meter.enter(plugin.as_str());
            let from = self.output.commands.len();
            handler(&env, params, &mut self.output.commands);
            let reserved = Reserved::of(&self.shared.settings, &self.shared.registry);
            vet_reserved(&mut self.output.commands, from, plugin.as_str(), &reserved);
        }
        let keys: Vec<_> = screens.iter().map(|&i| self.stack[i].serial).collect();
        for key in keys {
            // Each where it is now: one may have closed meanwhile.
            if let Some(index) = self.stack.iter().position(|s| s.serial == key) {
                self.with_screen(index, |screen, cx| {
                    screen.on_event(event, params, cx);
                    Handled::Yes
                });
            }
        }
    }

    /// Settings, a book's own, plugins switched or removed, secrets,
    /// menus, key bindings and slots changed.
    fn apply_setting(&mut self, command: Command) {
        match command {
            Command::SetSetting(id, value) if self.shared.settings.goes_to_scope(id.as_str()) => {
                self.set_in_scope(&id, value);
            }
            Command::SetSetting(id, value) => {
                let old = self.setting_text_if_heard(&id);
                match self.shared.settings.set(id.as_str(), value) {
                    Ok(()) => {
                        self.persist_setting(&id);
                        self.setting_changed(&id);
                        let changed = events::SettingChanged::new(id.clone()).old(old);
                        self.output
                            .commands
                            .push(events::SETTING_CHANGED.emit(changed));
                    }
                    Err(error) => {
                        self.warn_runtime(format!("`{id}`: {error}"));
                    }
                }
            }
            Command::ResetSetting(id) => self.reset_setting(&id),
            Command::PreviewSettings(values) => self.preview_settings(values),
            Command::SetScope(path) => self.set_scope(path),
            Command::SetPluginEnabled(id, enabled) => self.set_plugin_enabled(&id, enabled),
            Command::SetSecret(change) => self.set_secret(change),
            Command::SetMenu(menu, entries) => self.set_menu(menu, entries),
            Command::Bind {
                layer,
                input,
                action,
                ..
            } => self.bind(&layer, &input, action),
            _ => {}
        }
    }

    /// What is drawn: again, as a screenshot, the row of hints.
    fn apply_drawing(&mut self, command: Command) {
        match command {
            Command::Redraw(mode) => {
                self.output.redraw = true;
                if mode == RefreshMode::Full {
                    // Everything, also beneath an overlay.
                    let bounds = self.framebuffer.bounds();
                    self.invalidate(bounds);
                }
                self.output.request_refresh(mode);
            }
            Command::Screenshot(path) => {
                // The whole screen is drawn anew, and saved as it is.
                self.frame.screenshot = Some(path);
                let bounds = self.framebuffer.bounds();
                self.invalidate(bounds);
            }
            Command::ShowHints => self.show_hints(),
            _ => {}
        }
    }

    /// What the shell does for the kernel: firmware, Wi-Fi, the clipboard,
    /// speech, servers, sockets and HTTP.
    /// Speaks `speech` with the voice the settings choose where it speaks
    /// the language (see `paperpaw_sdk::voices`), else with the system's.
    pub(crate) fn speak(&mut self, speech: paperpaw_sdk::Speech) {
        let screens = self.stack.len();
        let env = self.shared.env(self.now);
        let system = self.io.speech.system;
        if let Some(voice) = self.shared.voices.chosen(&env, &speech.language, system) {
            // In place of what the system or another voice says now,
            // unless after it.
            if !speech.queue {
                self.io.speech.stop();
                let other = self
                    .io
                    .speech
                    .voice
                    .as_ref()
                    .is_some_and(|(id, _)| *id != voice.id);
                if other
                    && let Some((id, _)) = self.io.speech.voice.take()
                    && let Some(old) = self.shared.voices.get(id.as_str())
                {
                    (old.stop)(&env, &mut self.output.commands);
                }
            }
            (voice.speak)(&env, &speech, &mut self.output.commands);
            self.io.speech.voice = Some((voice.id.clone(), screens));
            return;
        }
        // In place of what a plugin's voice says, unless after it.
        if !speech.queue {
            self.stop_voice();
        }
        self.io
            .speech
            .speak(speech, system, screens, &mut self.output.commands);
    }

    /// Stops a plugin's voice, if one speaks.
    fn stop_voice(&mut self) {
        if let Some((id, _)) = self.io.speech.voice.take() {
            let env = self.shared.env(self.now);
            if let Some(voice) = self.shared.voices.get(id.as_str()) {
                (voice.stop)(&env, &mut self.output.commands);
            }
        }
    }

    /// Stops speaking: the system's voice and a plugin's.
    pub(crate) fn stop_speaking(&mut self) {
        self.io.speech.stop();
        self.stop_voice();
    }

    fn apply_io(&mut self, command: Command) {
        let (network, screens) = (self.shared.caps.network, self.stack.len());
        let out = &mut self.output.commands;
        match command {
            Command::Firmware(command) => {
                if self.shared.firmware.is_some() {
                    self.io.firmware_calls.push(command);
                } else {
                    self.warn_runtime(String::from("PaperPaw is not this device's firmware"));
                }
            }
            Command::NoNetwork => {
                if self.network_work_waiting() {
                    self.fail_waiting_network();
                }
            }
            Command::Wifi(command) => match &mut self.shared.wifi {
                Some(wifi) => {
                    if command == WifiCommand::Scan {
                        wifi.scanning = true;
                    }
                    self.io.wifi_calls.push(command);
                }
                None => self.warn_runtime(String::from(
                    "Wi-Fi is not managed by PaperPaw on this device",
                )),
            },
            Command::SetClipboard(text) => {
                self.shared.clipboard = clipped(&text);
                self.io.clipboard.copy(self.shared.clipboard.clone());
            }
            Command::ReadClipboard(reply, params) if self.io.clipboard.system => {
                self.io.clipboard.read(reply, params);
            }
            Command::ReadClipboard(reply, params) => self.reply_clipboard(reply, params),
            Command::Speak(speech) => {
                self.speak(speech);
                self.io.focus_spoken = false;
            }
            Command::StopSpeaking => {
                self.stop_speaking();
                self.io.focus_spoken = false;
            }
            Command::Serve {
                server,
                port,
                events,
                ..
            } => self
                .io
                .servers
                .serve(server, port, events, network, screens, out),
            Command::StopServing(server) => self.io.servers.stop(&server, out),
            Command::Respond(response) => self.io.servers.respond(response, &*self.shared.storage),
            command @ (Command::Connect { .. }
            | Command::SocketSend { .. }
            | Command::SocketRead { .. }
            | Command::Disconnect(_)
            | Command::Broadcast { .. }) => {
                self.io
                    .sockets
                    .command(command, network, screens, &*self.shared.storage, out);
            }
            Command::Http(request) => {
                let reader = self.shared.reader;
                self.io
                    .http
                    .request(request, network, reader, &*self.shared.storage, out)
            }
            _ => {}
        }
    }
}

/// Whether `command` changes how the device is set up: settings, secrets,
/// plugins, menus, key bindings, slots.
fn changes_setup(command: &Command) -> bool {
    matches!(
        command,
        Command::SetSetting(..)
            | Command::ResetSetting(_)
            | Command::PreviewSettings(_)
            | Command::SetScope(_)
            | Command::SetPluginEnabled(..)
            | Command::RemovePlugin(_)
            | Command::SetSecret(_)
            | Command::SetMenu(..)
            | Command::Bind { .. }
            | Command::BindSlot(..)
            | Command::LoadPlugin(_)
            | Command::InstallPlugin(..)
    )
}

/// What `command` does, for a warning.
fn command_name(command: &Command) -> &'static str {
    match command {
        Command::SetSetting(..) | Command::ResetSetting(_) | Command::PreviewSettings(_) => {
            "settings changed"
        }
        Command::SetScope(_) => "a book's settings",
        Command::SetPluginEnabled(..)
        | Command::RemovePlugin(_)
        | Command::LoadPlugin(_)
        | Command::InstallPlugin(..) => "plugins changed",
        Command::SetSecret(_) => "secrets changed",
        Command::SetMenu(..) => "menus changed",
        Command::Bind { .. } => "keys bound",
        Command::BindSlot(..) => "slots bound",
        _ => "this",
    }
}

/// What only some may change: other plugins' secrets, settings that keep
/// the device safe, the sleep slot (see [`vet_reserved`]).
pub(crate) struct Reserved<'a> {
    settings: &'a paperpaw_sdk::Settings,
    /// The plugin of the settings editor (the screen of the settings
    /// slot): the reader's own editor.
    editor: Option<&'a str>,
}

impl<'a> Reserved<'a> {
    pub(crate) fn of(
        settings: &'a paperpaw_sdk::Settings,
        registry: &'a crate::registry::Registry,
    ) -> Self {
        let editor = registry
            .resolve_slot(slots::SETTINGS.as_str())
            .map(Id::namespace);
        Self { settings, editor }
    }
}

/// Drops from `out[from..]` what `owner` may not ask, and tells whether
/// it did: another plugin's secret, a setting that keeps the device safe
/// (`SettingSpec::guarded`) of another plugin, the sleep slot. The
/// settings editor may: it is how the reader changes them.
pub(crate) fn vet_reserved(
    out: &mut Vec<Command>,
    from: usize,
    owner: &str,
    reserved: &Reserved<'_>,
) -> bool {
    let editor = reserved.editor == Some(owner);
    let guarded = |id: &Id| {
        id.namespace() != owner
            && reserved
                .settings
                .spec(id.as_str())
                .is_some_and(|spec| spec.guarded)
    };
    let before = out.len();
    let mut index = 0;
    out.retain(|command| {
        let refused = index >= from
            && !editor
            && match command {
                Command::SetSecret(change) => change.id.split('.').next() != Some(owner),
                Command::SetSetting(id, _) | Command::ResetSetting(id) => guarded(id),
                // The sleep screen shows over the lock; the settings
                // editor has these rights.
                Command::BindSlot(slot, _) => *slot == slots::SLEEP || *slot == slots::SETTINGS,
                _ => false,
            };
        index += 1;
        !refused
    });
    out.len() != before
}
