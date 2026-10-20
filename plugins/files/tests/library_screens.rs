//! The library with the real kernel and the standard app, on a device
//! with buttons only and on one with touch only: an empty library, a
//! library of many books (every one reachable on both), books in folders
//! and the folders as collections on the shelves, books with odd names,
//! both orders, the grid of covers, books removed while the list shows,
//! and what a book's sheet writes to the storage.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;

const TEXT: &[u8] = b"Some text of a book.\n\nAnd more of it.\n";

/// What the row above the books says while all books show.
const ALL_BOOKS: &str = "All books";

/// A library of `count` books named `Book 01.txt` and on in `books`.
fn many(count: usize) -> Vec<(String, Vec<u8>)> {
    (1..=count)
        .map(|n| (format!("books/Book {n:02}.txt"), TEXT.to_vec()))
        .collect()
}

fn storage_of(config: &str, files: &[(String, Vec<u8>)]) -> Shared {
    let files: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    Shared::memory(config, &files)
}

fn library(device: DeviceInfo, config: &str, files: &[(String, Vec<u8>)]) -> App {
    let mut app = App::new(device, storage_of(config, files));
    app.run(actions::LIBRARY);
    assert_eq!(app.screen(), "files.browser");
    app
}

/// The rows the list shows now, by the first part of their label (a
/// book's title, a shelf's name).
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| first(&p.label))
        .collect()
}

/// The title in a row's label: its first part.
fn first(label: &str) -> String {
    label.split(", ").next().unwrap_or_default().to_string()
}

/// Every row of the list, page by page: with buttons by going down from
/// row to row (right from cover to cover in the grid, where down goes a
/// row of covers), on touch screens by the arrow to the next page.
fn every_row(app: &mut App, touch: bool) -> Vec<String> {
    every_entry(app, touch, actions::DOWN)
}

fn every_entry(app: &mut App, touch: bool, step: Id) -> Vec<String> {
    // From the first book: the first page; with buttons up to the row
    // above the books, then on.
    app.repeat(actions::PREV_PAGE, 20);
    if !touch {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| first(&f) == ALL_BOOKS) {
                break;
            }
            app.run(actions::UP);
        }
        app.run(step.clone());
    }
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..200 {
        if touch {
            let before = seen.len();
            for row in rows(app) {
                if !seen.contains(&row) {
                    seen.push(row);
                }
            }
            if seen.len() == before {
                break;
            }
            app.tap_next_page();
        } else {
            let Some(focused) = app.focused() else {
                break;
            };
            let focused = first(&focused);
            // Past the last book, the row above the books; then the first.
            if seen.first() == Some(&focused) {
                break;
            }
            if focused != ALL_BOOKS && !seen.contains(&focused) {
                seen.push(focused);
            }
            app.run(step.clone());
        }
    }
    seen
}

/// Chooses what the library shows (`option` of the row above the books):
/// with buttons up to that row and confirm, on touch screens a tap on it.
fn show(app: &mut App, option: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(ALL_BOOKS);
    } else {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| first(&f) == ALL_BOOKS) {
                break;
            }
            app.run(actions::UP);
        }
        app.run(actions::CONFIRM);
    }
    assert_eq!(app.screen(), "files.list.show", "{}", app.described());
    choose(app, option);
    assert_eq!(app.screen(), "files.browser", "{}", app.described());
}

