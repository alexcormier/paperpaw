//! The kernel's side of what the shell does for it: HTTP, servers and
//! sockets, speech, the clipboard, Wi-Fi and firmware updates.

#[allow(unused_imports)]
use super::*;

/// What the shell is to do for the kernel, and what it does: HTTP,
/// servers, sockets, speech, the clipboard, Wi-Fi, firmware updates.
#[derive(Debug)]
pub(crate) struct IoState {
    /// Plugins' requests ([`Io::Http`]).
    pub(crate) http: io::http::Http,
    /// Plugins' servers ([`Io::Server`]).
    pub(crate) servers: io::servers::Servers,
    /// Plugins' sockets and broadcasts ([`Io::Socket`]).
    pub(crate) sockets: io::sockets::Sockets,
    /// Text spoken ([`Io::Speech`]).
    pub(crate) speech: io::speech::Speech,
    /// The system's clipboard ([`Io::Copy`], [`Io::ReadClipboard`]).
    pub(crate) clipboard: io::clipboard::Clipboard,
    /// What the shell is to do with the radio ([`Io::Wifi`]).
    pub(crate) wifi_calls: Vec<WifiCommand>,
    /// What the shell is to do with the firmware ([`Io::Firmware`]).
    pub(crate) firmware_calls: Vec<FirmwareCommand>,
    /// Since when plugins' network work has waited for a network.
    pub(crate) network_waiting: Option<Instant>,
    /// Something used the network at the end of the last call.
    pub(crate) network_busy: bool,
    /// What is spoken is the kernel's own word of the focus
    /// (`Kernel::speak_focus`), which the next replaces.
    pub(crate) focus_spoken: bool,
    /// The focus last told to plugins (`events::FOCUS_CHANGED`).
    pub(crate) focus_told: Option<paperpaw_sdk::semantics::Part>,
}

impl Kernel {
    /// What happened with Wi-Fi: plugins hear of it, and network work that
    /// waited goes on once there is a network.
    pub fn wifi_event(&mut self, now: Instant, event: WifiEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // While the reader waits for the network, what Wi-Fi says is theirs.
        self.shared.reader = self.io.http.reader_waits();
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
                    .push(events::WIFI_FAILED.emit(events::WifiFailed::new(ssid, reason)));
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
        self.io.http.waiting() || self.io.servers.waiting() || self.io.sockets.waiting()
    }

    /// After every call, where PaperPaw manages Wi-Fi: tells plugins that
    /// work waits for a network ([`events::NETWORK_WANTED`]) or that
    /// nothing uses it any more ([`events::NETWORK_IDLE`]).
    pub(crate) fn check_network(&mut self) {
        let work = self.network_work_waiting();
        let Some(wifi) = &mut self.shared.wifi else {
            return;
        };
        let waiting = work && !wifi.state.is_online();
        match (waiting, self.io.network_waiting) {
            (true, None) => {
                self.io.network_waiting = Some(self.now);
                self.output.commands.push(events::NETWORK_WANTED.emit(()));
            }
            (false, Some(_)) => self.io.network_waiting = None,
            _ => {}
        }
        // Also once waiting work failed (`fail_waiting_network`): nothing
        // wants the radio then.
        wifi.wanted = self.io.network_waiting.is_some();
        let busy = self.io.http.busy() || self.io.servers.busy() || self.io.sockets.any();
        wifi.busy = busy;
        if self.io.network_busy && !busy {
            self.output.commands.push(events::NETWORK_IDLE.emit(()));
        }
        self.io.network_busy = busy;
    }

    /// When network work that waits fails, if it does not get one first.
    pub(crate) fn network_deadline(&self) -> Option<Instant> {
        self.io
            .network_waiting
            .map(|since| since.add_millis(paperpaw_sdk::wifi::NETWORK_WAIT_MS))
    }

    /// Fails the network work that waited too long, as without a network.
    pub(crate) fn fail_waiting_network(&mut self) {
        const REASON: &str = "no network";
        let storage = &*self.shared.storage;
        let out = &mut self.output.commands;
        self.io.http.fail_waiting(REASON, storage, out);
        self.io.servers.fail_waiting(REASON, out);
        self.io.sockets.fail_waiting(REASON, storage, out);
        self.io.network_waiting = None;
    }

    /// Whether plugins have sockets or broadcasts open, e.g. to keep
    /// polling for what they bring.
    pub fn sockets_open(&self) -> bool {
        self.io.sockets.any()
    }

    /// What happened with a socket or a broadcast.
    pub fn socket_event(&mut self, now: Instant, event: SocketEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // Servers and sockets are the reader's connection.
        self.shared.reader = true;
        let storage = &*self.shared.storage;
        self.io
            .sockets
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// What happened with a server: started, failed, a request.
    pub fn server_event(&mut self, now: Instant, event: ServerEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // Servers and sockets are the reader's connection.
        self.shared.reader = true;
        let storage = &*self.shared.storage;
        self.io
            .servers
            .event(event, storage, &mut self.output.commands);
        self.finish()
    }

    /// Delivers the answer to a request ([`Io::Http`]).
    pub fn http_response(&mut self, now: Instant, response: HttpResponse) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        // The answer to the reader's request is the reader's.
        self.shared.reader = self.io.http.of_reader(response.id);
        let storage = &*self.shared.storage;
        self.io
            .http
            .answer(response, storage, &mut self.output.commands);
        self.finish()
    }

    /// What the shell is to do: requests, servers, sockets, speech, the
    /// clipboard, the radio, the firmware (see [`Io`]). Check after every
    /// kernel call. Network work waits here until there is a network,
    /// where PaperPaw manages Wi-Fi.
    pub fn take_io(&mut self) -> Vec<Io> {
        let mut io: Vec<Io> = self.io.wifi_calls.drain(..).map(Io::Wifi).collect();
        io.extend(self.io.firmware_calls.drain(..).map(Io::Firmware));
        io.extend(self.io.speech.take().into_iter().map(Io::Speech));
        io.extend(self.io.clipboard.take_copied().map(Io::Copy));
        if self.io.clipboard.take_read() {
            io.push(Io::ReadClipboard);
        }
        if self.network_ready() {
            io.extend(self.io.http.take().into_iter().map(Io::Http));
            io.extend(self.io.servers.take().into_iter().map(Io::Server));
            io.extend(self.io.sockets.take().into_iter().map(Io::Socket));
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
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.shared.storage.read_at(path, at, buffer)
    }

    /// Answers a [`Command::ReadClipboard`] with the text known here.
    pub(crate) fn reply_clipboard(&mut self, reply: Id, params: Params) {
        let typed = paperpaw_sdk::Typed {
            text: self.shared.clipboard.clone(),
            typing: false,
            context: params,
        };
        self.output
            .commands
            .push(Command::Emit(reply, typed.to_params()));
    }

    /// Whether plugins serve, e.g. to keep polling for requests.
    pub fn serving(&self) -> bool {
        self.io.servers.busy()
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
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.io.speech.done(id, spoken, &mut self.output.commands);
        self.finish()
    }

    /// Whether a text is being spoken, e.g. to keep watching for its end.
    pub fn speaking(&self) -> bool {
        self.io.speech.speaking()
    }

    /// The system clipboard's text, for the plugins that asked for it
    /// ([`Io::ReadClipboard`]); `None` if it could not be read (the text
    /// copied here is used).
    pub fn clipboard_text(&mut self, now: Instant, text: Option<String>) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some(text) = text {
            self.shared.clipboard = clipped(&text);
        }
        for (reply, params) in self.io.clipboard.answered() {
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
