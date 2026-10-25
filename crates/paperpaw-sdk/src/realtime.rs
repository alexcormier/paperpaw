//! Screens that run in real time: games, emulators, animations, drawings
//! that follow the pen ([ADR 0030]).
//!
//! A screen says once how it runs ([`Screen::mode`], a [`ScreenMode`]):
//! full screen, buttons pressed and released, refreshes it chooses
//! itself, the fastest refresh, a pace. The device says what it can do
//! and what it did ([`Env::panel`], a [`PanelInfo`]): how fast each
//! refresh is, and how the last frame went. A plugin paces itself by
//! that, so faster panels make it faster without a new build.
//!
//! Both are values of a fixed size with room to grow: a later option or
//! number takes a free place, which a plugin built before leaves empty
//! (meaning as before), and which a firmware built before ignores.
//!
//! [ADR 0030]: https://github.com/hthoene/paperpaw/blob/main/docs/adr/0030-real-time-screens.md
//! [`Screen::mode`]: crate::Screen::mode
//! [`Env::panel`]: crate::Env::panel

use core::time::Duration;

use paperpaw_platform::{Instant, Rect, RefreshMode};

/// How long *Back* is held to leave a screen that takes raw keys
/// ([`ScreenMode::raw_keys`]), whatever the screen does with it.
pub const ESCAPE_HOLD: Duration = Duration::from_secs(2);

/// Bits of [`ScreenMode`]'s flags.
mod flags {
    pub const FULL_SCREEN: u32 = 1;
    pub const RAW_KEYS: u32 = 1 << 1;
    pub const CLEANS_ITSELF: u32 = 1 << 2;
    pub const FASTEST: u32 = 1 << 3;
}

/// Places of [`ScreenMode`]'s numbers.
mod slots {
    /// Milliseconds between frames.
    pub const FRAME: usize = 0;
    /// Milliseconds before sleeping by itself, at least.
    pub const AWAKE: usize = 1;
    /// Grays that are enough.
    pub const GRAYS: usize = 2;
}

/// How a screen runs while it is on top: what it asks of the panel, the
/// buttons and the device ([`Screen::mode`](crate::Screen::mode)). Each
/// option asks for an intent, which every device meets as well as it can;
/// the default asks for nothing, as screens of pages need.
///
/// ```
/// # use core::time::Duration;
/// # use paperpaw_sdk::realtime::ScreenMode;
/// let mode = ScreenMode::new()
///     .full_screen()
///     .raw_keys()
///     .cleans_itself()
///     .fastest()
///     .grays(2)
///     .frame_every(Duration::from_millis(100));
/// assert!(mode.takes_raw_keys());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScreenMode {
    flags: u32,
    /// Numbers by [`slots`]; 0 is not asked. More than are used, for
    /// options to come.
    numbers: [u32; 7],
}

impl ScreenMode {
    /// A screen of pages: nothing asked.
    pub const fn new() -> Self {
        Self {
            flags: 0,
            numbers: [0; 7],
        }
    }

    const fn with(mut self, flag: u32) -> Self {
        self.flags |= flag;
        self
    }

    const fn number(mut self, slot: usize, value: u32) -> Self {
        self.numbers[slot] = value;
        self
    }

    /// The screen has all of the panel: no row of hints, no bars.
    pub const fn full_screen(self) -> Self {
        self.with(flags::FULL_SCREEN)
    }

    /// Buttons come pressed and released ([`Screen::on_key`]), not as
    /// actions once let go; [`Env::keys_held`] says which are down. The
    /// power button stays the device's, and holding *Back* for
    /// [`ESCAPE_HOLD`] leaves the screen.
    ///
    /// [`Screen::on_key`]: crate::Screen::on_key
    /// [`Env::keys_held`]: crate::Env::keys_held
    pub const fn raw_keys(self) -> Self {
        self.with(flags::RAW_KEYS)
    }

    /// The screen chooses its refreshes: the kernel adds no full refresh
    /// every few pages and no refresh in grays for pictures. It asks for
    /// one with [`Cx::request_refresh`](crate::Cx::request_refresh) when
    /// it wants the ghosts gone.
    pub const fn cleans_itself(self) -> Self {
        self.with(flags::CLEANS_ITSELF)
    }

