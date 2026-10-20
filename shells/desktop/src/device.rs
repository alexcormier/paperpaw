//! The emulated device: kernel + panel + clock, independent of any window.

use std::time::{Duration, Instant as StdInstant};

use paperpaw_core::{Effects, Io, Kernel, KernelBuilder, KernelError, SpeechCall, Warning};
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{
    Battery, ButtonState, DeviceInfo, InputEvent, Instant, Rotation, SystemStatus, WallClock,
    buttons,
};

use crate::budget;
use crate::panel::Panel;
use crate::profiles::Profile;
use paperpaw_fs::DirStorage;

/// What the emulated device's storage says it has free (`--free-space`),
/// where set: screenshots that look the same on every computer.
static FREE_SPACE: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// Makes the storage say it has `bytes` free, whatever the computer's
/// disk has.
pub fn set_free_space(bytes: u64) {
    let _ = FREE_SPACE.set(bytes);
}

/// The emulated device's storage in the folder `root`.
pub fn library(root: &std::path::Path) -> std::io::Result<DirStorage> {
    let storage = DirStorage::new(root)?;
    Ok(match FREE_SPACE.get() {
        Some(&bytes) => storage.with_free_space(bytes),
        None => storage,
    })
}

/// How long closing the window waits for answers to what plugins sent as
/// they saved (e.g. reading progress).
const SHUT_DOWN_WAIT: Duration = Duration::from_secs(5);

/// Adds plugins to the standard app, after its own: e.g. a plugin being
/// written, for `paperpaw-dev` (see [`Device::with_plugins`]). Called
/// again at each restart, as a device builds its kernel at each start.
pub type AddPlugins = fn(KernelBuilder) -> KernelBuilder;

/// A running device. All kernel calls go through here, so they are
/// counted against the profile's heap budget.
#[derive(Debug)]
pub struct Device {
    pub profile: &'static Profile,
    pub kernel: Kernel,
    /// What a restart needs to build the kernel again.
    user_config: Option<String>,
    root: std::path::PathBuf,
    /// Plugins added to the standard app.
    add: Option<AddPlugins>,
    /// The panel controller's frame memory: the kernel writes changed rows
    /// into it, possibly band by band. Not part of the heap budget, as on
    /// the device.
    controller: Framebuffer,
    pub panel: Panel,
    /// When the kernel wants to be ticked.
    pub wake_at: Option<Instant>,
    clock: Clock,
    /// HTTP requests in flight.
    pub http: crate::http::Http,
    /// Plugins' servers.
    pub servers: paperpaw_serve::Servers,
    pub sockets: paperpaw_serve::Sockets,
    /// The pretend radio, where the profile's device manages Wi-Fi.
    wifi: crate::wifi::Wifi,
    /// Sleeping (see `Effects::sleep`): only the power button wakes it.
    asleep: bool,
    /// How the screen is turned (see `Kernel::rotation`).
    rotation: Rotation,
    /// The texts being spoken (emulated), in order: number and when each
    /// ends.
    speech: std::collections::VecDeque<(u32, Instant)>,
    /// The kernel's warnings since start (the last [`KEPT_WARNINGS`]),
    /// besides printed: for tools that check there are none.
    pub warnings: Vec<Warning>,
}

/// How many of the kernel's warnings [`Device::warnings`] keeps.
const KEPT_WARNINGS: usize = 1000;

/// Either real time (interactive) or simulated time (headless, tests).
#[derive(Debug)]
enum Clock {
    Real(StdInstant),
    Simulated(u64),
}

impl Device {
    pub fn new(
        profile: &'static Profile,
        user_config: Option<String>,
        storage: DirStorage,
        simulated_time: bool,
    ) -> Result<Self, KernelError> {
        Self::with_plugins(profile, user_config, storage, simulated_time, None)
    }

