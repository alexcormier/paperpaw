//! The screen stack as plugins read and steer it:
//! `Env::stack`, closing a screen of one's own, replacing, going back to
//! a screen, slots chosen for the user, asking before closing, taps
//! outside, input passed through, a screen's own row of hints.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{CloseCause, Gesture, Hints, Outside, Params};

const SCREEN: Id = Id::new_static("s.screen");
const HOME: Id = Id::new_static("s.home");
const CLOSE: Id = Id::new_static("s.close");
const WHO: Id = Id::new_static("s.who");

/// A screen that does what its params say: `name` (logged), `overlay`
/// (the lower half), `refuse` (to close), `outside` (`ignore`, `pass`),
/// `pass` (input through), `hints` (`hidden`).
struct Configured {
    log: Log,
    params: Params,
}

impl Configured {
    fn name(&self) -> &str {
        self.params.get("name").unwrap_or("?")
    }

    fn says(&self, key: &str) -> Option<&str> {
        self.params.get(key)
    }

    fn note(&self, what: &str) {
        let line = alloc::format!("{} {what}", self.name());
        self.log.borrow_mut().push(line);
    }
}

impl Screen for Configured {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        self.says("overlay").map(|_| Rect::new(0, 45, 90, 45))
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == CLOSE {
            cx.close();
        } else if *action == WHO {
            let slot = cx.slot().map_or("-", Id::as_str);
            let line = alloc::format!("is {} for {slot}", cx.screen_id().unwrap());
            self.note(&line);
        } else if *action == actions::NEXT_PAGE && self.says("pass").is_none() {
            self.note(&alloc::format!("action {action}"));
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        self.note(&alloc::format!("gesture {:?}", gesture.kind));
        Handled::Yes
    }

    fn outside(&self) -> Outside {
        match self.says("outside") {
            Some("ignore") => Outside::Ignore,
            Some("pass") => Outside::PassThrough,
            _ => Outside::Close,
        }
    }

    fn passes_through(&self) -> bool {
        self.says("pass").is_some()
    }

    fn hints(&self) -> Option<Hints> {
        self.says("hints").map(|_| Hints::Hidden)
    }

    fn on_close_request(&mut self, why: CloseCause, _: &mut Cx<'_, '_>) -> Handled {
        if self.says("refuse").is_some() {
            self.note(&alloc::format!("keeps open on {why:?}"));
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_show(&mut self, _: &mut Cx<'_, '_>) {
        self.note("show");
    }

    fn on_close(&mut self, _: &mut Cx<'_, '_>) {
        self.note("close");
    }
}

struct StackPlugin(Log);

impl Plugin for StackPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("s"), "Stack", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, home) in [(SCREEN, false), (HOME, true)] {
            let log = self.0.clone();
            r.add_screen(ScreenSpec::new(
                id,
                "screen",
                Box::new(move |_, params| {
                    let params = if home {
                        Params::new().with("name", "home")
                    } else {
                        params.clone()
                    };
                    Box::new(Configured {
                        log: log.clone(),
                        params,
                    })
                }),
            ))?;
        }
        r.bind_slot(slots::HOME, HOME, Priority::DEFAULT)?;
        r.bind_slot(slots::MENU, SCREEN, Priority::DEFAULT)?;
        for action in [CLOSE, WHO] {
            r.add_action(ActionSpec::new(action, "s.action"))?;
        }
        let log = self.0.clone();
        r.on(events::SCREEN_SHOWN, move |_, shown, _| {
            let slot = shown.slot.as_ref().map_or("-", Id::as_str);
            log.borrow_mut()
                .push(alloc::format!("shown {} for {slot}", shown.screen));
        })
    }
}

fn named(name: &str) -> Params {
    Params::new().with("name", String::from(name))
}

fn stack_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(StackPlugin(log.clone()))
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    log.borrow_mut().clear();
    kernel
}

fn names(kernel: &Kernel) -> Vec<String> {
    let env = kernel.shared.env(Instant(0));
    env.stack
        .iter()
        .map(|entry| {
            let overlay = if entry.overlay { " (overlay)" } else { "" };
            alloc::format!("{}{overlay}", entry.screen)
        })
        .collect()
}

/// Plugins read the stack, a screen knows itself, and plugins hear which
/// screen came on top.
#[test]
fn the_stack_is_read_and_a_screen_knows_itself() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_slot(Instant(1), slots::MENU, named("menu").with("overlay", "y"));
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    let env = kernel.shared.env(Instant(0));
    assert_eq!(
        env.top().and_then(|top| top.slot.clone()),
        Some(slots::MENU)
    );
    kernel.run_action(Instant(2), WHO);
    assert_eq!(
        *log.borrow(),
        [
            "menu show",
            "shown s.screen for core.menu",
            "menu is s.screen for core.menu"
        ]
    );
    log.borrow_mut().clear();
    kernel.run_action(Instant(3), actions::BACK);
    assert_eq!(
        *log.borrow(),
        ["menu close", "home show", "shown s.home for core.home"]
    );
}

