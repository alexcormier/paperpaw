//! Recording reading with the real kernel, from the reader's events:
//! what a stay on a page counts (KOReader's shortest and longest page),
//! pauses while suspended, books registered, moved and finished, no
//! clock; and how the numbers come out: reading across midnight and over
//! the changes to and from summer time counted on the right days.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{App, Card, DAY, DIR, X4};
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::{Instant, MemoryStorage, Params, Storage};

/// 14 October 2026, 10:41 in Berlin (UTC+2), in Unix seconds.
const NOW: i64 = 1_791_967_260;
const BOOK: &str = "books/Der Process.txt";

fn card() -> MemoryStorage {
    MemoryStorage::new().with(
        BOOK,
        "Jemand mußte Josef K. verleumdet haben ...".repeat(50),
    )
}

fn reading(page: Option<(u32, u32)>, progress: f32) -> events::Reading {
    events::Reading {
        path: BOOK.to_string(),
        title: String::new(),
        progress,
        section: 0,
        offset: 0,
        page: page.map(|p| p.0),
        pages: page.map(|p| p.1),
        chapter: String::new(),
        chapter_left: None,
    }
}

impl App {
    /// `seconds` after the app started.
    fn at(&mut self, seconds: u64) -> Instant {
        self.now = self.now.max(seconds * 1000);
        Instant(self.now)
    }

    fn opened(&mut self, seconds: u64, page: u32, pages: u32) {
        let at = self.at(seconds);
        let reading = reading(Some((page, pages)), 0.0);
        let effects =
            self.kernel
                .send_event(at, events::BOOK_OPENED.id().clone(), reading.to_params());
        self.settle(effects);
    }

    fn turned(&mut self, seconds: u64, page: u32, pages: u32) {
        let at = self.at(seconds);
        let reading = reading(Some((page, pages)), 0.0);
        let effects =
            self.kernel
                .send_event(at, events::PAGE_TURNED.id().clone(), reading.to_params());
        self.settle(effects);
    }

    fn books_ini(&self) -> String {
        String::from_utf8(
            self.storage
                .get(&format!("{DIR}/books.ini"))
                .unwrap_or_default(),
        )
        .unwrap()
    }
}

fn reader(config: &str) -> App {
    App::new(X4, card(), Some(NOW * 1000), config, false)
}

#[test]
fn stays_count_as_koreader_counts_them() {
    let mut app = reader("");
    app.opened(10, 1, 100);
    // 30 s on page 1, 2 s on page 2 (leafed through), an hour on page 3
    // (left open: two minutes at most).
    app.turned(40, 2, 100);
    app.turned(42, 3, 100);
    app.turned(3642, 4, 100);
    let start = NOW as u32 + 10;
    assert_eq!(
        common::stays(&app.storage),
        [(1, 1, start, 30, 100), (1, 3, start + 32, 120, 100)]
    );
    // The same page again (e.g. after a menu) is no turn.
    app.turned(3700, 4, 100);
    assert_eq!(common::stays(&app.storage).len(), 2);
}

#[test]
fn the_shortest_and_longest_page_are_settings() {
    let mut app = reader("[stats]\nmin-seconds = 1\nmax-seconds = 30\n");
    app.opened(10, 1, 100);
    app.turned(12, 2, 100);
    app.turned(112, 3, 100);
    let stays: Vec<u16> = common::stays(&app.storage).iter().map(|s| s.3).collect();
    assert_eq!(stays, [2, 30]);
}

#[test]
fn reading_pauses_while_suspended() {
    let mut app = reader("");
    app.opened(10, 1, 100);
    let at = app.at(30);
    let effects = app.kernel.suspend(at);
    app.settle(effects);
    // An hour asleep, then a minute on the page.
    let at = app.at(3630);
    let effects = app.kernel.resume(at);
    app.settle(effects);
    app.turned(3690, 2, 100);
    let start = NOW as u32;
    assert_eq!(
        common::stays(&app.storage),
        [(1, 1, start + 10, 20, 100), (1, 1, start + 3630, 60, 100)]
    );
}

#[test]
fn nothing_is_recorded_without_a_clock_or_when_switched_off() {
    let mut no_clock = App::new(X4, card(), None, "", false);
    no_clock.opened(10, 1, 100);
    no_clock.turned(40, 2, 100);
    assert!(common::stays(&no_clock.storage).is_empty());
    assert!(no_clock.storage.files().is_empty());
    let mut off = reader("[stats]\nenabled = false\n");
    off.opened(10, 1, 100);
    off.turned(40, 2, 100);
    assert!(common::stays(&off.storage).is_empty());
}