    /// Changes show with the fastest refresh the panel has: quickly
    /// first, finely second.
    pub const fn fastest(self) -> Self {
        self.with(flags::FASTEST)
    }

    /// `levels` grays are enough (2: black and white): the device may
    /// take a refresh that shows no more.
    pub const fn grays(self, levels: u8) -> Self {
        self.number(slots::GRAYS, levels as u32)
    }

    /// The pace the screen would like: a frame every `interval`. The
    /// device keeps ready for it (it does not sleep lightly for longer)
    /// and shows each frame as soon as the panel can take it; the screen
    /// asks for its timer as it does otherwise ([`Cx::wake_at`]).
    ///
    /// [`Cx::wake_at`]: crate::Cx::wake_at
    pub fn frame_every(self, interval: Duration) -> Self {
        self.number(slots::FRAME, millis(interval).max(1))
    }

    /// While it shows, the device sleeps by itself no sooner than after
    /// `idle` without input, where the user's setting is shorter (a game
    /// played by watching).
    pub fn awake_for(self, idle: Duration) -> Self {
        self.number(slots::AWAKE, millis(idle).max(1))
    }

    /// Whether the screen has all of the panel.
    pub const fn is_full_screen(&self) -> bool {
        self.flags & flags::FULL_SCREEN != 0
    }

    /// Whether buttons come pressed and released.
    pub const fn takes_raw_keys(&self) -> bool {
        self.flags & flags::RAW_KEYS != 0
    }

    /// Whether the screen chooses its refreshes.
    pub const fn does_clean_itself(&self) -> bool {
        self.flags & flags::CLEANS_ITSELF != 0
    }

    /// Whether changes show with the fastest refresh.
    pub const fn wants_fastest(&self) -> bool {
        self.flags & flags::FASTEST != 0
    }

    /// The grays that are enough, if the screen said.
    pub const fn grays_enough(&self) -> Option<u8> {
        match self.numbers[slots::GRAYS] {
            0 => None,
            levels => Some(if levels > 255 { 255 } else { levels as u8 }),
        }
    }

    /// The pace the screen would like, if any.
    pub const fn frame_interval(&self) -> Option<Duration> {
        duration(self.numbers[slots::FRAME])
    }

    /// How long the device stays awake without input at least, if the
    /// screen said.
    pub const fn awake(&self) -> Option<Duration> {
        duration(self.numbers[slots::AWAKE])
    }

    /// Whether the screen runs at a pace ([`ScreenMode::frame_every`]).
    pub const fn is_paced(&self) -> bool {
        self.numbers[slots::FRAME] != 0
    }
}

fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

const fn duration(millis: u32) -> Option<Duration> {
    match millis {
        0 => None,
        ms => Some(Duration::from_millis(ms as u64)),
    }
}

/// The places of refresh modes in [`PanelInfo`]'s table, by their order
/// ([`RefreshMode`] is ordered from the lightest to the strongest).
const MODES: usize = 8;

fn mode_code(mode: RefreshMode) -> usize {
    match mode {
        RefreshMode::Mono => 0,
        RefreshMode::Fast => 1,
        RefreshMode::Partial => 2,
        RefreshMode::Gray => 3,
        RefreshMode::Full => 4,
        RefreshMode::Clear => 5,
        // Modes to come take the free places.
        _ => MODES - 1,
    }
}

fn mode_of(code: u8) -> Option<RefreshMode> {
    Some(match code {
        0 => RefreshMode::Mono,
        1 => RefreshMode::Fast,
        2 => RefreshMode::Partial,
        3 => RefreshMode::Gray,
        4 => RefreshMode::Full,
        5 => RefreshMode::Clear,
        _ => return None,
    })
}

/// Bits of [`PanelInfo`]'s flags.
mod panel_flags {
    pub const OVERLAPS: u32 = 1;
}

/// What the panel can do, and how the last frame went
/// ([`Env::panel`](crate::Env::panel)). Times are measured as the device
/// runs (the fastest seen of each mode), so they grow better with faster
/// drivers, and are known once a refresh of the mode was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelInfo {
    flags: u32,
    gray_levels: u32,
    /// Microseconds of each mode by [`mode_code`]; 0 is not known.
    refresh_us: [u32; MODES],
    /// Room for numbers to come.
    numbers: [u32; 4],
    last: FrameReport,
}

