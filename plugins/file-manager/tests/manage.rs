//! The file manager with the real kernel on a folder of the computer's
//! file system (as the desktop emulator's card), by buttons only and by
//! touch only: going into folders and back, renaming (names that would
//! leave the folder or name nothing are refused, a name taken gets a
//! number), deleting a folder with everything in it, copying and moving
//! (also into itself, which is refused), new folders, hidden files, and
//! what ends up on the storage each time.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::path::PathBuf;
use std::rc::Rc;

use common::*;

const BROWSER: &str = "file-manager.browser";
const CHOICE: &str = "core.choice-dialog";
const INVALID: &str = "That name or place does not work.";

/// A folder of the computer's file system as the device's storage, gone
/// when the test is.
struct Card {
    root: PathBuf,
}

impl Card {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "paperpaw-file-manager-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        // The storage is a folder inside, so that what escapes it shows.
        std::fs::create_dir_all(root.join("card")).unwrap();
        Self { root }
    }

    /// The storage with a few books and folders, anew.
    fn storage(&self) -> Shared {
        let _ = std::fs::remove_dir_all(self.root.join("card"));
        let storage = Shared(Rc::new(
            paperpaw_fs::DirStorage::new(self.root.join("card")).unwrap(),
        ));
        storage.set_config("");
        storage.write(SETUP_DONE, b"").unwrap();
        for (path, text) in [
            ("books/alpha.txt", "Alpha text."),
            ("books/beta.txt", "Beta text."),
            ("books/Series/one.txt", "One."),
            ("books/Series/Deeper/two.txt", "Two."),
            ("notes.txt", "Notes."),
        ] {
            storage.write(path, text.as_bytes()).unwrap();
        }
        storage.create_dir("Empty").unwrap();
        storage
    }

    /// What is next to the storage's folder: nothing may get there.
    fn beside(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn manager(device: DeviceInfo, storage: Shared) -> App {
    let mut app = App::new(device, storage);
    app.run(Id::new_static("file-manager.open"));
    assert_eq!(app.screen(), BROWSER);
    app
}

/// The names of the entries shown.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// Focuses the entry `name` with the buttons (from wherever the focus
/// is, down), for touch screens nothing.
fn focus_on(app: &mut App, name: &str) {
    if touch(app) {
        return;
    }
    for _ in 0..40 {
        if app
            .focused()
            .is_some_and(|f| f.split(", ").next() == Some(name))
        {
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no entry {name}:\n{}", app.described());
}

/// Opens folder `name`: confirm on it, or a tap.
fn open(app: &mut App, name: &str) {
    if touch(app) {
        app.tap_on(name);
    } else {
        focus_on(app, name);
        app.run(actions::CONFIRM);
    }
}

/// The options of entry `name`: held on touch screens, the menu button
/// with buttons.
fn options(app: &mut App, name: &str) {
    if touch(app) {
        app.long_press_on(name);
    } else {
        focus_on(app, name);
        app.run(actions::MENU);
    }
    assert_eq!(app.screen(), CHOICE, "{}", app.described());
}

/// Chooses `option` of the overlay shown: down to it, or up where it is
/// above the focus (a question's yes is before the no it starts on).
fn choose(app: &mut App, option: &str) {
    if touch(app) {
        app.tap_on(option);
        return;
    }
    for step in [actions::DOWN, actions::UP] {
        for _ in 0..12 {
            if app.focused().as_deref() == Some(option) {
                app.run(actions::CONFIRM);
                return;
            }
            app.run(step.clone());
        }
    }
    panic!("no option {option}:\n{}", app.described());
}

/// Presses the bar's button `label`: down past the last entry to the
/// bar, or a tap.
fn press(app: &mut App, label: &str) {
    if touch(app) {
        app.tap_on(label);
        return;
    }
    for _ in 0..40 {
        if app.focused().as_deref() == Some(label) {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no button {label}:\n{}", app.described());
}

/// Back: the button, or the arrow in the header.
fn back(app: &mut App) {
    if touch(app) {
        app.tap_back();
    } else {
        app.run(actions::BACK);
    }
}

#[test]
fn folders_open_and_back_goes_up_to_the_one_left_then_closes() {
    let card = Card::new("browse");
    for device in DEVICES {
        let mut app = manager(device, card.storage());
        // Hidden files (PaperPaw's own) do not show; folders first.
        assert_eq!(
            rows(&mut app),
            ["books", "Empty", "notes.txt", "paperpaw.ini"]
        );
        open(&mut app, "books");
        assert_eq!(rows(&mut app), ["Series", "alpha.txt", "beta.txt"]);
        open(&mut app, "Series");
        open(&mut app, "Deeper");
        assert_eq!(rows(&mut app), ["two.txt"]);
        back(&mut app);
        back(&mut app);
        assert_eq!(rows(&mut app), ["Series", "alpha.txt", "beta.txt"]);
        if !device.input.touch {
            assert!(app.focused().unwrap().starts_with("Series"));
        }
        back(&mut app);
        open(&mut app, "Empty");
        assert!(app.shows("This folder is empty."), "{}", app.described());
        back(&mut app);
        back(&mut app);
        assert_ne!(app.screen(), BROWSER);
    }
}

#[test]
fn hidden_files_show_when_the_setting_says_so() {
    let card = Card::new("hidden");
    let storage = card.storage();
    storage.set_config("[file-manager]\nhidden = on\n");
    let mut app = manager(X4, storage);
    assert!(rows(&mut app).contains(&".paperpaw".to_string()));
}

#[test]
fn a_file_is_renamed_on_the_storage() {
    let card = Card::new("rename");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "alpha.txt");
        app.will_type("  Gamma.txt ");
        choose(&mut app, "Rename");
        // The keyboard started from the old name.
        let asked = app.asked();
        let question = asked.last().unwrap();
        assert_eq!(question.get(params::TEXT), Some("alpha.txt"));
        assert_eq!(app.screen(), BROWSER);
        assert!(!storage.exists("books/alpha.txt"));
        assert_eq!(
            storage.text("books/Gamma.txt").as_deref(),
            Some("Alpha text.")
        );
        assert_eq!(rows(&mut app), ["Series", "beta.txt", "Gamma.txt"]);
    }
}

#[test]
fn names_that_would_leave_the_folder_or_name_nothing_are_refused() {
    let card = Card::new("traversal");
    for device in DEVICES {
        let storage = card.storage();
        let before_books = storage.names("books");
        let before_top = storage.names("");
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        for name in [
            "..",
            ".",
            "../escaped.txt",
            "../../escaped.txt",
            "/absolute.txt",
            "sub/inner.txt",
            "back\\slash.txt",
            "..\\escaped.txt",
            "C:escaped.txt",
            "nul\0byte.txt",
            "line\nbreak.txt",
            "",
            "   ",
        ] {
            options(&mut app, "alpha.txt");
            app.will_type(name);
            choose(&mut app, "Rename");
            assert_eq!(app.screen(), BROWSER, "{name:?}");
            assert_eq!(storage.names("books"), before_books, "{name:?}");
            assert_eq!(storage.names(""), before_top, "{name:?}");
            if !name.trim().is_empty() || name.is_empty() {
                assert!(app.shows(INVALID), "{name:?}: {}", app.described());
            }
        }
        assert_eq!(card.beside(), ["card"]);
    }
}

#[test]
fn renaming_to_a_name_taken_keeps_both() {
    let card = Card::new("taken");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "alpha.txt");
        app.will_type("beta.txt");
        choose(&mut app, "Rename");
        assert_eq!(
            storage.text("books/beta.txt").as_deref(),
            Some("Beta text.")
        );
        assert_eq!(
            storage.text("books/beta (2).txt").as_deref(),
            Some("Alpha text.")
        );
        assert!(!storage.exists("books/alpha.txt"));
        // A folder's name taken by a file: the folder keeps its files.
        options(&mut app, "Series");
        app.will_type("beta.txt");
        choose(&mut app, "Rename");
        assert_eq!(
            storage.text("books/beta.txt").as_deref(),
            Some("Beta text.")
        );
        let names = storage.names("books");
        assert!(names.contains(&"beta (3).txt/".to_string()), "{names:?}");
        assert_eq!(
            storage.text("books/beta (3).txt/Deeper/two.txt").as_deref(),
            Some("Two.")
        );
    }
}

#[test]
fn renaming_to_the_same_name_changes_nothing() {
    let card = Card::new("same");
    let storage = card.storage();
    let mut app = manager(X4, storage.clone());
    open(&mut app, "books");
    options(&mut app, "alpha.txt");
    app.will_type("alpha.txt");
    choose(&mut app, "Rename");
    assert_eq!(storage.names("books"), ["Series/", "alpha.txt", "beta.txt"]);
    assert!(!app.shows(INVALID));
}

#[test]
fn a_folder_with_everything_in_it_is_deleted_after_asking() {
    let card = Card::new("delete");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        // Asked first; cancelled, all stays.
        options(&mut app, "Series");
        choose(&mut app, "Delete");
        assert!(app.shows("Delete the folder"), "{}", app.described());
        choose(&mut app, "Cancel");
        assert_eq!(app.screen(), BROWSER);
        assert!(storage.exists("books/Series/Deeper/two.txt"));
        // Yes: the folder and all in it gone, nothing else.
        options(&mut app, "Series");
        choose(&mut app, "Delete");
        choose(&mut app, "Yes, delete");
        assert_eq!(app.screen(), BROWSER);
        assert_eq!(storage.names("books"), ["alpha.txt", "beta.txt"]);
        assert!(!storage.exists("books/Series/one.txt"));
        assert_eq!(
            storage.names(""),
            [
                ".paperpaw/",
                "Empty/",
                "books/",
                "notes.txt",
                "paperpaw.ini"
            ]
        );
        assert_eq!(rows(&mut app), ["alpha.txt", "beta.txt"]);
    }
}

