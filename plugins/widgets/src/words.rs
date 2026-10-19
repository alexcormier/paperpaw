//! Words and one's own: the quote of the day (`quote`), a picture from
//! the card (`picture`), a note (`note`), a to-do list (`todo`) and a
//! greeting by the time of day (`greeting`).
//!
//! What a widget keeps of its own (its own quotes, the to-do list, which
//! picture or quote is turned to) is in the plugin's folder,
//! `.paperpaw/plugins/widgets`. A tap that changes what a widget shows
//! (the next quote, an item ticked off) writes it there and has the
//! screen read its widgets anew.

mod greeting;
mod note;
mod picture;
mod quote;
mod text;
mod todo;

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets;
use paperpaw_sdk::{Canvas, Command, Env, Id, Rect, RegisterError, Registrar, Storage, time};

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/words.en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/words.de.ini")))?;
    quote::register(r)?;
    picture::register(r)?;
    note::register(r)?;
    todo::register(r)?;
    greeting::register(r)?;
    Ok(())
}

/// The path of `name` in the plugin's folder.
fn data(name: &str) -> String {
    format!(
        "{}/{name}",
        paperpaw_sdk::Env::plugin_dir(crate::PLUGIN.as_str())
    )
}

/// Counts the changes of what these widgets keep (see [`changed`]): a
/// widget that read it at another count reads it anew.
static CHANGES: AtomicU32 = AtomicU32::new(0);

/// The changes so far.
fn changes() -> u32 {
    CHANGES.load(Ordering::Relaxed)
}

/// What widget `id` keeps changed (a quote turned, an item ticked off):
/// the screens showing it read it anew.
fn changed(out: &mut Vec<Command>, id: &Id) {
    // Load and store: the device has no atomic adding, and one thread.
    CHANGES.store(changes().wrapping_add(1), Ordering::Relaxed);
    out.push(widgets::changed(id));
}

/// Today as days since 1970 in the local time zone; 0 where the device
/// does not know the date.
fn today(env: &Env<'_>) -> i64 {
    time::unix_seconds(env).map_or(0, |unix| time::local_seconds(env, unix).div_euclid(86_400))
}

/// The height `widget`'s frame takes above and below its content.
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 100, 100);
    100 - widgets::look::look(env, widget, probe).area.height
}

/// The one of `count` things shown on `day`: each of them once before
/// any comes again, neighbours far apart (the order of a file says little
/// about the order of the days).
fn of_the_day(day: i64, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    // A prime: with any count it does not divide, a step that reaches
    // every one.
    const STEP: u64 = 7919;
    let step = if (count as u64).is_multiple_of(STEP) {
        1
    } else {
        STEP
    };
    let day = day.rem_euclid(count as i64) as u64;
    (day * step % count as u64) as usize
}

/// How far a widget was turned on (to the next quote, the next picture),
/// kept in its file: the day it was turned on and how often since.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Turns {
    day: i64,
    turns: u32,
}

impl Turns {
    /// As kept at `path` (`<day> <turns>`); none if not kept.
    fn load(storage: &dyn Storage, path: &str) -> Self {
        let Ok(data) = paperpaw_sdk::read_file(storage, path, 64) else {
            return Self::default();
        };
        let text = String::from_utf8_lossy(&data);
        let mut numbers = text.split_whitespace();
        let day = numbers.next().and_then(|d| d.parse().ok()).unwrap_or(0);
        let turns = numbers.next().and_then(|t| t.parse().ok()).unwrap_or(0);
        Self { day, turns }
    }

    fn save(&self, storage: &dyn Storage, path: &str) {
        let _ = storage.write(path, format!("{} {}\n", self.day, self.turns).as_bytes());
    }

    /// The turns of `today`: none since the day began, where what is shown
    /// changes daily.
    fn on(self, today: i64, daily: bool) -> u32 {
        if daily && self.day != today {
            0
        } else {
            self.turns
        }
    }

    /// Turned on once more on `today`, kept at `path`.
    fn turn(storage: &dyn Storage, path: &str, today: i64, daily: bool) {
        let turns = Self::load(storage, path).on(today, daily);
        Self {
            day: today,
            turns: turns.wrapping_add(1),
        }
        .save(storage, path);
    }
}

