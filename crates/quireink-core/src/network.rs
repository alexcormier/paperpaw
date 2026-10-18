//! The kernel's side of what the shell does for it: HTTP, servers and
//! sockets, speech, the clipboard, Wi-Fi and firmware updates.

#[allow(unused_imports)]
use super::*;

impl Kernel {
    /// What happened with Wi-Fi: plugins hear of it, and network work that
    /// waited goes on once there is a network.
    pub fn wifi_event(&mut self, now: Instant, event: WifiEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let Some(wifi) = &mut self.shared.wifi else {
            return self.finish();
        };
        match event {
            WifiEvent::State(state) => {
                if state != wifi.state {
                    if !state.is_on() {
                        wifi.scanning = false;
                    }
                    wifi.state = state;
                    self.output.commands.push(events::WIFI_CHANGED.emit(()));
                    // Status lines show it (`wifi.status`).
                    self.output.commands.push(events::STATUS_CHANGED.emit(()));
                }
            }
            WifiEvent::Scanned(mut networks) => {
                // Strongest first, each name once (several access points
                // may carry one network).
                networks.sort_by_key(|n| core::cmp::Reverse(n.signal));
                let mut seen: Vec<String> = Vec::new();
                networks.retain(|n| {
                    let new = !n.ssid.is_empty() && !seen.contains(&n.ssid);
                    if new {
                        seen.push(n.ssid.clone());
                    }
                    new
                });
                wifi.networks = networks;
                wifi.scanning = false;
                self.output.commands.push(events::WIFI_SCANNED.emit(()));
            }
            WifiEvent::Failed { ssid, reason } => {
                self.output
                    .commands
                    .push(events::WIFI_FAILED.emit(events::WifiFailed { ssid, reason }));
            }
        }
        self.finish()
    }

    /// Plugins' network work may go to the shell: there is a network, or
    /// the system manages it.
    pub(crate) fn network_ready(&self) -> bool {
        self.shared
            .wifi
            .as_ref()
            .is_none_or(|wifi| wifi.state.is_online())
    }

    /// Network work waiting for the shell (requests, servers, sockets).
    pub(crate) fn network_work_waiting(&self) -> bool {
        self.http.waiting() || self.servers.waiting() || self.sockets.waiting()
    }

    /// After every call, where QuireInk manages Wi-Fi: tells plugins that
    /// work waits for a network ([`events::NETWORK_WANTED`]) or that
    /// nothing uses it any more ([`events::NETWORK_IDLE`]).
    pub(crate) fn check_network(&mut self) {
        let work = self.network_work_waiting();
        let Some(wifi) = &mut self.shared.wifi else {
            return;
        };
        let waiting = work && !wifi.state.is_online();
        match (waiting, self.network_waiting) {
            (true, None) => {
                self.network_waiting = Some(self.now);
                wifi.wanted = true;
                self.output.commands.push(events::NETWORK_WANTED.emit(()));
            }
            (false, Some(_)) => {
                self.network_waiting = None;
                wifi.wanted = false;
            }
            _ => {}
        }
        let busy = self.http.busy() || self.servers.busy() || self.sockets.any();
        wifi.busy = busy;
        if self.network_busy && !busy {
            self.output.commands.push(events::NETWORK_IDLE.emit(()));
        }
        self.network_busy = busy;
    }

    /// When network work that waits fails, if it does not get one first.
    pub(crate) fn network_deadline(&self) -> Option<Instant> {
        self.network_waiting
            .map(|since| since.add_millis(quireink_sdk::wifi::NETWORK_WAIT_MS))
    }

    /// Fails the network work that waited too long, as without a network.
    pub(crate) fn fail_waiting_network(&mut self) {
        const REASON: &str = "no network";
        let storage = &*self.shared.storage;
        let out = &mut self.output.commands;
        self.http.fail_waiting(REASON, storage, out);
        self.servers.fail_waiting(REASON, out);
        self.sockets.fail_waiting(REASON, storage, out);
        self.network_waiting = None;
    }

    /// Whether plugins have sockets or broadcasts open, e.g. to keep
    /// polling for what they bring.
    pub fn sockets_open(&self) -> bool {
        self.sockets.any()
    }

