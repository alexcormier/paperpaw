use alloc::string::String;
use alloc::vec::Vec;

use crate::id::Id;
use crate::params::Params;
use crate::settings::SettingValue;
use paperpaw_platform::RefreshMode;

/// An HTTP request of a plugin. When it is answered the kernel emits
/// `reply` with `params` plus `status` (the HTTP status; `0` when the
/// request failed, e.g. without network) and `body` (the response as text).
///
/// Made with [`get`](Self::get) or [`new`](Self::new) and the methods
/// that set the rest; every request is answered once, also when it is
/// refused or taken back ([`HttpReply::refused`](crate::HttpReply)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRequest {
    /// `GET`, `PUT`, `POST`, …
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// A file in storage to send as the body instead, e.g. a database to
    /// upload.
    pub body_from: Option<String>,
    /// A file in storage to write the response's body to (also large
    /// ones, e.g. a download); the reply then has no `body`.
    pub save_to: Option<String>,
    pub reply: Id,
    pub params: Params,
    /// What few requests set, kept apart so that commands stay small
    /// ([`options`](Self::options)).
    options: Option<alloc::boxed::Box<HttpOptions>>,
}

/// What few requests set ([`HttpRequest::options`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpOptions {
    /// How long it may take in all, the connection and TLS counted;
    /// `None`: the default, 30 s, or 10 minutes for a download to a file
    /// ([`HttpRequest::save_to`]).
    pub timeout: Option<core::time::Duration>,
    /// Headers of the answer to pass back in
    /// [`HttpReply::headers`](crate::HttpReply), by name (any case).
    /// `ETag` and `Content-Type` always come back.
    pub want_headers: Vec<String>,
    /// The longest body kept in memory, in bytes: a longer one is cut
    /// there. `None`: by the device's memory (64 KiB on small devices,
    /// 1 MiB on others); at most 1 MiB. Large answers go to a file
    /// ([`HttpRequest::save_to`]).
    pub max_body: Option<usize>,
}

/// The options of a request that sets none.
static NO_OPTIONS: HttpOptions = HttpOptions {
    timeout: None,
    want_headers: Vec::new(),
    max_body: None,
};

impl HttpRequest {
    /// A request without headers or body; its answer comes as `reply`, an
    /// [`Event<HttpReply>`](crate::replies::HttpReply) (or an id).
    pub fn new(method: &str, url: &str, reply: impl Into<Id>) -> Self {
        Self {
            method: String::from(method),
            url: String::from(url),
            headers: Vec::new(),
            body: None,
            body_from: None,
            save_to: None,
            reply: reply.into(),
            params: Params::new(),
            options: None,
        }
    }

    /// Its timeout, the headers it wants back and the longest body.
    pub fn options(&self) -> &HttpOptions {
        self.options.as_deref().unwrap_or(&NO_OPTIONS)
    }

    fn options_mut(&mut self) -> &mut HttpOptions {
        self.options.get_or_insert_with(Default::default)
    }

    /// A `GET` of `url`.
    pub fn get(url: &str, reply: impl Into<Id>) -> Self {
        Self::new("GET", url, reply)
    }

    /// With this header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// With this body, as text.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// With the file at this storage path as its body.
    pub fn body_from(mut self, path: impl Into<String>) -> Self {
        self.body_from = Some(path.into());
        self
    }

    /// Its answer's body written to the file at this storage path.
    pub fn save_to(mut self, path: impl Into<String>) -> Self {
        self.save_to = Some(path.into());
        self
    }

    /// With these parameters, passed back with the reply.
    pub fn with(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// Taking at most this long in all.
    pub fn timeout(mut self, timeout: core::time::Duration) -> Self {
        self.options_mut().timeout = Some(timeout);
        self
    }

    /// Passing back these headers of the answer.
    pub fn want_headers(mut self, names: &[&str]) -> Self {
        self.options_mut().want_headers = names.iter().map(|n| String::from(*n)).collect();
        self
    }

    /// Keeping at most this many bytes of the answer's body in memory.
    pub fn max_body(mut self, bytes: usize) -> Self {
        self.options_mut().max_body = Some(bytes);
        self
    }
}

/// The answer to a request a plugin's server received (see
/// [`Command::Serve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerResponse {
    /// The request's number ([`server_params::REQUEST`]).
    pub request: u32,
    pub status: u16,
    pub content_type: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// A file in storage to send as the body instead.
    pub body_from: Option<String>,
}

