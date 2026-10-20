//! Inputs of more kinds (SDK gap E1): double presses and taps, taps of
//! two fingers, a layer's own zones, inputs raised by plugins, key map
//! layers put in front, filters that cancel or replace, plugins' own
//! recognizers.

use super::*;
use paperpaw_sdk::keys::{GestureRecognizer, GestureSpec, InputMode, InputSpec, Recognition};
use paperpaw_sdk::{Gesture, GestureKind};

/// A kernel showing plugin `a`'s screen, with its key map `keymap`.
fn kernel_with(log: &Log, keymap: &'static str) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin {
            keymap: Some(keymap),
            ..TestPlugin::new("a", log)
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

/// Finger `id` going down or lifted at `at`.
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

/// A press bound twice waits for a second: two make the double press,
/// one alone is a press once the time for a second is over.
#[test]
fn a_button_pressed_twice_is_a_double_press() {
    let log = log();
    let mut kernel = kernel_with(&log, "[keymap.a.screen]\ndouble.confirm = a.special\n");
    press(&mut kernel, 1000, "confirm");
    assert!(log.borrow().is_empty(), "the first press waits");
    press(&mut kernel, 1150, "confirm");
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    let effects = press(&mut kernel, 3000, "confirm");
    assert!(log.borrow().is_empty());
    // Held for a long press, it counts as it is let go.
    let wake = effects.wake_at.unwrap();
    assert!(wake <= Instant(3350), "{wake:?}");
    kernel.tick(wake);
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    // Another button meanwhile: the press waiting comes first.
    log.borrow_mut().clear();
    press(&mut kernel, 5000, "confirm");
    press(&mut kernel, 5100, "back");
    assert_eq!(log.borrow()[0], "a:core.confirm");
}

/// Two taps close together: a double tap where one is bound, else two
/// taps at once, so that pages turn as fast as they are tapped.
#[test]
fn two_quick_taps_are_a_double_tap_where_one_is_bound() {
    let log = log();
    let keymap = "[keymap.a.screen]\ndouble-tap.center = a.special\ntap.center = core.confirm\ntap.left = core.confirm\n";
    let mut kernel = kernel_with(&log, keymap);
    tap(&mut kernel, 1000, 45, 45);
    assert!(log.borrow().is_empty(), "the first tap waits");
    tap(&mut kernel, 1150, 46, 44);
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 3000, 5, 45);
    tap(&mut kernel, 3150, 5, 45);
    assert_eq!(*log.borrow(), ["a:core.confirm", "a:core.confirm"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 5000, 45, 45);
    kernel.tick(Instant(5500));
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
}

/// Logs the gestures it gets; takes double taps where told to (`None`:
/// says it takes none).
struct Taps {
    log: Log,
    doubles: Option<bool>,
}

impl Screen for Taps {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn takes_double_taps(&self) -> bool {
        self.doubles.is_some()
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let taken = match gesture.kind {
            GestureKind::Tap => true,
            GestureKind::DoubleTap => self.doubles == Some(true),
            _ => false,
        };
        if taken {
            let kind = alloc::format!("{:?}", gesture.kind);
            self.log.borrow_mut().push(kind);
        }
        if taken { Handled::Yes } else { Handled::No }
    }
}

struct TapsPlugin(Log, Option<bool>);

impl Plugin for TapsPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("taps"), "Taps", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (log, doubles) = (self.0.clone(), self.1);
        r.add_screen(ScreenSpec::new(
            Id::new_static("taps.screen"),
            "taps",
            Box::new(move |_, _| {
                Box::new(Taps {
                    log: log.clone(),
                    doubles,
                })
            }),
        ))?;
        r.bind_slot(
            slots::HOME,
            Id::new_static("taps.screen"),
            Priority::DEFAULT,
        )
    }
}

/// A screen that takes double taps gets the second tap as one, as a tap
/// where it does not take it; other screens get taps.
#[test]
fn screens_get_double_taps_or_taps() {
    let cases = [
        (Some(true), ["Tap", "DoubleTap"]),
        (Some(false), ["Tap", "Tap"]),
        (None, ["Tap", "Tap"]),
    ];
    for (doubles, seen) in cases {
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(TapsPlugin(log.clone(), doubles))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        tap(&mut kernel, 1000, 45, 45);
        tap(&mut kernel, 1150, 45, 45);
        assert_eq!(*log.borrow(), seen);
    }
}

/// Two fingers tapped together run `tap2`; taps in a layer's own zone
/// run what it binds there, before the grid's.
#[test]
fn two_fingers_and_own_zones() {
    let log = log();
    let keymap = "[zones.a.screen]\ncorner = 80 0 20 20\n[keymap.a.screen]\ntap2 = a.special\ntap.corner = core.confirm\n";
    let mut kernel = kernel_with(&log, keymap);
    finger(&mut kernel, 1000, 1, PointerPhase::Down, (30, 45));
    finger(&mut kernel, 1010, 2, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1080, 1, PointerPhase::Up, (30, 45));
    finger(&mut kernel, 1090, 2, PointerPhase::Up, (60, 45));
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    tap(&mut kernel, 2000, 85, 5);
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    // Outside the corner, the grid's: the top right turns the page.
    log.borrow_mut().clear();
    tap(&mut kernel, 4000, 85, 25);
    assert_eq!(*log.borrow(), ["a:core.next-page"]);
}

