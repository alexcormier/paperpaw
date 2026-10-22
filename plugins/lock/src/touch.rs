//! The touch lock: while it is on, taps, swipes and the pen do nothing,
//! so that a hand resting on the screen turns no page; the buttons go on
//! working. Off by default.
//!
//! - One switch, `lock.touch`, in the quick panel and the reader's menu
//!   (and Settings › Controls), on touch screens only; *Lock touch*
//!   (`lock.touch-toggle`) switches it from any button or rule.
//! - An input filter takes every finger and pen before screens, gestures
//!   and key maps see it, on every screen; not while the screen lock asks
//!   for its code.
//! - A way out that needs no menu and that a resting hand does not make
//!   ([`Escape`]): one finger taps twice and then stays down; or, where
//!   PaperPaw is the firmware, the power button held. A small lock in the
//!   status lines says it is on.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{groups, menus};
use paperpaw_sdk::filters::{InputFilterSpec, InputVerdict};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::status::{StatusItemSpec, StatusValue};
use paperpaw_sdk::{
    ActionSpec, Command, Env, Id, InputEvent, Instant, MenuEntry, MenuNeed, Point, PointerPhase,
    RegisterError, Registrar, SettingSpec,
};

use crate::Shared;

/// Whether touch is locked.
pub const LOCKED: Setting<bool> = Setting::new("lock.touch");
/// Locks or unlocks touch.
pub const TOGGLE: Id = Id::new_static("lock.touch-toggle");
/// The lock in the status lines while touch is locked.
pub const STATUS: Id = Id::new_static("lock.touch-status");
const FILTER: Id = Id::new_static("lock.touch-filter");

/// The way out by touch, which a hand resting on the screen does not do:
/// one finger taps twice, then stays down for [`HOLD_MS`] at the same
/// spot, each tap shorter than [`TAP_MS`] and the next touch within
/// [`GAP_MS`]. A second finger on the screen starts it over.
pub const TAP_MS: u64 = 300;
pub const GAP_MS: u64 = 600;
pub const HOLD_MS: u64 = 1_500;
/// How far the finger may stray from the first tap, in pixels.
const SLOP: i32 = 40;

/// How far the way out has come.
#[derive(Debug, Default)]
struct Escape {
    /// The fingers down now, by id.
    down: Vec<u8>,
    /// The finger that may go on with it: its id, when and where it came
    /// down.
    finger: Option<(u8, Instant, Point)>,
    /// Taps so far, where the first was, when the last ended.
    taps: u8,
    first: Option<Point>,
    last_up: Option<Instant>,
}

impl Escape {
    fn over(&mut self) {
        self.finger = None;
        self.taps = 0;
        self.first = None;
        self.last_up = None;
    }

    fn near(&self, point: Point) -> bool {
        self.first
            .is_none_or(|at| (point.x - at.x).abs() <= SLOP && (point.y - at.y).abs() <= SLOP)
    }

    /// Follows `pointer` (a finger or the pen) at `now`: whether it
    /// completed the way out.
    fn follow(&mut self, now: Instant, pointer: &paperpaw_sdk::PointerEvent) -> bool {
        let id = pointer.id;
        match pointer.phase {
            PointerPhase::Down => {
                if !self.down.contains(&id) {
                    self.down.push(id);
                }
                let late = self.last_up.is_some_and(|up| now.since(up) > GAP_MS);
                if self.down.len() > 1 || late || !self.near(pointer.point) {
                    self.over();
                }
                if self.down.len() == 1 {
                    self.finger = Some((id, now, pointer.point));
                    self.first.get_or_insert(pointer.point);
                }
            }
            PointerPhase::Move => {
                if self.finger.is_some_and(|(f, ..)| f == id) && !self.near(pointer.point) {
                    self.over();
                }
            }
            PointerPhase::Up => {
                self.down.retain(|d| *d != id);
                let Some((f, at, _)) = self.finger.take() else {
                    return false;
                };
                if f != id {
                    self.over();
                    return false;
                }
                let held = now.since(at);
                if held < TAP_MS && self.taps < 2 {
                    self.taps += 1;
                    self.last_up = Some(now);
                } else if held >= HOLD_MS && self.taps == 2 {
                    self.down.clear();
                    self.over();
                    return true;
                } else {
                    self.over();
                }
            }
            // A cancel ends every touch: the Android shell sends it for one
            // pointer alone, and the others left would count as a second
            // finger from then on.
            _ => {
                self.down.clear();
                self.over();
            }
        }
        false
    }
}