impl ServerResponse {
    /// An answer with `body` of `content_type`.
    pub fn new(request: u32, status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            request,
            status,
            content_type: String::from(content_type),
            headers: Vec::new(),
            body,
            body_from: None,
        }
    }
}

/// Parameters of a server's events (see [`Command::Serve`]).
pub mod server_params {
    /// What happened: [`STARTED`], [`REQUEST`], [`FAILED`] or [`STOPPED`].
    pub const KIND: &str = "kind";
    pub const STARTED: &str = "started";
    pub const REQUEST: &str = "request";
    pub const FAILED: &str = "failed";
    pub const STOPPED: &str = "stopped";
    /// Where the server is reached, e.g. `192.168.1.20:8080`.
    pub const ADDRESS: &str = "address";
    /// Why it failed.
    pub const REASON: &str = "reason";
    /// A request's method (`GET`, `PUT`, …) and target (path and query).
    pub const METHOD: &str = "method";
    pub const TARGET: &str = "target";
    /// Where a request's body was saved, if it had one; it is removed
    /// once the request is answered, unless moved.
    pub const BODY_FILE: &str = "body-file";
}

/// What a socket is to read next (see [`Command::SocketRead`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketRead {
    /// What has come, up to `max` bytes, as text (UTF-8; invalid bytes
    /// replaced): the event [`socket_params::RECEIVED`] with it.
    Text { max: u32 },
    /// Exactly `len` bytes, written to `path` in storage as they come (a
    /// file there is replaced): the event [`socket_params::SAVED`] once
    /// they all came, so files of any size never need to fit in memory.
    File { path: String, len: u64 },
}

/// Parameters of a socket's events (see [`Command::Connect`]) and of a
/// broadcast's (see [`Command::Broadcast`]).
pub mod socket_params {
    /// What happened: [`CONNECTED`], [`RECEIVED`], [`SAVED`], [`CLOSED`]
    /// or [`FAILED`]; for broadcasts [`ANSWER`] and [`DONE`].
    pub const KIND: &str = "kind";
    pub const CONNECTED: &str = "connected";
    /// Text came, in [`TEXT`]; empty when the other side closed.
    pub const RECEIVED: &str = "received";
    /// A file read is complete; its [`PATH`].
    pub const SAVED: &str = "saved";
    pub const CLOSED: &str = "closed";
    pub const FAILED: &str = "failed";
    /// An answer to a broadcast: its [`TEXT`] and where it came [`FROM`].
    pub const ANSWER: &str = "answer";
    /// No more answers to a broadcast will come.
    pub const DONE: &str = "done";
    pub const TEXT: &str = "text";
    pub const PATH: &str = "path";
    /// The address an answer came from, e.g. `192.168.1.20`.
    pub const FROM: &str = "from";
    /// Why it failed or closed.
    pub const REASON: &str = "reason";
}

/// Parameters of an HTTP reply event (see [`HttpRequest`]).
pub mod http_params {
    pub const STATUS: &str = "status";
    pub const BODY: &str = "body";
    /// The response's `ETag` header, if it had one.
    pub const ETAG: &str = "etag";
    /// The response's `Content-Type` header, if it had one, e.g.
    /// `text/html; charset=utf-8`.
    pub const CONTENT_TYPE: &str = "content-type";
    /// Where the answer came from, if redirected elsewhere (named apart
    /// from a request's own parameters, which come back too).
    pub const FINAL_URL: &str = "final-url";
    /// Before a header's name in lower case, for the headers asked for
    /// ([`HttpOptions::want_headers`](crate::HttpOptions::want_headers)), e.g. `header:last-modified`.
    pub const HEADER: &str = "header:";
}

