//! Snapshot tests: the emulator draws screens headless on several device
//! profiles, and each picture is compared with its reference in
//! `tests/snapshots`. Layout faults (text over a button, a cut line, a
//! widget out of place) then show as a failing test.
//!
//! The emulator's clock and battery are fixed, and every run starts from
//! a fresh library with the sample books and the pages of
//! `tests/fixtures` (for what the samples do not show, such as small
//! capitals), so the pictures repeat. A few
//! pixels may differ between computers where glyph edges round
//! differently; more than that fails.
//!
//! After a change meant to alter a screen, write the references anew:
//!
//! ```sh
//! PAPERPAW_BLESS=1 cargo test -p paperpaw-emulator --test snapshots
//! ```
//!
//! and look at the changed pictures before committing them. A failing
//! test leaves what it drew in `target/snapshots`.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A screen to draw: its name, the device profile, settings, input steps.
struct Case {
    name: &'static str,
    profile: &'static str,
    /// The book open at start, if any.
    book: Option<&'static str>,
    sets: &'static [&'static str],
    script: &'static str,
    /// On colour profiles: the share of pixels, in parts per ten thousand,
    /// that must be in colour (ADR 0015). The reference keeps the grays.
    colour: usize,
}

const CASES: &[Case] = &[
    // The first-run setup: its first page, and a number with a sample.
    Case {
        name: "x4-setup",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "x4-setup-size",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "page-forward,page-forward",
        colour: 0,
    },
    Case {
        name: "boox-setup",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "x4-home",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "x4-reader-epub",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "page-forward,page-forward",
        colour: 0,
    },
    Case {
        name: "x4-settings-round-strong",
        profile: "xteink-x4",
        book: None,
        sets: &[
            "slots.core.home=settings.list",
            "core.ui-corners=core.corners.round",
            "core.ui-contrast=core.contrast.strong",
        ],
        script: "right,right",
        colour: 0,
    },
    // The reading settings with a sample of text above them. On the X4
    // (buttons alone, compact rows) the text size changed with the
    // buttons, shown there before it is kept; on the Boox in German, its
    // sliders as touched.
    Case {
        name: "x4-reading-preview",
        profile: "xteink-x4",
        book: None,
        sets: &[],
        script: "run:core.settings,wait:300,right,confirm,wait:300,right,confirm,right,right,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-reading-de",
        profile: "boox-go-7",
        book: None,
        sets: &["core.language=de"],
        script: "swipe:600:1100:600:500,wait:300,tap-on:Einstellungen,wait:500,tap-on:Lesen,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-home",
        profile: "boox-go-7",
        book: None,
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-home-arranged",
        profile: "boox-go-7",
        book: None,
        // Placed in the grid: the clock and the date side by side, the
        // tools in a corner, empty cells between.
        sets: &[
            "home.page-1=home.clock 0/0 2x1, home.date 2/0 2x1, tool:core.library 0/4 1x1, tool:core.settings 3/4 1x1",
            "home.clock.date=false",
            "home.date.align=core.align.right",
        ],
        script: "",
        colour: 0,
    },
    // The ready-made layouts, each shown on the home screen before it is
    // kept, a book read: a look at one's reading on the X4, the clock on
    // the Boox.
    Case {
        name: "x4-home-layouts",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.home,run:home.choose-layout,page-forward,page-forward,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-home-layouts",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.home,run:home.choose-layout,run:core.next-page,run:core.next-page,                 run:core.next-page,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-reader-epub",
        profile: "boox-go-7",
        book: Some("books/die-verwandlung.epub"),
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-reader-txt",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.txt"),
        sets: &["core.language=de"],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-small-caps",
        profile: "boox-go-7",
        book: Some("books/small-caps.html"),
        sets: &[],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-settings",
        profile: "boox-go-7",
        book: None,
        sets: &["slots.core.home=settings.list"],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-widget-editor",
        profile: "boox-go-7",
        book: None,
        // A long press shows the grid; a tap on the clock its options.
        sets: &[],
        script: "long-tap:600:1400,wait:300,tap:600:120,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-gesture-tips",
        profile: "boox-go-7",
        book: None,
        sets: &["home.gesture-tips=true"],
        script: "",
        colour: 0,
    },
    Case {
        name: "boox-developer-settings",
        profile: "boox-go-7",
        book: None,
        // Developer options on: their group of settings.
        sets: &["settings.advanced=true", "core.developer=true"],
        script: "swipe:600:1100:600:500,wait:300,tap-on:Settings,wait:500,tap-on:Developer,wait:300",
        colour: 0,
    },
    Case {
        // The Wi-Fi page (the emulator pretends to manage Wi-Fi on the X4
        // profiles), switched on: the networks found.
        name: "x4-pro-wifi",
        profile: "xteink-x4-pro",
        book: None,
        sets: &[],
        script: "swipe:240:700:240:300,wait:300,tap-on:Settings,wait:500,page-forward,wait:300,                 tap-on:Wi-Fi,wait:500,tap:240:140,wait:2500",
        colour: 0,
    },
    // The plugin store on the X4, its list kept from before (the library
    // asked for does not answer): search, then the plugins for the X4 by
    // group.
    Case {
        name: "x4-store",
        profile: "xteink-x4",
        book: None,
        sets: &["store.catalog=http://127.0.0.1:9/plugins.json"],
        script: "run:store.open,wait:6000",
        colour: 0,
    },
    Case {
        name: "boox-all-tools",
        profile: "boox-go-7",
        book: None,
        // Up on the home screen: every tool.
        sets: &[],
        script: "swipe:600:1100:600:500,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-menu-drag",
        profile: "boox-go-7",
        book: None,
        // The main menu's first entry dragged by its grip to the fourth
        // place (the editor opened by its action: where the settings list
        // it depends on how many groups there are).
        sets: &[],
        script: "run:menu-editor.open,wait:300,tap-on:Main menu,wait:300,                 swipe:1220:159:1220:420,wait:300",
        colour: 0,
    },
    Case {
        name: "boox-lock-pin",
        profile: "boox-go-7",
        book: None,
        // Setting up a screen lock: the keypad for a new PIN.
        sets: &[],
        script: "swipe:600:1100:600:500,wait:300,tap-on:Settings,wait:300,                 tap-on:System,wait:300,tap-on:Screen lock,wait:300,tap-on:PIN,wait:300",
        colour: 0,
    },
    // The quick panel while reading, its sliders a fingertip high and
    // across the screen: on the X4 Pro while a finger moves the light's
    // colour (down far from the knob, which moves as far as the finger,
    // its value shown at once), on the Boox once it is lifted.
    Case {
        name: "x4-pro-quick-panel",
        profile: "xteink-x4-pro",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.quick-panel,wait:300,down:300:410,move:330:415,move:360:420,wait:100",
        colour: 0,
    },
    Case {
        name: "boox-quick-panel",
        profile: "boox-go-7",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:core.quick-panel,wait:300,down:1000:565,move:950:570,move:800:575,up:800:575,wait:300",
        colour: 0,
    },
    // The reader's panel on the X4, the row that opens the fonts focused:
    // its whole name and value on the focus bar, aligned with the rows
    // below.
    Case {
        name: "x4-reader-panel",
        profile: "xteink-x4",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "run:reader.quick-settings,wait:300,right,wait:200",
        colour: 0,
    },
    Case {
        name: "kobo-colour-cover",
        profile: "kobo-libra-colour",
        book: Some("books/alice-in-wonderland.epub"),
        sets: &[],
        script: "",
        colour: 1_000,
    },
];

