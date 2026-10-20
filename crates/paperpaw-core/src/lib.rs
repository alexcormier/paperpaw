//! The PaperPaw kernel.
//!
//! The kernel hosts plugins and connects them: it keeps the registries of
//! contributions, resolves slots to screens, turns raw input into gestures
//! and actions through layered key maps, applies plugin commands and decides
//! how the e-ink panel is refreshed. It contains no product UI: even its own
//! contributions are registered as the `core` plugin.
//!
//! The kernel is *sans-I/O*: shells feed it [`InputEvent`]s and time, and
//! execute the returned [`Effects`]. It never blocks, spawns threads or
//! talks to an operating system, so it runs unchanged on microcontrollers,
//! Linux, Android and in the browser.
//!
//! Drawing is separate from event handling: after a call whose effects
//! contain a refresh, the shell calls [`Kernel::render`], which draws what
//! changed and hands it over as rows of pixels: the whole region at once,
//! or band by band on devices that cannot hold a full frame in RAM
//! ([`KernelBuilder::frame_rows`]). The panel controller keeps the frame.
//!
//! ```ignore
//! let mut kernel = Kernel::builder(device).plugin(MyPlugin).build()?;
//! let mut effects = kernel.start(clock.now());
//! loop {
//!     if let Some((region, mode)) = effects.refresh() {
//!         kernel.render(&mut |top, rows| panel.write(top, rows));
//!         panel.refresh(region, mode);
//!     }
//!     arm_timer(effects.wake_at());
//!     effects = match wait_for_input_or_timer() {
//!         Input(event) => kernel.handle(clock.now(), event),
//!         Timer => kernel.tick(clock.now()),
//!     };
//! }
//! ```

#![no_std]

extern crate alloc;

mod builder;
mod code_plugin;
mod commands;
mod config;
mod core_plugin;
mod data_plugin;
mod drawing;
mod gestures;
mod hints;
mod input;
mod io;
mod keymap;
mod meter;
mod network;
mod plugins;
mod recall;
mod refresh;
mod registry;
mod screens;
#[cfg(test)]
mod tests;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::mem;

use paperpaw_graphics::{Framebuffer, GraphicsError};
use paperpaw_i18n::Translator;
use paperpaw_platform::{
    ButtonState, Color, DeviceInfo, FrameView, InputEvent, Instant, NoStorage, Point, PointerEvent,
    PointerPhase, Rect, RefreshMode, Storage, StorageError, SystemStatus,
};
use paperpaw_sdk::builtin::{
    CONFIG_FILE, GLOBAL_KEYMAP_LAYER, events, keymap_presets, params as builtin_params,
    settings as core_settings, slots,
};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::native::{Native, SafeMode};
use paperpaw_sdk::setting::{Setting, SettingType};
use paperpaw_sdk::{
    Actions, Capabilities, Command, Contribution, Cx, Direction, Env, EnvParts, EventHandler,
    Fonts, Formats, Gesture, GestureKind, Handled, Hints, Hyphenation, Id, Manifest, Menus,
    Metrics, Output, Params, Plugin, PluginInfo, PluginState, Plugins, RegisterError, RequiredFor,
    SDK_VERSION, Screen, Settings, StatusItems,
};

pub use crate::builder::KernelBuilder;
pub use crate::code_plugin::{CodeFile, CodeLoader};
use crate::config::{mono_coverage, plugin_switches, read_config};
use crate::core_plugin::{CORE, CorePlugin};
use crate::input::{Recognized, Recognizer, Thresholds};
pub use crate::io::{
    HttpCall, HttpResponse, Io, MAX_FILE, MAX_RESPONSE, SOCKET_PART, ServedFile, ServerCall,
    ServerEvent, SocketCall, SocketEvent, SpeechCall, WifiEvent,
};
use crate::keymap::{InputKey, Keymap, Origin, Zone};
use crate::refresh::RefreshPolicy;
use crate::registry::{Registry, Staging};
use crate::screens::uncovered;
use paperpaw_sdk::Registrar;
use paperpaw_sdk::filters::InputVerdict;
pub use paperpaw_sdk::firmware::{Firmware, FirmwareCommand, FirmwareState, InstallFailure};
pub use paperpaw_sdk::wifi::{JoinFailure, WifiCommand, WifiNetwork, WifiState};

pub use paperpaw_sdk as sdk;