/// Text to speak aloud (see [`Command::Speak`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Speech {
    pub text: String,
    /// The language of the text, e.g. `de` or `en-GB`, for the voice;
    /// empty for the system's.
    pub language: String,
    /// How fast, in percent of the voice's normal speed.
    pub rate: u16,
    /// Emitted with `params` once the text has been spoken, with
    /// [`speech_params::FAILED`] if it could not be.
    pub reply: Id,
    pub params: Params,
    /// Spoken after the texts given before instead of replacing them: the
    /// next sentence handed over while one is spoken follows without a
    /// pause.
    pub queue: bool,
}

impl Speech {
    /// `text` in the system's language at normal speed.
    pub fn new(text: impl Into<String>, reply: Id) -> Self {
        Self {
            text: text.into(),
            language: String::new(),
            rate: 100,
            reply,
            params: Params::new(),
            queue: false,
        }
    }

    /// In the language with this tag (empty: the system's).
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    /// At `rate` percent of the normal speed.
    pub fn rate(mut self, rate: u16) -> Self {
        self.rate = rate;
        self
    }

    /// With `params` passed back in the reply.
    pub fn params(mut self, params: Params) -> Self {
        self.params = params;
        self
    }

    /// After what is being spoken, instead of in its place.
    pub fn queue(mut self, queue: bool) -> Self {
        self.queue = queue;
        self
    }

    /// Its reply: it was `spoken`, or could not be (a plugin's voice
    /// answers so, see [`voices`](crate::voices)).
    pub fn answer(&self, spoken: bool) -> Command {
        let params = match spoken {
            true => self.params.clone(),
            false => self.params.clone().with(speech_params::FAILED, "1"),
        };
        Command::Emit(self.reply.clone(), params)
    }
}

/// Parameters of the reply to [`Command::Speak`].
pub mod speech_params {
    /// Present (`1`) when the text could not be spoken, e.g. without a
    /// voice for it.
    pub const FAILED: &str = "failed";
}

