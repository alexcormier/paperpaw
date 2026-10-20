//! The main loop: the kernel and the X4's parts, one step at a time. Each
//! step feeds the watchdogs, moves the network on, carries out what the
//! kernel asked of the firmware (an update, a restart), passes on the
//! battery and the time, the presses that came, what came over USB
//! (`remote`), wakes the kernel when it asked to be, and rests until there
//! is something to do. Whatever the kernel gives back is shown
//! ([`Firmware::apply`]); when it says so, the X4 sleeps.

use esp_hal::delay::Delay;
use esp_hal::gpio::{Event, Output, WakeupConfig};
use log::{info, warn};
use paperpaw_core::{
    Effects, FirmwareCommand, FirmwareState, InstallFailure, Io, Kernel, SpeechCall,
};
use paperpaw_platform::{
    Battery, ButtonState, InputEvent, Instant, Point, PointerEvent, PointerPhase, PointerTool,
    Rect, RefreshMode, SystemStatus,
};
use paperpaw_x4::display::{Planes, Refresh};
use paperpaw_x4::sent::{Check, Sent};

use crate::board::Panel;
use crate::network::{Network, NetworkIo};
use crate::ota::Ota;
use crate::{clock, device, input, now, power, stack};

/// How long the main loop rests when there is nothing to do.
const POLL_MS: u64 = 10;
/// How often the battery is read.
const STATUS_MS: u64 = 60_000;
/// On battery, the longest the processor sleeps between readings of the
/// buttons (a press lasts 80 ms and more).
const IDLE_MS: u64 = 30;
/// How long after the last work what was written is written back to the
/// card (not at once: a download writes in many pieces).
const SYNC_MS: u64 = 2_000;
/// An update on trial is taken as good once it ran this long (its start
/// and the book it opens at once behind it), or when it goes to sleep or
/// restarts by itself.
const TRIAL_MS: u64 = 30_000;

/// The X4's firmware: the kernel, the parts it drives, and where the main
/// loop is.
pub struct Firmware {
    pub kernel: Kernel,
    pub panel: Panel,
    pub network: Network,
    /// Updates, where the flash has room for them.
    pub ota: Option<Ota>,
    /// Holds the battery's power on.
    pub power_latch: Output<'static>,
    /// High while on USB power.
    pub usb_power: crate::board::UsbPower,
    pub delay: Delay,
    #[cfg(feature = "remote")]
    pub remote: crate::remote::Remote,
    /// The X4 Pro's front light.
    #[cfg(feature = "x4-pro")]
    pub light: Option<crate::light::Light>,
    pub state: State,
}

/// Where the main loop is.
#[derive(Debug, Default)]
pub struct State {
    /// The kernel asked to sleep.
    sleep: bool,
    /// When the kernel wants to be woken.
    wake_at: Option<Instant>,
    /// When the battery was read last.
    status_read: Option<Instant>,
    /// The latest time the kernel was given: presses that waited in the
    /// queue keep their own time, but never one before it.
    kernel_time: u64,
    /// A button pressed over USB, and when to let it go.
    #[cfg(feature = "remote")]
    held: Option<(&'static str, Instant)>,
    /// Light sleeps on USB too until then (`idle sleep` over USB), for
    /// trying them.
    #[cfg(feature = "remote")]
    sleep_on_usb_until: Option<Instant>,
    /// The panel shows grays (a frame whose pictures are best so, or the
    /// gray test over USB): the next change is shown with the whole screen
    /// drawn again.
    gray_shown: bool,
    /// When the kernel first worked since the card was last written back,
    /// if what it wrote may not be on the card yet: written back
    /// [`SYNC_MS`] after, so nothing is lost when the X4 is cut off
    /// (reset, flashed, an empty battery).
    unsynced: Option<u64>,
}

impl Firmware {
    /// Starts the kernel, then runs the main loop.
    pub fn run(mut self) -> ! {
        let effects = self.kernel.start(now());
        self.apply(effects);
        // Its first frame is shown: its code plugins started cleanly.
        crate::safe_mode::started();
        loop {
            self.step();
        }
    }

