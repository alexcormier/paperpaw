//! The words and personal widgets on the real home screen: each draws
//! what it should (the quote of the day and who said it, a greeting, the
//! note, the open to-dos, how to add pictures), and a tap on a to-do
//! ticks it off.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_widgets::Widgets;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
    Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Small,
};

const TODO: &str = ".paperpaw/plugins/widgets/todo.txt";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

struct Home {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Home {
    /// The home screen showing `page`, with `settings` set.
    fn new(page: &str, settings: &[(&str, &str)]) -> Self {
        let storage = Shared::default();
        storage
            .write(TODO, b"Return the library book\nBuy bread\n")
            .unwrap();
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(Widgets)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut home = Self {
            kernel,
            storage,
            now: 0,
        };
        home.set("home.page-1", page);
        for (id, value) in settings {
            home.set(id, value);
        }
        home
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        self.kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
    }

    /// The texts on the screen, in order.
    fn shown(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

fn has(shown: &[String], text: &str) -> bool {
    shown.iter().any(|l| l.contains(text))
}

#[test]
fn the_widgets_show_what_they_should() {
    let mut home = Home::new(
        "widgets.greeting 0/0 4x1, widgets.quote 0/1 4x2, widgets.note 0/3 2x1, \
         widgets.todo 2/3 2x2, widgets.picture 0/4 2x1",
        &[
            ("widgets.greeting.name", "Anna"),
            ("widgets.note.text", "If found, please return to Anna"),
        ],
    );
    let shown = home.shown();
    // The time is not known: the afternoon, the first quote.
    assert!(has(&shown, "Good afternoon, Anna"), "{shown:?}");
    assert!(has(&shown, "Some books are to be tasted"), "{shown:?}");
    assert!(has(&shown, "\u{201c}Some books"), "{shown:?}");
    assert!(has(&shown, "Francis Bacon"), "{shown:?}");
    assert!(has(&shown, "If found"), "{shown:?}");
    assert!(has(&shown, "To do · 2"), "{shown:?}");
    assert!(has(&shown, "Return the library"), "{shown:?}");
    assert!(has(&shown, "Put pictures"), "{shown:?}");
}

#[test]
fn quotes_are_in_the_language_read() {
    let mut home = Home::new(
        "widgets.quote 0/0 4x3, widgets.greeting 0/3 4x1",
        &[("core.language", "de")],
    );
    let shown = home.shown();
    assert!(has(&shown, "\u{201e}Dort wo man"), "{shown:?}");
    assert!(has(&shown, "Heinrich Heine"), "{shown:?}");
    assert!(has(&shown, "Guten Tag"), "{shown:?}");
}

#[test]
fn a_to_do_is_ticked_off_from_the_widget() {
    let mut home = Home::new("widgets.todo 0/0 4x3", &[("widgets.todo.caption", "false")]);
    let shown = home.shown();
    assert!(has(&shown, "Return the library"), "{shown:?}");
    // The focus on the first item, which confirm ticks off.
    home.now += 100;
    home.kernel.run_action(
        Instant(home.now),
        paperpaw_sdk::builtin::actions::RIGHT.clone(),
    );
    home.now += 100;
    home.kernel.run_action(
        Instant(home.now),
        paperpaw_sdk::builtin::actions::CONFIRM.clone(),
    );
    let kept = paperpaw_sdk::read_file(&home.storage, TODO, 1024).unwrap();
    let kept = String::from_utf8(kept).unwrap();
    assert!(kept.starts_with("x "), "{kept}");
    assert!(kept.contains("Return the library book"), "{kept}");
    assert!(kept.ends_with("Buy bread\n"), "{kept}");
}

#[test]
fn empty_ones_say_how_to_fill_them() {
    let mut home = Home::new(
        "widgets.note 0/0 4x2, widgets.quote 0/2 4x2",
        &[("widgets.quote.source", "widgets.quote.source.own")],
    );
    let shown = home.shown();
    assert!(has(&shown, "Press to write a note"), "{shown:?}");
    assert!(has(&shown, "quotes.txt"), "{shown:?}");
}