    /// A device whose app has the plugins `add` adds besides the standard
    /// app's.
    pub fn with_plugins(
        profile: &'static Profile,
        user_config: Option<String>,
        storage: DirStorage,
        simulated_time: bool,
        add: Option<AddPlugins>,
    ) -> Result<Self, KernelError> {
        budget::set_limit(profile.heap_budget);
        let root = storage.root().to_path_buf();
        let config = user_config.as_deref();
        let mut kernel = build_kernel(profile, config, storage, Rotation::Deg0, add)?;
        // Turned by the settings: built again for the turned size.
        let rotation = kernel.rotation();
        if rotation != Rotation::Deg0
            && let Ok(storage) = library(&root)
        {
            drop(kernel);
            kernel = build_kernel(profile, user_config.as_deref(), storage, rotation, add)?;
        }
        let display = turned(profile.device, rotation).display;
        let controller = Framebuffer::new(display.width, display.height, display.format)
            .map_err(KernelError::Graphics)?;
        let mut device = Self {
            profile,
            kernel,
            user_config,
            root,
            add,
            controller,
            panel: Panel::new(display.width, display.height, display.color),
            wake_at: None,
            http: crate::http::Http::new(),
            servers: servers(),
            sockets: paperpaw_serve::Sockets::new(|| {}),
            wifi: crate::wifi::Wifi::default(),
            speech: std::collections::VecDeque::new(),
            warnings: Vec::new(),
            asleep: false,
            rotation,
            clock: if simulated_time {
                Clock::Simulated(0)
            } else {
                Clock::Real(StdInstant::now())
            },
        };
        device.report_warnings();
        Ok(device)
    }