/// A request from a plugin to the kernel.
///
/// Plugins never mutate kernel state directly; commands are queued and
/// applied by the kernel after the plugin returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Shows the screen bound to a slot, passing `Params` to it. If that
    /// slot is already open, the screens above it are closed.
    OpenSlot(Id, Params),
    /// Opens a specific screen on top of the current one.
    OpenScreen(Id, Params),
    /// Closes the current screen (no-op on the root screen).
    Back,
    /// Closes the open screen `.0` wherever it is in the stack, leaving
    /// the screens above it (see [`Cx::close`](crate::Cx::close)); none
    /// where it is the only one.
    Close(crate::ScreenKey),
    /// Opens screen `.0` with `.1` in place of the one on top, in one
    /// step: the screen below it is not shown between (the next step of
    /// a guide).
    Replace(Id, Params),
    /// Closes the screens above the topmost open screen `.0`, or the
    /// screen of slot `.0`: back to it in one step. Nothing where none is
    /// open.
    BackTo(Id),
    /// Opens screen `.1` for slot `.0` with `.2` as [`Command::OpenSlot`]
    /// opens the slot's own: shown again where the slot is open with it
    /// and these params, else in place of what the slot shows (one
    /// launcher of several, chosen once).
    OpenInSlot(Id, Id, Params),
    /// Binds slot `.0` to screen `.1` for the user, or back to what the
    /// plugins bind it to (`None`): at once and kept in `paperpaw.ini`
    /// (`[slots]`), e.g. "use this as the home screen?".
    BindSlot(Id, Option<Id>),
    /// Shows the row of hints for a few seconds on a screen that has it
    /// only on a press ([`Hints::OnPress`](crate::Hints)).
    ShowHints,
    /// Changes a setting; invalid values are rejected with a warning.
    SetSetting(Id, SettingValue),
    /// Forgets the value set for a setting: its default applies again (in
    /// the active scope, where changes go there: the scope's value, the
    /// global one applies again). See `Settings::reset`.
    ResetSetting(Id),
    /// Shows settings with these values for a while without keeping them
    /// (`None`: as if not set), e.g. a layout before it is chosen: every
    /// screen drawn from them shows them, until the next
    /// `PreviewSettings` (empty: the values kept show again) or until the
    /// screen that asked closes; the event
    /// [`PREVIEW_CHANGED`](crate::builtin::events::PREVIEW_CHANGED)
    /// follows each change. What the kernel does when a setting changes
    /// (the language, the turn of the screen) waits for the value kept.
    /// See `Settings::set_preview`.
    PreviewSettings(Vec<(Id, Option<SettingValue>)>),
    /// Redraws the current screen completely; with
    /// [`RefreshMode::Full`] the whole screen, flashing.
    Redraw(RefreshMode),
    /// Notifies all subscribers of an event, then every open screen.
    Emit(Id, Params),
    /// Runs an action as if the user had triggered it (e.g. from a menu).
    RunAction(Id),
    /// Switches a plugin on or off in `paperpaw.ini`; applies after a
    /// restart. The kernel refuses plugins that cannot be switched off
    /// (see [`PluginInfo::can_switch_off`](crate::PluginInfo)).
    SetPluginEnabled(Id, bool),
    /// Loads the data plugin in this folder on the storage (e.g.
    /// `plugins/font-fredoka`, just downloaded) at once, as if PaperPaw
    /// had found it at start: its fonts can be chosen, its translations,
    /// key bindings and presets apply. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows; problems are warnings, as at start.
    LoadPlugin(String),
    /// Removes a data plugin: its folder is deleted and its fonts and
    /// presets go at once (a font chosen from it gives way to the
    /// default); what else it brought (translations, key bindings,
    /// styles, defaults) is gone after the next start. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows. Plugins with code are not removed this way.
    RemovePlugin(Id),
    /// Installs the data plugin `Id` downloaded into this folder (one
    /// PaperPaw does not load, e.g. `plugins/.airy.part`, see
    /// [`install`](crate::install)): it takes the place of every version
    /// of that plugin on the storage (loaded, switched off or refused at
    /// start), becomes the folder named as it beside the one it was
    /// downloaded into, and is loaded at once. A plugin switched off stays
    /// off; what was chosen from the version before (e.g. its font for
    /// reading) stays chosen where the new one offers it too. The event
    /// [`PLUGINS_CHANGED`](crate::builtin::events::PLUGINS_CHANGED)
    /// follows. A folder that holds another plugin, or none that loads,
    /// is deleted with a warning, and the version before stays.
    InstallPlugin(Id, String),
    /// Makes an HTTP request (see [`HttpRequest`]); the answer comes as
    /// the request's reply event. Check [`Capabilities::network`](crate::Capabilities) first.
    Http(HttpRequest),
    /// Uses the settings stored in the file at this storage path for
    /// [scoped](crate::SettingSpec::scoped) settings, e.g. those of the
    /// book being read, while the screen that sent it stays open; `None`
    /// ends the scope.
    SetScope(Option<String>),
    /// Redefines a menu with exactly these entries (actions, settings or
    /// submenus), saved in `[menus]` of `paperpaw.ini`; `None` returns to
    /// the entries plugins contribute.
    SetMenu(Id, Option<Vec<Id>>),
    /// Binds `input` (as in `paperpaw.ini`, e.g. `button.menu` or
    /// `tap.center`) in key map `layer` (a screen or slot id, or
    /// `global`) to `action`, saved in the user's `paperpaw.ini` and in
    /// effect at once; `None` returns it to what plugins and presets bind.
    /// Bind [`actions::NOTHING`](crate::builtin::actions::NOTHING) to make
    /// an input do nothing.
    #[non_exhaustive]
    Bind {
        layer: String,
        input: String,
        action: Option<Id>,
    },
    /// Shows a short message for a few seconds, e.g. "Sync done", by the
    /// screen bound to [`slots::NOTICE`](crate::builtin::slots::NOTICE),
    /// over whatever is open and without taking any input. A newer
    /// message replaces it. Only what the reader is doing shows one:
    /// background work's (declared work, events, actions run by rules)
    /// becomes the plugin's note instead (see [`work`](crate::work)).
    Notify(String),
    /// Shows a message that must reach the reader also from background
    /// work, as [`Notify`](Self::Notify) does: one they set up themself
    /// (a rule's, "the battery is at 80 %: unplug"), or one about the
    /// device itself (safe mode, plugins not installed at start, an
    /// update installed). Never for results of syncs, progress or offers:
    /// those belong on the plugin's own page.
    Announce(String),
    /// Restarts PaperPaw, e.g. to apply switched plugins. The shell builds
    /// a new kernel; the reading position is already saved.
    Restart,
    /// Saves what the screen shows as a gray PNG at this storage path,
    /// once the frame is drawn (so a menu that ran this is closed by
    /// then); a message tells where.
    Screenshot(String),
    /// Keeps or removes a password or key (see
    /// [`secrets`](crate::secrets)), saved at once.
    SetSecret(crate::secrets::SecretChange),
    /// Tells the radio what to do, where PaperPaw manages Wi-Fi
    /// ([`Env::wifi`](crate::Env)); see [`wifi`](crate::wifi). Ignored
    /// elsewhere.
    Wifi(crate::wifi::WifiCommand),
    /// No network is coming for now (none saved, none in reach): the
    /// requests, servers and connections waiting for one fail at once, as
    /// they would after [`NETWORK_WAIT_MS`](crate::wifi::NETWORK_WAIT_MS),
    /// so their screens say so instead of waiting. The plugin that runs
    /// Wi-Fi sends it.
    NoNetwork,
    /// Declares work for the kernel to run at the cheapest moment (see
    /// [`work`](crate::work)): the same id replaces what was declared.
    Work(crate::work::Work),
    /// Takes back the work declared with this id and key (none: every
    /// key of the id), if it has not run; requests made already are taken
    /// back with [`CancelHttp`](Self::CancelHttp).
    CancelWork(Id, Option<String>),
    /// Takes back the plugin's requests whose reply is this event; each
    /// is answered with the refusal `Cancelled`.
    CancelHttp(Id),
    /// The reader chose *Go online* where the mode is Offline (see
    /// [`power`](crate::power)): the network for the screen sending it
    /// until it closes; work held back runs on the same connection.
    GoOnline,
    /// Wears a power mode: its level for plugins and declared work, its
    /// values over the reader's own (see [`power`](crate::power)). Sent
    /// by the plugin that chooses the mode.
    Wear(crate::power::Wear),
    /// Installs a firmware update or restarts, where PaperPaw is the
    /// device's firmware ([`Env::firmware`](crate::Env)); see
    /// [`firmware`](crate::firmware). Ignored elsewhere.
    Firmware(crate::firmware::FirmwareCommand),
    /// Asks the shell to sleep once the frame is shown, where it can
    /// ([`Capabilities::sleep`](crate::Capabilities)); the kernel is told when it wakes.
    Sleep,
    /// Puts text on the clipboard, on devices with one also the system's,
    /// for other apps. Long text is cut at [`MAX_CLIPBOARD`] bytes.
    SetClipboard(String),
    /// Serves HTTP to the local network on `port` (shells that cannot say
    /// so with a failure). Everything about the server comes as the event
    /// `events`: that it started (with its address), each request (answer
    /// with [`Command::Respond`]), that it failed or stopped; see
    /// [`server_params`]. Serve only while the user wants it: a server
    /// also stops when the screen showing as it started is closed.
    #[non_exhaustive]
    Serve { server: Id, port: u16, events: Id },
    /// Stops serving.
    StopServing(Id),
    /// Opens a TCP connection to `host` and `port` (see
    /// [ADR 0014](https://github.com/hthoene/paperpaw/blob/main/docs/adr/0014-sockets-for-plugins.md)).
    /// Everything about it comes as the event `events`, with `id` the
    /// socket ([`params::ID`](crate::builtin::params::ID)) and what
    /// happened ([`socket_params`]): connected, received, saved, closed,
    /// failed. Nothing is read unasked ([`Command::SocketRead`]). A socket
    /// also closes when the screen showing as it was opened is closed.
    #[non_exhaustive]
    Connect {
        socket: Id,
        host: String,
        port: u16,
        events: Id,
    },
    /// Sends bytes on a socket.
    #[non_exhaustive]
    SocketSend { socket: Id, data: Vec<u8> },
    /// Asks a socket for what comes next (one read at a time).
    #[non_exhaustive]
    SocketRead { socket: Id, read: SocketRead },
    /// Closes a socket.
    Disconnect(Id),
    /// Sends `data` as a UDP datagram to the local network on each of
    /// `ports`, e.g. to find a service. Each answer within `wait_ms` comes
    /// as the event `events` ([`socket_params::ANSWER`], with `id`), then
    /// [`socket_params::DONE`].
    #[non_exhaustive]
    Broadcast {
        id: Id,
        ports: Vec<u16>,
        data: Vec<u8>,
        wait_ms: u32,
        events: Id,
    },
    /// Answers a request of a server.
    Respond(ServerResponse),
    /// Asks for the text on the clipboard (see
    /// [`Env::clipboard`](crate::Env)). It comes as the event `.0` with
    /// the parameters `.1` plus `text` ([`params::TEXT`](crate::builtin::params::TEXT)),
    /// empty if there is none: a [`Typed`](crate::replies::Typed). Read it only when the user asks to paste:
    /// some systems tell the user every time an app reads the clipboard.
    ReadClipboard(Id, Params),
    /// Speaks text aloud where the device can
    /// ([`Capabilities::speech`](crate::Capabilities)), instead of what is being spoken or
    /// after it ([`Speech::queue`]). Without speech the reply comes at
    /// once, failed. Speech stops when the screen that started it is
    /// closed.
    Speak(Speech),
    /// Stops speaking; no reply comes for the texts cut off.
    StopSpeaking,
}

