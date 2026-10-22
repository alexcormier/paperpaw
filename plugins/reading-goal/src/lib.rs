//! Reading goal: minutes to read a day (`reading-goal.minutes`, 20 at
//! first), and a cat whose mood follows how much of them were read today:
//! sad while nothing was, meh after a little, content from half the goal
//! (`reading-goal.content`, in percent), happy once it is reached
//! (`reading-goal.happy`).
//!
//! - *Reading cat* (`reading-goal.widget`), a widget for the home screen
//!   and the sleep screen's widgets: the cat of today's mood, as large as
//!   its place leaves, and below it the minutes read and the goal (part
//!   `minutes`). A tap opens the goal's setting.
//! - A sleep screen of its own (*Today's reading cat* among the sleep
//!   screen's choices): the cat large in the middle, the minutes and the
//!   goal below it.
//!
//! The pictures are PaperPaw's cats (see `NOTICE`), or one's own: the
//! folder `reading-goal.pictures` names holds `sad`, `meh`, `content` and
//! `happy` (`.png`, `.jpg`), any size; those missing are the cats'. A data
//! plugin brings a set of its own as such a folder and names it in its
//! `settings.ini` (`[reading-goal] pictures = plugins/<id>/cats`).
//!
//! Today's reading comes from the reading statistics (the `stats`
//! plugin), which tell it as they record it ([`events::READ_TODAY`]);
//! it is kept in `today.txt` (`<day> <seconds>`, version 1). Without
//! them, nothing is counted, and the widget says so.

#![no_std]

extern crate alloc;

mod mood;
mod pictures;
mod sleep;
mod widget;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

use paperpaw_sdk::builtin::{events, groups, slots};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{
    Env, Id, Manifest, Plugin, RegisterError, Registrar, ScreenSpec, SettingExtra, SettingSpec,
    Storage,
};

pub use mood::Mood;

pub const PLUGIN: Id = Id::new_static("reading-goal");
/// Setting: minutes to read a day.
pub const MINUTES: Setting<i64> = Setting::new("reading-goal.minutes");
/// Setting: from which share of the goal the cat is content, in percent.
pub const CONTENT: Setting<i64> = Setting::new("reading-goal.content");
/// Setting: from which share of the goal the cat is happy, in percent.
pub const HAPPY: Setting<i64> = Setting::new("reading-goal.happy");
/// Setting: a folder of one's own pictures for the moods (empty: the
/// cats).
pub const PICTURES: Setting<Text> = Setting::new("reading-goal.pictures");
/// The sleep screen's mode of this plugin (its screen).
pub const SLEEP_SCREEN: Id = Id::new_static(SLEEP_NAME);
const SLEEP_NAME: &str = "reading-goal.sleep";
/// What the sleep screen shows (the `sleep` plugin's setting).
const SLEEP_SHOW: Id = Id::new_static("sleep.show");

const SECTION: &str = "reading-goal.title";
static MINUTES_EXTRA: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .unit("core.unit.minutes")
    .step(5);
static PERCENT: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .unit("core.unit.percent")
    .step(5)
    .slider();
static FOLDER: SettingExtra = SettingExtra::new()
    .section(SECTION)
    .editor_slot(slots::CHOOSE_FOLDER);

static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::fixed_number(
        MINUTES.static_str(),
        "reading-goal.setting.minutes",
        5,
        600,
        20,
        groups::READING,
    )
    .with_extra(&MINUTES_EXTRA)
    .described("reading-goal.setting.minutes.help"),
    SettingSpec::fixed_number(
        CONTENT.static_str(),
        "reading-goal.setting.content",
        5,
        95,
        50,
        groups::READING,
    )
    .advanced()
    .with_extra(&PERCENT)
    .described("reading-goal.setting.content.help"),
    SettingSpec::fixed_number(
        HAPPY.static_str(),
        "reading-goal.setting.happy",
        50,
        200,
        100,
        groups::READING,
    )
    .advanced()
    .with_extra(&PERCENT)
    .described("reading-goal.setting.happy.help"),
    SettingSpec::fixed_text(
        PICTURES.static_str(),
        "reading-goal.setting.pictures",
        "",
        groups::READING,
    )
    .advanced()
    .with_extra(&FOLDER)
    .described("reading-goal.setting.pictures.help"),
];

pub(crate) fn data(name: &str) -> String {
    format!("{}/{name}", Env::plugin_dir(PLUGIN.as_str()))
}

/// What is known of today's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Today {
    /// The goal in minutes.
    pub goal: u32,
    /// Minutes read today.
    pub minutes: u32,
    pub mood: Mood,
    /// The reading statistics are off or missing: nothing is counted.
    pub uncounted: bool,
}

impl Today {
    /// Today as the statistics told it last, with the settings.
    pub(crate) fn now(env: &Env<'_>) -> Today {
        let goal = env.settings.read(&MINUTES).clamp(1, 600) as u32;
        let seconds = match (told(env.storage), today(env)) {
            (Some((day, seconds)), Some(today)) if day == today => seconds,
            _ => 0,
        };
        let content = env.settings.read(&CONTENT).clamp(1, 99) as u32;
        let happy = env.settings.read(&HAPPY).clamp(1, 1000) as u32;
        let stats = env
            .plugins
            .iter()
            .any(|p| p.manifest.id.as_str() == "stats" && p.enabled);
        Today {
            goal,
            minutes: seconds / 60,
            mood: Mood::of(seconds, goal, content, happy),
            uncounted: !stats,
        }
    }
}

/// Today: days since 1970 by the local time, if the device knows it.
fn today(env: &Env<'_>) -> Option<i32> {
    let unix = paperpaw_sdk::time::unix_seconds(env)?;
    Some(paperpaw_sdk::time::local_seconds(env, unix).div_euclid(86_400) as i32)
}

/// The day and the seconds the statistics told last (`today.txt`).
fn told(storage: &dyn Storage) -> Option<(i32, u32)> {
    let text = paperpaw_sdk::read_file(storage, &data("today.txt"), 64).ok()?;
    let text = core::str::from_utf8(&text).ok()?;
    let (day, seconds) = text.trim().split_once(' ')?;
    Some((day.parse().ok()?, seconds.parse().ok()?))
}

/// The reading goal plugin.
#[derive(Debug, Default)]
pub struct ReadingGoal;

impl Plugin for ReadingGoal {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Reading goal", paperpaw_sdk::package_version!())
            .description(
                "Minutes to read a day, and a cat on the home screen whose mood follows today's reading.",
            )
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.on(events::READ_TODAY, |env, read, _| {
            // Only when it changed: a write a page turn at most.
            if told(env.storage) != Some((read.day, read.seconds)) {
                let text = format!("{} {}\n", read.day, read.seconds);
                let _ = env.storage.write(&data("today.txt"), text.as_bytes());
            }
        })?;
        widget::register(r)?;
        r.add_screen(ScreenSpec::new(
            SLEEP_SCREEN,
            "reading-goal.sleep",
            Box::new(|env, _| Box::new(sleep::GoalSleep::new(env))),
        ))?;
        r.add_choice(SLEEP_SHOW, SLEEP_NAME)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