    /// An update on trial works: it is kept (see [`TRIAL_MS`]).
    fn confirm(&mut self) {
        if let Some(ota) = &mut self.ota {
            ota.confirm();
        }
    }

    /// Whether an update is being written: it goes on to its end, the X4
    /// awake.
    fn installing(&self) -> bool {
        self.ota.as_ref().is_some_and(Ota::installing)
    }

    fn step(&mut self) {
        if now().0 >= TRIAL_MS {
            self.confirm();
        }
        let installing = self.installing();
        if self.state.sleep && !installing {
            return self.sleep();
        }
        power::feed();
        power::mark(power::at::LOOP);
        stack::check();
        self.io();
        for effects in self.network.step(&mut self.kernel, now()) {
            self.apply(effects);
        }
        self.firmware_step();
        let light = self.light();
        // Not while the radio is on: its tasks run in between. Nor while
        // the X4 Pro's light is on: its PWM stops in a light sleep.
        let battery = !self.network.radio_on()
            && !installing
            && !light
            && ((self.usb_power.is_low() && self.kernel.settings().read(&device::LIGHT_SLEEP))
                || self.trying_sleep());
        power::allow(battery);
        self.status();
        self.buttons();
        #[cfg(feature = "remote")]
        self.remote();
        self.tick();
        self.rest(battery, installing);
    }

    /// Shows the front light as its settings say; whether it is on.
    #[cfg(feature = "x4-pro")]
    fn light(&mut self) -> bool {
        let settings = self.kernel.settings();
        let level = settings.read(&device::FRONTLIGHT);
        let warmth = settings.read(&device::WARMTH);
        if let Some(light) = self.light.as_mut() {
            light.show(level, warmth);
        }
        level > 0 && self.light.is_some()
    }

    /// The X4 and the X4 Classic have no light.
    #[cfg(not(feature = "x4-pro"))]
    fn light(&mut self) -> bool {
        false
    }

    /// The time for the kernel now, never before one it was given.
    #[cfg(feature = "remote")]
    fn kernel_now(&mut self) -> Instant {
        self.state.kernel_time = now().0.max(self.state.kernel_time);
        Instant(self.state.kernel_time)
    }

    /// What the kernel asks of the X4: the network's work goes to
    /// `network`, the firmware's is done here; speech and a system
    /// clipboard it does not have.
    fn io(&mut self) {
        for io in self.kernel.take_io() {
            match io {
                Io::Wifi(command) => self.network.ask(NetworkIo::Wifi(command)),
                Io::Http(call) => self.network.ask(NetworkIo::Http(call)),
                Io::Server(call) => self.network.ask(NetworkIo::Server(call)),
                Io::Socket(call) => self.network.ask(NetworkIo::Socket(call)),
                Io::Firmware(command) => self.firmware(command),
                Io::Speech(SpeechCall::Say { id, .. }) => {
                    let effects = self.kernel.speech_done(now(), id, false);
                    self.apply(effects);
                }
                Io::Speech(SpeechCall::Stop) | Io::Copy(_) => {}
                Io::ReadClipboard => {
                    let effects = self.kernel.clipboard_text(now(), None);
                    self.apply(effects);
                }
            }
        }
    }

    /// An update or a restart the kernel asked for.
    fn firmware(&mut self, command: FirmwareCommand) {
        match command {
            FirmwareCommand::Install { path } => {
                let state = match &mut self.ota {
                    Some(ota) => ota.begin(self.kernel.storage(), &path),
                    None => FirmwareState::Failed(InstallFailure::Write),
                };
                let effects = self.kernel.firmware_state(now(), state);
                self.apply(effects);
            }
            FirmwareCommand::Restart => self.restart(),
        }
    }

    /// Restarts the X4 once what plugins keep is saved and all written
    /// is on the card (the kernel's suspend writes it back).
    fn restart(&mut self) -> ! {
        info!("restarting");
        self.confirm();
        let _ = self.kernel.suspend(now());
        esp_hal::system::software_reset();
    }

