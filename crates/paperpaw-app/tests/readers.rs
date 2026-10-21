//! Reading apps besides the shipped reader: books open with the one
//! chosen for the book, else for its folder, else for its format, else
//! the default one, else the shipped reader; a reader is offered only
//! for the formats it reads; the choices outlast a restart; chosen with
//! the buttons of an X4 or by touch.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::readers::{self, Choice, ReaderSpec};
use paperpaw_sdk::{
    ActionSpec, Canvas, Command, Cx, Env, Handled, Id, Manifest, MemoryStorage, Plugin,
    RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
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

/// A touch screen, as a Boox.
const BOOX: DeviceInfo = DeviceInfo {
    id: "test-boox",
    name: "Test Boox",
    display: DisplayInfo {
        width: 1264,
        height: 1680,
        dpi: 300,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: false,
        touch: true,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

const TEXT: &str = "books/notes.txt";
const IN_FOLDER: &str = "books/plain/story.txt";
const EPUB: &str = "books/alice.epub";
const PLAIN: Id = Id::new_static("plain.app");
const PLAIN_SCREEN: Id = Id::new_static("plain.screen");
const SHIPPED: &str = "reader.screen";

/// A reader of plain text only, and actions that open books as the
/// library does.
struct Plain;

struct Page;

impl Screen for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Plain {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("plain"), "Plain", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            PLAIN_SCREEN,
            "plain.screen",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.add_reader(
            ReaderSpec::new(PLAIN, PLAIN_SCREEN).formats(&[Id::new_static("format-txt.txt")]),
        )?;
        r.add_translations(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nplain.app = Plain text reader\n"
                .into(),
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("plain.ask-book"), "plain.ask-book").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| out.push(readers::ask(Choice::Book(TEXT))),
            ),
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("plain.ask-folder"), "plain.ask-folder").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    out.push(readers::ask(Choice::Folder("books/plain")))
                },
            ),
        )?;
        for (action, path) in [
            ("plain.text", TEXT),
            ("plain.folder", IN_FOLDER),
            ("plain.epub", EPUB),
        ] {
            r.add_action(ActionSpec::new(Id::new_static(action), action).handler(
                move |_: &Env<'_>, out: &mut Vec<Command>| out.push(readers::open(path)),
            ))?;
        }
        Ok(())
    }
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    fn new(storage: MemoryStorage, config: &str) -> Self {
        Self::with(storage, config, true)
    }

    /// The same, with the plain reader or without (as shipped).
    fn with(storage: MemoryStorage, config: &str, plain: bool) -> Self {
        Self::on(DEVICE, storage, config, plain)
    }

    /// The same on `device`.
    fn on(device: DeviceInfo, storage: MemoryStorage, config: &str, plain: bool) -> Self {
        let storage = storage.with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(device)
            .storage(storage)
            // On touch screens without the tips on gestures over it all.
            .user_config(format!(
                "[home]\ngesture-tips = false\n[core]\nlanguage = en\n{config}"
            ));
        let mut builder = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs);
        if plain {
            builder = builder.plugin(Plain);
        }
        let mut kernel = builder.build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
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

    /// A tap on the middle of the row or button that starts with `label`,
    /// the topmost.
    fn tap(&mut self, label: &str) -> String {
        self.press_on(label, 60)
    }

    /// A finger held a second on what starts with `label`.
    fn long_press(&mut self, label: &str) -> String {
        self.press_on(label, 1000)
    }

    /// A finger on what starts with `label`, `ms` long.
    fn press_on(&mut self, label: &str, ms: u64) -> String {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let part = parts
            .iter()
            .rev()
            .find(|p| p.label.starts_with(label))
            .unwrap_or_else(|| panic!("{label}: {parts:?}"));
        let (x, y) = (
            i32::from(part.area.x) + i32::from(part.area.width) / 2,
            i32::from(part.area.y) + i32::from(part.area.height) / 2,
        );
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            let event = InputEvent::Pointer(PointerEvent {
                id: 0,
                phase,
                point: paperpaw_sdk::Point::new(x, y),
                tool: PointerTool::Finger,
                pressure: 1000,
            });
            self.now += 20;
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            } else {
                // Held: what a long press does happens meanwhile (no
                // waiting past it, which would hold a tap too long).
                self.now += ms;
                self.kernel.tick(Instant(self.now));
            }
        }
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }

    /// The screen a book opens in, opened by `action`.
    fn opens(&mut self, action: &'static str) -> String {
        self.now += 100;
        let effects = self
            .kernel
            .run_action(Instant(self.now), Id::new_static(action));
        self.settle(effects);
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

fn books() -> MemoryStorage {
    MemoryStorage::new()
        .with(TEXT, "Some notes.")
        .with(IN_FOLDER, "A story.")
        .with(
            EPUB,
            &include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")[..],
        )
}

#[test]
fn books_open_in_the_shipped_reader_until_another_is_chosen() {
    let mut app = App::new(books(), "");
    assert_eq!(app.opens("plain.text"), SHIPPED);
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn a_format_opens_with_its_reading_app() {
    let mut app = App::new(books(), "reading-app.format-txt.txt = plain.app\n");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // EPUB has its own: as the default.
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn the_default_reading_app_opens_what_it_reads() {
    let mut app = App::new(books(), "reading-app = plain.app\n");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // It does not read EPUB: the shipped reader does.
    assert_eq!(app.opens("plain.epub"), SHIPPED);
}

#[test]
fn a_book_and_a_folder_keep_their_reading_app() {
    let storage = books();
    readers::remember(&storage, Choice::Folder("books/plain"), Some(&PLAIN)).unwrap();
    let mut app = App::new(storage, "");
    assert_eq!(app.opens("plain.folder"), "plain.screen");
    assert_eq!(app.opens("plain.text"), SHIPPED);
    // The book's own goes before its folder's.
    let shipped = Id::new_static("reader.app");
    readers::remember(
        app.kernel.storage(),
        Choice::Book(IN_FOLDER),
        Some(&shipped),
    )
    .unwrap();
    assert_eq!(app.opens("plain.folder"), SHIPPED);
    readers::remember(app.kernel.storage(), Choice::Book(TEXT), Some(&PLAIN)).unwrap();
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn the_settings_offer_the_readers_of_each_format() {
    let app = App::new(books(), "");
    let settings = app.kernel.settings();
    let choices = |id: &str| {
        let spec = settings.spec(id).unwrap_or_else(|| panic!("no {id}"));
        match &spec.kind {
            paperpaw_sdk::SettingKind::Choice(choices) => choices.len(),
            _ => 0,
        }
    };
    // Text: the default, the shipped reader, the plain one.
    assert_eq!(choices("core.reading-app.format-txt.txt"), 3);
    // EPUB: only the shipped reader reads it, nothing to choose.
    assert_eq!(choices("core.reading-app.format-epub.epub"), 1);
    assert_eq!(choices("core.reading-app"), 3);
}

/// The options of the list of reading apps shown, and the focused one.
fn listed(app: &mut App) -> (Vec<String>, Option<String>) {
    app.kernel.render(&mut |_, _| {});
    let parts = app.kernel.describe();
    let rows = parts
        .iter()
        .filter(|p| p.role == paperpaw_sdk::semantics::Role::Row)
        .map(|p| p.label.clone())
        .collect();
    let focused = parts.iter().find(|p| p.focused).map(|p| p.label.clone());
    (rows, focused)
}

#[test]
fn open_with_lists_the_readers_of_the_book_and_keeps_the_one_chosen() {
    let mut app = App::new(books(), "");
    assert_eq!(app.opens("plain.ask-book"), "core.reading-apps");
    let (rows, _) = listed(&mut app);
    assert_eq!(
        rows,
        ["PaperPaw reader", "Plain text reader", "Standard"],
        "{rows:?}"
    );
    // With the buttons: the second, chosen.
    assert_eq!(app.opens("core.down"), "core.reading-apps");
    assert_eq!(app.opens("core.confirm"), "plain.screen");
    // Kept: it opens there again.
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn by_touch_a_reading_app_is_tapped_in_the_list_and_in_the_settings() {
    let mut app = App::on(BOOX, books(), "", true);
    assert_eq!(app.opens("plain.ask-book"), "core.reading-apps");
    assert_eq!(app.tap("Plain text reader"), "plain.screen");
    assert_eq!(app.opens("plain.text"), "plain.screen");
    // The default one, in the reading settings.
    let mut app = App::on(BOOX, books(), "", true);
    app.now += 100;
    let params = paperpaw_sdk::Params::new().with(
        paperpaw_sdk::builtin::params::ID,
        paperpaw_sdk::builtin::groups::READING.as_str(),
    );
    let effects = app.kernel.open_slot(
        Instant(app.now),
        paperpaw_sdk::builtin::slots::SETTINGS,
        params,
    );
    app.settle(effects);
    for _ in 0..10 {
        app.kernel.render(&mut |_, _| {});
        if app
            .kernel
            .describe()
            .iter()
            .any(|p| p.label.starts_with("Reading app"))
        {
            break;
        }
        app.opens("core.next-page");
    }
    app.tap("Reading app");
    app.tap("Plain text reader");
    assert_eq!(app.opens("plain.text"), "plain.screen");
}

#[test]
fn a_folders_reading_app_is_chosen_from_every_reader() {
    let mut app = App::new(books(), "");
    app.opens("plain.ask-folder");
    let (rows, _) = listed(&mut app);
    assert_eq!(rows.len(), 3, "{rows:?}");
    app.opens("core.down");
    app.opens("core.confirm");
    assert_eq!(app.opens("plain.folder"), "plain.screen");
    // Standard forgets it again.
    app.opens("plain.ask-folder");
    app.opens("core.down");
    app.opens("core.down");
    app.opens("core.confirm");
    assert_eq!(app.opens("plain.folder"), SHIPPED);
}

/// The screen as a picture (PGM, gray), for a look before and after.
fn shot(app: &mut App, dir: &std::path::Path, name: &str) {
    let display = app.kernel.device().display;
    let (w, h) = (usize::from(display.width), usize::from(display.height));
    let mut gray = vec![255u8; w * h];
    // All of it, also what was drawn before (once what waits is drawn).
    app.kernel.render(&mut |_, _| {});
    app.kernel.rerender(&mut |y, view| {
        for row in 0..view.height {
            for x in 0..view.width {
                let at = usize::from(y + row) * w + usize::from(x);
                if let Some(pixel) = gray.get_mut(at) {
                    *pixel = view.pixel(x, row).luma();
                }
            }
        }
    });
    let mut pgm = format!("P5\n{w} {h}\n255\n").into_bytes();
    pgm.extend_from_slice(&gray);
    std::fs::write(dir.join(format!("{name}.pgm")), pgm).unwrap();
}

/// Focuses the row that starts with `name`, down from where it is.
fn focus_on(app: &mut App, name: &str) {
    for _ in 0..40 {
        app.kernel.render(&mut |_, _| {});
        // The topmost focus: an overlay's, not the screen's below it.
        let focused = app
            .kernel
            .describe()
            .into_iter()
            .filter(|p| p.focused)
            .last();
        if focused.is_some_and(|p| p.label.starts_with(name)) {
            return;
        }
        app.opens("core.down");
    }
    panic!("no row {name}");
}

/// The file manager's options for a book and a folder, and the list of
/// reading apps, as pictures in `PAPERPAW_SHOTS` (a folder): without a
/// second reading app (as shipped) and with one.
#[test]
#[ignore = "pictures for a look, run by hand"]
fn shots() {
    let Some(dir) = std::env::var_os("PAPERPAW_SHOTS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (plain, when) in [(false, "before"), (true, "after")] {
        let mut app = App::with(books(), "", plain);
        app.opens("file-manager.open");
        focus_on(&mut app, "books");
        app.opens("core.confirm");
        focus_on(&mut app, "notes.txt");
        app.opens("core.menu");
        shot(&mut app, &dir, &format!("x4-book-options-{when}"));
        app.opens("core.back");
        focus_on(&mut app, "plain");
        app.opens("core.menu");
        shot(&mut app, &dir, &format!("x4-folder-options-{when}"));
        if plain {
            app.opens("core.back");
            focus_on(&mut app, "notes.txt");
            app.opens("core.menu");
            focus_on(&mut app, "Open with");
            app.opens("core.confirm");
            shot(&mut app, &dir, "x4-open-with-after");
        }
        // The reading settings at their part for reading apps.
        let mut app = App::with(books(), "", plain);
        app.now += 100;
        let params = paperpaw_sdk::Params::new().with(
            paperpaw_sdk::builtin::params::ID,
            paperpaw_sdk::builtin::groups::READING.as_str(),
        );
        let effects = app.kernel.open_slot(
            Instant(app.now),
            paperpaw_sdk::builtin::slots::SETTINGS,
            params,
        );
        app.settle(effects);
        // With two reading apps, their part; before, the last page.
        match plain {
            true => focus_on(&mut app, "Reading app"),
            false => focus_on(&mut app, "Reset to defaults"),
        }
        shot(&mut app, &dir, &format!("x4-reading-settings-{when}"));

        // The same by touch: rows held for their options, tapped.
        let mut app = App::on(BOOX, books(), "", plain);
        app.opens("file-manager.open");
        app.tap("books");
        app.long_press("notes.txt");
        shot(&mut app, &dir, &format!("boox-book-options-{when}"));
        app.opens("core.back");
        app.long_press("plain");
        shot(&mut app, &dir, &format!("boox-folder-options-{when}"));
        if plain {
            app.opens("core.back");
            app.long_press("notes.txt");
            app.tap("Open with");
            shot(&mut app, &dir, "boox-open-with-after");
        }
        let mut app = App::on(BOOX, books(), "", plain);
        app.now += 100;
        let params = paperpaw_sdk::Params::new().with(
            paperpaw_sdk::builtin::params::ID,
            paperpaw_sdk::builtin::groups::READING.as_str(),
        );
        let effects = app.kernel.open_slot(
            Instant(app.now),
            paperpaw_sdk::builtin::slots::SETTINGS,
            params,
        );
        app.settle(effects);
        let wanted = if plain {
            "Reading app"
        } else {
            "Reset to defaults"
        };
        for _ in 0..10 {
            app.kernel.render(&mut |_, _| {});
            if app
                .kernel
                .describe()
                .iter()
                .any(|p| p.label.starts_with(wanted))
            {
                break;
            }
            app.opens("core.next-page");
        }
        shot(&mut app, &dir, &format!("boox-reading-settings-{when}"));
    }
}
