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
        let requested = self.output.refresh.take();
        // The screen on top, nothing over it, best in grays.
        let grays = self.stack.last().is_some_and(|top| {
            top.screen.region(&self.shared.env(self.now)).is_none() && top.screen.grays()
        });
        let mut effects = Effects {
            restart: mem::take(&mut self.restart),
            sleep: mem::take(&mut self.sleep),
            ..Effects::default()
        };
        if let Some(dirty) = self.frame.pending.take() {
            self.frame.unrendered = Some(self.frame.unrendered.map_or(dirty, |u| u.union(&dirty)));
            let interval = self
                .shared
                .settings
                .int(core_settings::FULL_REFRESH_INTERVAL.as_str())
                .clamp(0, i64::from(u32::MAX)) as u32;
            let screen = self.framebuffer.bounds();
            let mut requested = requested.unwrap_or(RefreshMode::Partial);
            // Leaving large pictures: a fast refresh would keep their ghost.
            let area = |r: Rect| u32::from(r.width) * u32::from(r.height);
            if self.frame.pictures_shown && area(dirty) * 2 >= area(screen) {
                requested = RefreshMode::Full;
            }
            let (mode, region) = self
                .frame
                .refresh
                .decide(requested, dirty, screen, interval);
            let refresh = paperpaw_sdk::filters::Refresh {
                mode,
                region,
                screen: self.stack.last().map(|top| top.id.clone()),
            };
            let env = self.shared.env(self.now);
            let refresh = self.shared.refresh_filters.apply(&env, refresh, screen);
            effects.refresh = Some((refresh.region, refresh.mode));
            effects.grays = grays;
            self.shared.usage.refreshed(refresh.mode, refresh.region);
            let top = self.stack.last().map_or(meter::CORE, |s| s.id.namespace());
            self.shared
                .meter
                .refreshed(top, refresh.mode, refresh.region);
        }
        if let Some(at) = self.output.wake_at.take() {
            self.screen_timer = Some(self.screen_timer.map_or(at, |t| t.min(at)));
        }
        let input = self.input.recognizer.next_deadline(&self.thresholds());
        let notice = self.notice.as_ref().map(|(_, until)| *until);
        let scheduled = self.scheduled.iter().map(|(at, ..)| *at).min();
        // After a screenshot is drawn, a message says where it went.
        let shot = self
            .frame
            .screenshot
            .is_some()
            .then(|| self.now.add_millis(300));
        effects.wake_at = [
            self.hint.until,
            input,
            self.screen_timer,
            notice,
            scheduled,
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
                continue;
            }
            for command in mem::take(&mut self.output.commands) {
                self.apply(command);
            }
        }
        if !self.output.commands.is_empty() {
            self.output.commands.clear();
            self.warn_runtime("too many chained commands; the rest was dropped");
        }
    }

    /// Carries out a command, by its kind.
    pub(crate) fn apply(&mut self, command: Command) {
        match command {
            Command::OpenSlot(slot, params) => self.open_slot_command(slot, params),
            Command::OpenScreen(screen, params) => self.open(screen, None, &params),
            Command::Back => {
                if self.stack.len() > 1 {
                    self.close_above(self.stack.len() - 2);
                }
            }
            Command::Notify(text) => self.notify(text),
            Command::Emit(event, params) => self.emit(&event, &params),
            Command::RunAction(action) => self.dispatch(action),
            Command::Schedule(event, params, at) => {
                self.scheduled.retain(|(_, e, _)| *e != event);
                self.scheduled.push((at, event, params));
            }
            Command::Restart => self.restart = true,
            Command::LoadPlugin(folder) => self.load_plugin(&folder),
            Command::RemovePlugin(id) => self.remove_plugin(&id),
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
    fn open_slot_command(&mut self, slot: Id, params: Params) {
        let open = self
            .stack
            .iter()
            .position(|s| s.slot.as_ref() == Some(&slot));
        if let Some(index) = open {
            if params.is_empty() || self.stack[index].params == params {
                if index + 1 < self.stack.len() {
                    self.close_above(index);
                }
                return;
            }
            // The slot for something else (e.g. another book): opened anew
            // in place of the old screen.
            self.close_from(index);
            self.stop_for_closed_screens();
        }
        match self.registry.resolve_slot(slot.as_str()).cloned() {
            Some(screen) => self.open(screen, Some(slot), &params),
            None => self.warn_runtime(format!("no screen for slot `{slot}`")),
        }
    }

    /// An event to its subscribers, then to every open screen.
    fn emit(&mut self, event: &Id, params: &Params) {
        // A page turned, also by itself, is use.
        if *event == events::PAGE_TURNED {
            self.last_activity = self.now;
        }
        let env = self.shared.env(self.now);
        for (_, plugin, handler) in self.subscriptions.iter().filter(|(id, ..)| id == event) {
            let _running = self.shared.meter.enter(plugin.as_str());
            handler(&env, params, &mut self.output.commands);
        }
        for index in 0..self.stack.len() {
            self.with_screen(index, |screen, cx| {
                screen.on_event(event, params, cx);
                Handled::Yes
            });
        }
    }

    /// Settings, a book's own, plugins switched, secrets, menus and key
    /// bindings changed.
    fn apply_setting(&mut self, command: Command) {
        match command {
            Command::SetSetting(id, value) if self.shared.settings.goes_to_scope(id.as_str()) => {
                self.set_in_scope(&id, value);
            }
            Command::SetSetting(id, value) => match self.shared.settings.set(id.as_str(), value) {
                Ok(()) => {
                    self.persist_setting(&id);
                    self.setting_changed(&id);
                    self.output.commands.push(
                        events::SETTING_CHANGED.emit(events::SettingChanged { id: id.clone() }),
                    );
                }
                Err(error) => {
                    self.warn_runtime(format!("`{id}`: {error}"));
                }
            },
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
                let available = self.shared.caps.speech;
                self.io.speech.speak(speech, available, screens, out);
            }
            Command::StopSpeaking => self.io.speech.stop(),
            Command::Serve {
                server,
                port,
                events,
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
                self.io
                    .http
                    .request(request, network, &*self.shared.storage, out)
            }
            _ => {}
        }
    }
}
