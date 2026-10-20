//! Memory on the small devices: the emulator charges what the kernel
//! and its plugins allocate against the profile's heap budget (256 KiB
//! on the X4, see `src/budget.rs`) and refuses what goes past it, as the
//! device would. Here the main flows run headless on the X4 profiles: a
//! book opened (EPUB and TXT), 30 pages forward and 30 back, home, the
//! library (with many books) opened and scrolled, the main menu, the
//! settings and a page of them.
//!
//! Each flow runs twice, under the profile's budget and without one. The
//! runs are deterministic (simulated time, a fixed clock), so the same
//! peak in both says no allocation was refused under the budget; the peak
//! must also stay a margin below the budget, room for what the emulator
//! does not model (fragmentation, the stack) and for growth to be noticed
//! before it fails on a device.
//!
//! A budget too small stops a run as the device would, and the heap
//! report lists what is live by call site.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The share of the budget, in percent, the peak may reach at most.
const MARGIN_PERCENT: f64 = 85.0;

/// The repository: two folders above this crate.
fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A fresh library named `name`: the sample books, sixty small ones more
/// so that the library has pages to scroll, and `book` open at start.
fn library(name: &str, book: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-memory-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    for entry in fs::read_dir(root().join("samples/library/books")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
    }
    for n in 0..60 {
        fs::write(
            books.join(format!("book-{n:02}.txt")),
            format!("Book {n}\n\nA short book, number {n}, to fill the library.\n"),
        )
        .unwrap();
    }
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    if let Some(book) = book {
        let state = dir.join(".paperpaw/plugins/reader");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("state.ini"),
            format!("[reader]\nlast = {book}\n"),
        )
        .unwrap();
    }
    dir
}

/// Runs the emulator headless on `profile` in a fresh library.
fn emulator(name: &str, profile: &str, book: Option<&str>, args: &[&str]) -> Output {
    let library = library(name, book);
    let mut command = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"));
    command
        .args(["--profile", profile, "--headless", "--library"])
        .arg(&library)
        .args(["--set", "home.gesture-tips=false"])
        .args(args);
    if book.is_none() {
        command.args(["--set", "reader.open-last=false"]);
    }
    let output = command.output().unwrap();
    let _ = fs::remove_dir_all(&library);
    output
}

/// What the last line of a run says: the screen shown, the heap's peak
/// and the budget (KiB), e.g.
/// `xteink-x4: screen home.dashboard, heap 119.3 KiB / 256 KiB (peak 188.3 KiB)`.
#[derive(Debug, PartialEq)]
struct Heap {
    screen: String,
    peak: f64,
    limit: Option<f64>,
}

fn heap(output: &Output) -> Heap {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout.lines().last().expect("a summary line");
    let number = |after: &str| -> Option<f64> {
        let rest = &line[line.find(after)? + after.len()..];
        rest.split_whitespace().next()?.parse().ok()
    };
    let screen = line
        .split("screen ")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or_default()
        .to_string();
    Heap {
        screen,
        peak: number("(peak ").expect(line),
        limit: number(" / "),
    }
}

/// `count` presses of the button `step`.
fn times(step: &str, count: usize) -> String {
    vec![step; count].join(",")
}

/// Reads 30 pages forward and 30 back, goes home, opens the library and
/// scrolls it, the main menu, the settings and the first page of them.
/// Actions run as buttons bound to them would, so the same flow works on
/// both X4 profiles (the X4 Pro has no *back* button).
fn reading_flow() -> String {
    [
        times("page-forward", 30),
        times("page-back", 30),
        "run:core.home,wait:300,run:core.library,wait:300".into(),
        times("page-forward", 3),
        "page-back,run:core.back,run:core.menu,wait:300,run:core.back".into(),
        "run:core.settings,wait:300,run:core.down,run:core.confirm,wait:300".into(),
        "run:core.back,run:core.back,run:core.home".into(),
    ]
    .join(",")
}

