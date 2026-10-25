//! An interlinear reading on the shipped reader: a text filter asks for
//! room above every line (`Block::leading_above`), the page layout gives
//! it (each line lower by it, the page ending sooner, each line's
//! `leading` saying how much), and a layer that asks for the page's words
//! gets them with their boxes, the words the reader has for the page,
//! with the room free above them. Where no layer asks, none are measured.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::filters::TextFilterSpec;
use paperpaw_sdk::layers::PageLayerSpec;
use paperpaw_sdk::reading::PageWords;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    ActionSpec, Commands, Env, Id, Manifest, MemoryStorage, Params, Plugin, Rect, RegisterError,
    Registrar, SDK_VERSION, SettingSpec,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-buttons",
    name: "Test buttons",
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
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

const BOOK: &str = "books/alice-in-wonderland.txt";
/// Whether the room above the lines is asked for.
const ON: Setting<bool> = Setting::new("interlinear.on");
static SETTINGS: [SettingSpec; 1] = [SettingSpec::switch(
    ON.static_str(),
    "interlinear.on",
    false,
    paperpaw_sdk::builtin::groups::READING,
)];
/// The room asked for: 70 hundredths of an em.
const LEADING: u16 = 70;
const ASK: Id = Id::new_static("interlinear.ask");
const HEARD: Event<PageWords> = Event::new("interlinear.words");

/// What the layers were shown last, and the page's words the reader told.
#[derive(Debug, Default)]
struct Seen {
    start: Locator,
    end: Locator,
    /// Each line's box and its room above.
    lines: Vec<(Rect, u16)>,
    /// The words the layer got.
    words: Vec<(Rect, String, Locator, Locator)>,
    told: Option<PageWords>,
}

/// Room above every line while switched on, and a layer that draws by
/// the page's words (it notes them), if it `asks` for them.
struct Interlinear {
    seen: Rc<RefCell<Seen>>,
    asks: bool,
}

impl Plugin for Interlinear {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("interlinear"), "Interlinear", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_text_filter(TextFilterSpec::new(
            Id::new_static("interlinear.room"),
            "interlinear.room",
            Box::new(|env: &Env<'_>, block| {
                if env.settings().read(&ON) {
                    block.leading_above = Some(LEADING);
                }
            }),
            Box::new(|env: &Env<'_>| u64::from(env.settings().read(&ON))),
        ))?;
        let seen = self.seen.clone();
        let layer = PageLayerSpec::new(
            Id::new_static("interlinear.words"),
            "words",
            move |_, _, page| {
                let mut seen = seen.borrow_mut();
                seen.start = page.start;
                seen.end = page.end;
                seen.lines = page.lines.iter().map(|l| (l.area, l.leading)).collect();
                seen.words = page
                    .words
                    .iter()
                    .map(|w| (w.area, w.text.clone(), w.start, w.end))
                    .collect();
            },
        );
        r.add_page_layer(if self.asks { layer.words() } else { layer })?;
        r.add_action(
            ActionSpec::new(ASK, "ask").handler(|_: &Env<'_>, _, out: &mut Commands| {
                out.push(paperpaw_sdk::reading::ask_page_words(&HEARD));
            }),
        )?;
        let seen = self.seen.clone();
        r.on(HEARD, move |_, words, _| {
            seen.borrow_mut().told = Some(words.clone());
        })
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
    /// The book open in the reader, the room asked for if `on`; its page
    /// drawn and its words told.
    fn new(on: bool) -> Self {
        Self::with(on, true)
    }

    /// The same, the layer asking for the words if `asks`.
    fn with(on: bool, asks: bool) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let config = format!(
            "[help]\nshow = false\n[core]\nlanguage = en\n[interlinear]\non = {}\n",
            if on { "on" } else { "off" }
        );
        let builder = Kernel::builder(DEVICE).storage(storage).user_config(config);
        let seen = Rc::new(RefCell::new(Seen::default()));
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Interlinear {
                seen: seen.clone(),
                asks,
            })
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
        app.show();
        app
    }

    /// The page drawn and its words told.
    fn show(&mut self) {
        self.kernel.render(&mut |_, _| {});
        self.run(ASK);
    }

    /// `pages` pages on, drawn and told.
    fn turn(&mut self, pages: usize) {
        for _ in 0..pages {
            self.run(paperpaw_sdk::builtin::actions::NEXT_PAGE);
        }
        self.show();
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
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
}

