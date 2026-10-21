//! The file manager with the real kernel: large folders a page at a time,
//! what it tells when folders move, settings that follow a renamed
//! folder, and what was picked going with a deleted folder.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass, MemoryStorage, Params,
    PixelFormat, Plugin, RegisterError, Registrar, Version,
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

/// The books said to have moved: from where, to where.
type Moved = Rc<RefCell<Vec<(String, String)>>>;

/// Hears which books were said to have moved.
struct Spy(Moved);

impl Plugin for Spy {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("spy"), "Spy", Version::new(0, 1, 0))
            .description("Hears books moving.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let moved = Rc::clone(&self.0);
        r.on(events::BOOK_MOVED, move |_, m, _| {
            moved.borrow_mut().push((m.from.clone(), m.path.clone()));
        })
    }
}

/// The file manager in folder `dir` of `storage`; and the books said to
/// have moved.
fn manager(storage: MemoryStorage, dir: &str) -> (Kernel, Moved) {
    let moved = Rc::new(RefCell::new(Vec::new()));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_files::Files)
        .plugin(paperpaw_plugin_file_manager::FileManager)
        .plugin(Spy(Rc::clone(&moved)))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects);
    let screen = Id::new_static("file-manager.browser");
    let effects = kernel.open_screen(Instant(0), screen, Params::new().with(params::PATH, dir));
    settle(&mut kernel, effects);
    (kernel, moved)
}

fn settle(kernel: &mut Kernel, mut effects: Effects) {
    let mut now = 0;
    for _ in 0..200 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
}

fn shown(kernel: &mut Kernel) -> Vec<String> {
    kernel.render(&mut |_, _| {});
    kernel.describe().into_iter().map(|p| p.label).collect()
}

fn focused(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .find(|p| p.focused)
        .map(|p| p.label)
        .unwrap_or_default()
}

fn rename(kernel: &mut Kernel, path: &str, name: &str) {
    let typed = Typed::new(name, Params::new().with(params::PATH, path));
    let event = Id::new_static("file-manager.renamed");
    let effects = kernel.send_event(Instant(0), event, typed.to_params());
    settle(kernel, effects);
}

fn choose(kernel: &mut Kernel, path: &str, option: &str) {
    let picked = Picked::new(path.into(), option.into());
    let event = Id::new_static("file-manager.chosen");
    let effects = kernel.send_event(Instant(0), event, picked.to_params());
    settle(kernel, effects);
}

/// A folder of many files goes a page at a time, the focus where the
/// buttons take it.
#[test]
fn a_large_folder_page_by_page() {
    let storage = (0..60).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("big/file {i:02}.dat"), "x")
    });
    let (mut kernel, _) = manager(storage, "big");
    for _ in 0..30 {
        let effects = kernel.run_action(Instant(0), Id::new_static("core.down"));
        settle(&mut kernel, effects);
    }
    assert!(
        focused(&mut kernel).contains("file 30.dat"),
        "{}",
        focused(&mut kernel)
    );
    for _ in 0..3 {
        let effects = kernel.run_action(Instant(0), Id::new_static("core.up"));
        settle(&mut kernel, effects);
    }
    assert!(
        focused(&mut kernel).contains("file 27.dat"),
        "{}",
        focused(&mut kernel)
    );
}

/// A folder renamed tells of the books in it moving, not of other files.
#[test]
fn only_books_are_told_moving() {
    let storage = MemoryStorage::new()
        .with("books/Series/a.txt", "A")
        .with("books/Series/b.txt", "B")
        .with("books/Series/cover.jpg", "picture");
    let (mut kernel, moved) = manager(storage, "books");
    rename(&mut kernel, "books/Series", "Saga");
    let mut moved = moved.borrow().clone();
    moved.sort();
    assert_eq!(
        moved,
        [
            ("books/Series/a.txt".into(), "books/Saga/a.txt".into()),
            ("books/Series/b.txt".into(), "books/Saga/b.txt".into()),
        ]
    );
}

/// The library folder renamed: the settings naming it, or a folder in it,
/// follow.
#[test]
fn folder_settings_follow_a_rename() {
    let storage = MemoryStorage::new().with("books/a.txt", "A");
    let (mut kernel, _) = manager(storage, "");
    rename(&mut kernel, "books", "Library");
    assert_eq!(kernel.settings().text("core.library-folder"), "Library");
    assert_eq!(kernel.settings().text("files.archive"), "Library/Archive");
}

/// The reading app chosen for a folder follows it renamed, also for the
/// folders in it.
#[test]
fn reading_apps_of_folders_follow_a_rename() {
    use paperpaw_sdk::readers::{Choice, remember, remembered};
    let storage = MemoryStorage::new().with("books/Comics/Old/a.txt", "A");
    let app = Id::new_static("comics.app");
    remember(&storage, Choice::Folder("books/Comics"), Some(&app)).unwrap();
    remember(&storage, Choice::Folder("books/Comics/Old"), Some(&app)).unwrap();
    let (mut kernel, _) = manager(storage, "books");
    rename(&mut kernel, "books/Comics", "Manga");
    for folder in ["books/Manga", "books/Manga/Old"] {
        let kept = remembered(kernel.storage(), Choice::Folder(folder));
        assert_eq!(kept.as_deref(), Some("comics.app"), "{folder}");
    }
    assert_eq!(
        remembered(kernel.storage(), Choice::Folder("books/Comics")),
        None
    );
}

/// A file picked to be moved goes with its folder when that is deleted:
/// nothing is left to paste.
#[test]
fn deleting_a_folder_drops_what_was_picked_in_it() {
    let storage = MemoryStorage::new()
        .with("books/Old/a.txt", "A")
        .with("books/b.txt", "B");
    let (mut kernel, _) = manager(storage, "books");
    choose(&mut kernel, "books/Old/a.txt", "move");
    assert!(
        shown(&mut kernel).iter().any(|l| l == "Paste here"),
        "{:?}",
        shown(&mut kernel)
    );
    choose(&mut kernel, "books/Old", "delete-yes");
    let after = shown(&mut kernel);
    assert!(!after.iter().any(|l| l == "Paste here"), "{after:?}");
}