    /// How an update goes.
    fn firmware_step(&mut self) {
        if let Some(ota) = &mut self.ota
            && let Some(state) = ota.step(self.kernel.storage())
        {
            let effects = self.kernel.firmware_state(now(), state);
            self.apply(effects);
        }
    }

    /// The battery and the time, every [`STATUS_MS`] and once the clock
    /// was set.
    fn status(&mut self) {
        let at = now();
        let due = self
            .state
            .status_read
            .is_none_or(|read| at.since(read) >= STATUS_MS);
        if !due && !clock::take_changed() {
            return;
        }
        power::mark(power::at::STATUS);
        self.state.status_read = Some(at);
        let millivolts = input::millivolts();
        let status = SystemStatus {
            clock: clock::wall(at),
            battery: Some(Battery {
                percent: input::percent(),
                charging: self.usb_power.is_high(),
                millivolts: Some(millivolts),
            }),
        };
        let effects = self.kernel.set_status(at, status);
        self.apply(effects);
    }

    /// The presses and releases that came, before any work in the
    /// background.
    fn buttons(&mut self) {
        while let Some(change) = input::next() {
            let (event, at) = match change {
                input::Change::Button(name, down, at) => {
                    let state = if down {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    };
                    (InputEvent::Button { name, state }, at)
                }
                input::Change::Touch(input::Touch::Home(down), at) => {
                    let state = if down {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    };
                    let name = paperpaw_platform::buttons::HOME;
                    (InputEvent::Button { name, state }, at)
                }
                input::Change::Touch(touch, at) => (self.pointer(touch), at),
            };
            self.state.kernel_time = at.max(self.state.kernel_time);
            let started = now();
            power::mark(power::at::INPUT);
            let effects = self.kernel.handle(Instant(self.state.kernel_time), event);
            if let InputEvent::Button { name, .. } = event {
                info!("{name}: {} ms", now().since(started));
            }
            self.apply(effects);
        }
    }

    /// A touch on the upright screen, on the screen PaperPaw draws (it may
    /// be turned).
    fn pointer(&self, touch: input::Touch) -> InputEvent {
        let (phase, x, y) = match touch {
            input::Touch::Down(x, y, raw) => {
                info!("touch at {x},{y} (controller {},{})", raw.0, raw.1);
                (PointerPhase::Down, x, y)
            }
            input::Touch::Move(x, y) => (PointerPhase::Move, x, y),
            input::Touch::Up(x, y) => (PointerPhase::Up, x, y),
            // A button, taken apart before.
            input::Touch::Home(_) => (PointerPhase::Cancel, 0, 0),
        };
        let (x, y) = paperpaw_x4::uc8179::turned(self.panel.rotation(), x, y);
        InputEvent::Pointer(PointerEvent {
            id: 0,
            phase,
            point: Point::new(i32::from(x), i32::from(y)),
            tool: PointerTool::Finger,
            pressure: 1000,
        })
    }

    /// Wakes the kernel when it asked to be.
    fn tick(&mut self) {
        if !self.state.wake_at.is_some_and(|due| due <= now()) {
            return;
        }
        power::mark(power::at::TICK);
        let started = now();
        self.state.kernel_time = started.0;
        let effects = self.kernel.tick(started);
        let took = now().since(started);
        if took > 50 {
            info!("tick: {took} ms");
        }
        self.apply(effects);
    }