#[test]
fn a_book_deleted_is_forgotten_by_the_library() {
    let card = Card::new("forget");
    let storage = card.storage();
    let mut app = manager(TOUCH, storage.clone());
    // Read once: in the recent books.
    app.open_slot(
        slots::READER,
        Params::new().with(params::PATH, "books/alpha.txt"),
    );
    app.run(actions::BACK);
    assert_eq!(
        paperpaw_sdk::library::opened_books(&storage)
            .first()
            .map(String::as_str),
        Some("books/alpha.txt")
    );
    open(&mut app, "books");
    options(&mut app, "alpha.txt");
    choose(&mut app, "Delete");
    choose(&mut app, "Yes, delete");
    assert!(!storage.exists("books/alpha.txt"));
    assert!(
        !paperpaw_sdk::library::opened_books(&storage)
            .iter()
            .any(|p| p == "books/alpha.txt")
    );
}

#[test]
fn a_file_copied_into_its_own_folder_gets_a_free_name() {
    let card = Card::new("copy");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "alpha.txt");
        choose(&mut app, "Copy");
        assert!(app.shows("Copying “alpha.txt”"), "{}", app.described());
        press(&mut app, "Paste here");
        assert_eq!(
            storage.text("books/alpha (2).txt").as_deref(),
            Some("Alpha text.")
        );
        assert_eq!(
            storage.text("books/alpha.txt").as_deref(),
            Some("Alpha text.")
        );
    }
}