/// Opens the row named `label`: by buttons, down to it and confirm; by
/// touch, a tap on it.
fn open(app: &mut App, label: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(label);
        return;
    }
    for _ in 0..200 {
        if app.focused().is_some_and(|f| first(&f) == label) {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no row {label}:\n{}", app.described());
}

#[test]
fn an_empty_library_says_where_books_come_from() {
    for device in DEVICES {
        let mut app = library(device, "", &[]);
        assert!(app.shows("No books here yet"), "{}", app.described());
        assert!(rows(&mut app).is_empty());
        // Buttons on nothing do nothing, and back leaves.
        app.run(actions::DOWN);
        app.run(actions::CONFIRM);
        assert_eq!(app.screen(), "files.browser");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

/// Used to panic: `Listing::page` underflowed for an empty folder (as the
/// library showed again).
#[test]
fn an_empty_library_shows_again_after_its_menu_closes() {
    for device in DEVICES {
        let mut app = library(device, "", &[]);
        app.run(actions::MENU);
        assert_eq!(app.screen(), "menu.list");
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
        assert!(app.shows("No books here yet"), "{}", app.described());
    }
}

#[test]
fn a_library_folder_that_is_missing_shows_as_empty() {
    for device in DEVICES {
        let mut app = library(device, "[files]\nroot = nowhere/at/all\n", &many(3));
        assert!(app.shows("No books here yet"), "{}", app.described());
        assert!(app.shows("nowhere/at/all"), "{}", app.described());
    }
}

#[test]
fn every_one_of_many_books_is_reached_by_buttons_and_by_touch() {
    let books = many(57);
    let names: Vec<String> = (1..=57).map(|n| format!("Book {n:02}")).collect();
    for device in DEVICES {
        let touch = device.input.touch;
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &books);
        let seen = every_row(&mut app, touch);
        assert_eq!(seen, names, "touch: {touch}");
    }
}

#[test]
fn page_buttons_turn_whole_pages_and_come_back() {
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &many(57));
    let first_page = rows(&mut app);
    assert!(first_page.len() > 2 && first_page.len() < 57);
    app.run(actions::NEXT_PAGE);
    let second = rows(&mut app);
    assert!(second.iter().all(|r| !first_page.contains(r)), "{second:?}");
    // The focus went along: on the second page.
    let focused = first(&app.focused().unwrap());
    assert!(second.contains(&focused), "{focused} in {second:?}");
    app.run(actions::PREV_PAGE);
    assert_eq!(rows(&mut app), first_page);
}

#[test]
fn the_grid_of_covers_reaches_every_book_too() {
    let books = many(23);
    let names: Vec<String> = (1..=23).map(|n| format!("Book {n:02}")).collect();
    for device in DEVICES {
        let touch = device.input.touch;
        let config = "[files]\nsort = files.sort.name\nview = files.view.covers\n";
        let mut app = library(device, config, &books);
        let seen = every_entry(&mut app, touch, actions::RIGHT);
        assert_eq!(seen, names, "touch: {touch}");
        if !touch {
            // Down goes a row of covers: from the first to the fourth.
            assert_eq!(first(&app.focused().unwrap()), "Book 01");
            app.run(actions::DOWN);
            assert_eq!(first(&app.focused().unwrap()), "Book 04");
        }
    }
}

#[test]
fn books_in_folders_are_listed_and_their_folders_are_collections() {
    let files = vec![
        ("books/Zeta.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Beta/Gamma/Deep.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Side.txt".to_string(), TEXT.to_vec()),
        ("books/Omega/Other.txt".to_string(), TEXT.to_vec()),
    ];
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        // Every book, those in folders too, by title.
        assert_eq!(rows(&mut app), ["Deep", "Other", "Side", "Zeta"]);
        // The folders are on the shelves, each with the books below it.
        show(&mut app, "Shelves");
        let shelves = rows(&mut app);
        assert!(shelves.contains(&"Alpha".to_string()), "{shelves:?}");
        assert!(shelves.contains(&"Omega".to_string()), "{shelves:?}");
        open(&mut app, "Alpha");
        assert_eq!(app.screen(), "files.list.shelf");
        assert!(app.shows("Alpha"), "the folder's name as the title");
        assert_eq!(rows(&mut app), ["Deep", "Side"]);
        // Back: the shelves as they were.
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
        assert!(rows(&mut app).contains(&"Alpha".to_string()));
        // Back at the library leaves it.
        app.run(actions::BACK);
        assert_eq!(app.screen(), "home.dashboard");
    }
}

