//! The library with the real kernel and the standard app, on a device
//! with buttons only and on one with touch only: an empty library, a
//! folder with many books (every one reachable on both), nested folders
//! and the way back out of them, books with odd names, both orders, the
//! grid of covers, books removed while the list shows, and what a book's
//! options write to the storage.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;

const TEXT: &[u8] = b"Some text of a book.\n\nAnd more of it.\n";

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

/// The rows the list shows now, by the first part of their label (the
/// title, or a folder's name).
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| first(&p.label))
        .collect()
}

/// The title in a row's label: its first part that is neither a letter
/// nor a format (a cover without a picture shows the title's first letter
/// and the format, in the grid before the title).
fn first(label: &str) -> String {
    let format = |part: &str| {
        part.len() <= 4
            && part
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    label
        .split(", ")
        .find(|part| part.chars().count() > 1 && !format(part))
        .or_else(|| label.split(", ").next())
        .unwrap_or_default()
        .to_string()
}

/// Every row of the list, page by page: with buttons by going down from
/// row to row (right from cover to cover in the grid, where down goes a
/// row of covers), on touch screens by the arrow to the next page.
fn every_row(app: &mut App, touch: bool) -> Vec<String> {
    every_entry(app, touch, actions::DOWN)
}

fn every_entry(app: &mut App, touch: bool, step: Id) -> Vec<String> {
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
            // Past the last row, the order in the header; then the first.
            if seen.first() == Some(&focused) {
                break;
            }
            if !focused.contains('›') && !seen.contains(&focused) {
                seen.push(focused);
            }
            app.run(step.clone());
        }
    }
    seen
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
    assert!(first_page.len() > 3 && first_page.len() < 57);
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
fn nested_folders_open_and_back_returns_to_the_folder_left() {
    let files = vec![
        ("books/Zeta.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Beta/Gamma/Deep.txt".to_string(), TEXT.to_vec()),
        ("books/Alpha/Side.txt".to_string(), TEXT.to_vec()),
        ("books/Omega/Other.txt".to_string(), TEXT.to_vec()),
    ];
    // Buttons: confirm opens the focused folder, back goes up.
    let mut app = library(X4, "", &files);
    assert_eq!(rows(&mut app), ["Alpha", "Omega", "Zeta"]);
    app.run(actions::CONFIRM);
    assert_eq!(rows(&mut app), ["Beta", "Side"]);
    app.run(actions::CONFIRM);
    app.run(actions::CONFIRM);
    assert_eq!(rows(&mut app), ["Deep"]);
    assert!(app.shows("Gamma"), "the folder's name as the title");
    app.run(actions::BACK);
    app.run(actions::BACK);
    app.run(actions::BACK);
    // At the top again, the folder left focused.
    assert_eq!(rows(&mut app), ["Alpha", "Omega", "Zeta"]);
    assert_eq!(first(&app.focused().unwrap()), "Alpha");
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    assert_eq!(rows(&mut app), ["Other"]);
    app.run(actions::BACK);
    assert_eq!(first(&app.focused().unwrap()), "Omega");
    // Back at the library folder leaves the library.
    app.run(actions::BACK);
    assert_eq!(app.screen(), "home.dashboard");

    // Touch: a tap opens a folder, the arrow in the header goes up.
    let mut app = library(TOUCH, "", &files);
    app.tap_on("Alpha");
    app.tap_on("Beta");
    app.tap_on("Gamma");
    assert_eq!(rows(&mut app), ["Deep"]);
    app.tap_back();
    assert_eq!(rows(&mut app), ["Gamma"]);
    app.tap_back();
    app.tap_back();
    assert_eq!(rows(&mut app), ["Alpha", "Omega", "Zeta"]);
    app.tap_back();
    assert_eq!(app.screen(), "home.dashboard");
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
        let shown = rows(&mut app);
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
            if device.input.touch {
                app.tap_on(label);
            } else {
                // From the first row, down to this one.
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
fn the_order_by_name_ignores_case_and_folders_come_first() {
    let files = vec![
        ("books/banana.txt".to_string(), TEXT.to_vec()),
        ("books/Apple.txt".to_string(), TEXT.to_vec()),
        ("books/cherry.txt".to_string(), TEXT.to_vec()),
        ("books/zfolder/x.txt".to_string(), TEXT.to_vec()),
        ("books/Afolder/y.txt".to_string(), TEXT.to_vec()),
    ];
    let mut app = library(X4, "[files]\nsort = files.sort.name\n", &files);
    assert_eq!(
        rows(&mut app),
        ["Afolder", "zfolder", "Apple", "banana", "cherry"]
    );
}

#[test]
fn recently_read_books_come_first_in_the_order_they_were_read() {
    let files = many(5);
    for device in DEVICES {
        let mut app = library(device, "", &files);
        assert_eq!(
            rows(&mut app),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
        for book in ["books/Book 04.txt", "books/Book 02.txt"] {
            app.open_slot(slots::READER, Params::new().with(params::PATH, book));
            assert_eq!(app.screen(), "reader.screen");
            app.run(actions::BACK);
        }
        assert_eq!(app.screen(), "files.browser");
        assert_eq!(
            rows(&mut app),
            ["Book 02", "Book 04", "Book 01", "Book 03", "Book 05"]
        );
        // By name: as before; the order shows in the header.
        app.set("files.sort", "files.sort.name");
        assert!(app.shows("By name"), "{}", app.described());
        assert_eq!(
            rows(&mut app),
            ["Book 01", "Book 02", "Book 03", "Book 04", "Book 05"]
        );
    }
}

#[test]
fn the_order_in_the_header_opens_the_library_menu_by_buttons_and_touch() {
    // Buttons: up from the first book reaches it.
    let mut app = library(X4, "", &many(3));
    app.run(actions::UP);
    assert_eq!(app.focused().as_deref(), Some("Recently read ›"));
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "menu.list");
    assert!(app.shows("Order of books"), "{}", app.described());
    app.run(actions::BACK);
    assert_eq!(app.screen(), "files.browser");
    // Down from it, the first book again.
    app.run(actions::DOWN);
    assert_eq!(first(&app.focused().unwrap()), "Book 01");

    let mut app = library(TOUCH, "", &many(3));
    app.tap_on("Recently read");
    assert_eq!(app.screen(), "menu.list");
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
        // Gone without anyone telling: the list still shows it.
        app.storage.remove("books/Book 01.txt").unwrap();
        if device.input.touch {
            app.tap_on("Book 01");
        } else {
            app.run(actions::CONFIRM);
        }
        // Whatever the reader makes of it, back leads to the library,
        // which then lists what is there.
        app.run(actions::BACK);
        if app.screen() != "files.browser" {
            app.run(actions::BACK);
        }
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

/// The options of the focused book: held on touch screens, the menu
/// button with buttons only.
fn options_of(app: &mut App, title: &str) {
    if app.kernel.device().input.touch {
        app.long_press_on(title);
    } else {
        while first(&app.focused().unwrap_or_default()) != title {
            app.run(actions::DOWN);
        }
        app.run(actions::MENU);
    }
    assert_eq!(app.screen(), "files.choice", "{}", app.described());
}

/// Chooses `option` of the choice shown: by buttons, down to it and
/// confirm; by touch, a tap on it.
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
        options_of(&mut app, "Book 02");
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
        options_of(&mut app, "Book 02");
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
        options_of(&mut app, "Book 01");
        choose(&mut app, "Mark as finished");
        assert!(app.shows("Finished"), "{}", app.described());
        options_of(&mut app, "Book 01");
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
        // The archive is a folder of the library, first in the list.
        assert_eq!(rows(&mut app), ["Archive", "Book 02"]);
    }
}

#[test]
fn a_book_in_the_archive_is_not_offered_to_be_archived_again() {
    let files = vec![("books/Archive/Done.txt".to_string(), TEXT.to_vec())];
    let mut app = library(TOUCH, "", &files);
    app.tap_on("Archive");
    options_of(&mut app, "Done");
    assert!(!app.shows("Move to the archive"), "{}", app.described());
}

#[test]
fn book_information_opens_from_the_options() {
    for device in DEVICES {
        let mut app = library(device, "", &many(1));
        options_of(&mut app, "Book 01");
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
        // The catalog's freshness is known for the whole program, as on a
        // device: the books of this library are new to it.
        app.send(
            paperpaw_sdk::builtin::events::LIBRARY_CHANGED,
            Params::new(),
        );
        app.will_type("  moby ");
        app.run(Id::new_static("files.search"));
        assert_eq!(app.screen(), "files.shelf", "{}", app.described());
        // The catalog is made in the background.
        app.now += 5000;
        let effects = app.kernel.tick(Instant(app.now));
        app.settle(effects);
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
fn many_books_in_many_folders_do_not_slow_the_first_page() {
    // 400 books in 20 folders: the first page shows the folders at once.
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
    assert_eq!(shown.first().map(String::as_str), Some("Shelf 00"));
    app.run(actions::CONFIRM);
    assert_eq!(rows(&mut app).first().map(String::as_str), Some("Book 000"));
    let seen = every_row(&mut app, false);
    assert_eq!(seen.len(), 20);
}