impl Default for PanelInfo {
    fn default() -> Self {
        Self::new(2)
    }
}

impl PanelInfo {
    /// A panel that shows `gray_levels` grays, its refreshes not timed yet.
    pub const fn new(gray_levels: u8) -> Self {
        Self {
            flags: 0,
            gray_levels: gray_levels as u32,
            refresh_us: [0; MODES],
            numbers: [0; 4],
            last: FrameReport::none(),
        }
    }

    /// The next frame can be drawn and sent while one refreshes.
    pub const fn overlapping(mut self) -> Self {
        self.flags |= panel_flags::OVERLAPS;
        self
    }

    /// A refresh in `mode` takes `time` at best (as a device says it, or
    /// as measured).
    pub fn refresh(mut self, mode: RefreshMode, time: Duration) -> Self {
        self.timed(mode, time);
        self
    }

    /// Notes that a refresh in `mode` took `time`: the fastest counts.
    #[cfg(feature = "kernel")]
    pub fn measured(&mut self, mode: RefreshMode, time: Duration) {
        let slot = &mut self.refresh_us[mode_code(mode)];
        let us = micros(time).max(1);
        *slot = if *slot == 0 { us } else { (*slot).min(us) };
    }

    fn timed(&mut self, mode: RefreshMode, time: Duration) {
        self.refresh_us[mode_code(mode)] = micros(time).max(1);
    }

    /// The frame just shown.
    #[cfg(feature = "kernel")]
    pub fn set_last(&mut self, frame: FrameReport) {
        self.last = frame;
    }

    /// The grays the panel shows (2: black and white).
    pub const fn gray_levels(&self) -> u8 {
        if self.gray_levels > 255 {
            255
        } else {
            self.gray_levels as u8
        }
    }

    /// Whether the next frame can be drawn and sent while one refreshes.
    pub const fn overlaps(&self) -> bool {
        self.flags & panel_flags::OVERLAPS != 0
    }

    /// How long a refresh in `mode` takes at best, once known.
    pub fn refresh_time(&self, mode: RefreshMode) -> Option<Duration> {
        match self.refresh_us[mode_code(mode)] {
            0 => None,
            us => Some(Duration::from_micros(u64::from(us))),
        }
    }

    /// The fastest refresh there is, and how long it takes, once known.
    pub fn fastest(&self) -> Option<(RefreshMode, Duration)> {
        (0..MODES as u8)
            .filter_map(|code| Some((mode_of(code)?, self.refresh_us[usize::from(code)])))
            .filter(|(_, us)| *us > 0)
            .min_by_key(|(_, us)| *us)
            .map(|(mode, us)| (mode, Duration::from_micros(u64::from(us))))
    }

    /// The last frame shown, if one was.
    pub fn last_frame(&self) -> Option<FrameReport> {
        (self.last.mode != NO_MODE).then_some(self.last)
    }
}

fn micros(duration: Duration) -> u32 {
    u32::try_from(duration.as_micros()).unwrap_or(u32::MAX)
}

/// No frame shown yet.
const NO_MODE: u8 = u8::MAX;

/// How a frame went: what was shown, in which mode, how long it took to
/// draw and to show ([`PanelInfo::last_frame`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameReport {
    area: Rect,
    mode: u8,
    /// Frames asked for since the one before, shown as this one.
    merged: u16,
    drawn_us: u32,
    shown_us: u32,
    at: Instant,
    /// Room for numbers to come.
    numbers: [u32; 2],
}

impl FrameReport {
    const fn none() -> Self {
        Self {
            area: Rect::new(0, 0, 0, 0),
            mode: NO_MODE,
            merged: 0,
            drawn_us: 0,
            shown_us: 0,
            at: Instant(0),
            numbers: [0; 2],
        }
    }