/// Calls `line` with each line of the file at `path` (no longer than
/// `MAX_LINE` bytes, longer ones cut there), read a little at a time, so
/// that a long file never is in memory at once; `line` returns `false` to
/// stop.
fn each_line(storage: &dyn Storage, path: &str, line: &mut dyn FnMut(&str) -> bool) {
    const MAX_LINE: usize = 1024;
    // Enough for thousands of lines; beyond, nothing is read.
    const MAX_FILE: u64 = 512 * 1024;
    let mut chunk = [0u8; 256];
    let mut current: Vec<u8> = Vec::new();
    let mut offset = 0u64;
    let mut done = false;
    let mut flush = |current: &mut Vec<u8>, done: &mut bool| {
        if !*done {
            let text = String::from_utf8_lossy(current);
            *done = !line(text.trim_end_matches('\r'));
        }
        current.clear();
    };
    while !done && offset < MAX_FILE {
        let Ok(n) = storage.read_at(path, offset, &mut chunk) else {
            return;
        };
        if n == 0 {
            break;
        }
        offset += n as u64;
        for &byte in &chunk[..n] {
            if byte == b'\n' {
                flush(&mut current, &mut done);
            } else if current.len() < MAX_LINE {
                current.push(byte);
            }
        }
    }
    if !current.is_empty() {
        flush(&mut current, &mut done);
    }
}

/// Draws `text` in the middle of `look`'s area, e.g. how to add what
/// a widget would show: in the body size where it fits, else small.
fn draw_empty(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &widgets::Look, text: &str) {
    let style = UiStyle::new(env);
    let area = look.area;
    let fits = style.font(env).is_some_and(|font| {
        let lines = font.wrap(style.text, text, f32::from(area.width)).len() as u16;
        lines * widgets::line_height(env, style.text) <= area.height
    });
    if fits {
        widgets::draw_message(canvas, env, look, text);
    } else {
        let center = widgets::Align::Center;
        widgets::draw_wrapped(canvas, env, area, center, style.small, text, look.ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn each_day_has_its_own_until_all_were_shown() {
        for count in [1usize, 2, 7, 60, 86, 7919] {
            let mut seen = alloc::vec![false; count];
            for day in 20_000..20_000 + count as i64 {
                let pick = of_the_day(day, count);
                assert!(pick < count);
                assert!(!seen[pick], "{count}: {pick} twice");
                seen[pick] = true;
            }
        }
        // The same each time on the same day; another the next.
        assert_eq!(of_the_day(20_375, 86), of_the_day(20_375, 86));
        assert_ne!(of_the_day(20_375, 86), of_the_day(20_376, 86));
        assert_eq!(of_the_day(-3, 5), of_the_day(2, 5));
        assert_eq!(of_the_day(5, 0), 0);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn turns_of_version_1_are_read() {
        let storage = MemoryStorage::new().with("t.txt", include_str!("../testdata/turns.txt.v1"));
        let turns = Turns::load(&storage, "t.txt");
        assert_eq!(
            turns,
            Turns {
                day: 20_375,
                turns: 3
            }
        );
    }

    #[test]
    fn turns_begin_again_each_day() {
        let storage = MemoryStorage::new();
        let path = data("turns.txt");
        Turns::turn(&storage, &path, 100, true);
        Turns::turn(&storage, &path, 100, true);
        assert_eq!(Turns::load(&storage, &path).on(100, true), 2);
        assert_eq!(Turns::load(&storage, &path).on(101, true), 0);
        // Not daily: they go on counting.
        assert_eq!(Turns::load(&storage, &path).on(101, false), 2);
        Turns::turn(&storage, &path, 101, true);
        assert_eq!(Turns::load(&storage, &path), Turns { day: 101, turns: 1 });
    }

    #[test]
    fn lines_are_read_a_little_at_a_time() {
        let long = "x".repeat(2000);
        let text = format!("first\r\n\nsecond line\n{long}\nlast");
        let storage = MemoryStorage::new().with("f.txt", text.as_str());
        let mut lines: Vec<String> = Vec::new();
        each_line(&storage, "f.txt", &mut |line| {
            lines.push(String::from(line));
            true
        });
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "first");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "second line");
        assert_eq!(lines[3].len(), 1024);
        assert_eq!(lines[4], "last");
        // Stopped at the second.
        let mut count = 0;
        each_line(&storage, "f.txt", &mut |_| {
            count += 1;
            count < 2
        });
        assert_eq!(count, 2);
        // No file: no lines.
        each_line(&storage, "none.txt", &mut |_| panic!("no lines"));
    }
}
