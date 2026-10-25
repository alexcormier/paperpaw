//! Screens that run in real time (ADR 0030): raw keys and their way
//! out, the buttons held, full screen, refreshes the screen chooses, the
//! fastest refresh, the pace, staying awake, how frames went.

use core::time::Duration;

use super::*;
use paperpaw_sdk::Params;
use paperpaw_sdk::realtime::{KeyEvent, ScreenMode};

/// A plugin with a page on the home slot and a game screen of `mode`.
struct Game {
    log: Log,
    mode: ScreenMode,
}

impl Plugin for Game {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("game"), "Game", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.log.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("game.page"),
            "page",
            Box::new(move |_, _| {
                Box::new(TestScreen {
                    name: "page",
                    log: log.clone(),
                })
            }),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("game.page"), Priority::DEFAULT)?;
        let (log, mode) = (self.log.clone(), self.mode);
        r.add_screen(ScreenSpec::new(
            Id::new_static("game.play"),
            "play",
            Box::new(move |env, _| {
                log.borrow_mut()
                    .push(alloc::format!("made {}", env.content().height));
                Box::new(Play {
                    log: log.clone(),
                    mode,
                })
            }),
        ))
    }
}

/// The game: takes every key but *Menu*, says what it got and which
/// keys are held.
struct Play {
    log: Log,
    mode: ScreenMode,
}

impl Screen for Play {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn mode(&self) -> ScreenMode {
        self.mode
    }

    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        self.log
            .borrow_mut()
            .push(alloc::format!("action {action}"));
        Handled::Yes
    }

    fn on_key(&mut self, key: &KeyEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if key.name() == "menu" {
            return Handled::No;
        }
        let held: Vec<&str> = cx.env().keys_held().iter().collect();
        let what = if key.is_press() { "down" } else { "up" };
        self.log.borrow_mut().push(alloc::format!(
            "{what} {} at {} held {held:?}",
            key.name(),
            key.at().0
        ));
        cx.request_redraw();
        Handled::Yes
    }
}

fn kernel(builder: crate::KernelBuilder, mode: ScreenMode, log: &Log) -> Kernel {
    let mut kernel = builder
        .plugin(Game {
            log: log.clone(),
            mode,
        })
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(10), Id::new_static("game.play"), Params::new());
    render(&mut kernel);
    log.borrow_mut().clear();
    kernel
}

fn button(kernel: &mut Kernel, at: u64, name: &'static str, state: ButtonState) -> crate::Effects {
    let effects = kernel.handle(Instant(at), InputEvent::Button { name, state });
    render(kernel);
    effects
}

/// Presses and releases come as they happen, with their times and the
/// buttons held; one the screen does not take goes to the key map; the
/// power button never comes.
#[test]
fn raw_keys_come_pressed_and_released() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    button(&mut kernel, 100, "left", ButtonState::Pressed);
    button(&mut kernel, 120, "up", ButtonState::Pressed);
    button(&mut kernel, 130, "left", ButtonState::Released);
    button(&mut kernel, 180, "up", ButtonState::Released);
    assert_eq!(
        *log.borrow(),
        [
            "down left at 100 held [\"left\"]",
            "down up at 120 held [\"left\", \"up\"]",
            "up left at 130 held [\"up\"]",
            "up up at 180 held []",
        ]
    );
    log.borrow_mut().clear();
    press(&mut kernel, 300, "menu");
    press(&mut kernel, 500, "power");
    assert!(
        log.borrow()
            .iter()
            .all(|l| !l.starts_with("down") && !l.starts_with("up")),
        "{:?}",
        log.borrow()
    );
}

/// Without raw keys, the same screen gets actions as before.
#[test]
fn without_raw_keys_buttons_are_actions() {
    let log = log();
    let mut kernel = kernel(Kernel::builder(DEVICE), ScreenMode::new(), &log);
    press(&mut kernel, 100, "confirm");
    assert_eq!(*log.borrow(), ["action core.confirm"]);
}

/// *Back* held for two seconds leaves the screen, whatever it does with
/// *Back* otherwise; its release is told first.
#[test]
fn back_held_leaves_a_screen_of_raw_keys() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    let effects = button(&mut kernel, 1000, "back", ButtonState::Pressed);
    let due = effects.wake_at().unwrap();
    assert_eq!(due, Instant(3000));
    kernel.tick(Instant(2500));
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.play"));
    kernel.tick(due);
    render(&mut kernel);
    assert_eq!(kernel.active_screen().map(Id::as_str), Some("game.page"));
    assert_eq!(
        log.borrow().as_slice(),
        [
            "down back at 1000 held [\"back\"]",
            "up back at 3000 held [\"back\"]"
        ]
    );
    // Let go later, on the page: nothing more for the game.
    button(&mut kernel, 3200, "back", ButtonState::Released);
    assert_eq!(log.borrow().len(), 2);
}