#[test]
fn room_asked_above_the_lines_lowers_them_and_ends_the_page_sooner() {
    let plain = App::new(false);
    let spaced = App::new(true);
    let (plain, spaced) = (plain.seen.borrow(), spaced.seen.borrow());
    assert_eq!(plain.start, spaced.start);
    assert!(plain.lines.len() > 5, "{:?}", plain.lines);
    assert!(plain.lines.iter().all(|&(_, leading)| leading == 0));
    // The same room above every line of the paragraphs.
    let leading = spaced.lines[0].1;
    assert!(leading > 5, "{leading} px for 70 % of an em");
    assert!(
        spaced.lines.iter().all(|&(_, l)| l == leading),
        "{:?}",
        spaced.lines
    );
    // Each line lower by it than the one before, as without it.
    let step = |lines: &[(Rect, u16)]| i32::from(lines[1].0.y) - i32::from(lines[0].0.y);
    let (plain_step, spaced_step) = (step(&plain.lines), step(&spaced.lines));
    assert!(
        (spaced_step - plain_step - i32::from(leading)).abs() <= 1,
        "{plain_step} + {leading} = {spaced_step}"
    );
    // Fewer lines fit: the page ends sooner, and the next starts there.
    assert!(spaced.lines.len() < plain.lines.len());
    assert!(spaced.end < plain.end, "{:?} {:?}", spaced.end, plain.end);
}

#[test]
fn a_layer_that_asks_gets_the_pages_words_with_the_room_free_above_them() {
    // A page of the story, past the title.
    let mut app = App::new(true);
    app.turn(3);
    let seen = app.seen.borrow();
    assert!(seen.words.len() > 20, "{:?}", seen.words);
    // The words the reader has for the page, in reading order, each in a
    // line and below its room.
    let told = seen.told.as_ref().expect("the page's words told");
    assert_eq!(told.start, seen.start);
    let starts: Vec<Locator> = seen.words.iter().map(|w| w.2).collect();
    let told_starts: Vec<Locator> = told.words.iter().map(|w| w.start).collect();
    assert_eq!(starts, told_starts);
    for (area, text, start, end) in &seen.words {
        assert!(!text.is_empty() && start < end, "{text}");
        assert!(seen.start <= *start && *end <= seen.end, "{text}");
        let (line, leading) = seen
            .lines
            .iter()
            .find(|(l, _)| l.y <= area.y && u32::from(area.y) < l.bottom())
            .unwrap_or_else(|| panic!("{text} {area:?} in no line"));
        let room_top = i32::from(line.y) - i32::from(*leading);
        assert!(room_top >= 0, "{text}: its room on the page");
        // No other line's box reaches into the room above the word.
        for (other, _) in &seen.lines {
            if other != line {
                assert!(
                    other.bottom() as i32 <= room_top + 1
                        || i32::from(other.y) >= line.bottom() as i32,
                    "{text}: {other:?} in the room above {line:?}"
                );
            }
        }
    }
}

#[test]
fn without_a_layer_that_asks_no_words_are_given() {
    let app = App::with(true, false);
    let seen = app.seen.borrow();
    assert!(!seen.lines.is_empty());
    assert!(seen.words.is_empty());
}

#[test]
fn switching_the_room_on_lays_the_page_out_anew() {
    let mut app = App::new(false);
    let before = app.seen.borrow().end;
    assert!(app.seen.borrow().lines.iter().all(|&(_, l)| l == 0));
    app.now += 100;
    let effects = app
        .kernel
        .set_setting(Instant(app.now), "interlinear.on", "on")
        .unwrap();
    app.settle(effects);
    app.show();
    let seen = app.seen.borrow();
    assert!(seen.lines.iter().all(|&(_, l)| l > 0), "{:?}", seen.lines);
    assert!(seen.end < before, "{:?} {before:?}", seen.end);
}