/// Commands may trigger further commands; this bounds runaway loops.
const MAX_COMMAND_ROUNDS: usize = 16;
/// A frame with pictures over this share of the screen (percent) is left
/// with a full refresh: e-paper keeps ghosts of large pictures where text
/// leaves none. Covers in a list stay below it.
const PICTURES_PERCENT: u32 = 15;
/// How long the row of hints shows on a screen that has it on a press.
const HINTS_MS: u64 = 4000;
/// Work that took this long last time (opening a screen, before it is
/// drawn) shows the busy mark first (see `KernelBuilder::busy_mark`):
/// about as long as the panel takes to show the mark, which it does
/// meanwhile, so the mark costs next to nothing.
const SLOW_MS: u32 = 400;
/// What was timed is remembered for this many things at most.
const TIMED: usize = 24;
/// Largest file of passwords and keys read.
const MAX_SECRETS: usize = 64 * 1024;
/// Default memory for glyphs rasterized at runtime.
const DEFAULT_GLYPH_CACHE: usize = 32 * 1024;

/// Something the shell has to do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Pass `region` on to the panel with [`Kernel::render`] and refresh it.
    Refresh { region: Rect, mode: RefreshMode },
    /// Call [`Kernel::tick`] at (or soon after) this time.
    WakeAt(Instant),
    /// Build a new kernel, e.g. to apply switched plugins.
    Restart,
    /// Sleep once the frame is shown (the sleep screen): call
    /// [`Kernel::suspend`], and [`Kernel::resume`] on waking.
    Sleep,
    /// The frame is best shown in grays (see [`Effects::grays`]).
    Grays,
    /// Call [`Kernel::proceed`] while the panel shows the busy mark (see
    /// [`Effects::proceed`]).
    Proceed,
}

/// The effects of one kernel call. Refreshes are already coalesced: there
/// is at most one per call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    refresh: Option<(Rect, RefreshMode)>,
    grays: bool,
    wake_at: Option<Instant>,
    restart: bool,
    sleep: bool,
    proceed: bool,
}

impl Effects {
    /// Whether the kernel showed the busy mark and holds the work back:
    /// the shell starts showing the mark (`refresh`), then calls
    /// [`Kernel::proceed`] while the panel refreshes (see
    /// [`KernelBuilder::busy_mark`]).
    pub fn proceed(&self) -> bool {
        self.proceed
    }

    pub fn refresh(&self) -> Option<(Rect, RefreshMode)> {
        self.refresh
    }

    /// Whether the frame is best shown in grays (the screen on top, with
    /// nothing over it, says so: [`Screen::grays`]):
    /// shells whose panel shows more grays with a refresh of its own draw
    /// the whole screen with [`Kernel::render_gray`] and refresh it so;
    /// others refresh as [`Effects::refresh`] says.
    pub fn grays(&self) -> bool {
        self.grays
    }

    pub fn wake_at(&self) -> Option<Instant> {
        self.wake_at
    }

    /// Whether the shell should build a new kernel (see [`Effect::Restart`]).
    pub fn restart(&self) -> bool {
        self.restart
    }

    /// Whether the shell should sleep once the frame is shown (see
    /// [`Effect::Sleep`]).
    pub fn sleep(&self) -> bool {
        self.sleep
    }

    pub fn is_empty(&self) -> bool {
        self.refresh.is_none()
            && self.wake_at.is_none()
            && !self.restart
            && !self.sleep
            && !self.proceed
    }

    pub fn iter(&self) -> impl Iterator<Item = Effect> {
        let refresh = self
            .refresh
            .map(|(region, mode)| Effect::Refresh { region, mode });
        refresh
            .into_iter()
            .chain(self.wake_at.map(Effect::WakeAt))
            .chain(self.restart.then_some(Effect::Restart))
            .chain(self.sleep.then_some(Effect::Sleep))
            .chain(self.grays.then_some(Effect::Grays))
            .chain(self.proceed.then_some(Effect::Proceed))
    }
}

/// Where a warning comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningSource {
    Plugin(Id),
    DeviceKeymap,
    UserConfig,
    Runtime,
}

/// A non-fatal problem. PaperPaw keeps running with sensible defaults and
/// reports what it ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub source: WarningSource,
    pub line: Option<u32>,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.source {
            WarningSource::Plugin(id) => write!(f, "plugin `{id}`")?,
            WarningSource::DeviceKeymap => f.write_str("device key map")?,
            WarningSource::UserConfig => f.write_str("user configuration")?,
            WarningSource::Runtime => f.write_str("runtime")?,
        }
        if let Some(line) = self.line {
            write!(f, ", line {line}")?;
        }
        write!(f, ": {}", self.message)
    }
}

/// Errors that prevent the kernel from starting.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KernelError {
    Graphics(GraphicsError),
}

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelError::Graphics(error) => write!(f, "cannot create framebuffer: {error}"),
        }
    }
}

impl core::error::Error for KernelError {}