#[test]
fn a_folder_copied_elsewhere_takes_everything_in_it() {
    let card = Card::new("copy-folder");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "Series");
        choose(&mut app, "Copy");
        back(&mut app);
        open(&mut app, "Empty");
        press(&mut app, "Paste here");
        assert_eq!(
            storage.text("Empty/Series/one.txt").as_deref(),
            Some("One.")
        );
        assert_eq!(
            storage.text("Empty/Series/Deeper/two.txt").as_deref(),
            Some("Two.")
        );
        assert!(storage.exists("books/Series/one.txt"));
        assert_eq!(rows(&mut app), ["Series"]);
    }
}

#[test]
fn a_folder_copied_or_moved_into_itself_is_refused() {
    let card = Card::new("into-itself");
    for device in DEVICES {
        for option in ["Copy", "Move"] {
            let storage = card.storage();
            let before = storage.names("books/Series");
            let mut app = manager(device, storage.clone());
            open(&mut app, "books");
            options(&mut app, "Series");
            choose(&mut app, option);
            // Into itself.
            open(&mut app, "Series");
            press(&mut app, "Paste here");
            assert!(app.shows(INVALID), "{option}: {}", app.described());
            assert_eq!(storage.names("books/Series"), before, "{option}");
            // Into a folder inside it.
            back(&mut app);
            options(&mut app, "Series");
            choose(&mut app, option);
            open(&mut app, "Series");
            open(&mut app, "Deeper");
            press(&mut app, "Paste here");
            assert!(app.shows(INVALID), "{option}: {}", app.described());
            assert_eq!(
                storage.names("books/Series/Deeper"),
                ["two.txt"],
                "{option}"
            );
            assert_eq!(storage.names("books/Series"), before, "{option}");
            assert!(storage.exists("books/Series/one.txt"));
        }
    }
}