#[test]
fn books_with_odd_names_show_and_open() {
    let long = "L".repeat(180);
    let files = vec![
        ("books/Ünïcödé 日本語 📚.txt".to_string(), TEXT.to_vec()),
        (format!("books/{long}.txt"), TEXT.to_vec()),
        ("books/README".to_string(), TEXT.to_vec()),
        ("books/.hidden.txt".to_string(), TEXT.to_vec()),
        ("books/two.dots.in.name.txt".to_string(), TEXT.to_vec()),
        ("books/UPPER.TXT".to_string(), TEXT.to_vec()),
        ("books/empty.txt".to_string(), Vec::new()),
        ("books/Book.sdr/metadata.lua".to_string(), TEXT.to_vec()),
    ];
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        let touch = device.input.touch;
        let shown = every_row(&mut app, touch);
        // Back to the first page.
        app.repeat(actions::PREV_PAGE, 5);
        // No extension, hidden files and other readers' folders are no books.
        assert_eq!(shown.len(), 5, "{shown:?}");
        assert!(
            shown.iter().any(|r| r.contains("Ünïcödé 日本語")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|r| r.starts_with("LLLL")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("README")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("hidden")), "{shown:?}");
        assert!(!shown.iter().any(|r| r.contains("sdr")), "{shown:?}");
        // Each opens in the reader and comes back.
        for label in &shown {
            if touch {
                // On the page it is on.
                for _ in 0..5 {
                    if app.find(label).is_some() {
                        break;
                    }
                    app.tap_next_page();
                }
                app.tap_on(label);
            } else {
                // From where the focus is, down to this one.
                while first(&app.focused().unwrap_or_default()) != *label {
                    app.run(actions::DOWN);
                }
                app.run(actions::CONFIRM);
            }
            assert_eq!(app.screen(), "reader.screen", "{label}");
            app.run(actions::BACK);
            assert_eq!(app.screen(), "files.browser", "{label}");
        }
    }
}

#[test]
fn the_order_by_title_ignores_case_and_folders() {
    let files = vec![
        ("books/banana.txt".to_string(), TEXT.to_vec()),
        ("books/Apple.txt".to_string(), TEXT.to_vec()),
        ("books/cherry.txt".to_string(), TEXT.to_vec()),
        ("books/zfolder/Damson.txt".to_string(), TEXT.to_vec()),
        ("books/Afolder/Elder.txt".to_string(), TEXT.to_vec()),
    ];
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &files);
    let shown = every_row(&mut app, false);
    let shown: Vec<String> = shown.iter().map(|r| r.to_lowercase()).collect();
    assert_eq!(shown, ["apple", "banana", "cherry", "damson", "elder"]);
}

#[test]
fn recently_read_books_come_first_in_the_order_they_were_read() {
    let files = many(5);
    for device in DEVICES {
        let touch = device.input.touch;
        let mut app = library(device, "", &files);
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
        for book in ["books/Book 04.txt", "books/Book 02.txt"] {
            app.open_slot(slots::READER, Params::new().with(params::PATH, book));
            assert_eq!(app.screen(), "reader.screen");
            app.run(actions::BACK);
        }
        assert_eq!(app.screen(), "files.browser");
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 02", "Book 04", "Book 01", "Book 03", "Book 05"]
        );
        // By title: as before.
        app.set("files.sort", "files.sort.name");
        assert_eq!(
            every_row(&mut app, touch),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
    }
}

#[test]
fn the_row_above_the_books_chooses_what_shows_and_the_title_opens_the_menu() {
    // Buttons: up from the first book reaches the row above the books.
    let mut app = library(X4, "", &many(3));
    app.run(actions::UP);
    assert_eq!(app.focused().as_deref(), Some(ALL_BOOKS));
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "files.list.show");
    app.run(actions::BACK);
    assert_eq!(app.screen(), "files.browser");
    // Down from it, the first book again.
    app.run(actions::DOWN);
    assert_eq!(first(&app.focused().unwrap()), "Book 01");
    // The menu button: the book's sheet; on the row above, the menu.
    app.run(actions::UP);
    app.run(actions::MENU);
    assert_eq!(app.screen(), "menu.list");
    assert!(app.shows("Order of books"), "{}", app.described());

    let mut app = library(TOUCH, "", &many(3));
    app.tap_on("Library");
    assert_eq!(app.screen(), "menu.list");
    app.run(actions::BACK);
    app.tap_on(ALL_BOOKS);
    assert_eq!(app.screen(), "files.list.show");
}