/// A screen on the navigation stack.
struct ActiveScreen {
    id: Id,
    /// Tells this opening of the screen from every other, also once it was
    /// made anew (see `Kernel::remake_screens`): e.g. the owner of a
    /// settings scope.
    serial: u32,
    /// The slot it was opened for, if any.
    slot: Option<Id>,
    /// Made for the part above the row of hints (when the row shows), as
    /// it asked ([`Hints::Shown`]).
    hinted: bool,
    /// What it was opened with, e.g. the book.
    params: Params,
    screen: Box<dyn Screen>,
    /// When it wants [`Screen::on_timer`] ([`Cx::wake_at`]); below another
    /// screen, once it is on top again.
    timer: Option<Instant>,
}

/// A finger that came down at an edge (see [`Kernel::edge_lifted`]).
#[derive(Debug, Clone, Copy)]
struct EdgeTouch {
    id: u8,
    side: Direction,
    start: Point,
    tool: paperpaw_platform::PointerTool,
}

/// The PaperPaw kernel. See the [crate documentation](crate).
pub struct Kernel {
    shared: Shared,
    framebuffer: Framebuffer,
    registry: Registry,
    subscriptions: Vec<(Id, Id, EventHandler)>,
    /// The screen on top changed: it is told ([`Screen::on_show`]) once
    /// the commands of the call are done, so that one shown for a moment
    /// (e.g. below a menu that closes to open another screen) is not.
    show_pending: bool,
    /// Events plugins asked for at a time ([`Command::Schedule`]).
    scheduled: Vec<(Instant, Id, Params)>,
    /// The short message showing ([`Command::Notify`]) and until when.
    notice: Option<(Box<dyn Screen>, Instant)>,
    stack: Vec<ActiveScreen>,
    output: Output,
    warnings: Vec<Warning>,
    /// A restart was requested (see [`Command::Restart`]).
    restart: bool,
    /// Sleep was requested (see [`Command::Sleep`]).
    sleep: bool,
    /// The row of hints over the buttons below the screen.
    hint: hints::HintRow,
    /// See [`KernelBuilder::busy_mark`].
    busy_mark: bool,
    /// The work held back while the mark is shown ([`Kernel::proceed`]).
    withheld: Vec<Command>,
    /// When the device was last used (input, a page turned), for sleeping
    /// by itself ([`settings::SLEEP_AFTER`](core_settings::SLEEP_AFTER)).
    last_activity: Instant,
    /// Since when the screen on top has been counted as shown.
    shown_since: Instant,
    /// The active settings scope: a book's own settings.
    scope: Option<config::Scope>,
    /// The screen that asked for the values shown without being kept
    /// ([`Command::PreviewSettings`]), if any (its `serial`).
    preview_owner: Option<u32>,
    /// The `serial` of the screen opened next.
    next_serial: u32,
    /// `paperpaw.ini` is there but could not be read at start: the user
    /// is told once the first screen shows.
    config_unreadable: bool,
    /// The file of passwords and keys is there but could not be read at
    /// start: it is not written over.
    secrets_unreadable: bool,
    /// A screen was opened or closed since the last call ended; then the
    /// frame drawn for it is not shown yet (see [`Kernel::shown`]).
    screens_changed: bool,
    /// Setting defaults from plugins, applied once all are registered:
    /// `(plugin, setting, value)`.
    plugin_defaults: Vec<(Id, Id, Cow<'static, str>)>,
    now: Instant,
    input: gestures::InputState,
    io: network::IoState,
    frame: drawing::FrameState,
    keys: plugins::KeySources,
    timing: drawing::Timing,
}

impl fmt::Debug for Kernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Kernel")
            .field("device", &self.shared.device.id)
            .field("plugins", &self.shared.plugins.len())
            .field(
                "stack",
                &self.stack.iter().map(|s| &s.id).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Kernel {
    /// How the screen should be turned, by the setting
    /// [`settings::ROTATION`](paperpaw_sdk::builtin::settings::ROTATION):
    /// quarter turns from upright. Shells that turn it (see
    /// [`KernelBuilder::rotation`]) check it after changes and build the
    /// kernel anew for the turned size.
    pub fn rotation(&self) -> paperpaw_platform::Rotation {
        use paperpaw_platform::Rotation;
        use paperpaw_sdk::builtin::settings;
        let value = self.shared.settings.read(&settings::ROTATION);
        match settings::ROTATIONS.iter().position(|r| *r == value) {
            Some(1) => Rotation::Deg90,
            Some(2) => Rotation::Deg180,
            Some(3) => Rotation::Deg270,
            _ => Rotation::Deg0,
        }
    }

    /// The colours black and white are shown in, by the settings
    /// [`settings::TEXT_COLOR`](paperpaw_sdk::builtin::settings::TEXT_COLOR)
    /// and [`settings::PAPER_COLOR`](paperpaw_sdk::builtin::settings::PAPER_COLOR),
    /// as red, green and blue; the grays between lie between them. Shells
    /// that tint the screen (see [`KernelBuilder::colors`]) show each gray
    /// of the frame so; the frame does not change when they do.
    pub fn colors(&self) -> [[u8; 3]; 2] {
        use paperpaw_sdk::builtin::settings;
        let text = |id: &Id| self.shared.settings.text(id.as_str());
        // A preset, or one's own colour code; black and white otherwise.
        let rgb = |id: &Id, own: &str, code: &Id, fallback: [u8; 3]| {
            let value = text(id);
            if value == own {
                settings::parse_rgb(text(code))
            } else {
                settings::rgb(value)
            }
            .unwrap_or(fallback)
        };
        [
            rgb(
                &settings::TEXT_COLOR,
                settings::OWN_TEXT,
                &settings::TEXT_COLOR_OWN,
                [0, 0, 0],
            ),
            rgb(
                &settings::PAPER_COLOR,
                settings::OWN_PAPER,
                &settings::PAPER_COLOR_OWN,
                [255, 255, 255],
            ),
        ]
    }

    /// Opens the home slot and emits [`events::STARTED`]. Call once after
    /// building.
    pub fn start(&mut self, now: Instant) -> Effects {
        // The kernel's work, but for the plugins' it calls.
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(Command::open_slot(slots::HOME));
        self.output.commands.push(events::STARTED.emit(()));
        if mem::take(&mut self.config_unreadable) {
            let text = self.config_notice();
            self.output.commands.push(Command::Notify(text));
        }
        self.finish()
    }

    /// Handles an input event.
    pub fn handle(&mut self, now: Instant, event: InputEvent) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        if self.stale(now, &event) {
            return self.finish();
        }
        self.hint.pressed = None;
        self.now = now;
        self.last_activity = now;
        self.shared.usage.inputs = self.shared.usage.inputs.saturating_add(1);
        let event = self.page_buttons_swapped(event);
        // Plugins' input filters come first.
        let verdict = {
            let env = self.shared.env(now);
            self.shared.input_filters.apply(&env, &event)
        };
        match verdict {
            InputVerdict::Pass => {}
            InputVerdict::Take => return self.finish(),
            InputVerdict::Run(action) => {
                self.dispatch(action);
                self.run_commands();
                return self.finish();
            }
        }
        match event {
            InputEvent::Button { name, state } => {
                // Without a long press to tell apart, at once.
                let long = InputKey::LongButton(Cow::Borrowed(name));
                let at_once = self.resolve(&long).is_none();
                if let Some(recognized) = self.input.recognizer.button(name, state, now, at_once) {
                    self.recognized(recognized);
                }
            }
            InputEvent::Pointer(pointer) => self.pointer(pointer),
            _ => {}
        }
        if let Some(effects) = self.mark_if_slow() {
            return effects;
        }
        // The screens as the input left them, to say where the focus is.
        self.run_commands();
        self.speak_focus();
        self.finish()
    }

    /// `event` with the page buttons the other way round while reading,
    /// where the user asked for it ([`core_settings::SWAP_PAGE_BUTTONS`]):
    /// a button swapped when pressed stays swapped when let go, also if
    /// another screen came up meanwhile.
    fn page_buttons_swapped(&mut self, event: InputEvent) -> InputEvent {
        use paperpaw_platform::buttons::{PAGE_BACK, PAGE_FORWARD};
        let InputEvent::Button { name, state } = event else {
            return event;
        };
        if name != PAGE_FORWARD && name != PAGE_BACK {
            return event;
        }
        let swap = match state {
            ButtonState::Pressed => {
                let reading = self
                    .stack
                    .last()
                    .and_then(|top| top.slot.as_ref())
                    .is_some_and(|slot| *slot == slots::READER);
                let swap = reading && self.shared.settings.read(&core_settings::SWAP_PAGE_BUTTONS);
                self.input.swapped.retain(|b| *b != name);
                if swap {
                    self.input.swapped.push(name);
                }
                swap
            }
            _ => {
                let held = self.input.swapped.iter().position(|b| *b == name);
                held.map(|at| self.input.swapped.remove(at)).is_some()
            }
        };
        let name = match (swap, name) {
            (false, name) => name,
            (true, PAGE_FORWARD) => PAGE_BACK,
            (true, _) => PAGE_FORWARD,
        };
        InputEvent::Button { name, state }
    }

    /// Goes on with the work an input started, once the shell began showing
    /// the busy mark ([`Effects::proceed`]).
    pub fn proceed(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        if let Some((area, _)) = self.frame.mark.take() {
            self.invalidate(area);
        }
        let mut commands = mem::take(&mut self.withheld);
        commands.append(&mut self.output.commands);
        self.output.commands = commands;
        self.run_commands();
        self.speak_focus();
        self.finish()
    }

    /// What the commands of an input start that may be slow: a screen
    /// opened or closed, or an action run; the key its time is kept by.
    fn work_key(&self, command: &Command) -> Option<String> {
        match command {
            Command::OpenSlot(slot, _) => Some(format!("slot:{slot}")),
            Command::OpenScreen(screen, _) => Some(format!("screen:{screen}")),
            Command::Back => {
                let below = self.stack.len().checked_sub(2)?;
                Some(format!("back:{}", self.stack[below].id))
            }
            Command::RunAction(action) => Some(format!("run:{action}")),
            _ => None,
        }
    }

    /// Notes how long the work timed took, once the call is done (with the
    /// mark shown, once it went on).
    fn timed(&mut self) {
        if self.frame.mark.is_some() {
            return;
        }
        let (Some((key, started)), Some(now)) =
            (self.timing.running.take(), self.shared.meter.micros())
        else {
            return;
        };
        let ms = (now.saturating_sub(started) / 1000).min(u64::from(u32::MAX)) as u32;
        self.timing.durations.retain(|(k, _)| *k != key);
        if self.timing.durations.len() >= TIMED {
            self.timing.durations.remove(0);
        }
        self.timing.durations.push((key, ms));
    }

    /// The frame last rendered is on the panel now, at `now` (on the
    /// clock the input's times are on): shells call it after each refresh.
    /// After a screen was opened or closed, presses made before it showed
    /// are dropped: pressing again while nothing seems to happen means
    /// "nothing happened", not "do it once more". On the same screen
    /// (turning pages, moving a focus quickly) presses wait at most a
    /// frame: one made before the frame before the last showed is
    /// dropped, so that pressing on while the device is slow does not
    /// line up a long tail of steps. Shells that never call it keep every
    /// press.
    pub fn shown(&mut self, now: Instant) {
        self.input.shown_at = [self.input.shown_at[1], Some(now)];
        if mem::take(&mut self.input.settling) {
            self.input.stale_before = Some(now);
        }
    }

    /// Says what has the focus now, if it changed and the user asked for
    /// it ([`core_settings::SPEAK_FOCUS`]): in place of what it said of
    /// the focus before, if that is still spoken; not while a plugin
    /// reads aloud.
    fn speak_focus(&mut self) {
        if !self.shared.caps.speech
            || !self
                .shared
                .settings
                .bool(core_settings::SPEAK_FOCUS.as_str())
            || (self.io.speech.speaking() && !self.io.focus_spoken)
        {
            return;
        }
        let parts = self.describe();
        let said = paperpaw_sdk::semantics::focused(&parts).map(|part| {
            let state = match part.on {
                Some(true) => self.shared.translator.tr("core.state.on"),
                Some(false) => self.shared.translator.tr("core.state.off"),
                None => "",
            };
            if state.is_empty() {
                part.label.clone()
            } else {
                alloc::format!("{}, {state}", part.label)
            }
        });
        if said == self.io.speech.focus {
            return;
        }
        if let Some(text) = &said {
            let language = self.shared.translator.language().to_string();
            let speech = paperpaw_sdk::Speech {
                text: text.clone(),
                language,
                rate: 100,
                reply: Id::new_static("core.focus-spoken"),
                params: Params::new(),
                queue: false,
            };
            let (available, screens) = (self.shared.caps.speech, self.stack.len());
            self.io
                .speech
                .speak(speech, available, screens, &mut self.output.commands);
            self.io.focus_spoken = true;
        }
        self.io.speech.focus = said;
    }

    /// Tells the kernel the time of day and the battery's charge. Shells
    /// call it at start, when the clock is set or its time zone changes,
    /// and when the battery changes (checking about once a minute is
    /// plenty). Status lines redraw when what they show changed
    /// ([`events::STATUS_CHANGED`]).
    pub fn set_status(&mut self, now: Instant, status: SystemStatus) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let old = self.shared.status;
        self.shared.status = status;
        let offset = |s: &SystemStatus| s.clock.map(|c| c.utc_offset_minutes);
        // A new wall clock reading alone changes nothing visible. Compared
        // wide: readings that make no sense must not overflow.
        let jumped = match (old.clock, status.clock) {
            (Some(a), Some(b)) => {
                let base = |c: &paperpaw_platform::WallClock| {
                    i128::from(c.unix_millis) - i128::from(c.at.millis())
                };
                (base(&a) - base(&b)).abs() > 30_000
            }
            (a, b) => a.is_some() != b.is_some(),
        };
        // What status lines show: not the voltage, which moves a little at
        // every reading.
        let shown = |s: &SystemStatus| s.battery.map(|b| (b.percent, b.charging));
        if shown(&old) != shown(&status) || offset(&old) != offset(&status) || jumped {
            self.output.commands.push(events::STATUS_CHANGED.emit(()));
        }
        if status.battery.is_some() {
            self.output.commands.push(events::BATTERY_MEASURED.emit(()));
        }
        self.finish()
    }