    /// The folder emulating the device's storage.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn now(&self) -> Instant {
        match self.clock {
            Clock::Real(start) => Instant(start.elapsed().as_millis() as u64),
            Clock::Simulated(ms) => Instant(ms),
        }
    }

    /// Advances simulated time; no-op for real time.
    pub fn advance(&mut self, millis: u64) {
        if let Clock::Simulated(ms) = &mut self.clock {
            *ms += millis;
        }
    }

    pub fn start(&mut self) {
        let now = self.now();
        let status = self.status(now);
        let effects = budget::track(|| self.kernel.set_status(now, status));
        self.execute(effects);
        let effects = budget::track(|| self.kernel.start(now));
        self.execute(effects);
    }

    /// The emulated clock and battery: the computer's time and time zone
    /// (with simulated time a fixed one, so screenshots repeat) and a
    /// battery at 80 %.
    fn status(&self, now: Instant) -> SystemStatus {
        let clock = match self.clock {
            Clock::Real(_) => {
                let local = chrono::Local::now();
                WallClock {
                    at: now,
                    unix_millis: local.timestamp_millis(),
                    utc_offset_minutes: Some(local.offset().local_minus_utc() / 60),
                }
            }
            // 14 October 2026, 10:41 in Berlin.
            Clock::Simulated(_) => WallClock {
                at: now,
                unix_millis: 1_791_967_260_000,
                utc_offset_minutes: Some(120),
            },
        };
        SystemStatus {
            clock: Some(clock),
            battery: Some(Battery {
                percent: 80,
                charging: false,
                millivolts: None,
            }),
        }
    }

    /// Runs an action as a button bound to it would (scripts' `run:`).
    pub fn run_action(&mut self, action: paperpaw_core::sdk::Id) {
        let now = self.now();
        let effects = budget::track(|| self.kernel.run_action(now, action));
        self.execute(effects);
    }

    /// Opens a screen as a plugin would (scripts' `open:`).
    pub fn open_screen(
        &mut self,
        screen: paperpaw_core::sdk::Id,
        params: paperpaw_core::sdk::Params,
    ) {
        let now = self.now();
        let effects = budget::track(|| self.kernel.open_screen(now, screen, params));
        self.execute(effects);
    }

    /// Makes a call of the kernel's at the emulated time, counted against
    /// the heap budget, and does what it asks (draws, ticks, requests):
    /// for tools, e.g. `|kernel, now| kernel.set_setting(now, id, value)`.
    pub fn call(&mut self, call: impl FnOnce(&mut Kernel, Instant) -> Effects) {
        let now = self.now();
        let kernel = &mut self.kernel;
        let effects = budget::track(|| call(kernel, now));
        self.execute(effects);
    }

    /// Draws the whole screen again, nothing on it changed (scripts'
    /// `redraw`).
    pub fn draw_again(&mut self) {
        let now = self.now();
        let effects = budget::track(|| self.kernel.draw_again(now));
        self.execute(effects);
    }

    pub fn input(&mut self, event: InputEvent) {
        let now = self.now();
        if self.asleep {
            // Asleep, only the power button wakes the device.
            if let InputEvent::Button {
                name: buttons::POWER,
                state: ButtonState::Pressed,
            } = event
            {
                self.asleep = false;
                eprintln!("{}: awake", self.profile.device.id);
                let effects = budget::track(|| self.kernel.resume(now));
                self.execute(effects);
            }
            return;
        }
        let effects = budget::track(|| self.kernel.handle(now, event));
        self.execute(effects);
    }

    /// Delivers HTTP answers that have arrived; with `wait`, waits that
    /// long for the requests still in flight.
    pub fn poll_http(&mut self, wait: Option<std::time::Duration>) {
        while self.http.in_flight > 0 {
            let Some(response) = self.http.next(wait) else {
                break;
            };
            let now = self.now();
            let effects = budget::track(|| self.kernel.http_response(now, response));
            self.execute(effects);
        }
    }

    /// Passes requests to plugins' servers on, and what their sockets
    /// brought.
    pub fn poll_servers(&mut self) {
        while let Some(event) = self.servers.next() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.server_event(now, event));
            self.execute(effects);
        }
        while let Some(event) = self.sockets.take_event() {
            let now = self.now();
            let effects = budget::track(|| self.kernel.socket_event(now, event));
            self.execute(effects);
        }
        let kernel = &self.kernel;
        self.servers
            .pump(|path, at, buffer| budget::track(|| kernel.read_served(path, at, buffer).ok()));
    }

    /// When something is due next: a wake-up the kernel asked for, the end
    /// of speech, the pretend radio. Nothing while asleep: only the power
    /// button wakes the device then.
    pub fn next_wake(&self) -> Option<Instant> {
        if self.asleep {
            return None;
        }
        let speech = self.speech.front().map(|&(_, end)| end);
        [self.wake_at, speech, self.wifi.next_due()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Ticks the kernel if a requested wake-up is due.
    pub fn tick_if_due(&mut self) {
        if self.asleep {
            return;
        }
        self.poll_http(None);
        self.poll_servers();
        let now = self.now();
        while let Some(event) = self.wifi.take_due(now) {
            let effects = budget::track(|| self.kernel.wifi_event(now, event));
            self.execute(effects);
        }
        while let Some(&(id, end)) = self.speech.front()
            && end <= now
        {
            self.speech.pop_front();
            let effects = budget::track(|| self.kernel.speech_done(now, id, true));
            self.execute(effects);
        }
        if self.wake_at.is_some_and(|at| at <= now) {
            let effects = budget::track(|| self.kernel.tick(now));
            self.execute(effects);
        }
    }

    /// Before the window closes: plugins save and send what is pending, as
    /// when the device goes to sleep, and answers to what they sent are
    /// waited for a little.
    pub fn shut_down(&mut self) {
        if !self.asleep {
            let now = self.now();
            let effects = budget::track(|| self.kernel.suspend(now));
            self.execute(effects);
        }
        let end = StdInstant::now() + SHUT_DOWN_WAIT;
        while self.http.in_flight > 0 {
            let left = end.saturating_duration_since(StdInstant::now());
            let Some(response) = self.http.next(Some(left)).filter(|_| !left.is_zero()) else {
                break;
            };
            let now = self.now();
            let effects = budget::track(|| self.kernel.http_response(now, response));
            self.execute(effects);
        }
    }

    /// Whether `name` is a button of this device.
    pub fn has_button(&self, name: &str) -> bool {
        self.profile.buttons.contains(&name)
    }

    /// Whether the device sleeps: only the power button wakes it.
    pub fn asleep(&self) -> bool {
        self.asleep
    }

    fn execute(&mut self, effects: Effects) {
        if effects.restart() {
            return self.restart();
        }
        if let Some((region, mode)) = effects.refresh() {
            let (kernel, controller) = (&mut self.kernel, &mut self.controller);
            let mut frame = crate::trace::Frame::start();
            budget::track(|| {
                kernel.render(&mut |top, rows| {
                    controller.write_rows(top, rows);
                    if let Some(frame) = &mut frame {
                        frame.band();
                    }
                });
            });
            if let Some(frame) = frame {
                let screen = self.kernel.active_screen().map(ToString::to_string);
                frame.done(screen.as_deref().unwrap_or_default());
            }
            let colors = self.profile.colors.then(|| self.kernel.colors());
            self.panel
                .refresh(self.controller.view(), region, mode, colors);
            // Slow work ahead: its mark shows first, as on the X4 (whose
            // panel refreshes it while the kernel works on), then what the
            // work brought.
            if effects.proceed() {
                let now = self.now();
                let effects = budget::track(|| self.kernel.proceed(now));
                return self.execute(effects);
            }
        }
        self.wake_at = effects.wake_at();
        if effects.sleep() && !self.asleep {
            // The sleep screen is shown: plugins save, the device sleeps.
            self.asleep = true;
            eprintln!("{}: asleep", self.profile.device.id);
            let now = self.now();
            let effects = budget::track(|| self.kernel.suspend(now));
            self.execute(effects);
        }
        for io in self.kernel.take_io() {
            match io {
                Io::Http(mut call) => {
                    if let Err(e) = call.load_body(self.kernel.storage()) {
                        let now = self.now();
                        let failed = paperpaw_core::HttpResponse::failed(call.id, &e.to_string());
                        let effects = budget::track(|| self.kernel.http_response(now, failed));
                        self.execute(effects);
                        continue;
                    }
                    self.http.start(call);
                }
                Io::Server(call) => self.servers.apply(call),
                Io::Socket(call) => self.sockets.apply(call),
                Io::Speech(call) => self.speak(call),
                Io::Wifi(call) => {
                    let now = self.now();
                    self.wifi.apply(call, now);
                }
                // The emulator's clipboard is the kernel's own: nothing to
                // copy to, and it is not asked to read one.
                Io::Copy(_) => {}
                Io::ReadClipboard => {
                    let now = self.now();
                    let effects = budget::track(|| self.kernel.clipboard_text(now, None));
                    self.execute(effects);
                }
                // The emulator is no device's firmware: it is not asked.
                Io::Firmware(_) => {}
            }
        }
        self.report_warnings();
        // Turned: the same screens, made anew for the turned size.
        if self.kernel.rotation() != self.rotation {
            self.rotation = self.kernel.rotation();
            let display = turned(self.profile.device, self.rotation).display;
            if let Ok(controller) = Framebuffer::new(display.width, display.height, display.format)
            {
                self.controller = controller;
                self.panel = Panel::new(display.width, display.height, display.color);
                let now = self.now();
                let effects =
                    budget::track(|| self.kernel.resize(now, display.width, display.height));
                self.execute(effects);
            }
        }
    }

    /// Speech, emulated: the text is logged and takes as long as it would
    /// to speak, at about 160 words a minute.
    fn speak(&mut self, call: SpeechCall) {
        match call {
            SpeechCall::Say {
                id,
                text,
                language,
                rate,
                queue,
            } => {
                if !queue {
                    self.speech.clear();
                }
                let words = text.split_whitespace().count() as u64;
                let millis = words * 375 * 100 / u64::from(rate.max(1));
                let start: String = text.chars().take(60).collect();
                let queued = if queue { ", queued" } else { "" };
                eprintln!("speech ({language}, {rate} %, {millis} ms{queued}): {start}");
                let after = self.speech.back().map_or(self.now(), |&(_, end)| end);
                self.speech
                    .push_back((id, after.max(self.now()).add_millis(millis)));
            }
            SpeechCall::Stop => {
                eprintln!("speech stopped");
                self.speech.clear();
            }
        }
    }

    /// Builds a new kernel, as the device would after a reboot.
    fn restart(&mut self) {
        let storage = match library(&self.root) {
            Ok(storage) => storage,
            Err(e) => return eprintln!("restart: {}: {e}", self.root.display()),
        };
        // As after a reboot, what the old kernel asked for is gone: its
        // requests' answers would otherwise reach the new kernel's (they
        // count from 1 again).
        self.http = crate::http::Http::new();
        self.servers = servers();
        self.sockets = paperpaw_serve::Sockets::new(|| {});
        self.wifi = crate::wifi::Wifi::default();
        self.speech.clear();
        // The old kernel goes first, as on the device: a minimal one (one
        // frame row, no plugins) holds its place meanwhile.
        let profile = self.profile;
        let kernel = &mut self.kernel;
        let swapped = budget::track(|| {
            let placeholder = Kernel::builder(profile.device).frame_rows(1).build()?;
            drop(std::mem::replace(kernel, placeholder));
            Ok::<(), KernelError>(())
        });
        if let Err(e) = swapped {
            return eprintln!("restart: {e}");
        }
        match build_kernel(
            self.profile,
            self.user_config.as_deref(),
            storage,
            self.rotation,
            self.add,
        ) {
            Ok(new) => {
                let kernel = &mut self.kernel;
                budget::track(|| drop(std::mem::replace(kernel, new)));
                let display = turned(self.profile.device, self.rotation).display;
                if (display.width, display.height) != (self.panel.width(), self.panel.height()) {
                    if let Ok(controller) =
                        Framebuffer::new(display.width, display.height, display.format)
                    {
                        self.controller = controller;
                    }
                    self.panel = Panel::new(display.width, display.height, display.color);
                }
                self.report_warnings();
                self.start();
            }
            Err(e) => eprintln!("restart: {e}"),
        }
    }

    fn report_warnings(&mut self) {
        let kernel = &mut self.kernel;
        let warnings = budget::track(|| kernel.take_warnings());
        for warning in warnings {
            eprintln!("warning: {warning}");
            if self.warnings.len() == KEPT_WARNINGS {
                self.warnings.remove(0);
            }
            // Copied outside the kernel's budget, which the kernel's own
            // copy leaves as it is dropped.
            self.warnings.push(warning.clone());
        }
    }
}

/// The standard app for `profile` (with the plugins `add` adds), counted
/// against its heap budget.
fn build_kernel(
    profile: &'static Profile,
    user_config: Option<&str>,
    storage: DirStorage,
    rotation: Rotation,
    add: Option<AddPlugins>,
) -> Result<Kernel, KernelError> {
    budget::track(|| {
        let fonts = if profile.font_packs {
            paperpaw_app::Fonts::Packs
        } else {
            paperpaw_app::Fonts::Outline
        };
        let mut device = turned(profile.device, rotation);
        device.input.keys = profile.buttons;
        let mut builder = Kernel::builder(device)
            .storage(crate::trace::Traced(storage))
            .clock(micros)
            .random(random)
            .glyph_cache(profile.glyph_cache)
            .busy_mark(profile.busy_mark)
            .colors(profile.colors)
            .network(true)
            .wifi(profile.wifi)
            .speech(profile.speech)
            .sleep(profile.sleep)
            .button_row(profile.button_row)
            .rotation(true);
        // Scalable fonts only where the emulated device renders them.
        if !profile.font_packs {
            builder = builder.font_dir("fonts");
        }
        let mut builder = paperpaw_app::install(builder, fonts);
        #[cfg(feature = "hello")]
        {
            builder = builder.plugin(paperpaw_sample_hello::Hello);
        }
        if let Some(light) = profile.light {
            builder = builder.plugin(crate::light::Emulated(light));
        }
        if let Some(add) = add {
            builder = add(builder);
        }
        if let Some(rows) = profile.frame_rows {
            builder = builder.frame_rows(rows);
        }
        builder = builder.row_align(profile.row_align);
        if let Some(name) = profile.catalog_device {
            builder = builder.catalog_device(name);
        }
        for (id, value) in profile.settings {
            builder = builder.setting_default_text(id, value);
        }
        if let Some(config) = user_config {
            builder = builder.user_config(config);
        }
        builder.build()
    })
}

/// `device` with its screen turned by `rotation`: on its side, width and
/// height trade places.
fn turned(mut device: DeviceInfo, rotation: Rotation) -> DeviceInfo {
    if matches!(rotation, Rotation::Deg90 | Rotation::Deg270) {
        let display = &mut device.display;
        (display.width, display.height) = (display.height, display.width);
    }
    device
}

/// Random bytes from the operating system.
fn random(buf: &mut [u8]) -> bool {
    getrandom::fill(buf).is_ok()
}

/// Microseconds since start, for measuring what plugins take.
fn micros() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_micros() as u64
}