#[test]
fn a_book_moved_takes_what_is_kept_for_it_along() {
    let card = Card::new("move");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        app.open_slot(
            slots::READER,
            Params::new().with(params::PATH, "books/beta.txt"),
        );
        app.run(actions::BACK);
        open(&mut app, "books");
        options(&mut app, "beta.txt");
        choose(&mut app, "Move");
        assert!(app.shows("Moving “beta.txt”"), "{}", app.described());
        open(&mut app, "Series");
        press(&mut app, "Paste here");
        assert!(!storage.exists("books/beta.txt"));
        assert_eq!(
            storage.text("books/Series/beta.txt").as_deref(),
            Some("Beta text.")
        );
        // The recent books know where it is now.
        let opened = paperpaw_sdk::library::opened_books(&storage);
        assert_eq!(
            opened.first().map(String::as_str),
            Some("books/Series/beta.txt")
        );
        assert_eq!(rows(&mut app), ["Deeper", "beta.txt", "one.txt"]);
    }
}

#[test]
fn moving_into_the_same_folder_or_cancelling_changes_nothing() {
    let card = Card::new("stay");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "alpha.txt");
        choose(&mut app, "Move");
        press(&mut app, "Paste here");
        assert_eq!(storage.names("books"), ["Series/", "alpha.txt", "beta.txt"]);
        options(&mut app, "alpha.txt");
        choose(&mut app, "Copy");
        press(&mut app, "Cancel");
        assert!(!app.shows("Paste here"), "{}", app.described());
        assert_eq!(storage.names("books"), ["Series/", "alpha.txt", "beta.txt"]);
    }
}

#[test]
fn a_new_folder_is_made_here_and_bad_names_are_refused() {
    let card = Card::new("new-folder");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        app.will_type(" Fresh ");
        press(&mut app, "New folder");
        assert!(storage.names("books").contains(&"Fresh/".to_string()));
        assert!(rows(&mut app).contains(&"Fresh".to_string()));
        // The name taken: another beside it.
        app.will_type("Fresh");
        press(&mut app, "New folder");
        assert!(storage.names("books").contains(&"Fresh (2)/".to_string()));
        let before = storage.names("books");
        for name in ["..", "../up", "a/b", "a\\b", "x:y"] {
            app.will_type(name);
            press(&mut app, "New folder");
            assert!(app.shows(INVALID), "{name:?}: {}", app.described());
            assert_eq!(storage.names("books"), before, "{name:?}");
        }
        // Nothing typed: nothing made, nothing said.
        app.will_type("  ");
        press(&mut app, "New folder");
        assert_eq!(storage.names("books"), before);
        assert_eq!(card.beside(), ["card"]);
    }
}

#[test]
fn holding_confirm_offers_what_can_be_done_with_buttons_only() {
    let card = Card::new("hold");
    let mut app = manager(X4, card.storage());
    focus_on(&mut app, "notes.txt");
    app.hold("confirm");
    assert_eq!(app.screen(), CHOICE, "{}", app.described());
    assert!(app.shows("Rename"), "{}", app.described());
    app.run(actions::BACK);
    assert_eq!(app.screen(), BROWSER);
}

#[test]
fn a_book_opens_from_its_options_other_files_offer_no_opening() {
    let card = Card::new("open");
    for device in DEVICES {
        let storage = card.storage();
        storage.write("books/data.bin", &[0, 1, 2]).unwrap();
        let mut app = manager(device, storage);
        open(&mut app, "books");
        options(&mut app, "data.bin");
        assert!(!app.shows("Open"), "{}", app.described());
        app.run(actions::BACK);
        options(&mut app, "alpha.txt");
        choose(&mut app, "Open");
        assert_eq!(app.screen(), "reader.screen");
    }
}

#[test]
fn an_entry_gone_from_the_storage_meanwhile_tells_so() {
    let card = Card::new("vanished");
    for device in DEVICES {
        let storage = card.storage();
        let mut app = manager(device, storage.clone());
        open(&mut app, "books");
        options(&mut app, "alpha.txt");
        app.storage.remove("books/alpha.txt").unwrap();
        app.will_type("renamed.txt");
        choose(&mut app, "Rename");
        assert!(
            app.shows("It is not there any more."),
            "{}",
            app.described()
        );
        assert!(!storage.exists("books/renamed.txt"));
    }
}