    /// When the device is to go to sleep by itself, where it does.
    fn sleep_due(&self) -> Option<Instant> {
        let minutes = self
            .shared
            .settings
            .int(core_settings::SLEEP_AFTER.as_str());
        (self.shared.caps.sleep && minutes > 0)
            .then(|| self.last_activity.add_millis(minutes as u64 * 60_000))
    }

    /// Advances time, e.g. when a requested [`Effect::WakeAt`] is due.
    pub fn tick(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.hide_hints();
        if self.sleep_due().is_some_and(|at| at <= now) {
            self.last_activity = now;
            // Busy: reading aloud, downloading, serving. Else asleep.
            if !self.io.speech.speaking() && !self.io.http.busy() && !self.io.servers.busy() {
                self.output
                    .commands
                    .push(Command::RunAction(paperpaw_sdk::builtin::actions::SLEEP));
            }
        }
        if self.network_deadline().is_some_and(|at| at <= now) {
            self.fail_waiting_network();
        }
        let thresholds = self.thresholds();
        for recognized in self.input.recognizer.tick(now, &thresholds) {
            self.recognized(recognized);
        }
        if let Some(top) = self.stack.last_mut()
            && top.timer.is_some_and(|at| at <= now)
        {
            top.timer = None;
            self.with_top(|screen, cx| {
                screen.on_timer(cx);
                Handled::Yes
            });
        }
        if self.notice.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.end_notice();
        }
        while let Some(due) = self.scheduled.iter().position(|(at, ..)| *at <= now) {
            let (_, event, params) = self.scheduled.remove(due);
            self.output.commands.push(Command::Emit(event, params));
        }
        self.finish()
    }

    /// The framebuffer, for tests of a kernel that holds the whole screen.
    #[cfg(test)]
    pub(crate) fn frame(&self) -> FrameView<'_> {
        self.framebuffer.view()
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.shared.device
    }

    /// The panel took `millis` for a refresh: shells that know say so
    /// ([`Usage::panel_ms`](paperpaw_sdk::usage::Usage::panel_ms)).
    pub fn panel_busy(&mut self, millis: u64) {
        self.shared.usage.panel_ms = self.shared.usage.panel_ms.saturating_add(millis);
    }

    /// The device's storage, e.g. for a shell that copies files onto it.
    pub fn storage(&self) -> &dyn Storage {
        self.shared.storage.as_ref()
    }

    pub fn settings(&self) -> &Settings {
        &self.shared.settings
    }

    /// The passwords and keys (see [`paperpaw_sdk::secrets`]), e.g. for a
    /// shell that joins networks itself.
    pub fn secrets(&self) -> &paperpaw_sdk::secrets::Secrets {
        &self.shared.secrets
    }

    /// Keeps or removes a password or key, as [`Command::SetSecret`] of a
    /// plugin does (e.g. a network's password a shell was given).
    ///
    /// The file is read again first: where it is there but cannot be
    /// read, it is left as it is (written, the others in it would be
    /// lost), the change applies until the end, and the user is told; one
    /// readable again takes the place of what could not be read at start.
    pub fn set_secret(&mut self, change: paperpaw_sdk::secrets::SecretChange) {
        use paperpaw_sdk::secrets::{self, Secrets};
        let storage = self.shared.storage.clone();
        let read =
            paperpaw_sdk::files::load_text_for_update(storage.as_ref(), secrets::FILE, MAX_SECRETS);
        if let Ok(text) = &read
            && self.secrets_unreadable
        {
            self.shared.secrets = Secrets::decode(text.as_deref().unwrap_or_default());
            self.secrets_unreadable = false;
        }
        self.shared.secrets.apply(change);
        if let Err(error) = read {
            self.secrets_unreadable = true;
            let file = secrets::FILE;
            self.warn_runtime(format!(
                "{file} cannot be read ({error}): not saved, left as it is"
            ));
            let file = paperpaw_sdk::Arg::Str(file);
            let text = self
                .shared
                .translator
                .format("core.secrets-unreadable", &[("file", file)]);
            self.output.commands.push(Command::Notify(text));
            return;
        }
        let text = self.shared.secrets.encode();
        let storage = self.shared.storage.as_ref();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        let saved = if self.shared.secrets.is_empty() {
            storage.remove(secrets::FILE).or(Ok(()))
        } else {
            storage.write(secrets::FILE, text.as_bytes())
        };
        if let Err(error) = saved {
            let message = format!("{}: {error}", secrets::FILE);
            self.warn_runtime(message);
        }
    }

    pub fn translator(&self) -> &Translator {
        &self.shared.translator
    }

    pub fn fonts(&self) -> &Fonts {
        &self.shared.fonts
    }

    pub fn formats(&self) -> &Formats {
        &self.shared.formats
    }

    /// The widgets every plugin added (e.g. for a test that draws each).
    pub fn widgets(&self) -> &paperpaw_sdk::widgets::Widgets {
        &self.shared.widgets
    }

    /// Every plugin with its state, `core` first.
    pub fn plugins(&self) -> &Plugins {
        &self.shared.plugins
    }

    /// The id of the screen currently shown.
    pub fn active_screen(&self) -> Option<&Id> {
        self.stack.last().map(|s| &s.id)
    }

    /// The ids of the screens open, the one shown last.
    pub fn screens(&self) -> impl Iterator<Item = &Id> {
        self.stack.iter().map(|s| &s.id)
    }

    /// The ids of every screen plugins added, whether open or not.
    pub fn known_screens(&self) -> impl Iterator<Item = &Id> {
        self.shared.screens.iter().map(|(id, _)| id)
    }

    /// What the device did since start (see [`paperpaw_sdk::usage`]).
    pub fn usage(&self) -> &paperpaw_sdk::usage::Usage {
        &self.shared.usage
    }

    /// What each plugin took since start, `core` first.
    pub fn plugin_usage(&self) -> Vec<paperpaw_sdk::usage::PluginUsage> {
        self.shared.meter.table.borrow().clone()
    }

    /// Every action plugins added.
    pub fn actions(&self) -> &paperpaw_sdk::Actions {
        &self.shared.actions
    }

    /// Sends `event` with `params` as a plugin would, to every plugin and
    /// screen that hears it, e.g. asked by a shell's remote control or a
    /// test.
    pub fn send_event(&mut self, now: Instant, event: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.output.commands.push(Command::Emit(event, params));
        self.run_commands();
        self.finish()
    }

    /// Returns and clears the collected warnings.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        mem::take(&mut self.warnings)
    }

    /// Opens the screen bound to `slot` with `params`, e.g. the reader with
    /// a book's path: for a shell asked to open a file (Android's "open
    /// with", a remote control).
    pub fn open_slot(&mut self, now: Instant, slot: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(Command::OpenSlot(slot, params));
        self.run_commands();
        self.finish()
    }

    /// Opens the screen `screen` with `params`, as a plugin would, e.g.
    /// asked by a shell's remote control or a test.
    pub fn open_screen(&mut self, now: Instant, screen: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.output
            .commands
            .push(Command::OpenScreen(screen, params));
        self.run_commands();
        self.finish()
    }

    /// Runs `action` as a button bound to it would (the screen shown has it
    /// first), e.g. asked by a shell's remote control or a test.
    pub fn run_action(&mut self, now: Instant, action: Id) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.dispatch(action);
        self.finish()
    }

    /// Sets setting `id` from its text as configuration files write it
    /// (`on`, `12`, a choice's value), as the settings screen would: saved,
    /// what follows from it done, plugins told. E.g. for a shell's remote
    /// control or a test; why not, where the setting is unknown or the
    /// text no value of it.
    pub fn set_setting(
        &mut self,
        now: Instant,
        id: &str,
        text: &str,
    ) -> Result<Effects, &'static str> {
        // A placement's too (`widgets.clock@2.face`).
        let spec = self.shared.settings.spec(id).ok_or("no such setting")?;
        let value = spec.kind.parse(text).ok_or("not a value of this setting")?;
        let id = Id::parse(id).map_err(|_| "no such setting")?;
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        self.output.commands.push(Command::SetSetting(id, value));
        self.run_commands();
        Ok(self.finish())
    }

    /// The device goes to sleep or the app to the background; plugins hear
    /// [`events::SUSPEND`].
    pub fn suspend(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.shared.usage.sleeps = self.shared.usage.sleeps.saturating_add(1);
        self.output.commands.push(events::SUSPEND.emit(()));
        let effects = self.finish();
        // What plugins saved, and what storage kept back, written.
        let _ = self.shared.storage.sync();
        effects
    }

    /// Back from [`Kernel::suspend`].
    pub fn resume(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.last_activity = now;
        // The time asleep is no screen's.
        self.shown_since = now;
        self.output.commands.push(events::RESUME.emit(()));
        self.finish()
    }

    fn warn(&mut self, source: WarningSource, line: Option<u32>, message: String) {
        self.warnings.push(Warning {
            source,
            line,
            message,
        });
    }

    /// A warning about something that went wrong while running, of no
    /// file or plugin in particular.
    fn warn_runtime(&mut self, message: impl Into<String>) {
        self.warn(WarningSource::Runtime, None, message.into());
    }
}