/// Inputs raised by command run what they are bound to; a layer put in
/// front binds before the slot's and the global one.
#[test]
fn inputs_raised_and_layers_put_in_front() {
    let log = log();
    let mut kernel = kernel_with(&log, "[keymap.book.mine]\nbutton.confirm = a.special\n");
    kernel.apply(Command::Input(String::from("button.confirm")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    log.borrow_mut().clear();
    kernel.apply(Command::SetKeyLayer(Some(Id::new_static("book.mine"))));
    kernel.apply(Command::Input(String::from("button.confirm")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    kernel.apply(Command::SetKeyLayer(None));
    press(&mut kernel, 2000, "confirm");
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    kernel.apply(Command::Input(String::from("no.such-input")));
    kernel.finish();
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("no.such-input"))
    );
}

/// Cancels the back button and the second finger; makes the menu button
/// confirm.
struct Filters;

impl Plugin for Filters {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("filters"), "Filters", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::filters::{InputFilterSpec, InputVerdict};
        r.add_input_filter(InputFilterSpec::new(
            Id::new_static("filters.input"),
            "",
            Box::new(|_, event| match *event {
                InputEvent::Button { name: "back", .. } => InputVerdict::Cancel,
                InputEvent::Button {
                    name: "menu",
                    state,
                } => InputVerdict::Replace(InputEvent::Button {
                    name: "confirm",
                    state,
                }),
                InputEvent::Pointer(p) if p.id == 2 && p.phase == PointerPhase::Move => {
                    InputVerdict::Cancel
                }
                _ => InputVerdict::Pass,
            }),
        ))
    }
}

/// A filter cancels an input: nothing comes of it, nothing waits for a
/// long press. It replaces one: the other runs. What the reader used last
/// is known.
#[test]
fn filters_cancel_and_replace_inputs() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .plugin(Filters)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    press(&mut kernel, 1000, "back");
    press(&mut kernel, 1200, "menu");
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
    assert_eq!(kernel.shared.last_input, InputMode::Buttons);
    // A finger held, cancelled as it moves: no long press comes of it.
    log.borrow_mut().clear();
    finger(&mut kernel, 2000, 2, PointerPhase::Down, (45, 45));
    finger(&mut kernel, 2100, 2, PointerPhase::Move, (45, 46));
    kernel.tick(Instant(4000));
    finger(&mut kernel, 4100, 2, PointerPhase::Up, (45, 46));
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
    assert_eq!(kernel.shared.last_input, InputMode::Touch);
}

/// Recognizes a flick off the right side, and a finger held a second.
#[derive(Default)]
struct Flick {
    held_until: Option<Instant>,
}

impl GestureRecognizer for Flick {
    fn pointer(&mut self, event: &PointerEvent, now: Instant) -> Recognition {
        match event.phase {
            PointerPhase::Down => {
                self.held_until = Some(now.add_millis(1000));
                Recognition::Nothing
            }
            PointerPhase::Up => {
                self.held_until = None;
                if event.point.x > 80 {
                    Recognition::Recognized
                } else {
                    Recognition::Nothing
                }
            }
            _ => Recognition::Nothing,
        }
    }

    fn tick(&mut self, _: Instant) -> Recognition {
        self.held_until = None;
        Recognition::Recognized
    }

    fn next_tick(&self) -> Option<Instant> {
        self.held_until
    }
}

struct FlickPlugin;

impl Plugin for FlickPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("g"), "Gestures", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_gesture(GestureSpec::new(
            Id::new_static("g.flick"),
            "g.flick",
            Box::new(|| Box::new(Flick::default())),
        ))?;
        r.add_input(InputSpec::new(Id::new_static("g.chord"), "g.chord"))?;
        r.add_input(
            InputSpec::new(Id::new_static("g.light"), "g.light")
                .need(paperpaw_sdk::MenuNeed::Frontlight),
        )?;
        r.add_keymap(Cow::Borrowed(
            "[keymap.global]\ngesture.g.flick = a.special\ninput.g.chord = core.confirm\n",
        ))
    }
}

/// A plugin's recognizer sees the fingers beside the kernel and runs
/// what its gesture is bound to, also when the time it asked for comes;
/// a plugin's input is raised by command. Inputs the device cannot make
/// are not offered.
#[test]
fn plugins_recognize_gestures_and_add_inputs() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(TestPlugin::new("a", &log))
        .plugin(FlickPlugin)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let ids: Vec<&str> = kernel.shared.inputs.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["g.chord"]);
    finger(&mut kernel, 1000, 0, PointerPhase::Down, (60, 45));
    finger(&mut kernel, 1100, 0, PointerPhase::Up, (85, 45));
    assert_eq!(*log.borrow(), ["a:a.special"]);
    log.borrow_mut().clear();
    finger(&mut kernel, 2000, 0, PointerPhase::Down, (45, 45));
    let effects = kernel.tick(Instant(2100));
    assert!(effects.wake_at.is_some_and(|at| at <= Instant(3000)));
    kernel.tick(Instant(3000));
    assert!(
        log.borrow().contains(&String::from("a:a.special")),
        "{:?}",
        log.borrow()
    );
    finger(&mut kernel, 3100, 0, PointerPhase::Up, (45, 45));
    log.borrow_mut().clear();
    kernel.apply(Command::Input(String::from("input.g.chord")));
    kernel.finish();
    assert_eq!(*log.borrow(), ["a:core.confirm"]);
}