#[test]
fn a_book_is_registered_by_its_fingerprint() {
    let mut app = reader("");
    app.opened(10, 5, 100);
    let books = app.books_ini();
    // Named after its file without a title of its own.
    assert!(books.contains("[book.1]\ntitle = Der Process\n"), "{books}");
    assert!(books.contains("authors = N/A\n"), "{books}");
    assert!(books.contains("pages = 100\n"), "{books}");
    assert!(books.contains("path = books/Der Process.txt\n"), "{books}");
    assert!(
        books.contains(&format!("last-open = {}\n", NOW + 10)),
        "{books}"
    );
    let md5 = books
        .lines()
        .find_map(|l| l.strip_prefix("md5 = "))
        .unwrap();
    assert_eq!(md5.len(), 32, "{books}");
    // Counted anew (e.g. another font size): the book's pages follow.
    app.turned(40, 7, 140);
    assert!(app.books_ini().contains("pages = 140\n"));
    // Moved: found where it is now; the same file again is the same book.
    let moved = events::BookMoved {
        from: BOOK.to_string(),
        path: String::from("books/kafka/Der Process.txt"),
    };
    let at = app.at(50);
    let effects = app
        .kernel
        .send_event(at, events::BOOK_MOVED.id().clone(), moved.to_params());
    app.settle(effects);
    assert!(
        app.books_ini()
            .contains("path = books/kafka/Der Process.txt\n")
    );
    // Read to the end, where it is now.
    let at = app.at(60);
    let finished = events::Reading {
        path: String::from("books/kafka/Der Process.txt"),
        ..reading(Some((140, 140)), 100.0)
    };
    let effects =
        app.kernel
            .send_event(at, events::BOOK_FINISHED.id().clone(), finished.to_params());
    app.settle(effects);
    let books = app.books_ini();
    assert!(
        books.contains(&format!("finished = {}\n", NOW + 60)),
        "{books}"
    );
    assert!(!books.contains("[book.2]"), "{books}");
}

#[test]
fn without_counted_pages_the_progress_stands_for_them() {
    let mut app = reader("");
    let at = app.at(10);
    let effects = app.kernel.send_event(
        at,
        events::BOOK_OPENED.id().clone(),
        reading(None, 42.5).to_params(),
    );
    app.settle(effects);
    let at = app.at(40);
    let effects = app.kernel.send_event(
        at,
        events::PAGE_TURNED.id().clone(),
        reading(None, 43.0).to_params(),
    );
    app.settle(effects);
    assert_eq!(
        common::stays(&app.storage),
        [(1, 426, NOW as u32 + 10, 30, 1000)]
    );
    assert!(app.books_ini().contains("pages = 1000\n"));
}

#[test]
fn the_statistics_opened_while_reading_include_the_page_being_read() {
    let mut app = reader("");
    app.opened(10, 1, 100);
    // Four minutes on the page, then the statistics.
    app.at(250);
    app.run(paperpaw_plugin_stats::OPEN);
    assert_eq!(app.screen(), "stats.overview");
    let shown = app.shown();
    assert!(shown.contains("This book\n"), "{shown}");
    assert!(shown.contains("Today\n2 min\n1 page\n"), "{shown}");
    // And reading goes on from there: the statistics opened a tenth of a
    // second after 250 s, so the next stay lasts 29.9 s, counted in whole
    // seconds as the device's clock measures it.
    app.turned(280, 2, 100);
    let stays: Vec<u16> = common::stays(&app.storage).iter().map(|s| s.3).collect();
    assert_eq!(stays, [120, 29]);
}

/// `books.ini` of over 1 MiB is read line by line like any other: a book
/// opened then is added to all the others, none lost.
#[test]
fn many_books_are_kept_when_another_is_opened() {
    let mut text = String::new();
    for id in 1..=4000 {
        text.push_str(&format!(
            "[book.{id}]\ntitle = \"{}\"\nmd5 = {id:032x}\npages = 300\npath = \"books/{id}.epub\"\n\n",
            "A long title of a book ".repeat(10)
        ));
    }
    assert!(text.len() > 1 << 20);
    let storage = card().with(&format!("{DIR}/books.ini"), text);
    let mut app = App::new(X4, storage, Some(NOW * 1000), "", false);
    app.opened(10, 1, 100);
    let books = app.books_ini();
    assert!(books.contains("[book.4000]"), "books lost");
    assert!(books.contains("[book.4001]"), "the new book not added");
}