    /// On battery the processor sleeps until there is something to do or
    /// the buttons are to be read again; on USB it stays in reach of the
    /// computer (see `power`).
    fn rest(&mut self, battery: bool, installing: bool) {
        #[cfg(feature = "remote")]
        if self
            .state
            .sleep_on_usb_until
            .is_some_and(|until| until <= now())
        {
            // Started again: whatever the light sleeps did to the USB link,
            // the computer finds the X4 again.
            power::mark(power::at::TRIED);
            esp_hal::system::software_reset();
        }
        let mut until = now().0
            + if battery {
                IDLE_MS
            } else if self.network.moving() || installing {
                // Data on the way, or an update: on at once.
                0
            } else {
                POLL_MS
            };
        if let Some(at) = self.state.wake_at {
            until = until.min(at.0);
        }
        if let Some(worked) = self.state.unsynced {
            let due = worked + SYNC_MS;
            if now().0 >= due {
                self.state.unsynced = None;
                if self.kernel.storage().sync().is_err() {
                    warn!("card: not all written back");
                }
            } else {
                until = until.min(due);
            }
        }
        if let Some(read) = self.state.status_read {
            until = until.min(read.0 + STATUS_MS);
        }
        if !power::nap_until(until) {
            self.delay
                .delay_millis(until.saturating_sub(now().0) as u32);
        }
    }

    /// Whether light sleeps are tried on USB (`idle sleep` over USB).
    fn trying_sleep(&self) -> bool {
        #[cfg(feature = "remote")]
        return self.state.sleep_on_usb_until.is_some();
        #[cfg(not(feature = "remote"))]
        false
    }

