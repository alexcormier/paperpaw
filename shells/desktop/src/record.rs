//! Recording in the window (`--record <file>`): what is pressed, tapped
//! and swiped, written as the steps of a flow (one a line, as
//! `paperpaw-dev flows` replays them, see `tools/dev/src/flows.rs`), with
//! `wait:` for the pauses between them.
//!
//! A tap on a row, button or heading is written as `tap-on:` with the
//! beginning of its text, if that finds it, so the flow still works
//! where the screen is laid out a little differently; elsewhere as
//! `tap:x:y`. Steps are written at once, so closing the window keeps them.

use std::io::Write;
use std::path::Path;
use std::time::Instant as StdInstant;

use paperpaw_core::sdk::semantics::{self, Part, Role};
use paperpaw_platform::{Point, PointerPhase};

use paperpaw_emulator::device::Device;

/// Pauses shorter than this are not written (a step itself takes time).
const PAUSE_MS: u64 = 500;
/// A finger that moves further than this swipes.
const SWIPE_PX: i32 = 24;
/// The kernel's long press if the setting is not there.
const LONG_MS: u64 = 500;

/// Writes the steps of what is done in the window.
pub struct Recorder<W: Write> {
    out: W,
    /// When the last step ended.
    last: Option<StdInstant>,
    /// The button held, since when.
    button: Option<(&'static str, StdInstant)>,
    /// The finger down: where, the step a tap there would be, since when;
    /// and where it is now.
    finger: Option<(Point, String, StdInstant, Point)>,
}

impl Recorder<std::fs::File> {
    /// Starts recording into `path`, for the device `profile`.
    pub fn create(path: &Path, profile: &str) -> Result<Self, String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut recorder = Recorder::new(file);
        let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
        recorder.line(&format!(
            "# Recorded with paperpaw-emu on {profile}, {date}."
        ))?;
        // Taps are where they were on this device: replayed there.
        recorder.line(&format!("profiles: {profile}"))?;
        recorder.line("")?;
        Ok(recorder)
    }
}

impl<W: Write> Recorder<W> {
    pub fn new(out: W) -> Self {
        Recorder {
            out,
            last: None,
            button: None,
            finger: None,
        }
    }

    fn line(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.out, "{line}")
            .and_then(|()| self.out.flush())
            .map_err(|e| format!("recording: {e}"))
    }

    /// Writes `step`, begun at `began`, ended now, after a `wait:` for the
    /// pause before it.
    fn step(&mut self, step: &str, began: StdInstant, now: StdInstant) {
        if let Some(wait) = self
            .last
            .and_then(|last| wait_step(millis(began.saturating_duration_since(last))))
        {
            self.report(wait);
        }
        self.report(step.to_string());
        self.last = Some(now);
    }

    fn report(&mut self, line: String) {
        if let Err(error) = self.line(&line) {
            eprintln!("{error}");
        }
    }

    /// A button was pressed or let go.
    pub fn button(&mut self, name: &'static str, pressed: bool, device: &Device) {
        self.button_at(name, pressed, long_ms(device), StdInstant::now());
    }

    fn button_at(&mut self, name: &'static str, pressed: bool, long: u64, now: StdInstant) {
        if pressed {
            self.button = Some((name, now));
            return;
        }
        if let Some((held, since)) = self.button.take()
            && held == name
        {
            let step = press_step(name, millis(now.saturating_duration_since(since)), long);
            self.step(&step, since, now);
        }
    }

    /// The finger did `phase` at `point` (before the device was told, so
    /// a tap is named by what it lands on).
    pub fn pointer(&mut self, phase: PointerPhase, point: Point, device: &mut Device) {
        let now = StdInstant::now();
        match phase {
            PointerPhase::Down => {
                let parts = device.kernel.describe();
                let tap = tap_step(&parts, point);
                self.finger = Some((point, tap, now, point));
            }
            PointerPhase::Move => {
                if let Some(finger) = &mut self.finger {
                    finger.3 = point;
                }
            }
            PointerPhase::Up => {
                if let Some((start, tap, since, _)) = self.finger.take() {
                    let held = millis(now.saturating_duration_since(since));
                    let step = touch_step(start, point, &tap, held, long_ms(device));
                    self.step(&step, since, now);
                }
            }
            _ => self.finger = None,
        }
    }
}

fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn long_ms(device: &Device) -> u64 {
    u64::try_from(device.kernel.settings().int("core.long-press-ms"))
        .ok()
        .filter(|ms| *ms > 0)
        .unwrap_or(LONG_MS)
}

/// A `wait:` for a pause of `millis`, in tenths of a second, if it is
/// long enough to matter.
fn wait_step(millis: u64) -> Option<String> {
    (millis >= PAUSE_MS).then(|| format!("wait:{}", millis / 100 * 100))
}

/// A press of `name` held `held` ms.
fn press_step(name: &str, held: u64, long: u64) -> String {
    if held >= long {
        format!("long:{name}")
    } else {
        name.to_string()
    }
}

/// What a finger put down at `start` and lifted at `end` after `held` ms
/// did; `tap` is the step a tap at `start` is.
fn touch_step(start: Point, end: Point, tap: &str, held: u64, long: u64) -> String {
    if (end.x - start.x).abs() > SWIPE_PX || (end.y - start.y).abs() > SWIPE_PX {
        format!("swipe:{}:{}:{}:{}", start.x, start.y, end.x, end.y)
    } else if held >= long {
        format!("long-tap:{}:{}", start.x, start.y)
    } else {
        tap.to_string()
    }
}

/// A tap at `point`: on the topmost row, button or heading there by its
/// text up to the first comma (a row's text is that of the texts in it,
/// joined with commas) if that finds it, else at the point.
fn tap_step(parts: &[Part], point: Point) -> String {
    let at = |p: &&Part| {
        let a = p.area;
        point.x >= i32::from(a.x)
            && point.y >= i32::from(a.y)
            && point.x < i32::from(a.x) + i32::from(a.width)
            && point.y < i32::from(a.y) + i32::from(a.height)
    };
    let target = parts.iter().rev().filter(at).find(|p| {
        matches!(
            p.role,
            Role::Row | Role::Button | Role::Switch | Role::Slider | Role::Heading
        ) && !p.label.trim().is_empty()
    });
    if let Some(part) = target {
        let label = part.label.split(',').next().unwrap_or_default().trim();
        if semantics::find(parts, label).is_some_and(|found| found.area == part.area) {
            return format!("tap-on:{label}");
        }
    }
    format!("tap:{}:{}", point.x, point.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::Rect;
    use std::time::Duration;

    fn part(role: Role, label: &str, area: Rect) -> Part {
        Part::new(role, label.to_string(), area)
    }

    #[test]
    fn presses_are_short_or_long() {
        assert_eq!(press_step("confirm", 80, 500), "confirm");
        assert_eq!(press_step("confirm", 700, 500), "long:confirm");
        assert_eq!(wait_step(300), None);
        assert_eq!(wait_step(1260).as_deref(), Some("wait:1200"));
    }

    #[test]
    fn taps_name_what_they_land_on() {
        let parts = [
            part(Role::Heading, "Settings", Rect::new(0, 0, 480, 60)),
            part(
                Role::Row,
                "Night mode, Inverts the screen",
                Rect::new(0, 100, 480, 60),
            ),
            part(Role::Row, "Night light", Rect::new(0, 160, 480, 60)),
            part(Role::Text, "10:41", Rect::new(0, 700, 100, 30)),
        ];
        assert_eq!(tap_step(&parts, Point::new(30, 120)), "tap-on:Night mode");
        assert_eq!(tap_step(&parts, Point::new(30, 170)), "tap-on:Night light");
        assert_eq!(tap_step(&parts, Point::new(30, 710)), "tap:30:710");
        let start = Point::new(400, 400);
        assert_eq!(
            touch_step(start, Point::new(100, 410), "tap:400:400", 150, 500),
            "swipe:400:400:100:410"
        );
        assert_eq!(
            touch_step(start, Point::new(405, 402), "tap:400:400", 900, 500),
            "long-tap:400:400"
        );
    }

    #[test]
    fn steps_follow_with_pauses() {
        let mut recorder = Recorder::new(Vec::new());
        let t = StdInstant::now();
        recorder.button_at("right", true, 500, t);
        recorder.button_at("right", false, 500, t + Duration::from_millis(80));
        recorder.button_at("confirm", true, 500, t + Duration::from_millis(2100));
        recorder.button_at("confirm", false, 500, t + Duration::from_millis(2900));
        let text = String::from_utf8(recorder.out).unwrap();
        assert_eq!(text, "right\nwait:2000\nlong:confirm\n");
    }
}