/// Everything plugins can read through [`Env`].
struct Shared {
    device: DeviceInfo,
    settings: Settings,
    translator: Translator,
    fonts: Fonts,
    formats: Formats,
    hyphenation: Hyphenation,
    menus: Menus,
    actions: Actions,
    plugins: Plugins,
    status: SystemStatus,
    /// What the device did since start (see [`paperpaw_sdk::usage`]).
    usage: paperpaw_sdk::usage::Usage,
    status_items: StatusItems,
    widgets: paperpaw_sdk::widgets::Widgets,
    setup: paperpaw_sdk::setup::SetupSteps,
    services: paperpaw_sdk::services::Services,
    text_filters: paperpaw_sdk::filters::TextFilters,
    /// Marks on the words of book pages, from every plugin.
    decorators: paperpaw_sdk::decorations::Decorators,
    /// Filters of every input and of every refresh, from every plugin.
    input_filters: paperpaw_sdk::filters::InputFilters,
    refresh_filters: paperpaw_sdk::filters::RefreshFilters,
    /// Plugins' stylesheets for books.
    stylesheets: Vec<Cow<'static, str>>,
    /// Folders of what plugins keep for each book.
    book_data: Vec<paperpaw_sdk::library::BookData>,
    /// What the device and its shell can do.
    caps: Capabilities,
    /// Wi-Fi, where PaperPaw manages it (see [`KernelBuilder::wifi`]).
    wifi: Option<paperpaw_sdk::wifi::Wifi>,
    /// Firmware updates, where PaperPaw is the firmware (see
    /// [`KernelBuilder::firmware`]).
    firmware: Option<Firmware>,
    /// How code plugins are loaded, where they are (see
    /// [`KernelBuilder::code_plugins`]).
    native: Option<paperpaw_sdk::native::Native>,
    /// Text copied here.
    clipboard: String,
    /// The system's clipboard has text (see
    /// [`Kernel::set_clipboard_available`]).
    system_clipboard: bool,
    /// Slots some screen is bound to.
    slots: Vec<Id>,
    /// Every screen, with the translation key of its name.
    screens: Vec<(Id, Cow<'static, str>)>,
    /// Folders fonts are read from.
    font_dirs: Vec<String>,
    /// The folders of data plugins, by plugin, for removing them.
    data_folders: Vec<(Id, String)>,
    /// The folders data plugins are found in.
    plugin_dirs: Vec<String>,
    /// What plugin libraries call this kind of device (see
    /// [`KernelBuilder::catalog_device`]).
    catalog_device: Option<&'static str>,
    /// Passwords and keys (see [`paperpaw_sdk::secrets`]).
    secrets: paperpaw_sdk::secrets::Secrets,
    /// Random bytes (see [`KernelBuilder::random`]).
    random: paperpaw_sdk::random::Random,
    /// Shared with font packs read where they are (see `Fonts::load`).
    storage: alloc::rc::Rc<dyn Storage>,
    /// What the storage read during the call going on (see `recall`).
    recall: alloc::rc::Rc<recall::Recall>,
    keymap: Keymap,
    /// What each plugin takes.
    meter: alloc::rc::Rc<meter::Meter>,
}

impl Shared {
    fn env(&self, now: Instant) -> Env<'_> {
        Env::from(EnvParts {
            device: &self.device,
            now,
            metrics: Metrics::new(self.device.display.dpi),
            settings: &self.settings,
            i18n: &self.translator,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.meter.table,
            status_items: &self.status_items,
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            stylesheets: &self.stylesheets,
            book_data: &self.book_data,
            caps: self.caps,
            wifi: self.wifi.as_ref(),
            firmware: self.firmware.as_ref(),
            native: self.native.as_ref(),
            clipboard: self.system_clipboard || !self.clipboard.is_empty(),
            slots: &self.slots,
            screens: &self.screens,
            font_dirs: &self.font_dirs,
            catalog_device: self.catalog_device,
            secrets: &self.secrets,
            random: self.random,
            storage: self.storage.as_ref(),
            keys: &self.keymap,
        })
    }
}

/// `text` cut to what the clipboard holds, at a character boundary.
fn clipped(text: &str) -> String {
    let mut end = text.len().min(paperpaw_sdk::MAX_CLIPBOARD);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    String::from(&text[..end])
}

/// Whether this build renders OpenType/TrueType fonts.
fn paperpaw_text_outlines() -> bool {
    paperpaw_sdk::text::OUTLINES
}
