//! Input recognition: presses dropped while a frame was not shown yet,
//! long presses, fingers resting together, the edges.

use super::*;

/// A button going down or up at `at`.
fn button(kernel: &mut Kernel, at: u64, name: &'static str, state: ButtonState) {
    kernel.handle(Instant(at), InputEvent::Button { name, state });
}

/// A finger going down, moving or lifted at `at`.
fn finger(kernel: &mut Kernel, at: u64, id: u8, phase: PointerPhase, (x, y): (i32, i32)) {
    let event = PointerEvent {
        id,
        phase,
        point: Point::new(x, y),
        tool: PointerTool::Finger,
        pressure: 1000,
    };
    kernel.handle(Instant(at), InputEvent::Pointer(event));
}

/// Whether nothing is held: no button and no finger waits for its long
/// press.
fn nothing_held(kernel: &Kernel) -> bool {
    kernel
        .input
        .recognizer
        .next_deadline(&kernel.thresholds())
        .is_none()
}

/// A kernel whose home screen showed at 1000: presses made before are
/// dropped.
fn shown_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel.shown(Instant(1000));
    kernel
}

#[test]
fn a_dropped_press_pressed_again_after_the_frame_does_not_stick() {
    let log = log();
    let mut kernel = shown_kernel(&log);
    // Pressed while the screen came up: dropped. Pressed again once it
    // showed (its release missed, or the press came twice): taken, and
    // let go it is a short press, not one held on to a long press.
    button(&mut kernel, 500, "back", ButtonState::Pressed);
    button(&mut kernel, 1200, "back", ButtonState::Pressed);
    button(&mut kernel, 1250, "back", ButtonState::Released);
    kernel.tick(Instant(3000));
    assert_eq!(*log.borrow(), ["a:core.back"]);
    assert!(nothing_held(&kernel));
}

#[test]
fn a_held_press_pressed_again_before_the_frame_keeps_its_release() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Pressed before anything was dropped; the press comes again, late,
    // from before the screen showed: the release still ends the press.
    button(&mut kernel, 900, "back", ButtonState::Pressed);
    kernel.shown(Instant(1000));
    button(&mut kernel, 950, "back", ButtonState::Pressed);
    button(&mut kernel, 1100, "back", ButtonState::Released);
    kernel.tick(Instant(3000));
    assert_eq!(*log.borrow(), ["a:core.back"]);
    assert!(nothing_held(&kernel));
}

#[test]
fn a_press_dropped_after_the_clock_went_back_does_not_stick() {
    let log = log();
    let mut kernel = shown_kernel(&log);
    kernel.shown(Instant(2000));
    button(&mut kernel, 5000, "back", ButtonState::Pressed);
    // The clock went back: the press again, then its release, seem older
    // than the frames shown.
    button(&mut kernel, 100, "back", ButtonState::Pressed);
    button(&mut kernel, 150, "back", ButtonState::Released);
    kernel.tick(Instant(9000));
    assert!(!log.borrow().iter().any(|l| l == "a:core.home"));
    assert!(nothing_held(&kernel));
}

#[test]
fn a_finger_down_again_before_the_frame_does_not_stick() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Down before anything was dropped; down again (its lift missed)
    // from before the screen showed: dropped, and the first touch is
    // over, no long press of it later.
    finger(&mut kernel, 900, 0, PointerPhase::Down, (45, 45));
    kernel.shown(Instant(1000));
    finger(&mut kernel, 950, 0, PointerPhase::Down, (45, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (45, 45));
    kernel.tick(Instant(3000));
    assert!(!kernel.settings().read(&settings::NIGHT_MODE));
    assert!(nothing_held(&kernel));
}

/// A small generator of pseudo random numbers, the same every run.
struct Dice(u32);

impl Dice {
    fn roll(&mut self, sides: u32) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.0 >> 16) % sides
    }
}

#[test]
fn any_order_of_presses_leaves_the_buttons_working() {
    let mut dice = Dice(7);
    for _ in 0..300 {
        let log = log();
        let mut kernel = shown_kernel(&log);
        // Presses and releases of two buttons in any order, at times
        // going back and forth around frames shown.
        for _ in 0..dice.roll(10) {
            let at = u64::from(dice.roll(40)) * 100;
            let name = ["back", "confirm"][dice.roll(2) as usize];
            match dice.roll(5) {
                0 | 1 => button(&mut kernel, at, name, ButtonState::Pressed),
                2 | 3 => button(&mut kernel, at, name, ButtonState::Released),
                _ => kernel.shown(Instant(at)),
            }
        }
        // Pressed and let go after all that: nothing stays held.
        for name in ["back", "confirm"] {
            button(&mut kernel, 10_000, name, ButtonState::Pressed);
            button(&mut kernel, 10_050, name, ButtonState::Released);
        }
        assert!(nothing_held(&kernel));
        assert!(kernel.input.dropped_buttons.is_empty());
    }
}

#[test]
fn two_fingers_resting_wake_the_shell_no_more() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    // Two fingers down and held: no long press, so nothing to wake for.
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (30, 45));
    finger(&mut kernel, 1010, 1, PointerPhase::Down, (60, 45));
    for at in [1600, 2000, 5000] {
        let wake = kernel.tick(Instant(at)).wake_at();
        assert!(wake.is_none_or(|w| w > Instant(at)), "{wake:?} at {at}");
    }
    // One lifted, the other rests on: still no long press of it.
    finger(&mut kernel, 5100, 1, PointerPhase::Up, (60, 45));
    kernel.tick(Instant(6000));
    assert!(!kernel.settings().read(&settings::NIGHT_MODE));
    assert!(nothing_held(&kernel));
}