/// Plugins' servers, on this computer only: the emulator is for trying
/// and testing, and so asks no firewall for the network; a browser on the
/// same computer reaches them at 127.0.0.1.
fn servers() -> paperpaw_serve::Servers {
    paperpaw_serve::Servers::new(|| {}).listening_on(std::net::Ipv4Addr::LOCALHOST.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_core::WifiCommand;

    /// The heap budget counts every thread's kernel: one device at a time,
    /// and none while the budget's own tests read its counters.
    use crate::budget::accounting_tests::LOCK as ONE_AT_A_TIME;

    fn device(name: &str) -> (Device, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("paperpaw-device-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let storage = DirStorage::new(&root).unwrap();
        let profile = crate::profiles::find("xteink-x4").unwrap();
        let mut device = Device::new(profile, None, storage, true).unwrap();
        device.start();
        (device, root)
    }

    #[test]
    fn asleep_nothing_is_due() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut device, root) = device("asleep");
        device.wake_at = Some(Instant(0));
        device.asleep = true;
        // A wake-up the kernel asked for before would be due at once, for
        // ever: the window's loop would spin.
        assert_eq!(device.next_wake(), None);
        device.asleep = false;
        assert_eq!(device.next_wake(), Some(Instant(0)));
        drop(device);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_restart_forgets_what_the_old_kernel_asked_for() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut device, root) = device("restart");
        let now = device.now();
        device.speech.push_back((1, now.add_millis(10_000)));
        device.wifi.apply(WifiCommand::On, now);
        device.restart();
        assert!(device.speech.is_empty());
        // The new kernel's radio starts off: nothing of the old is due.
        assert_eq!(device.wifi.next_due(), None);
        drop(device);
        let _ = std::fs::remove_dir_all(root);
    }
}