    /// Plugins save what is pending, the screen stays as it is, and the X4
    /// sleeps: on battery its power is cut, and the power button starts it
    /// again where it was; on USB power it stays in reach of a computer
    /// (its log, remote control) until the power button wakes it.
    #[expect(
        clippy::expect_used,
        reason = "the power button is taken back only here; without it the X4 could not wake"
    )]
    fn sleep(&mut self) {
        // A press over USB that put it to sleep is let go first.
        #[cfg(feature = "remote")]
        self.release_held();
        self.confirm();
        let effects = self.kernel.suspend(now());
        self.apply(effects);
        let _ = self.panel.sleep();
        #[cfg(feature = "x4-pro")]
        if let Some(light) = self.light.as_mut() {
            light.off();
        }
        info!("sleeping");
        // Released first: held, the button would wake it at once.
        self.wait_for_power_release();
        if self.usb_power.is_high() && self.sleep_on_usb() {
            return;
        }
        let mut power = input::stop().expect("the power button");
        // The ESP32-S3 devices hold their latch through resets: let go
        // first.
        #[cfg(feature = "esp32s3")]
        self.power_latch.set_pad_hold(false);
        self.power_latch.set_low();
        power.listen(Event::LowLevel);
        let _ = power.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true));
        power::deep();
    }

    fn wait_for_power_release(&self) {
        while input::power_down() {
            power::feed();
            self.delay.delay_millis(10);
        }
    }

    /// Asleep on USB power: the sleep screen shows until the power button,
    /// or `press power` over USB, wakes it (`true`), or until the USB
    /// power is gone (`false`): then it sleeps as on battery.
    fn sleep_on_usb(&mut self) -> bool {
        loop {
            self.delay.delay_millis(POLL_MS as u32);
            power::feed();
            if input::power_down() {
                break;
            }
            #[cfg(feature = "remote")]
            if self.remote_while_asleep() {
                break;
            }
            if self.usb_power.is_low() {
                info!("off USB: sleeping as on battery");
                return false;
            }
        }
        self.wait_for_power_release();
        input::rearm();
        if let Err(e) = self.panel.init() {
            warn!("display: {e:?}");
        }
        info!("awake");
        self.state.sleep = false;
        let effects = self.kernel.resume(now());
        self.apply(effects);
        true
    }

    /// Carries out what the kernel asks: shows what changed, notes when to
    /// wake it, restarts, notes whether to sleep.
    pub fn apply(&mut self, mut effects: Effects) {
        // From the first work not written back: work coming again and
        // again does not put it off.
        self.state.unsynced.get_or_insert(now().0);
        loop {
            if effects.restart() {
                self.restart();
            }
            // Turned (the setting): the same screens, made anew for the
            // turned size and drawn whole, instead of what was asked.
            let rotation = self.kernel.rotation();
            if rotation != self.panel.rotation() {
                self.state.sleep |= effects.sleep();
                self.panel.set_rotation(rotation);
                let (width, height) = self.panel.size();
                info!("turned {rotation:?}: {width}x{height}");
                effects = self.kernel.resize(now(), width, height);
                continue;
            }
            // Slow work ahead: its mark is shown first, the panel
            // refreshing it while the kernel works on, then the frame the
            // work drew. After grays a refresh drives the whole screen
            // and takes long: the work is done without a mark then.
            if effects.proceed()
                && let Some((area, _)) = effects.refresh()
            {
                effects = if self.state.gray_shown {
                    self.kernel.proceed(now())
                } else {
                    self.show_busy(area)
                };
                continue;
            }
            // Pictures in grays: the whole screen, with the gray waveform;
            // small changes over them (the time, the battery) wait for the
            // next page rather than refresh it all.
            if effects.grays()
                && let Some((area, _)) = effects.refresh()
            {
                let screen = self.kernel.device().display;
                let small = u32::from(area.width) * u32::from(area.height) * 4
                    < u32::from(screen.width) * u32::from(screen.height);
                if !(self.state.gray_shown && small) {
                    self.show_gray(area);
                }
            } else if self.state.gray_shown && effects.refresh().is_some() {
                // After grays, the whole screen, not only what changed.
                self.state.gray_shown = false;
                effects = self.kernel.draw_again(now());
                continue;
            } else if let Some((area, mode)) = effects.refresh() {
                self.show(area, mode);
            }
            for warning in self.kernel.take_warnings() {
                warn!("{warning}");
            }
            self.state.wake_at = effects.wake_at();
            self.state.sleep |= effects.sleep();
            return;
        }
    }

    /// Shows the busy mark in `area` and lets the kernel work while the
    /// panel refreshes; what the work brought.
    fn show_busy(&mut self, area: Rect) -> Effects {
        power::mark(power::at::SHOWING);
        let mut sent = Sent::new(heap_free);
        let panel = &mut self.panel;
        self.kernel.render(&mut |top, rows| {
            if let Err(e) = panel.write_rows(top, rows, Planes::NEW) {
                warn!("display: {e:?}");
            }
            sent.keep(top, &rows);
        });
        let started = now();
        let refreshing = self.panel.start_refresh(Refresh::Fast, false).is_ok();
        let next = self.kernel.proceed(now());
        let worked = now();
        if refreshing && let Err(e) = self.panel.wait() {
            warn!("display: {e:?}");
        }
        // As after every refresh, both memories hold what is shown.
        let panel = &mut self.panel;
        let kept = sent.replay(&mut |top, rows| {
            if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                warn!("display: {e:?}");
            }
        });
        if !kept {
            warn!("busy mark not kept");
        }
        info!(
            "mark {},{} {}x{}: work took {} ms, waited {} ms more",
            area.x,
            area.y,
            area.width,
            area.height,
            worked.since(started),
            now().since(worked)
        );
        next
    }

    /// Shows the whole screen in four grays, its pictures best so (see
    /// `Effects::grays`); the next change draws it whole again. Where
    /// there is no memory for it, as a full refresh of `area`.
    fn show_gray(&mut self, area: Rect) {
        power::mark(power::at::SHOWING);
        let started = now();
        let panel = &mut self.panel;
        let mut failed = None;
        let drawn = self.kernel.render_gray(&mut |top, rows| {
            if let Err(e) = panel.write_gray_rows(top, rows) {
                failed = Some(e);
            }
        });
        if !drawn {
            warn!("no memory for grays");
            return self.show(area, RefreshMode::Full);
        }
        let written = now();
        if let Err(e) = self.panel.refresh_gray() {
            failed = Some(e);
        }
        let shown = now();
        self.kernel.panel_busy(shown.since(written));
        self.kernel.shown(shown);
        self.state.gray_shown = true;
        if let Some(e) = failed {
            warn!("display: {e:?}");
        }
        info!(
            "frame in grays: drawn in {} ms, shown in {} ms",
            written.since(started),
            shown.since(written)
        );
    }

    /// Shows the frame changed in `area`.
    fn show(&mut self, area: Rect, mode: RefreshMode) {
        power::mark(power::at::SHOWING);
        let refresh = match mode {
            // Every pixel driven with a warm panel's waveform (1.6 s), as
            // the stock firmware cleans the panel every few pages.
            RefreshMode::Full => Refresh::Half,
            RefreshMode::Partial | RefreshMode::Fast => Refresh::Fast,
        };
        let planes = if refresh == Refresh::Fast {
            Planes::NEW
        } else {
            Planes::BOTH
        };
        let mut failed = None;
        let started = now();
        // What is sent is kept for sending again after a fast refresh.
        let mut sent = Sent::new(heap_free);
        // What went into each memory of the controller, to check that the
        // frame shown is the frame sent.
        let (mut written, mut again_written) = (Check::default(), Check::default());
        let panel = &mut self.panel;
        self.kernel.render(&mut |top, rows| {
            if let Err(e) = panel.write_rows(top, rows, planes) {
                failed = Some(e);
            }
            written.add(top, &rows);
            sent.keep(top, &rows);
        });
        let drawn = now();
        if let Err(e) = self.panel.refresh(refresh, false) {
            failed = Some(e);
        }
        let shown = now();
        self.kernel.panel_busy(shown.since(drawn));
        self.kernel.shown(shown);
        // After every refresh, both memories again hold the frame now
        // shown, as the stock firmware does: the refresh may leave the
        // memory of the new frame changed, and the next fast refresh
        // compares the two (sent again as kept, or drawn anew).
        let panel = &mut self.panel;
        let mut again = |top: u16, rows: paperpaw_platform::FrameView<'_>| {
            if let Err(e) = panel.write_rows(top, rows, Planes::BOTH) {
                failed = Some(e);
            }
            again_written.add(top, &rows);
        };
        let kept = sent.replay(&mut again);
        if !kept {
            self.kernel.rerender(&mut again);
        }
        if again_written != written {
            warn!(
                "frame shown differs from frame sent ({}): {written:?} then {again_written:?}",
                if kept { "kept" } else { "drawn anew" }
            );
        }
        if let Some(e) = failed {
            warn!("display: {e:?}");
        }
        info!(
            "frame {mode:?} {},{} {}x{}: drawn in {} ms, shown in {} ms, sent again in {} ms",
            area.x,
            area.y,
            area.width,
            area.height,
            drawn.since(started),
            shown.since(drawn),
            now().since(shown)
        );
        // Where the heap stands, for finding what takes it (test builds).
        #[cfg(feature = "remote")]
        info!(
            "heap {} used, peak {}",
            esp_alloc::HEAP.used(),
            esp_alloc::HEAP.stats().max_usage
        );
    }
}