/// A day in the calendar: what its detail says.
fn day_detail(app: &mut App, back: u32) -> String {
    app.open("stats.calendar", Params::new());
    for _ in 0..back {
        app.press("left");
    }
    let shown = app.shown();
    let at = shown
        .lines()
        .position(|l| l.contains(", "))
        .unwrap_or_else(|| panic!("no day: {shown}"));
    shown
        .lines()
        .skip(at)
        .take(2)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_stay_across_midnight_counts_on_the_day_it_began() {
    // 23:59 in Berlin yesterday, two minutes on a page.
    let start = (20_740 - 1) * DAY + 21 * 3600 + 59 * 60;
    let card = Card::new()
        .book(1, "Late", 300, start)
        .stay(common::record(1, 1, start, 120, 300))
        .build();
    let mut app = App::new(X4, card, Some(NOW * 1000), "", false);
    assert_eq!(day_detail(&mut app, 0), "Wednesday, October 14\nNot read");
    assert_eq!(
        day_detail(&mut app, 1),
        "Tuesday, October 13\n2 min · 1 page"
    );
}

const BERLIN: &str = "[core]\ntime-zone-rule = \"CET-1CEST,M3.5.0,M10.5.0/3\"\n";

#[test]
fn days_follow_the_change_to_summer_time() {
    // Summer time began on Sunday, 29 March 2026. 22:30 UTC on the 28th
    // is 23:30 that day in Berlin (an hour ahead); 22:30 UTC on the 29th
    // is 00:30 on the 30th (two hours ahead).
    let card = Card::new()
        .book(1, "Spring", 300, 20_541 * DAY)
        .stay(common::record(
            1,
            1,
            20_540 * DAY + 22 * 3600 + 1800,
            300,
            300,
        ))
        .stay(common::record(
            1,
            2,
            20_541 * DAY + 22 * 3600 + 1800,
            600,
            300,
        ))
        .build();
    // 30 March 2026, 10:00 UTC.
    let now = (20_542 * DAY + 10 * 3600) * 1000;
    let mut app = App::new(X4, card, Some(now), BERLIN, false);
    assert_eq!(day_detail(&mut app, 0), "Monday, March 30\n10 min · 1 page");
    assert_eq!(day_detail(&mut app, 1), "Sunday, March 29\nNot read");
    assert_eq!(
        day_detail(&mut app, 2),
        "Saturday, March 28\n5 min · 1 page"
    );
    app.open("stats.overview", Params::new());
    assert!(
        app.shown().contains("Today\n10 min\n1 page\n"),
        "{}",
        app.shown()
    );
}

#[test]
fn days_follow_the_change_back_from_summer_time() {
    // Summer time ended on Sunday, 25 October 2026: 22:30 UTC on the
    // 24th is 00:30 on the 25th in Berlin, 22:30 UTC on the 25th 23:30
    // the same day.
    let card = Card::new()
        .book(1, "Autumn", 300, 20_751 * DAY)
        .stay(common::record(
            1,
            1,
            20_750 * DAY + 22 * 3600 + 1800,
            300,
            300,
        ))
        .stay(common::record(
            1,
            2,
            20_751 * DAY + 22 * 3600 + 1800,
            600,
            300,
        ))
        .build();
    // 26 October 2026, 10:00 UTC.
    let now = (20_752 * DAY + 10 * 3600) * 1000;
    let mut app = App::new(X4, card, Some(now), BERLIN, false);
    assert_eq!(day_detail(&mut app, 0), "Monday, October 26\nNot read");
    assert_eq!(
        day_detail(&mut app, 1),
        "Sunday, October 25\n15 min · 2 pages"
    );
    assert_eq!(day_detail(&mut app, 2), "Saturday, October 24\nNot read");
    // The book's days alike.
    app.open("stats.book", Params::new().with(params::ID, "1"));
    assert!(
        app.shown().contains("1 day\nReading days\n"),
        "{}",
        app.shown()
    );
}

#[test]
fn streaks_and_the_week_count_local_days() {
    // Read on the last three days and five days ago, ten minutes each.
    let mut card = Card::new().book(1, "Daily", 300, 20_740 * DAY);
    for back in [5, 2, 1, 0] {
        card = card.read(1, (20_740 - back) * DAY + 6 * 3600, 10);
    }
    let mut app = App::new(X4, card.build(), Some(NOW * 1000), "", false);
    app.open("stats.overview", Params::new());
    let shown = app.shown();
    assert!(shown.contains("Week\n40 min\n"), "{shown}");
    assert!(shown.contains("Streak\n3 days\nin a row\n"), "{shown}");
    app.open("stats.calendar", Params::new());
    assert!(app.shown().contains("3 days in a row"), "{}", app.shown());
}

#[test]
fn widgets_with_no_reading_draw_and_say_so() {
    let widgets = [
        ("stats.widget.today", "0 min"),
        ("stats.widget.week", "Last 7 days: 0 min"),
        ("stats.widget.goal", "Set a daily goal"),
        ("stats.widget.calendar", "October 2026"),
        ("stats.widget.year", "Books in 2026"),
        ("stats.widget.streak", "Read today to begin one"),
    ];
    for (widget, said) in widgets {
        for clock in [Some(NOW * 1000), None] {
            let config = format!("[home]\npage-1 = \"{widget} 0/0 4x3\"\n");
            let mut app = App::new(X4, MemoryStorage::new(), clock, &config, false);
            let shown = app.shown();
            if clock.is_some() {
                assert!(shown.contains(said), "{widget}: {shown}");
            } else {
                assert!(!shown.trim().is_empty(), "{widget} without a clock");
            }
            // Nothing written for showing nothing.
            assert!(
                !app.storage.0.exists(&format!("{DIR}/pages.bin")),
                "{widget}"
            );
        }
    }
}
