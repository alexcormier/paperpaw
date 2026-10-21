//! An update that waits for plugins: the update server named a newer
//! PaperPaw, but code plugins in use have no build for it yet
//! (`waiting`), or plugins built in now are not built for it yet
//! (`missing … not-ready`). A search in the background does not offer it;
//! it is kept, shown on the update page, and asked for again once a day
//! at a cheap moment, until it no longer waits. If the reader does not
//! open the update page within a week, the page opens once, at a start
//! (on the X4 every wake), and never again for that version.
//!
//! `waiting.txt` keeps it across starts (version 1): the version, then
//! `since <unix seconds>` (when it began to wait, `-` while the clock is
//! not known), `checked <unix seconds>` (the last answer), and `seen` or
//! `told` lines once the reader looked or the page opened by itself.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;

use paperpaw_sdk::work::Work;
use paperpaw_sdk::{Command, Env, Storage};

use crate::{CHECK_DUE, PLUGIN, file};

const WAITING: &str = "waiting.txt";
/// How long an update waits before the page opens by itself.
pub(crate) const WEEK_SECONDS: i64 = 7 * 24 * 3600;
/// How often the server is asked again while an update waits.
const DAY_SECONDS: i64 = 24 * 3600;

/// What `waiting.txt` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Waiting {
    pub version: String,
    pub since: Option<i64>,
    pub checked: Option<i64>,
    pub seen: bool,
    pub told: bool,
}

impl Waiting {
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let version = lines.next()?.trim().to_string();
        if !crate::online::is_version(&version) {
            return None;
        }
        let mut waiting = Self {
            version,
            ..Self::default()
        };
        let seconds = |rest: &str| rest.trim().parse::<i64>().ok();
        for line in lines {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("since ") {
                waiting.since = seconds(rest);
            } else if let Some(rest) = line.strip_prefix("checked ") {
                waiting.checked = seconds(rest);
            } else if line == "seen" {
                waiting.seen = true;
            } else if line == "told" {
                waiting.told = true;
            }
        }
        Some(waiting)
    }

    fn to_text(&self) -> String {
        let seconds = |s: Option<i64>| s.map_or(String::from("-"), |s| s.to_string());
        let mut text = format!(
            "{}\nsince {}\nchecked {}\n",
            self.version,
            seconds(self.since),
            seconds(self.checked)
        );
        if self.seen {
            text.push_str("seen\n");
        }
        if self.told {
            text.push_str("told\n");
        }
        text
    }
}

pub(crate) fn read(storage: &dyn Storage) -> Option<Waiting> {
    let text = paperpaw_sdk::read_file(storage, &file(WAITING), 1024).ok()?;
    Waiting::parse(&String::from_utf8(text).ok()?)
}

fn write(storage: &dyn Storage, waiting: &Waiting) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&file(WAITING), waiting.to_text().as_bytes());
}

/// Nothing waits (any more).
pub(crate) fn clear(storage: &dyn Storage) {
    let _ = storage.remove(&file(WAITING));
}

/// An answer named `version`, which waits, at `now` (unix seconds, if
/// known): kept, a newer version waiting anew.
pub(crate) fn answered(storage: &dyn Storage, version: &str, now: Option<i64>) {
    let mut waiting = match read(storage) {
        Some(w) if w.version == version => w,
        _ => Waiting {
            version: version.to_string(),
            ..Waiting::default()
        },
    };
    waiting.since = waiting.since.or(now);
    waiting.checked = now.or(waiting.checked);
    write(storage, &waiting);
}

/// The reader looked at the update page.
pub(crate) fn seen(storage: &dyn Storage) {
    if let Some(mut waiting) = read(storage)
        && !waiting.seen
    {
        waiting.seen = true;
        write(storage, &waiting);
    }
}

/// The question again, once a day at a cheap moment.
fn ask_again() -> Command {
    let within = Duration::from_secs(DAY_SECONDS as u64);
    Command::Work(Work::when_cheap(CHECK_DUE.id().clone(), within))
}

/// After an answer that waits: asked again in a day.
pub(crate) fn later(out: &mut Vec<Command>) {
    out.push(ask_again());
}

/// At a start: whether the update page opens by itself (a week waited,
/// the reader did not look, not opened before for this version), and the
/// question asked again where a day passed since the last answer.
pub(crate) fn at_start(storage: &dyn Storage, now: Option<i64>, out: &mut Vec<Command>) -> bool {
    let Some(mut waiting) = read(storage) else {
        return false;
    };
    // The clock known only now: the week counts from here.
    if waiting.since.is_none() && now.is_some() {
        waiting.since = now;
        write(storage, &waiting);
    }
    let due = |at: Option<i64>, seconds: i64| match (at, now) {
        (Some(at), Some(now)) => now - at >= seconds,
        _ => false,
    };
    if waiting.checked.is_none() || due(waiting.checked, DAY_SECONDS) {
        out.push(ask_again());
    }
    if waiting.seen || waiting.told || !due(waiting.since, WEEK_SECONDS) {
        return false;
    }
    waiting.told = true;
    write(storage, &waiting);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_version_1_wrote_is_read() {
        let text = include_str!("../testdata/waiting.txt.v1");
        let waiting = Waiting::parse(text).unwrap();
        assert_eq!(
            waiting,
            Waiting {
                version: String::from("0.5.0"),
                since: Some(1_760_000_000),
                checked: Some(1_760_086_400),
                seen: false,
                told: true,
            }
        );
        assert_eq!(Waiting::parse(&waiting.to_text()), Some(waiting));
        assert_eq!(
            Waiting::parse("0.5.0\nsince -\nchecked -\n").map(|w| w.since),
            Some(None)
        );
        assert_eq!(Waiting::parse("soon\n"), None);
    }

    #[test]
    fn the_page_opens_once_after_a_week_unseen() {
        let storage = paperpaw_sdk::MemoryStorage::new();
        let day = DAY_SECONDS;
        answered(&storage, "0.5.0", Some(1000));
        let mut out = Vec::new();
        assert!(!at_start(&storage, Some(1000 + 6 * day), &mut out));
        assert_eq!(out.len(), 1, "asked again: a day passed");
        assert!(at_start(&storage, Some(1000 + 7 * day), &mut out));
        assert!(!at_start(&storage, Some(1000 + 8 * day), &mut out), "once");
        // A newer version waits anew; looked at, it does not open.
        answered(&storage, "0.6.0", Some(1000 + 8 * day));
        seen(&storage);
        assert!(!at_start(&storage, Some(1000 + 20 * day), &mut out));
        // The clock unknown when it began: counted from the first start
        // that knows it.
        clear(&storage);
        answered(&storage, "0.7.0", None);
        assert!(!at_start(&storage, None, &mut out));
        assert!(!at_start(&storage, Some(50 * day), &mut out));
        assert!(at_start(&storage, Some(57 * day), &mut out));
    }
}