    /// A frame of `area` shown in `mode` at `at`, drawn (and sent) in
    /// `drawn` and on the panel for `shown`; `merged` frames asked for
    /// before it were shown as this one.
    #[cfg(feature = "kernel")]
    pub fn new(
        area: Rect,
        mode: RefreshMode,
        at: Instant,
        drawn: Duration,
        shown: Duration,
        merged: u16,
    ) -> Self {
        Self {
            area,
            mode: mode_code(mode) as u8,
            merged,
            drawn_us: micros(drawn),
            shown_us: micros(shown),
            at,
            numbers: [0; 2],
        }
    }

    /// The part of the screen shown.
    pub const fn area(&self) -> Rect {
        self.area
    }

    /// Its refresh mode, where it is one this SDK knows.
    pub fn mode(&self) -> Option<RefreshMode> {
        mode_of(self.mode)
    }

    /// How long it took to draw and send.
    pub const fn drawn(&self) -> Duration {
        Duration::from_micros(self.drawn_us as u64)
    }

    /// How long the panel took to show it.
    pub const fn shown(&self) -> Duration {
        Duration::from_micros(self.shown_us as u64)
    }

    /// When it was shown.
    pub const fn at(&self) -> Instant {
        self.at
    }

    /// Frames asked for meanwhile that were shown as this one (0: none
    /// were dropped).
    pub const fn merged(&self) -> u16 {
        self.merged
    }
}

/// A button pressed or let go, for screens that take raw keys
/// ([`Screen::on_key`](crate::Screen::on_key)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    name: &'static str,
    pressed: bool,
    at: Instant,
    /// Room for what more a key may say later (a repeat, a keyboard's
    /// modifiers).
    more: u32,
}

impl KeyEvent {
    /// Button `name` pressed at `at`.
    pub const fn pressed(name: &'static str, at: Instant) -> Self {
        Self {
            name,
            pressed: true,
            at,
            more: 0,
        }
    }

    /// Button `name` let go at `at`.
    pub const fn released(name: &'static str, at: Instant) -> Self {
        Self {
            name,
            pressed: false,
            at,
            more: 0,
        }
    }

    /// The button's name on the device
    /// ([`buttons`](paperpaw_platform::buttons)).
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Whether it went down (else it was let go).
    pub const fn is_press(&self) -> bool {
        self.pressed
    }

    /// When it happened.
    pub const fn at(&self) -> Instant {
        self.at
    }
}

/// The buttons held down now ([`Env::keys_held`](crate::Env::keys_held)).
#[derive(Debug, Clone, Copy, Default)]
pub struct KeysHeld<'a> {
    names: &'a [&'static str],
}

impl<'a> KeysHeld<'a> {
    pub(crate) const fn of(names: &'a [&'static str]) -> Self {
        Self { names }
    }

    /// Whether button `name` is held down.
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(&name)
    }

    /// Whether none is.
    pub const fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Their names, in the order they went down.
    pub fn iter(&self) -> impl Iterator<Item = &'static str> + 'a {
        self.names.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mode_says_what_was_asked() {
        let mode = ScreenMode::new();
        assert!(!mode.is_full_screen() && !mode.takes_raw_keys() && !mode.is_paced());
        assert_eq!(mode.frame_interval(), None);
        let mode = mode
            .full_screen()
            .raw_keys()
            .cleans_itself()
            .fastest()
            .grays(2)
            .frame_every(Duration::from_millis(40))
            .awake_for(Duration::from_secs(600));
        assert!(mode.is_full_screen() && mode.takes_raw_keys());
        assert!(mode.does_clean_itself() && mode.wants_fastest());
        assert_eq!(mode.grays_enough(), Some(2));
        assert_eq!(mode.frame_interval(), Some(Duration::from_millis(40)));
        assert_eq!(mode.awake(), Some(Duration::from_secs(600)));
    }

    #[test]
    fn a_panel_knows_its_fastest_refresh() {
        let panel = PanelInfo::new(2)
            .refresh(RefreshMode::Full, Duration::from_millis(1600))
            .refresh(RefreshMode::Fast, Duration::from_millis(420));
        assert_eq!(
            panel.fastest(),
            Some((RefreshMode::Fast, Duration::from_millis(420)))
        );
        assert_eq!(panel.refresh_time(RefreshMode::Partial), None);
        assert!(panel.last_frame().is_none());
        assert!(!panel.overlaps());
    }
}