/// Pixels whose gray differs by more than this count as different.
const LEVELS: u8 = 48;
/// The share of pixels that may differ, in parts per ten thousand.
const ALLOWED: usize = 20;

/// The repository: two folders above this crate.
fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A library with only the sample books (and the book to reopen).
fn library(case: &Case) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "paperpaw-snapshot-{}-{}",
        case.name,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for dir in [root().join("samples/library/books"), fixtures] {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
            }
        }
    }
    // The plugin store's index of the list, as if kept from a visit
    // before.
    if case.name.contains("store") {
        let store = dir.join(".paperpaw/plugins/store");
        fs::create_dir_all(&store).unwrap();
        let kept = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store");
        for entry in fs::read_dir(kept).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), store.join(entry.file_name())).unwrap();
        }
    }
    // The first-run setup only where it is what is shown.
    if !case.name.contains("setup") {
        let setup = dir.join(".paperpaw/plugins/setup");
        fs::create_dir_all(&setup).unwrap();
        fs::write(setup.join("done"), "").unwrap();
    }
    if let Some(book) = case.book {
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

/// The gray of each pixel of a PNG, with its size.
fn grays(path: &Path) -> (u32, u32, Vec<u8>) {
    let (width, height, gray, _) = pixels(path);
    (width, height, gray)
}

/// The gray of each pixel of a PNG, its size, and how many of its pixels
/// are in colour.
fn pixels(path: &Path) -> (u32, u32, Vec<u8>, usize) {
    let decoder = png::Decoder::new(File::open(path).unwrap());
    let mut reader = decoder.read_info().unwrap();
    let mut data = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut data).unwrap();
    let data = &data[..info.buffer_size()];
    let colourful = match info.color_type {
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|p| p.iter().max().unwrap() - p.iter().min().unwrap() > 24)
            .count(),
        _ => 0,
    };
    let gray = match info.color_type {
        png::ColorType::Grayscale => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                    as u8
            })
            .collect(),
        other => panic!("{}: unexpected {other:?}", path.display()),
    };
    (info.width, info.height, gray, colourful)
}