/// The switch, with the controls.
pub(crate) static SETTINGS: [SettingSpec; 1] =
    [
        SettingSpec::fixed_switch(LOCKED.static_str(), "lock.touch", false, groups::CONTROLS)
            .described("lock.touch.help"),
    ];

/// While touch is locked, the power button held unlocks it.
const ESCAPE: &str = "[keymap.global]\nlong.power = lock.touch-toggle\n";

/// What the filter does with `event` while touch is locked: taken, or
/// the toggle once the way out is done.
fn locked(now: Instant, event: &InputEvent, escape: &mut Escape) -> InputVerdict {
    let InputEvent::Pointer(pointer) = event else {
        return InputVerdict::Pass;
    };
    if escape.follow(now, pointer) {
        return InputVerdict::Run(TOGGLE);
    }
    InputVerdict::Take
}

/// Adds the touch lock; `state` is the screen lock's, whose code entry
/// it leaves alone.
pub(crate) fn register(r: &mut Registrar, state: Shared) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.add_action(ActionSpec::new(TOGGLE, "lock.touch-toggle").handler(
        |env: &Env<'_>, out: &mut Vec<Command>| {
            let on = !env.settings.read(&LOCKED);
            out.push(Command::set(&LOCKED, on));
            let told = match (on, env.caps.firmware) {
                (true, true) => "lock.touch.locked.buttons",
                (true, false) => "lock.touch.locked",
                (false, _) => "lock.touch.unlocked",
            };
            out.push(Command::Notify(env.i18n.tr(told).into()));
        },
    ))?;
    r.add_icon(TOGGLE, Cow::Borrowed("lock"))?;
    let escape: Rc<RefCell<Escape>> = Rc::default();
    r.add_input_filter(InputFilterSpec::new(
        FILTER,
        "lock.touch",
        Box::new(move |env, event| {
            let mut escape = escape.borrow_mut();
            if !env.settings.read(&LOCKED) || state.locked.get() {
                *escape = Escape::default();
                return InputVerdict::Pass;
            }
            locked(env.now, event, &mut escape)
        }),
    ))?;
    r.add_keymap_when(&LOCKED, Cow::Borrowed(ESCAPE))?;
    r.add_status_item(StatusItemSpec::new(
        STATUS,
        "lock.touch-status",
        Box::new(|env, _| {
            env.settings
                .read(&LOCKED)
                .then_some(StatusValue::Icon(Cow::Borrowed("lock")))
        }),
    ))?;
    // Beside night mode in the quick panel.
    for (menu, order) in [(menus::QUICK, 11), (menus::READER, 60)] {
        r.add_menu_entry(MenuEntry::new(menu, LOCKED.id().clone()).order(order))?;
    }
    r.add_icon(LOCKED.id().clone(), Cow::Borrowed("lock"))?;
    // Only where there is touch to lock.
    r.set_menu_need(LOCKED.id().clone(), MenuNeed::Touch)?;
    r.set_menu_need(TOGGLE, MenuNeed::Touch)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{PointerEvent, PointerTool};

    fn finger(id: u8, phase: PointerPhase, (x, y): (i32, i32)) -> InputEvent {
        InputEvent::Pointer(PointerEvent {
            id,
            phase,
            point: Point::new(x, y),
            tool: PointerTool::Finger,
            pressure: 1000,
        })
    }

    /// Runs `steps` (when, which finger, what, where): the verdicts.
    fn run(steps: &[(u64, u8, PointerPhase, (i32, i32))]) -> Vec<InputVerdict> {
        let mut escape = Escape::default();
        steps
            .iter()
            .map(|&(at, id, phase, point)| {
                locked(Instant(at), &finger(id, phase, point), &mut escape)
            })
            .collect()
    }

    use PointerPhase::{Cancel, Down, Move, Up};
    const AT: (i32, i32) = (200, 300);

    #[test]
    fn two_taps_then_a_hold_unlock() {
        let verdicts = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, (210, 305)),
            (500, 0, Up, (210, 305)),
            (800, 0, Down, AT),
            (800 + HOLD_MS, 0, Up, AT),
        ]);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
        assert!(verdicts[..5].iter().all(|v| *v == InputVerdict::Take));
    }

    #[test]
    fn a_resting_hand_does_not_unlock() {
        // A palm still for a long while, then lifted.
        assert!(
            !run(&[(0, 0, Down, AT), (10_000, 0, Up, AT)]).contains(&InputVerdict::Run(TOGGLE))
        );
        // A palm and a thumb: one lifts after the other's taps and hold.
        let verdicts = run(&[
            (0, 0, Down, AT),
            (100, 1, Down, (300, 600)),
            (150, 1, Up, (300, 600)),
            (400, 1, Down, (300, 600)),
            (450, 1, Up, (300, 600)),
            (700, 1, Down, (300, 600)),
            (700 + HOLD_MS, 1, Up, (300, 600)),
            (5_000, 0, Up, AT),
        ]);
        assert!(
            !verdicts.contains(&InputVerdict::Run(TOGGLE)),
            "{verdicts:?}"
        );
    }

    #[test]
    fn taps_too_slow_or_too_far_apart_do_not_unlock() {
        let slow = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (100 + GAP_MS + 1, 0, Down, AT),
            (200 + GAP_MS, 0, Up, AT),
            (500 + GAP_MS, 0, Down, AT),
            (500 + GAP_MS + HOLD_MS, 0, Up, AT),
        ]);
        assert!(!slow.contains(&InputVerdict::Run(TOGGLE)));
        let far = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, (400, 700)),
            (500, 0, Up, (400, 700)),
            (800, 0, Down, (400, 700)),
            (800 + HOLD_MS, 0, Up, (400, 700)),
        ]);
        assert!(!far.contains(&InputVerdict::Run(TOGGLE)));
        let moved = run(&[
            (0, 0, Down, AT),
            (100, 0, Up, AT),
            (400, 0, Down, AT),
            (500, 0, Up, AT),
            (800, 0, Down, AT),
            (900, 0, Move, (350, 300)),
            (800 + HOLD_MS, 0, Up, (350, 300)),
        ]);
        assert!(!moved.contains(&InputVerdict::Run(TOGGLE)));
    }

    #[test]
    fn a_cancel_for_one_finger_ends_them_all() {
        // Two fingers down, the gesture cancelled for one of them alone:
        // the way out works afterwards all the same.
        let verdicts = run(&[
            (0, 0, Down, AT),
            (10, 1, Down, (300, 600)),
            (20, 0, Cancel, AT),
            (1_000, 2, Down, AT),
            (1_100, 2, Up, AT),
            (1_400, 2, Down, AT),
            (1_500, 2, Up, AT),
            (1_800, 2, Down, AT),
            (1_800 + HOLD_MS, 2, Up, AT),
        ]);
        assert_eq!(verdicts.last(), Some(&InputVerdict::Run(TOGGLE)));
    }

    #[test]
    fn buttons_pass() {
        let mut escape = Escape::default();
        let press = InputEvent::Button {
            name: "page-forward",
            state: paperpaw_sdk::ButtonState::Pressed,
        };
        assert_eq!(locked(Instant(0), &press, &mut escape), InputVerdict::Pass);
    }
}