/// A full screen has all of the panel: no row of hints below it.
#[test]
fn a_full_screen_has_no_row_of_hints() {
    let builder = || Kernel::builder(DEVICE).button_row(&["back", "confirm", "left", "right"]);
    let log = log();
    let mut normal = kernel(builder(), ScreenMode::new(), &log);
    assert!(normal.hint.height > 0);
    assert!(normal.shared.content.height < 90);
    drop(normal.stack.pop());
    let full = kernel(builder(), ScreenMode::new().full_screen(), &log);
    assert_eq!(full.shared.content.height, 90);
}

/// A screen that cleans itself gets no full refresh every few frames; the
/// fastest refresh is black and white where that is enough; the pace is
/// told to the shell.
#[test]
fn a_screen_chooses_its_refreshes_and_pace() {
    let config = "[core]\nfull-refresh-interval = 2\n";
    let modes = |mode: ScreenMode| {
        let log = log();
        let builder = Kernel::builder(DEVICE).user_config(config);
        let mut kernel = kernel(builder, mode, &log);
        (1..=3)
            .flat_map(|n| {
                let down = button(&mut kernel, n * 100, "left", ButtonState::Pressed);
                let up = button(&mut kernel, n * 100 + 50, "left", ButtonState::Released);
                [down, up]
            })
            .map(|effects| (effects.refresh().map(|(_, m)| m), effects.pace()))
            .collect::<Vec<_>>()
    };
    let paced = ScreenMode::new()
        .raw_keys()
        .cleans_itself()
        .fastest()
        .grays(2)
        .frame_every(Duration::from_millis(50));
    let frames = modes(paced);
    assert!(
        frames
            .iter()
            .all(|f| *f == (Some(RefreshMode::Mono), Some(Duration::from_millis(50)))),
        "{frames:?}"
    );
    let frames = modes(ScreenMode::new().raw_keys());
    assert!(
        frames.iter().any(|(m, _)| *m == Some(RefreshMode::Full)),
        "a full refresh every two: {frames:?}"
    );
}

/// The device stays awake longer while a screen asks for it.
#[test]
fn a_screen_keeps_the_device_awake_longer() {
    let log = log();
    let config = "[core]\nsleep-after = 1\n";
    let builder = || Kernel::builder(DEVICE).user_config(config).sleep(true);
    let mut normal = kernel(builder(), ScreenMode::new(), &log);
    let due = normal.sleep_due().unwrap();
    let mode = ScreenMode::new().awake_for(Duration::from_secs(600));
    let mut longer = kernel(builder(), mode, &log);
    let later = longer.sleep_due().unwrap();
    assert_eq!(later.since(due), 540_000);
    drop((normal.stack.pop(), longer.stack.pop()));
}

/// What the shell says of a frame shown reaches plugins: its time counts
/// for the mode, merged frames are counted.
#[test]
fn frames_shown_are_told_to_plugins() {
    let log = log();
    let mode = ScreenMode::new().raw_keys();
    let mut kernel = kernel(Kernel::builder(DEVICE), mode, &log);
    assert!(kernel.shared.panel.last_frame().is_none());
    let area = Rect::new(0, 0, 10, 10);
    // The frame of its opening, shown.
    kernel.frame_shown(
        area,
        RefreshMode::Full,
        Duration::from_millis(5),
        Duration::from_millis(1600),
        Instant(50),
    );
    button(&mut kernel, 100, "left", ButtonState::Pressed);
    button(&mut kernel, 110, "left", ButtonState::Released);
    kernel.frame_shown(
        area,
        RefreshMode::Fast,
        Duration::from_millis(3),
        Duration::from_millis(400),
        Instant(520),
    );
    let panel = kernel.shared.env(Instant(600)).panel();
    let frame = panel.last_frame().unwrap();
    assert_eq!(frame.area(), area);
    assert_eq!(frame.mode(), Some(RefreshMode::Fast));
    assert_eq!(frame.merged(), 1, "two frames asked, one shown");
    assert_eq!(
        panel.refresh_time(RefreshMode::Fast),
        Some(Duration::from_millis(400))
    );
}
