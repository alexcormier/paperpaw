//! Bars along the edges of the screen (SDK gap A1): what they leave to
//! the screens, where things are drawn, where taps go, when they show.

use super::*;
use paperpaw_sdk::chrome::{ChromeSpec, Edge};
use paperpaw_sdk::{Gesture, Params, StackEntry};

const PAGE: Id = Id::new_static("bar.page");
const BOOK: Id = Id::new_static("bar.book");
const SIZE: u16 = 20;

/// A screen black in its top left corner (10 by 10) that logs the size
/// it was made for and the taps it gets.
struct Corner {
    name: &'static str,
    log: Log,
}

impl Screen for Corner {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, _: &mut Cx<'_, '_>) -> Handled {
        let (x, y) = (gesture.start.x, gesture.start.y);
        let line = alloc::format!("{} {:?} at {x},{y}", self.name, gesture.kind);
        self.log.borrow_mut().push(line);
        Handled::Yes
    }
}

fn corner(name: &'static str, log: &Log) -> paperpaw_sdk::ScreenFactory {
    let log = log.clone();
    Box::new(move |env, _| {
        let display = env.device.display;
        let line = alloc::format!("{name} made {}x{}", display.width, display.height);
        log.borrow_mut().push(line);
        Box::new(Corner {
            name,
            log: log.clone(),
        })
    })
}

/// A bar along `edge`, not over the book, for touch screens.
struct BarPlugin(Log, Edge);

impl Plugin for BarPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("bar"), "Bar", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(PAGE, "page", corner("page", &self.0)))?;
        r.add_screen(ScreenSpec::new(BOOK, "book", corner("book", &self.0)))?;
        r.bind_slot(slots::HOME, PAGE, Priority::DEFAULT)?;
        r.bind_slot(slots::READER, BOOK, Priority::DEFAULT)?;
        let not_over_books = |_: &Env<'_>, entry: &StackEntry| entry.screen != BOOK;
        r.add_chrome(
            ChromeSpec::new(Id::new_static("bar.tabs"), self.1, corner("bar", &self.0))
                .size(|_| SIZE)
                .shown(not_over_books)
                .need(paperpaw_sdk::MenuNeed::Touch),
        )
    }
}

fn bar_kernel(log: &Log, edge: Edge, device: DeviceInfo) -> Kernel {
    let mut kernel = Kernel::builder(device)
        .plugin(BarPlugin(log.clone(), edge))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

/// Along the bottom: the screen gets the rest, the bar its strip; each
/// draws from its own corner and gets the taps in its part, in its own
/// coordinates.
#[test]
fn a_bar_at_the_bottom_leaves_the_rest_to_the_screen() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Bottom, DEVICE);
    assert_eq!(*log.borrow(), ["page made 90x70", "bar made 90x20"]);
    assert_eq!(
        kernel.shared.env(Instant(0)).content,
        Rect::new(0, 0, 90, 70)
    );
    assert_eq!(
        kernel.frame().pixel(5, 5),
        Color::BLACK,
        "the page's corner"
    );
    assert_eq!(
        kernel.frame().pixel(5, 75),
        Color::BLACK,
        "the bar's corner"
    );
    assert_eq!(kernel.frame().pixel(5, 65), Color::WHITE);
    log.borrow_mut().clear();
    tap(&mut kernel, 100, 30, 80);
    tap(&mut kernel, 200, 30, 40);
    assert_eq!(*log.borrow(), ["bar Tap at 30,10", "page Tap at 30,40"]);
}

/// Along the top: the screen is drawn below it, from its own corner, and
/// a tap in it is in its coordinates.
#[test]
fn a_bar_at_the_top_moves_the_screen_below_it() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Top, DEVICE);
    assert_eq!(
        kernel.frame().pixel(5, 25),
        Color::BLACK,
        "the page's corner"
    );
    assert_eq!(kernel.frame().pixel(15, 25), Color::WHITE);
    log.borrow_mut().clear();
    tap(&mut kernel, 100, 30, 50);
    assert_eq!(*log.borrow(), ["page Tap at 30,30"]);
}

/// A bar shows over the screens it says: not over the book, which gets
/// all of the screen; back on the page, it shows again.
#[test]
fn a_bar_shows_over_the_screens_it_says() {
    let log = log();
    let mut kernel = bar_kernel(&log, Edge::Left, DEVICE);
    log.borrow_mut().clear();
    kernel.open_slot(
        Instant(10),
        slots::READER,
        Params::new().with("book", "a.txt"),
    );
    render(&mut kernel);
    assert_eq!(*log.borrow(), ["book made 90x90"]);
    assert!(kernel.chrome_strips.is_empty());
    assert!(kernel.chromes[0].screen.is_none(), "let go");
    kernel.run_action(Instant(20), actions::BACK);
    render(&mut kernel);
    assert_eq!(kernel.chrome_strips, [(0, Rect::new(0, 0, SIZE, 90))]);
    assert_eq!(
        kernel.frame().pixel(25, 5),
        Color::BLACK,
        "the page beside it"
    );
}

/// A bar for touch screens is not there on a device without one.
#[test]
fn a_bar_for_touch_needs_a_touch_screen() {
    let log = log();
    let buttons = DeviceInfo {
        input: paperpaw_platform::InputCaps {
            touch: false,
            ..DEVICE.input
        },
        ..DEVICE
    };
    let kernel = bar_kernel(&log, Edge::Bottom, buttons);
    assert!(kernel.chromes.is_empty());
    assert_eq!(*log.borrow(), ["page made 90x90"]);
}