/// A screen closes itself wherever it is: the screens above it stay.
#[test]
fn a_screen_closes_itself_below_others() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), SCREEN, named("a"));
    kernel.open_screen(Instant(2), SCREEN, named("b").with("overlay", "y"));
    let a = kernel.shared.stack[1].key;
    log.borrow_mut().clear();
    kernel.apply(Command::Close(a));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    assert_eq!(*log.borrow(), ["a close"]);
    // The only one is never closed.
    kernel.run_action(Instant(3), CLOSE);
    kernel.run_action(Instant(4), CLOSE);
    assert_eq!(names(&kernel), ["s.home"]);
}

/// One screen in place of another in one step, and back to a screen (or
/// a slot's) in one step.
#[test]
fn screens_are_replaced_and_gone_back_to() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(Instant(1), SCREEN, named("step 1"));
    log.borrow_mut().clear();
    kernel.apply(Command::Replace(SCREEN, named("step 2")));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home", "s.screen"]);
    assert_eq!(
        *log.borrow(),
        ["step 1 close", "step 2 show", "shown s.screen for -"],
        "the home screen is not shown between"
    );
    kernel.open_screen(Instant(2), SCREEN, named("step 3"));
    kernel.apply(Command::BackTo(slots::HOME));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home"]);
    kernel.apply(Command::BackTo(Id::new_static("s.none")));
    kernel.finish();
    assert_eq!(names(&kernel), ["s.home"]);
}

/// A screen opened for a slot of the plugin's choosing, and a slot bound
/// for the user: at once and kept.
#[test]
fn slots_take_a_screen_chosen() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.apply(Command::OpenInSlot(slots::LIBRARY, SCREEN, named("shelf")));
    kernel.finish();
    let env = kernel.shared.env(Instant(0));
    assert_eq!(
        env.top().and_then(|top| top.slot.clone()),
        Some(slots::LIBRARY)
    );
    kernel.apply(Command::BindSlot(slots::HOME, Some(SCREEN)));
    kernel.finish();
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::HOME.as_str()),
        Some(&SCREEN)
    );
    let saved = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
    assert!(String::from_utf8_lossy(&saved).contains("core.home = s.screen"));
    kernel.apply(Command::BindSlot(slots::HOME, None));
    kernel.finish();
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::HOME.as_str()),
        Some(&HOME)
    );
    let saved = paperpaw_sdk::read_file(kernel.storage(), "paperpaw.ini", 4096).unwrap();
    assert!(!String::from_utf8_lossy(&saved).contains("core.home"));
}

/// A screen asked before it closes may stay open: on *Back*, a tap
/// outside it, its slot shown again.
#[test]
fn a_screen_may_keep_itself_open() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    let form = named("form").with("refuse", "y").with("overlay", "y");
    kernel.open_screen(Instant(1), SCREEN, form);
    log.borrow_mut().clear();
    kernel.run_action(Instant(2), actions::BACK);
    tap(&mut kernel, 100, 45, 10);
    kernel.open_slot(Instant(200), slots::HOME, Params::new());
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    assert_eq!(
        *log.borrow(),
        [
            "form keeps open on Back",
            "form keeps open on TapOutside",
            "form keeps open on Slot"
        ]
    );
}

/// A tap outside an overlay closes it, is taken by it, or goes to the
/// screen below, as it says; input it lets pass goes below it.
#[test]
fn overlays_say_what_a_tap_outside_and_input_they_leave_do() {
    let log = log();
    let mut kernel = stack_kernel(&log);
    kernel.open_screen(
        Instant(1),
        SCREEN,
        named("bar").with("overlay", "y").with("outside", "ignore"),
    );
    tap(&mut kernel, 100, 45, 10);
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    kernel.apply(Command::Replace(
        SCREEN,
        named("skim")
            .with("overlay", "y")
            .with("outside", "pass")
            .with("pass", "y"),
    ));
    kernel.finish();
    log.borrow_mut().clear();
    tap(&mut kernel, 200, 45, 10);
    kernel.run_action(Instant(300), actions::NEXT_PAGE);
    assert_eq!(names(&kernel), ["s.home", "s.screen (overlay)"]);
    assert_eq!(
        *log.borrow(),
        ["home gesture Tap", "home action core.next-page"]
    );
}

/// A screen says itself that it has no row of hints: it is made for all
/// of the screen, where its plugin said nothing.
#[test]
fn a_screen_says_its_own_row_of_hints() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(StackPlugin(log.clone()))
        .button_row(&["back", "confirm", "left", "right"])
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(kernel.stack[0].hinted);
    kernel.open_screen(Instant(1), SCREEN, named("page").with("hints", "hidden"));
    assert!(!kernel.stack[1].hinted);
    assert_eq!(kernel.shared.device.display.height, 90);
}
