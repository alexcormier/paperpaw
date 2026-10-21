//! Page layers and frames on the shipped reader: a plugin reserves room
//! beside the text and draws in it beside every line, hears taps on it
//! (by touch and with the buttons), and sets pages in two columns.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::layers::{PageFrameSpec, PageLayerSpec, Side};
use paperpaw_sdk::{
    Color, Id, Manifest, MemoryStorage, Params, Plugin, Point, Rect, RegisterError, Registrar,
    SDK_VERSION,
};

/// Buttons and a touch screen.
const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-both",
    name: "Test both",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: true,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

const BOOK: &str = "books/alice-in-wonderland.txt";
const BAR: Id = Id::new_static("margin.bar");
const TWO: Id = Id::new_static("margin.two");

/// What the layer was shown last, and the taps it heard.
#[derive(Debug, Default)]
struct Seen {
    room: Option<Rect>,
    columns: Vec<Rect>,
    lines: Vec<(Rect, Locator)>,
    tapped: Vec<(Id, Option<Locator>, i32)>,
}

/// A bar beside every line, in a room at the right; and two columns.
struct Margin(Rc<RefCell<Seen>>);

impl Plugin for Margin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("margin"), "Margin", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let seen = self.0.clone();
        r.add_page_layer(
            PageLayerSpec::new(BAR, "margin.bar", move |canvas, _, page| {
                let mut seen = seen.borrow_mut();
                seen.room = page.room;
                seen.columns = page.columns.to_vec();
                seen.lines = page.lines.iter().map(|l| (l.area, l.start)).collect();
                if let Some(room) = page.room {
                    for line in page.lines {
                        let (y, height) = (line.area.y, line.area.height);
                        canvas.fill_rect(
                            i32::from(room.x),
                            i32::from(y),
                            4,
                            u32::from(height),
                            Color::BLACK,
                        );
                    }
                }
            })
            .reserve(|env| Some((Side::Right, env.metrics.mm(8) as u16)))
            .taps(),
        )?;
        let seen = self.0.clone();
        r.on(events::PAGE_LAYER_TAPPED, move |_, tapped, _| {
            let heard = (tapped.layer.clone(), tapped.line(), tapped.y);
            seen.borrow_mut().tapped.push(heard);
        })?;
        r.add_page_frame(PageFrameSpec::columns(TWO, 2, 24))
    }
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
    seen: Rc<RefCell<Seen>>,
}

impl App {
    /// The book open in the reader, with `config` besides English.
    fn new(config: &str) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"));
        let seen = Rc::new(RefCell::new(Seen::default()));
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Margin(seen.clone()))
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            now: 0,
            seen,
        };
        app.settle(effects);
        app.now += 100;
        let open = Params::new().with(params::PATH, BOOK);
        let effects = app.kernel.open_slot(Instant(app.now), slots::READER, open);
        app.settle(effects);
        app
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn tap(&mut self, point: Point) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 30;
            let event = InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point,
                tool: PointerTool::Finger,
                pressure: 1000,
            });
            let effects = self.kernel.handle(Instant(self.now), event);
            // Not held long: no time passes before it is lifted.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    /// The screen, by rows of luma.
    fn image(&mut self) -> Vec<Vec<u8>> {
        let display = DEVICE.display;
        let mut image = vec![vec![255u8; usize::from(display.width)]; usize::from(display.height)];
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                if let Some(line) = image.get_mut(usize::from(y + row)) {
                    for x in 0..view.width {
                        line[usize::from(x)] = view.pixel(x, row).luma();
                    }
                }
            }
        });
        image
    }
}

/// Whether any pixel from column `from` to before `to` is inked, in rows
/// `rows`.
fn inked(image: &[Vec<u8>], rows: std::ops::Range<usize>, from: usize, to: usize) -> bool {
    rows.into_iter()
        .any(|y| image[y][from..to].iter().any(|&luma| luma < 128))
}