/// The heap's free bytes, for keeping what was sent.
fn heap_free() -> usize {
    esp_alloc::HEAP.free()
}

/// What came over USB.
#[cfg(feature = "remote")]
impl Firmware {
    fn remote(&mut self) {
        if let Some(command) = self.remote.poll()
            && let Some(command) = self.remote.answer(&mut self.kernel, command)
        {
            self.remote_command(command);
        }
        if self.state.held.is_some_and(|(_, at)| at <= now()) {
            self.release_held();
        }
    }

    /// Lets go of the button pressed over USB, if one is held, and says
    /// so.
    fn release_held(&mut self) {
        let Some((name, _)) = self.state.held.take() else {
            return;
        };
        let started = now();
        let at = self.kernel_now();
        let state = ButtonState::Released;
        let effects = self.kernel.handle(at, InputEvent::Button { name, state });
        info!("{name}: {} ms", now().since(started));
        self.apply(effects);
        crate::remote::ok();
    }

    /// Carries out a command over USB that changes the X4's state.
    fn remote_command(&mut self, command: paperpaw_x4::remote::Command) {
        use crate::remote;
        use paperpaw_x4::remote::Command;
        match command {
            Command::Network(ask) => self.network.ask_remote(ask),
            Command::IdleSleep(seconds) => {
                // At most ten minutes: the link is gone meanwhile.
                self.state.sleep_on_usb_until = Some(Instant(now().0 + seconds.min(600) * 1000));
                remote::ok();
            }
            Command::Run(action) => {
                let Ok(id) = paperpaw_sdk::Id::parse(&action) else {
                    return remote::error(&action);
                };
                let at = self.kernel_now();
                let started = now();
                let effects = self.kernel.run_action(at, id);
                info!("{action}: {} ms", now().since(started));
                self.apply(effects);
                remote::ok();
            }
            Command::Set { id, value } => {
                let at = self.kernel_now();
                match self.kernel.set_setting(at, &id, &value) {
                    Ok(effects) => {
                        info!("{id} = {value}");
                        self.apply(effects);
                        remote::ok();
                    }
                    Err(why) => remote::error(why),
                }
            }
            Command::Open(path) => {
                let at = self.kernel_now();
                let params =
                    paperpaw_sdk::Params::new().with(paperpaw_sdk::builtin::params::PATH, path);
                let reader = paperpaw_sdk::builtin::slots::READER;
                let effects = self.kernel.open_slot(at, reader, params);
                self.apply(effects);
                remote::ok();
            }
            Command::Gray(picture) => {
                let shown = crate::gray_test::show(
                    &mut self.panel,
                    self.kernel.storage(),
                    picture.as_deref(),
                );
                if let Err(e) = shown {
                    warn!("display: {e:?}");
                }
                self.state.gray_shown = true;
                remote::ok();
            }
            Command::Refresh { full } => {
                let refresh = if full { Refresh::Full } else { Refresh::Fast };
                remote::refreshed(self.panel.refresh(refresh, false));
            }
            Command::Emit { event, params } => {
                let Ok(id) = paperpaw_sdk::Id::parse(&event) else {
                    return remote::error(&event);
                };
                let mut with = paperpaw_sdk::Params::new();
                for (key, value) in params {
                    with.set(key, value);
                }
                let at = self.kernel_now();
                let effects = self.kernel.send_event(at, id, with);
                self.apply(effects);
                remote::ok();
            }
            Command::Touch { phase, x, y } => {
                use paperpaw_x4::remote::Touch;
                let phase = match phase {
                    Touch::Down => PointerPhase::Down,
                    Touch::Move => PointerPhase::Move,
                    Touch::Up => PointerPhase::Up,
                };
                let event = InputEvent::Pointer(PointerEvent {
                    id: 0,
                    phase,
                    point: Point::new(x, y),
                    tool: PointerTool::Finger,
                    pressure: 1000,
                });
                let at = self.kernel_now();
                let effects = self.kernel.handle(at, event);
                self.apply(effects);
                remote::ok();
            }
            Command::SetTime(unix_ms) => {
                crate::clock::set(now(), unix_ms);
                remote::ok();
            }
            Command::Restart => {
                // What plugins keep, saved and on the card first.
                let at = self.kernel_now();
                let _ = self.kernel.suspend(at);
                remote::ok();
                esp_hal::system::software_reset();
            }
            Command::Press { button, ms } => {
                if self.state.held.is_some() {
                    return remote::busy();
                }
                let at = self.kernel_now();
                let state = ButtonState::Pressed;
                let started = now();
                let effects = self.kernel.handle(
                    at,
                    InputEvent::Button {
                        name: button,
                        state,
                    },
                );
                info!("{button} pressed: {} ms", now().since(started));
                self.apply(effects);
                self.state.held = Some((button, Instant(now().0.saturating_add(ms))));
            }
            // Only asking: answered by `remote`.
            _ => {}
        }
    }

    /// A command while asleep on USB: what only asks is answered, and
    /// `press power` wakes the X4 (`true`).
    fn remote_while_asleep(&mut self) -> bool {
        use paperpaw_x4::remote::Command;
        let Some(command) = self.remote.poll() else {
            return false;
        };
        match self.remote.answer(&mut self.kernel, command) {
            None => false,
            Some(Command::Press { button, .. }) => {
                crate::remote::ok();
                button == paperpaw_platform::buttons::POWER
            }
            Some(_) => {
                crate::remote::error("asleep");
                false
            }
        }
    }
}