impl Command {
    /// [`Command::Bind`]: `input` in key map `layer` to `action` (`None`:
    /// back to what plugins and presets bind).
    pub fn bind(layer: impl Into<String>, input: impl Into<String>, action: Option<Id>) -> Self {
        Command::Bind {
            layer: layer.into(),
            input: input.into(),
            action,
        }
    }

    /// [`Command::Serve`]: server `server` on `port`, heard as `events`.
    pub fn serve(server: Id, port: u16, events: Id) -> Self {
        Command::Serve {
            server,
            port,
            events,
        }
    }

    /// [`Command::Connect`]: socket `socket` to `host` and `port`, heard as
    /// `events`.
    pub fn connect(socket: Id, host: impl Into<String>, port: u16, events: Id) -> Self {
        Command::Connect {
            socket,
            host: host.into(),
            port,
            events,
        }
    }

    /// [`Command::SocketSend`]: `data` on `socket`.
    pub fn socket_send(socket: Id, data: Vec<u8>) -> Self {
        Command::SocketSend { socket, data }
    }

    /// [`Command::SocketRead`]: what comes next on `socket`, as `read` says.
    pub fn socket_read(socket: Id, read: SocketRead) -> Self {
        Command::SocketRead { socket, read }
    }

    /// [`Command::Broadcast`]: `data` to `ports` on the local network,
    /// answers within `wait_ms` heard as `events` with `id`.
    pub fn broadcast(id: Id, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32, events: Id) -> Self {
        Command::Broadcast {
            id,
            ports,
            data,
            wait_ms,
            events,
        }
    }
}

/// The most text the clipboard holds, in bytes.
pub const MAX_CLIPBOARD: usize = 4096;

impl Command {
    /// Changes `setting` to `value` (see [`setting`](crate::setting)).
    pub fn set<T: crate::setting::SettingType>(
        setting: &crate::setting::Setting<T>,
        value: T::Input,
    ) -> Command {
        Command::SetSetting(setting.id().clone(), T::write(value))
    }

    /// Opens a slot without parameters.
    pub fn open_slot(slot: Id) -> Command {
        Command::OpenSlot(slot, Params::new())
    }
}