#[test]
fn a_layer_draws_in_the_room_it_reserves() {
    let mut app = App::new("");
    let image = app.image();
    let seen = app.seen.borrow();
    let room = seen.room.expect("a room");
    // At the right of the text, 8 mm wide, inside the page's margin.
    assert!(room.right() < u32::from(DEVICE.display.width), "{room:?}");
    assert!(room.width > 60 && room.width < 75, "{room:?}");
    assert_eq!(seen.columns.len(), 1);
    let column = seen.columns[0];
    assert_eq!(
        column.right(),
        u32::from(room.x),
        "the text ends where the room begins"
    );
    assert!(seen.lines.len() > 8, "{} lines", seen.lines.len());
    // A bar beside each line, and no text in the room.
    let (area, _) = seen.lines[3];
    let rows = usize::from(area.y)..area.bottom() as usize;
    let x = usize::from(room.x);
    assert!(inked(&image, rows.clone(), x, x + 4));
    let text = usize::from(seen.lines[0].0.y)..seen.lines.last().unwrap().0.bottom() as usize;
    assert!(!inked(&image, text, x + 5, room.right() as usize));
}

#[test]
fn a_layers_room_hears_taps_and_the_buttons() {
    let mut app = App::new("");
    app.image();
    let (room, line) = {
        let seen = app.seen.borrow();
        (seen.room.expect("a room"), seen.lines[5])
    };
    // Beside the sixth line: told so.
    let y = i32::from(line.0.y) + i32::from(line.0.height) / 2;
    app.tap(Point::new(i32::from(room.x) + 10, y));
    assert_eq!(app.seen.borrow().tapped, [(BAR, Some(line.1), y)]);
    // The book's text has no links: choosing with the buttons comes to the
    // room first, and confirming tells the plugin too.
    app.image();
    app.run(Id::new_static("reader.links"));
    app.run(actions::CONFIRM);
    let seen = app.seen.borrow();
    assert_eq!(seen.tapped.len(), 2, "{:?}", seen.tapped);
    assert_eq!(seen.tapped[1].0, BAR);
}

#[test]
fn a_frame_sets_the_page_in_two_columns() {
    let mut app = App::new("page-frame = margin.two\n");
    let image = app.image();
    let seen = app.seen.borrow();
    let room = seen.room.expect("a room");
    let [left, right] = seen.columns[..] else {
        panic!("columns {:?}", seen.columns);
    };
    // Side by side, 24 pixels apart, up to the room.
    assert_eq!(left.right() + 24, u32::from(right.x));
    assert_eq!(right.right(), u32::from(room.x));
    // Lines in both, the right one going on where the left one ends.
    let in_right: Vec<_> = seen.lines.iter().filter(|(a, _)| a.x == right.x).collect();
    assert!(!in_right.is_empty() && in_right.len() < seen.lines.len());
    assert!(seen.lines.windows(2).all(|w| w[0].1 < w[1].1));
    // Ink on both sides of a gap without any.
    let text = usize::from(seen.lines[0].0.y)..seen.lines.last().unwrap().0.bottom() as usize;
    let gap = (left.right() as usize + 2)..(usize::from(right.x) - 2);
    assert!(!inked(&image, text.clone(), gap.start, gap.end));
    assert!(inked(&image, text.clone(), usize::from(left.x), gap.start));
    assert!(inked(&image, text, gap.end, usize::from(room.x)));
    drop(seen);
    // The next page goes on after the second column.
    let last = app.seen.borrow().lines.last().unwrap().1;
    app.run(actions::NEXT_PAGE);
    app.image();
    let first = app.seen.borrow().lines[0].1;
    assert!(first > last, "{first:?} after {last:?}");
}

#[test]
fn choosing_a_frame_lays_the_page_out_anew_where_it_was() {
    let mut app = App::new("");
    app.run(actions::NEXT_PAGE);
    app.image();
    let (columns, first) = {
        let seen = app.seen.borrow();
        (seen.columns.len(), seen.lines[0].1)
    };
    assert_eq!(columns, 1);
    app.now += 100;
    let effects = app
        .kernel
        .set_setting(Instant(app.now), "core.page-frame", "margin.two")
        .unwrap();
    app.settle(effects);
    app.image();
    let seen = app.seen.borrow();
    assert_eq!(seen.columns.len(), 2);
    assert_eq!(seen.lines[0].1, first, "from the same line");
}