#[test]
fn a_book_removed_while_the_list_shows_goes_from_it() {
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(4));
        assert_eq!(rows(&mut app).len(), 4);
        app.storage.remove("books/Book 02.txt").unwrap();
        // Told: the list as it is now.
        app.send(
            paperpaw_sdk::builtin::events::LIBRARY_CHANGED,
            Params::new(),
        );
        assert_eq!(rows(&mut app), ["Book 01", "Book 03", "Book 04"]);
    }
}

#[test]
fn opening_a_book_removed_unannounced_does_not_break_the_library() {
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(2));
        assert_eq!(rows(&mut app), ["Book 01", "Book 02"]);
        // Gone without anyone telling: the list still shows it.
        app.storage.remove("books/Book 01.txt").unwrap();
        if device.input.touch {
            app.tap_on("Book 01");
        } else {
            app.run(actions::CONFIRM);
        }
        // Not opened: the library says so and lists what is there.
        assert_eq!(app.screen(), "files.browser", "{}", app.described());
        assert_eq!(rows(&mut app), ["Book 02"]);
    }
}

/// Used to panic: `Listing::page` underflowed for an empty folder (as the
/// library kept its focus).
#[test]
fn every_book_of_the_whole_library_folder_removed_leaves_it_empty() {
    let mut app = library(TOUCH, "", &many(3));
    for n in 1..=3 {
        app.storage
            .remove(&format!("books/Book {n:02}.txt"))
            .unwrap();
    }
    app.send(
        paperpaw_sdk::builtin::events::LIBRARY_CHANGED,
        Params::new(),
    );
    assert!(app.shows("No books here yet"), "{}", app.described());
    // A tap where the rows were opens no book: with nothing there, a tap
    // in the middle opens the menu, as on any screen.
    app.tap(Point::new(240, 300));
    assert_eq!(app.screen(), "menu.list");
}

/// The sheet of the book `title`: held on touch screens, the menu button
/// with buttons only.
fn sheet_of(app: &mut App, title: &str) {
    if app.kernel.device().input.touch {
        app.long_press_on(title);
    } else {
        while first(&app.focused().unwrap_or_default()) != title {
            app.run(actions::DOWN);
        }
        app.run(actions::MENU);
    }
    assert_eq!(app.screen(), "files.list.sheet", "{}", app.described());
}

/// Chooses `option` of the choice or sheet shown: by buttons, down to it
/// and confirm; by touch, a tap on it.
fn choose(app: &mut App, option: &str) {
    if app.kernel.device().input.touch {
        app.tap_on(option);
        return;
    }
    for _ in 0..20 {
        if app
            .focused()
            .is_some_and(|f| f.to_lowercase().starts_with(&option.to_lowercase()))
        {
            app.run(actions::CONFIRM);
            return;
        }
        app.run(actions::DOWN);
    }
    panic!("no option {option}:\n{}", app.described());
}

#[test]
fn favourites_are_kept_and_shown_on_their_shelf() {
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &many(3));
        sheet_of(&mut app, "Book 02");
        choose(&mut app, "Add to favourites");
        assert_eq!(app.screen(), "files.browser");
        assert_eq!(
            paperpaw_sdk::library::favourites(&app.storage),
            ["books/Book 02.txt"]
        );
        app.run(Id::new_static("files.favourites"));
        assert!(app.shows("Book 02"), "{}", app.described());
        assert!(!app.shows("Book 01"), "{}", app.described());
        app.run(actions::BACK);
        // And off again.
        sheet_of(&mut app, "Book 02");
        choose(&mut app, "Remove from favourites");
        assert!(paperpaw_sdk::library::favourites(&app.storage).is_empty());
    }
}