fn write_gray(path: &Path, width: u32, height: u32, gray: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path).unwrap()), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(gray).unwrap();
}

/// Draws `case` and returns its picture in grays, and how many of its
/// pixels are in colour.
fn draw(case: &Case) -> (u32, u32, Vec<u8>, usize) {
    let library = library(case);
    let shot = library.join("shot.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"));
    command
        .args(["--profile", case.profile, "--headless", "--library"])
        .arg(&library)
        .args(["--screenshot"])
        .arg(&shot);
    if !case.script.is_empty() {
        command.args(["--script", case.script]);
    }
    // The hint on gestures would cover every first home screen.
    command.args(["--set", "home.gesture-tips=false"]);
    for set in case.sets {
        command.args(["--set", set]);
    }
    if case.book.is_none() {
        command.args(["--set", "reader.open-last=false"]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}: {}",
        case.name,
        String::from_utf8_lossy(&output.stderr)
    );
    let picture = pixels(&shot);
    let _ = fs::remove_dir_all(&library);
    picture
}

#[test]
fn screens_look_as_before() {
    let bless = std::env::var_os("PAPERPAW_BLESS").is_some();
    let references = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let failed_dir = root().join("target/snapshots");
    let mut failures = Vec::new();
    for case in CASES {
        let (width, height, gray, colourful) = draw(case);
        if colourful < gray.len() * case.colour / 10_000 {
            failures.push(format!(
                "{}: {colourful} pixels in colour, too few",
                case.name
            ));
        }
        let reference = references.join(format!("{}.png", case.name));
        if bless || !reference.exists() {
            write_gray(&reference, width, height, &gray);
            continue;
        }
        let (w, h, expected) = grays(&reference);
        let differing = if (w, h) == (width, height) {
            gray.iter()
                .zip(&expected)
                .filter(|(a, b)| a.abs_diff(**b) > LEVELS)
                .count()
        } else {
            usize::MAX
        };
        if differing > gray.len() * ALLOWED / 10_000 {
            let drawn = failed_dir.join(format!("{}.png", case.name));
            write_gray(&drawn, width, height, &gray);
            failures.push(format!(
                "{}: {} pixels differ (drawn: {})",
                case.name,
                if differing == usize::MAX {
                    format!("size {width}x{height} instead of {w}x{h}, all")
                } else {
                    differing.to_string()
                },
                drawn.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "screens changed; if meant, run with PAPERPAW_BLESS=1:\n{}",
        failures.join("\n")
    );
}

#[test]
fn screens_say_what_they_show() {
    let case = Case {
        name: "describe",
        profile: "boox-go-7",
        book: None,
        sets: &["slots.core.home=settings.list"],
        script: "",
        colour: 0,
    };
    let library = library(&case);
    let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(["--profile", case.profile, "--headless", "--library"])
        .arg(&library)
        .args(["--set", "reader.open-last=false"])
        .args(["--set", "home.gesture-tips=false"])
        .args(["--set", case.sets[0]])
        .args(["--script", "tap-on:Display,wait:300,describe"])
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&library);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The page tapped by its name, its rows with their text and state.
    assert!(text.contains("Heading \"Display\""), "{text}");
    assert!(
        text.contains("Row \"Night mode, White text on black.\" (off)"),
        "{text}"
    );
}

/// The ink of headers is in their middle: their title, notes, arrows and
/// page numbers together leave as much room above them as below (a pixel
/// apart at most), not as their lines of text would, with more room
/// above the letters. On the X4, the X4 Pro and the Boox, on screens with
/// a back arrow, page arrows, a note and an empty list.
#[test]
fn headers_are_centred_by_their_ink() {
    let screens = [
        "run:core.library,wait:300",
        "run:core.settings,wait:300",
        "run:core.library,wait:300,run:files.favourites,wait:300",
        "run:core.library,wait:300,run:files.by-tag,wait:300",
    ];
    let mut failures = Vec::new();
    for profile in ["xteink-x4", "xteink-x4-pro", "boox-go-7"] {
        for (n, script) in screens.iter().enumerate() {
            let case = Case {
                name: "header",
                profile,
                book: None,
                sets: &[],
                script: "",
                colour: 0,
            };
            let library = library(&case);
            let shot = library.join("shot.png");
            let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
                .args(["--profile", profile, "--headless", "--library"])
                .arg(&library)
                .args(["--screenshot"])
                .arg(&shot)
                .args(["--set", "reader.open-last=false"])
                .args(["--set", "home.gesture-tips=false"])
                .args(["--script", &format!("{script},describe")])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let (width, _, gray) = grays(&shot);
            let _ = fs::remove_dir_all(&library);
            // `Heading "…" at x,y wxh`
            let at = text
                .lines()
                .find(|l| l.starts_with("Heading "))
                .and_then(|l| l.rsplit_once(" at "))
                .map(|(_, at)| at.to_string())
                .unwrap_or_else(|| panic!("{profile} {script}: no header in {text}"));
            let numbers: Vec<u32> = at
                .split([',', 'x', ' '])
                .map(|n| n.trim().parse().unwrap())
                .collect();
            let [x, y, w, h] = numbers[..] else {
                panic!("{at}");
            };
            // Each row's dark pixels; a row dark across the header is its
            // line below, the header's end.
            let dark = |row: u32| {
                (x..x + w)
                    .filter(|col| gray[(row * width + col) as usize] < 128)
                    .count() as u32
            };
            let end = (y..y + h)
                .find(|row| dark(*row) >= w * 9 / 10)
                .unwrap_or(y + h);
            let inked: Vec<u32> = (y..end).filter(|row| dark(*row) > 0).collect();
            let (Some(first), Some(last)) = (inked.first(), inked.last()) else {
                failures.push(format!("{profile} screen {n}: no ink in the header"));
                continue;
            };
            let above = first - y;
            let below = end - last - 1;
            if above.abs_diff(below) > 1 {
                failures.push(format!(
                    "{profile} screen {n} ({script}): {above} px above the ink, {below} below"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