/// Runs `flow` with `book` on `profile` under its budget and without one,
/// and checks the peak.
fn stays_within_budget(name: &str, profile: &str, book: &str) -> Heap {
    let flow = reading_flow();
    let budgeted = heap(&emulator(name, profile, Some(book), &["--script", &flow]));
    let free = heap(&emulator(
        name,
        profile,
        Some(book),
        &["--heap-budget", "none", "--script", &flow],
    ));
    let limit = budgeted.limit.expect("the profile has a budget");
    assert_eq!(free.limit, None);
    assert_eq!(budgeted.screen, "home.dashboard", "the flow went home");
    assert_eq!(free.screen, budgeted.screen);
    // The same allocations with and without the budget: none refused.
    assert!(
        (budgeted.peak - free.peak).abs() < 0.05,
        "{name}: peak {} KiB under the budget, {} KiB without",
        budgeted.peak,
        free.peak
    );
    assert!(
        free.peak <= limit * MARGIN_PERCENT / 100.0,
        "{name}: peak {} KiB of {limit} KiB, over {MARGIN_PERCENT} %",
        free.peak
    );
    budgeted
}

#[test]
fn reading_an_epub_stays_within_the_x4_budget() {
    let heap = stays_within_budget("x4-epub", "xteink-x4", "books/alice-in-wonderland.epub");
    assert_eq!(heap.limit, Some(256.0));
}

#[test]
fn reading_a_txt_stays_within_the_x4_budget() {
    stays_within_budget("x4-txt", "xteink-x4", "books/die-verwandlung.txt");
}

/// The X4 Pro: its own panel (four grays, other bands), touch beside
/// the buttons.
#[test]
fn reading_an_epub_stays_within_the_x4_pro_budget() {
    stays_within_budget("x4-pro-epub", "xteink-x4-pro", "books/die-verwandlung.epub");
}

/// A budget below what the app needs to start: the allocation that does
/// not fit is refused and the run ends with an error, as the device
/// would stop, never quietly over the budget.
#[test]
fn a_budget_too_small_stops_the_run() {
    let output = emulator("too-small", "xteink-x4", None, &["--heap-budget", "64"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("memory allocation of"), "{stderr}");
}

/// A budget from the command line wins over the profile's, and `none`
/// lifts it: the large screens' profiles have none to begin with.
#[test]
fn a_budget_given_wins_over_the_profiles() {
    let given = heap(&emulator(
        "given",
        "xteink-x4",
        None,
        &["--heap-budget", "300"],
    ));
    assert_eq!(given.limit, Some(300.0));
    let boox = heap(&emulator("boox", "boox-go-7", None, &[]));
    assert_eq!(boox.limit, None);
    let capped = heap(&emulator(
        "boox-capped",
        "boox-go-7",
        None,
        &["--heap-budget", "4096"],
    ));
    assert_eq!(capped.limit, Some(4096.0));
    assert!(capped.peak <= 4096.0);
}

/// The heap report after a run: the live heap by call site, largest
/// first, adding up to what the summary line says is live.
#[test]
fn the_heap_report_lists_the_live_heap_by_call_site() {
    let output = emulator("report", "xteink-x4", None, &["--heap-report"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let summary = stdout
        .lines()
        .find(|l| l.starts_with("xteink-x4: screen"))
        .expect("the summary line");
    let live: f64 = summary
        .split("heap ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .expect(summary);
    let header = stdout
        .lines()
        .find(|l| l.starts_with("live heap by call site ("))
        .expect("the report's heading");
    let total: f64 = header
        .trim_start_matches("live heap by call site (")
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .expect(header);
    assert!(
        (total - live).abs() < 0.2,
        "report {total} KiB, live {live} KiB"
    );
    // The sites: largest first, in PaperPaw's code.
    let sizes: Vec<f64> = stdout
        .lines()
        .skip_while(|l| !l.starts_with("live heap by call site"))
        .skip(1)
        .filter(|l| l.contains(" KiB "))
        .map(|l| l.split_whitespace().next().unwrap().parse().unwrap())
        .collect();
    assert!(sizes.len() > 3, "{stdout}");
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
    assert!(stdout.contains("paperpaw_"), "{stdout}");
}