    /// What happened with a socket or a broadcast.
    pub fn socket_event(&mut self, now: Instant, event: SocketEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let storage = &*self.shared.storage;
        self.sockets
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// What happened with a server: started, failed, a request.
    pub fn server_event(&mut self, now: Instant, event: ServerEvent) -> Effects {
        self.now = now;
        let storage = &*self.shared.storage;
        self.servers
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// Delivers the answer to a request ([`Io::Http`]).
    pub fn http_response(&mut self, now: Instant, response: HttpResponse) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let storage = &*self.shared.storage;
        self.http
            .answer(response, storage, &mut self.output.commands);
        self.finish()
    }

    /// What the shell is to do: requests, servers, sockets, speech, the
    /// clipboard, the radio, the firmware (see [`Io`]). Check after every
    /// kernel call. Network work waits here until there is a network,
    /// where QuireInk manages Wi-Fi.
    pub fn take_io(&mut self) -> Vec<Io> {
        let mut io: Vec<Io> = self.wifi_calls.drain(..).map(Io::Wifi).collect();
        io.extend(self.firmware_calls.drain(..).map(Io::Firmware));
        io.extend(self.speech.take().into_iter().map(Io::Speech));
        io.extend(self.clipboard.take_copied().map(Io::Copy));
        if self.clipboard.take_read() {
            io.push(Io::ReadClipboard);
        }
        if self.network_ready() {
            io.extend(self.http.take().into_iter().map(Io::Http));
            io.extend(self.servers.take().into_iter().map(Io::Server));
            io.extend(self.sockets.take().into_iter().map(Io::Socket));
        }
        io
    }

    /// Reads part of a file being sent as an answer
    /// ([`ServerCall::Respond`] with a `file`), from `at` into `buffer`;
    /// returns how much was read, 0 at the end.
    pub fn read_served(
        &self,
        path: &str,
        at: u64,
        buffer: &mut [u8],
    ) -> Result<usize, quireink_platform::StorageError> {
        self.shared.storage.read_at(path, at, buffer)
    }

    /// Answers a [`Command::ReadClipboard`] with the text known here.
    pub(crate) fn reply_clipboard(&mut self, reply: Id, params: Params) {
        let typed = quireink_sdk::Typed {
            text: self.shared.clipboard.clone(),
            context: params,
        };
        self.output
            .commands
            .push(Command::Emit(reply, typed.to_params()));
    }

    /// Whether plugins serve, e.g. to keep polling for requests.
    pub fn serving(&self) -> bool {
        self.servers.busy()
    }

    /// How an install goes: plugins hear of it.
    pub fn firmware_state(&mut self, now: Instant, state: FirmwareState) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some(firmware) = &mut self.shared.firmware
            && firmware.state != state
        {
            firmware.state = state;
            self.output.commands.push(events::FIRMWARE_CHANGED.emit(()));
        }
        self.finish()
    }

    /// Text `id` of a [`SpeechCall::Say`] was spoken (`spoken`), or could
    /// not be.
    pub fn speech_done(&mut self, now: Instant, id: u32, spoken: bool) -> Effects {
        self.now = now;
        self.speech.done(id, spoken, &mut self.output.commands);
        self.finish()
    }

    /// Whether a text is being spoken, e.g. to keep watching for its end.
    pub fn speaking(&self) -> bool {
        self.speech.speaking()
    }

    /// The system clipboard's text, for the plugins that asked for it
    /// ([`Io::ReadClipboard`]); `None` if it could not be read (the text
    /// copied here is used).
    pub fn clipboard_text(&mut self, now: Instant, text: Option<String>) -> Effects {
        self.now = now;
        if let Some(text) = text {
            self.shared.clipboard = clipped(&text);
        }
        for (reply, params) in self.clipboard.answered() {
            self.reply_clipboard(reply, params);
        }
        self.finish()
    }

    /// Tells the kernel whether the system's clipboard has text, e.g. when
    /// the app comes back from another one (without reading it, which some
    /// systems announce to the user).
    pub fn set_clipboard_available(&mut self, available: bool) {
        self.shared.system_clipboard = available;
    }
}