#[test]
fn a_finished_book_moves_to_the_archive_folder_with_its_name_kept_free() {
    let mut files = many(2);
    // A book of the same name is in the archive already.
    files.push(("books/Archive/Book 01.txt".to_string(), b"old".to_vec()));
    for device in DEVICES {
        let mut app = library(device, "[files]\nsort = files.sort.name\n", &files);
        // The archive's books are not among the others.
        assert_eq!(rows(&mut app), ["Book 01", "Book 02"]);
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Mark as finished");
        assert!(app.shows("Finished"), "{}", app.described());
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Move to the archive");
        assert_eq!(app.screen(), "files.browser");
        assert!(!app.storage.exists("books/Book 01.txt"));
        // The one there before is kept; this one beside it.
        assert_eq!(
            app.storage.text("books/Archive/Book 01.txt").as_deref(),
            Some("old")
        );
        assert_eq!(
            app.storage.text("books/Archive/Book 01 (2).txt"),
            Some(String::from_utf8(TEXT.to_vec()).unwrap())
        );
        // Gone from the list at once; the archive is on the shelves.
        assert_eq!(rows(&mut app), ["Book 02"]);
        show(&mut app, "Shelves");
        open(&mut app, "Archive");
        assert_eq!(rows(&mut app).len(), 2, "{}", app.described());
    }
}

#[test]
fn a_book_in_the_archive_is_not_offered_to_be_archived_again() {
    let files = vec![("books/Archive/Done.txt".to_string(), TEXT.to_vec())];
    let mut app = library(TOUCH, "", &files);
    // Every book in the archive: the list says where they are.
    assert!(app.shows("archive"), "{}", app.described());
    show(&mut app, "Shelves");
    app.tap_on("Archive");
    sheet_of(&mut app, "Done");
    assert!(!app.shows("Move to the archive"), "{}", app.described());
}

#[test]
fn book_information_opens_from_the_sheet() {
    for device in DEVICES {
        let mut app = library(device, "", &many(1));
        sheet_of(&mut app, "Book 01");
        choose(&mut app, "Book information");
        assert_eq!(app.screen(), "files.info");
        assert!(app.shows("Book 01"), "{}", app.described());
        app.run(actions::BACK);
        assert_eq!(app.screen(), "files.browser");
    }
}

#[test]
fn searching_finds_books_by_their_title() {
    for device in DEVICES {
        let files = vec![
            ("books/Moby Dick.txt".to_string(), TEXT.to_vec()),
            ("books/Dracula.txt".to_string(), TEXT.to_vec()),
        ];
        let mut app = library(device, "", &files);
        app.will_type("  moby ");
        app.run(Id::new_static("files.search"));
        assert_eq!(app.screen(), "files.list.shelf", "{}", app.described());
        assert!(app.shows("Moby Dick"), "{}", app.described());
        assert!(!app.shows("Dracula"), "{}", app.described());
    }
}

#[test]
fn an_empty_search_opens_nothing() {
    let mut app = library(X4, "", &many(1));
    app.will_type("   ");
    app.run(Id::new_static("files.search"));
    assert_eq!(app.screen(), "files.browser");
}

#[test]
fn many_books_in_many_folders_show_at_once_and_their_folders_on_the_shelves() {
    // 400 books in 20 folders: the first page shows books at once.
    let files: Vec<(String, Vec<u8>)> = (0..400)
        .map(|n| {
            (
                format!("books/Shelf {:02}/Book {n:03}.txt", n % 20),
                TEXT.to_vec(),
            )
        })
        .collect();
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &files);
    let shown = rows(&mut app);
    assert_eq!(shown.first().map(String::as_str), Some("Book 000"));
    show(&mut app, "Shelves");
    let shelves = every_row(&mut app, false);
    let folders = shelves.iter().filter(|s| s.starts_with("Shelf ")).count();
    assert_eq!(folders, 20, "{shelves:?}");
}
