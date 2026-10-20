//! Screens over their life: made anew for another size, the times they
//! ask for, a book's own settings while it is open.

use super::*;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{MemoryStorage, Params};

const READER: Id = Id::new_static("book.reader");
const MENU: Id = Id::new_static("book.menu");
const FULL: Id = Id::new_static("book.full");
const CLOSED: Id = Id::new_static("book.closed");
const BOOK_SETTINGS: &str = "books/a/settings.ini";

/// A reader that keeps a book's own settings while it is open and asks
/// for a timer when a setting changes; a menu over the lower half of the
/// screen and a full screen, each saying when its timer comes.
struct BookPlugin(Log);

struct ReaderScreen {
    log: Log,
}

impl Screen for ReaderScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        cx.command(Command::SetScope(Some(String::from(BOOK_SETTINGS))));
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == CLOSED {
            self.log.borrow_mut().push(String::from("closed said"));
        }
        if *event == events::SETTING_CHANGED {
            let at = cx.env().now.add_millis(10);
            cx.wake_at(at);
        }
    }

    fn on_close(&mut self, cx: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("reader close"));
        cx.command(Command::Emit(CLOSED, Params::new()));
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
        self.log.borrow_mut().push(String::from("reader timer"));
    }
}

/// A screen that says when its timer comes, over `region` (all of the
/// screen without).
struct Other {
    name: &'static str,
    region: Option<Rect>,
    log: Log,
}

impl Screen for Other {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        self.region
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_timer(&mut self, _: &mut Cx<'_, '_>) {
        self.log
            .borrow_mut()
            .push(alloc::format!("{} timer", self.name));
    }
}

impl Plugin for BookPlugin {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("book"),
            name: Cow::Borrowed("Book"),
            description: Cow::Borrowed(""),
            version: Version::new(1, 0, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(ScreenSpec {
            id: READER,
            title: Cow::Borrowed("reader"),
            factory: Box::new(move |_, _| Box::new(ReaderScreen { log: log.clone() })),
        })?;
        r.bind_slot(slots::HOME, READER, Priority::DEFAULT)?;
        for (id, name, region) in [
            (MENU, "menu", Some(Rect::new(0, 45, 90, 45))),
            (FULL, "full", None),
        ] {
            let log = self.0.clone();
            r.add_screen(ScreenSpec {
                id,
                title: Cow::Borrowed("other"),
                factory: Box::new(move |_, _| {
                    Box::new(Other {
                        name,
                        region,
                        log: log.clone(),
                    })
                }),
            })?;
        }
        Ok(())
    }
}

fn book_kernel(log: &Log) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(BookPlugin(log.clone()))
        .storage(MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn text(kernel: &Kernel, path: &str) -> String {
    kernel.shared.recall.clear();
    let data = paperpaw_sdk::read_file(kernel.storage(), path, 1 << 20).unwrap_or_default();
    String::from_utf8(data).unwrap()
}

/// With "only this book" on, a book's settings stay its own while it is
/// open, also once its screen was made anew (the screen turned, the row
/// of hints changed in a menu over it).
#[test]
fn a_books_own_settings_last_while_it_is_open() {
    let log = log();
    let mut kernel = book_kernel(&log);
    kernel
        .set_setting(Instant(1), "core.scope-only", "on")
        .unwrap();
    kernel.open_screen(Instant(2), MENU, Params::new());
    kernel.resize(Instant(3), 80, 90);
    kernel
        .set_setting(Instant(4), "core.font-size", "14")
        .unwrap();
    assert!(text(&kernel, BOOK_SETTINGS).contains("font-size = 14"));
    assert!(!text(&kernel, paperpaw_sdk::builtin::CONFIG_FILE).contains("font-size"));
    // A full screen over it, and the menu that changes the setting over
    // that: still the book's.
    kernel.open_screen(Instant(5), FULL, Params::new());
    kernel.open_screen(Instant(6), MENU, Params::new());
    kernel
        .set_setting(Instant(7), "core.font-size", "15")
        .unwrap();
    assert!(text(&kernel, BOOK_SETTINGS).contains("font-size = 15"));
}
