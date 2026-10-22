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
//! - A way out that needs no menu: a finger held still for
//!   [`HOLD_MS`] anywhere, or, where PaperPaw is the firmware, the power
//!   button held. A small lock in the status lines says it is on.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::Cell;

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

/// How long a finger held still unlocks touch, in milliseconds: far
/// longer than a hand brushing the screen.
pub const HOLD_MS: u64 = 3_000;
/// How far the finger may move meanwhile, in pixels.
const HOLD_SLOP: i32 = 40;

/// The switch, with the controls.
pub(crate) static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_switch(
    LOCKED.static_str(),
    "lock.touch",
    false,
    groups::CONTROLS,
)
.described("lock.touch.help")];

/// While touch is locked, the power button held unlocks it.
const ESCAPE: &str = "[keymap.global]\nlong.power = lock.touch-toggle\n";

/// What the filter does with `event` while touch is locked, `held` the
/// finger put down (when, where) while it may still unlock: taken, or the
/// toggle once a finger held still long enough is lifted.
fn locked(now: Instant, event: &InputEvent, held: &Cell<Option<(Instant, Point)>>) -> InputVerdict {
    let InputEvent::Pointer(pointer) = event else {
        return InputVerdict::Pass;
    };
    match pointer.phase {
        PointerPhase::Down => held.set(Some((now, pointer.point))),
        PointerPhase::Move => {
            let far = held.get().is_some_and(|(_, at)| {
                (pointer.point.x - at.x).abs() > HOLD_SLOP
                    || (pointer.point.y - at.y).abs() > HOLD_SLOP
            });
            if far {
                held.set(None);
            }
        }
        PointerPhase::Up => {
            if held
                .take()
                .is_some_and(|(at, _)| now.since(at) >= HOLD_MS)
            {
                return InputVerdict::Run(TOGGLE);
            }
        }
        _ => held.set(None),
    }
    InputVerdict::Take
}

/// Adds the touch lock; `state` is the screen lock's, whose code entry
/// it leaves alone.
pub(crate) fn register(r: &mut Registrar, state: Shared) -> Result<(), RegisterError> {
    r.add_settings(&SETTINGS)?;
    r.add_action(
        ActionSpec::new(TOGGLE, "lock.touch-toggle").handler(
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
        ),
    )?;
    r.add_icon(TOGGLE, Cow::Borrowed("lock"))?;
    let held: Rc<Cell<Option<(Instant, Point)>>> = Rc::new(Cell::new(None));
    r.add_input_filter(InputFilterSpec::new(
        FILTER,
        "lock.touch",
        Box::new(move |env, event| {
            if !env.settings.read(&LOCKED) || state.locked.get() {
                held.set(None);
                return InputVerdict::Pass;
            }
            locked(env.now, event, &held)
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

    fn finger(phase: PointerPhase, x: i32, y: i32) -> InputEvent {
        InputEvent::Pointer(PointerEvent {
            id: 0,
            phase,
            point: Point::new(x, y),
            tool: PointerTool::Finger,
            pressure: 1000,
        })
    }

    #[test]
    fn a_finger_held_still_unlocks_a_tap_does_not() {
        let held = Cell::new(None);
        let at = Instant;
        // A tap: taken, nothing more.
        let down = finger(PointerPhase::Down, 100, 100);
        assert_eq!(locked(at(0), &down, &held), InputVerdict::Take);
        let up = finger(PointerPhase::Up, 100, 100);
        assert_eq!(locked(at(80), &up, &held), InputVerdict::Take);
        // Held still three seconds: the toggle.
        locked(at(1000), &down, &held);
        locked(at(2000), &finger(PointerPhase::Move, 110, 95), &held);
        let lifted = finger(PointerPhase::Up, 110, 95);
        assert_eq!(locked(at(1000 + HOLD_MS), &lifted, &held), InputVerdict::Run(TOGGLE));
        // Moved away meanwhile (a swipe, a hand sliding): not.
        locked(at(9000), &down, &held);
        locked(at(9500), &finger(PointerPhase::Move, 300, 100), &held);
        let away = finger(PointerPhase::Up, 300, 100);
        assert_eq!(locked(at(9000 + HOLD_MS + 1), &away, &held), InputVerdict::Take);
    }

    #[test]
    fn buttons_pass() {
        let held = Cell::new(None);
        let press = InputEvent::Button {
            name: "page-forward",
            state: paperpaw_sdk::ButtonState::Pressed,
        };
        assert_eq!(locked(Instant(0), &press, &held), InputVerdict::Pass);
    }
}
