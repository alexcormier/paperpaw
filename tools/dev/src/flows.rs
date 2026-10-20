//! Scripted flows: steps (presses, taps, swipes) with assertions about
//! what the screen shows, written by hand or recorded in the emulator's
//! window (`paperpaw-emu --record <file>`), replayed on every profile they
//! fit (`paperpaw-dev flows --flow a.flow --flow b.flow`).
//!
//! # The format
//!
//! A flow is a text file, one step a line; `#` starts a comment, empty
//! lines are left out. Lines before the first step may say where it runs
//! and how the device is set up:
//!
//! ```text
//! # Night mode from the menu, with buttons.
//! profiles: xteink-x4, xteink-x4-classic
//! set: core.ui-corners = core.corners.square
//!
//! long:confirm
//! expect screen menu.list
//! expect focus "Home"
//! right
//! expect no text "Reading statistics"
//! ```
//!
//! - `profiles: a, b`: only on these device profiles (of those the run
//!   looks at). Without it a flow runs on every profile it fits: one with
//!   every button it presses, and a touch screen if it taps or swipes.
//! - `set: <id> = <value>`: a setting, as `--set` gives it, before the
//!   app starts (repeatable).
//!
//! Steps are those of the emulator's scripts (`paperpaw-emu --script`,
//! see `shells/desktop/src/headless.rs`), one a line: a button's name for
//! a short press (`confirm`), `long:<button>`, `tap:x:y`, `tap-on:<text>`
//! (the row, button or heading with this text or beginning so; commas
//! are fine here), `long-tap:x:y`, `swipe:x1:y1:x2:y2`, `down:x:y`,
//! `move:x:y`, `up:x:y`, `wait:<ms>`, `run:<action>`,
//! `open:<screen>:<key>=<value>...`, `redraw`, `shot:<file>`. Time passes
//! as the steps take it (100 ms a step) and with `wait:`.
//!
//! Assertions begin with `expect`; texts may be quoted:
//!
//! | Assertion | Passes when |
//! |-----------|-------------|
//! | `expect screen <id>` | the screen shown is `<id>` |
//! | `expect stack <n>` | `<n>` screens are open (the home screen alone is 1) |
//! | `expect text "Hello"` | a part of the screen has this text in it |
//! | `expect no text "Hello"` | none has |
//! | `expect focus "Night mode"` | the part with the focus has this text, or one beginning so (case aside) |
//! | `expect on "Night mode"`, `expect off ...` | the switch (or the row with it) of this text is on, off |
//! | `expect setting <id> = <value>` | the setting has this value (as configuration files write it) |
//! | `expect no warnings` | the kernel warned about nothing since the flow began |
//! | `expect picture <name>` | the screen looks as `<snapshots>/<profile>/<name>.png` (written anew with `--bless`) |
//!
//! The pictures are in `--snapshots` (else `tests/snapshots`, else a
//! `snapshots` folder beside the flow). A flow stops at the first step or
//! assertion that fails; that is reported as `flows.failed` with the
//! line, what was expected, what was there and a picture of the screen.

use std::path::{Path, PathBuf};

use paperpaw_core::sdk::SettingValue;
use paperpaw_core::sdk::semantics::{self, Part};
use paperpaw_emulator::headless;
use paperpaw_emulator::profiles::Profile;
use paperpaw_platform::Rect;

use crate::cli::Context;
use crate::image::Image;
use crate::report::{Finding, Report};
use crate::session::{Session, Start};
use crate::variant::Variant;

/// Steps that need a touch screen.
const TOUCH: &[&str] = &["tap", "tap-on", "long-tap", "swipe", "down", "move", "up"];

/// Steps that are no button's name.
const NAMED: &[&str] = &[
    "long", "tap", "tap-on", "long-tap", "swipe", "down", "move", "up", "wait", "run", "open",
    "redraw", "shot", "serve", "describe",
];

/// A flow read from its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    /// Its file's name without `.flow`.
    pub name: String,
    /// Where it was read from (for the snapshots beside it).
    pub path: PathBuf,
    /// The profiles it is for; empty: every one it fits.
    pub profiles: Vec<String>,
    /// Settings before the app starts, as `(id, value)`.
    pub settings: Vec<(String, String)>,
    pub lines: Vec<Line>,
}

/// A step or assertion, with where it is in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// From 1.
    pub number: usize,
    /// As written.
    pub text: String,
    pub step: Step,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A step of the emulator's scripts.
    Script(String),
    /// A tap on the part with this text.
    TapOn(String),
    Expect(Expect),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    Screen(String),
    Stack(usize),
    Text(String),
    NoText(String),
    Focus(String),
    /// A switch on (`true`) or off.
    Switch(String, bool),
    Setting(String, String),
    NoWarnings,
    Picture(String),
}

impl Flow {
    /// Reads the flow in `path`.
    pub fn read(path: &Path) -> Result<Flow, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("flow"));
        let mut flow = parse(&name, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        flow.path = path.to_path_buf();
        Ok(flow)
    }

    /// Whether the flow runs on `profile`, else why not.
    pub fn fits(&self, profile: &Profile) -> Result<(), String> {
        let id = profile.device.id;
        if !self.profiles.is_empty() && !self.profiles.iter().any(|p| p == id) {
            return Err(format!("it is for {}", self.profiles.join(", ")));
        }
        for line in &self.lines {
            let word = match &line.step {
                Step::Script(step) => step.split(':').next().unwrap_or_default(),
                Step::TapOn(_) => "tap-on",
                Step::Expect(_) => continue,
            };
            if TOUCH.contains(&word) && !profile.device.input.touch {
                return Err(format!("line {} needs a touch screen", line.number));
            }
            let button = match &line.step {
                Step::Script(step) if !NAMED.contains(&word) => Some(step.as_str()),
                Step::Script(step) if word == "long" => step.split(':').nth(1),
                _ => None,
            };
            if let Some(button) = button
                && !profile.buttons.contains(&button)
            {
                return Err(format!(
                    "line {} presses `{button}`, which it does not have",
                    line.number
                ));
            }
        }
        Ok(())
    }
}

/// Reads a flow from its text (see the module's docs).
pub fn parse(name: &str, text: &str) -> Result<Flow, String> {
    let mut flow = Flow {
        name: name.to_string(),
        path: PathBuf::new(),
        profiles: Vec::new(),
        settings: Vec::new(),
        lines: Vec::new(),
    };
    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |e: String| format!("line {number}: {e}");
        if let Some(rest) = header(line, "profiles") {
            if !flow.lines.is_empty() {
                return Err(at("`profiles:` belongs before the first step".into()));
            }
            flow.profiles.extend(
                rest.split(',')
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(String::from),
            );
            continue;
        }
        if let Some(rest) = header(line, "set") {
            if !flow.lines.is_empty() {
                return Err(at("`set:` belongs before the first step".into()));
            }
            let (id, value) = rest
                .split_once('=')
                .ok_or_else(|| at(format!("`set:` expects <id> = <value>, got `{rest}`")))?;
            flow.settings
                .push((id.trim().to_string(), value.trim().to_string()));
            continue;
        }
        let step = step(line).map_err(at)?;
        flow.lines.push(Line {
            number,
            text: line.to_string(),
            step,
        });
    }
    if flow.lines.is_empty() {
        return Err("no steps".into());
    }
    Ok(flow)
}

/// The rest of a header line `<name>: rest`.
fn header<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.strip_prefix(name)?
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
}

/// A text, without the quotes around it.
fn unquoted(text: &str) -> String {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

fn step(line: &str) -> Result<Step, String> {
    if let Some(rest) = line.strip_prefix("expect ") {
        return expect(rest.trim()).map(Step::Expect);
    }
    if let Some(label) = line.strip_prefix("tap-on:") {
        let label = unquoted(label);
        if label.is_empty() {
            return Err("`tap-on:` needs a text".into());
        }
        return Ok(Step::TapOn(label));
    }
    if line.contains(',') {
        return Err(format!("`{line}`: one step a line"));
    }
    let parts: Vec<&str> = line.split(':').collect();
    let numbers = |n: usize| {
        parts.len() == n + 1 && parts[1..].iter().all(|p| p.trim().parse::<i64>().is_ok())
    };
    let valid = match parts[0] {
        "tap" | "long-tap" | "down" | "move" | "up" => numbers(2),
        "swipe" => numbers(4),
        "wait" | "serve" => numbers(1),
        "long" | "run" => parts.len() == 2 && !parts[1].is_empty(),
        "open" | "shot" => parts.len() >= 2 && !parts[1].is_empty(),
        "redraw" | "describe" => parts.len() == 1,
        word => {
            parts.len() == 1
                && !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
    };
    if !valid {
        return Err(format!("`{line}` is no step (see `paperpaw-emu --help`)"));
    }
    Ok(Step::Script(line.to_string()))
}

fn expect(rest: &str) -> Result<Expect, String> {
    let (word, arg) = rest.split_once(' ').unwrap_or((rest, ""));
    let arg = arg.trim();
    let needs = |what: &str| -> Result<String, String> {
        let text = unquoted(arg);
        if text.is_empty() {
            Err(format!("`expect {word}` needs {what}"))
        } else {
            Ok(text)
        }
    };
    Ok(match word {
        "screen" => Expect::Screen(needs("a screen's id")?),
        "stack" => Expect::Stack(
            arg.parse()
                .map_err(|_| format!("`expect stack` needs a number, got `{arg}`"))?,
        ),
        "text" => Expect::Text(needs("a text")?),
        "focus" => Expect::Focus(needs("a text")?),
        "on" => Expect::Switch(needs("a text")?, true),
        "off" => Expect::Switch(needs("a text")?, false),
        "setting" => {
            let (id, value) = arg
                .split_once('=')
                .ok_or_else(|| format!("`expect setting` expects <id> = <value>, got `{arg}`"))?;
            Expect::Setting(id.trim().to_string(), unquoted(value))
        }
        "picture" => Expect::Picture(needs("a name")?),
        "no" => {
            let (what, text) = arg.split_once(' ').unwrap_or((arg, ""));
            match what {
                "warnings" if text.is_empty() => Expect::NoWarnings,
                "text" if !unquoted(text).is_empty() => Expect::NoText(unquoted(text)),
                _ => {
                    return Err(format!(
                        "`expect no {arg}`: `no text \"...\"` or `no warnings`"
                    ));
                }
            }
        }
        _ => {
            return Err(format!(
                "`expect {word}`: screen, stack, text, no text, focus, on, off, setting, \
                 no warnings or picture"
            ));
        }
    })
}

/// Where the reference pictures are and whether they are written anew.
#[derive(Debug, Clone)]
pub struct Pictures {
    pub dir: PathBuf,
    pub bless: bool,
}

/// What failed, and how the screen looked then.
#[derive(Debug, Clone)]
pub struct Failure {
    pub line: usize,
    pub text: String,
    pub expected: String,
    pub found: String,
    pub image: Image,
    /// Where in the picture, if somewhere in particular.
    pub area: Option<Rect>,
    /// The screen shown.
    pub screen: Option<String>,
}

/// How a flow went on one device.
#[derive(Debug, Default)]
pub struct Played {
    /// Pictures written anew (`--bless`).
    pub blessed: Vec<PathBuf>,
    pub failure: Option<Failure>,
}

/// Plays `flow` on `session` (started as the flow says, see [`start`]).
pub fn play(flow: &Flow, session: &mut Session, pictures: &Pictures) -> Played {
    let mut played = Played::default();
    let warned = session.device.warnings.len();
    for line in &flow.lines {
        let result = match &line.step {
            Step::Script(step) => headless::run_script(&mut session.device, step)
                .map_err(|e| (String::from("the step to work"), e, None)),
            Step::TapOn(label) => tap_on(session, label),
            Step::Expect(expect) => check(expect, session, warned, pictures, &mut played),
        };
        if let Err((expected, found, area)) = result {
            played.failure = Some(Failure {
                line: line.number,
                text: line.text.clone(),
                expected,
                found,
                image: session.image(),
                area,
                screen: session.screen(),
            });
            break;
        }
    }
    played
}

/// What failed: expected, found, where.
type Failed = (String, String, Option<Rect>);

/// A tap in the middle of the part with `label`.
fn tap_on(session: &mut Session, label: &str) -> Result<(), Failed> {
    let parts = session.parts();
    let Some(part) = semantics::find(&parts, label) else {
        return Err((
            format!("a row, button or heading saying `{label}`"),
            texts(&parts),
            None,
        ));
    };
    let a = part.area;
    let step = format!(
        "tap:{}:{}",
        i32::from(a.x) + i32::from(a.width) / 2,
        i32::from(a.y) + i32::from(a.height) / 2
    );
    headless::run_script(&mut session.device, &step)
        .map_err(|e| (String::from("the tap to work"), e, Some(a)))
}

/// The texts of `parts`, for saying what was there.
fn texts(parts: &[Part]) -> String {
    let texts: Vec<String> = parts
        .iter()
        .filter(|p| !p.label.trim().is_empty())
        .map(|p| format!("\"{}\"", p.label))
        .collect();
    if texts.is_empty() {
        String::from("no text")
    } else {
        format!("the texts {}", shortened(&texts.join(", "), 300))
    }
}

fn shortened(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut short: String = text.chars().take(most).collect();
    short.push_str("...");
    short
}

fn value_text(value: Option<&SettingValue>) -> String {
    match value {
        Some(SettingValue::Bool(b)) => b.to_string(),
        Some(SettingValue::Int(n)) => n.to_string(),
        Some(SettingValue::Text(t)) => format!("`{t}`"),
        None => String::from("no value"),
        Some(_) => String::from("a value"),
    }
}

fn check(
    expect: &Expect,
    session: &mut Session,
    warned: usize,
    pictures: &Pictures,
    played: &mut Played,
) -> Result<(), Failed> {
    let fail = |expected: String, found: String| Err((expected, found, None));
    match expect {
        Expect::Screen(id) => {
            let shown = session.screen();
            if shown.as_deref() != Some(id.as_str()) {
                return fail(
                    format!("the screen `{id}`"),
                    format!("`{}`", shown.unwrap_or_default()),
                );
            }
        }
        Expect::Stack(n) => {
            let stack = session.stack();
            if stack.len() != *n {
                return fail(
                    format!("{n} screens open"),
                    format!("{} ({})", stack.len(), stack.join(", ")),
                );
            }
        }
        Expect::Text(text) => {
            let parts = session.parts();
            if !parts.iter().any(|p| p.label.contains(text.as_str())) {
                return fail(format!("the text \"{text}\""), texts(&parts));
            }
        }
        Expect::NoText(text) => {
            let parts = session.parts();
            if let Some(part) = parts.iter().find(|p| p.label.contains(text.as_str())) {
                return Err((
                    format!("no text \"{text}\""),
                    format!("{:?} \"{}\"", part.role, part.label),
                    Some(part.area),
                ));
            }
        }
        Expect::Focus(text) => {
            let parts = session.parts();
            let wanted = text.to_lowercase();
            match semantics::focused(&parts) {
                Some(part) if part.label.to_lowercase().starts_with(&wanted) => {}
                Some(part) => {
                    return Err((
                        format!("the focus on \"{text}\""),
                        format!("it on {:?} \"{}\"", part.role, part.label),
                        Some(part.area),
                    ));
                }
                None => return fail(format!("the focus on \"{text}\""), "no focus".into()),
            }
        }
        Expect::Switch(text, on) => {
            let parts = session.parts();
            let state = |on: bool| if on { "on" } else { "off" };
            let Some(part) = semantics::find(&parts, text) else {
                return fail(format!("a switch \"{text}\""), texts(&parts));
            };
            if part.on != Some(*on) {
                let found = match part.on {
                    Some(was) => format!("it {}", state(was)),
                    None => format!("{:?} \"{}\", no switch", part.role, part.label),
                };
                return Err((format!("\"{text}\" {}", state(*on)), found, Some(part.area)));
            }
        }
        Expect::Setting(id, text) => {
            let settings = session.device.kernel.settings();
            let Some(spec) = settings.spec(id) else {
                return fail(format!("a setting `{id}`"), "no such setting".into());
            };
            let Some(value) = spec.kind.parse(text) else {
                return fail(
                    format!("`{id}` = {text}"),
                    format!("`{text}` is no value of it"),
                );
            };
            let now = settings.get(id);
            if now != Some(&value) {
                return fail(format!("`{id}` = {text}"), value_text(now));
            }
        }
        Expect::NoWarnings => {
            let warnings: Vec<String> = session.device.warnings
                [warned.min(session.device.warnings.len())..]
                .iter()
                .map(ToString::to_string)
                .collect();
            if !warnings.is_empty() {
                return fail(
                    String::from("no warnings"),
                    shortened(&warnings.join("; "), 400),
                );
            }
        }
        Expect::Picture(name) => {
            let file = pictures
                .dir
                .join(session.variant.profile.device.id)
                .join(format!("{name}.png"));
            let image = session.image();
            if pictures.bless {
                image.save_png(&file).map_err(|e| {
                    (
                        format!("to write {}", crate::report::slashed(&file)),
                        e,
                        None,
                    )
                })?;
                played.blessed.push(file);
                return Ok(());
            }
            if !file.is_file() {
                return fail(
                    format!("the picture {}", crate::report::slashed(&file)),
                    String::from("none there yet: run with --bless to write it"),
                );
            }
            let reference = Image::load_png(&file).map_err(|e| {
                (
                    format!("the picture {}", crate::report::slashed(&file)),
                    e,
                    None,
                )
            })?;
            let differing = image.differing(&reference, 0);
            if differing > 0 {
                return Err((
                    format!("the screen as in {}", crate::report::slashed(&file)),
                    format!("{differing} pixels differing"),
                    difference(&image, &reference),
                ));
            }
        }
    }
    Ok(())
}

/// Where two pictures of the same size differ.
fn difference(a: &Image, b: &Image) -> Option<Rect> {
    if (a.width, a.height) != (b.width, b.height) {
        return None;
    }
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..a.height {
        for x in 0..a.width {
            if a.pixel(x, y) != b.pixel(x, y) {
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    let size = |n: u32| u16::try_from(n).unwrap_or(u16::MAX);
    (left <= right).then(|| {
        Rect::new(
            size(left),
            size(top),
            size(right - left + 1),
            size(bottom - top + 1),
        )
    })
}

/// A session for `flow` on `variant`: the run's settings and the flow's.
pub fn start(context: &Context, flow: &Flow, variant: &Variant) -> Result<Session, String> {
    let mut settings = context.settings.clone();
    settings.extend(flow.settings.iter().cloned());
    Session::start(
        context,
        variant,
        &Start {
            settings,
            ..Start::default()
        },
    )
}

/// Where a flow's reference pictures are.
pub fn pictures(context: &Context, flow: &Flow) -> Pictures {
    let dir = context.snapshots.clone().unwrap_or_else(|| {
        flow.path
            .parent()
            .unwrap_or(Path::new("."))
            .join("snapshots")
    });
    Pictures {
        dir,
        bless: context.bless,
    }
}

pub fn run(context: &Context, report: &mut Report) -> Result<(), String> {
    if context.flows.is_empty() {
        report.add(Finding::info(
            "flows.none",
            "flows",
            "No flows given: name them with --flow <file>.",
        ));
        return Ok(());
    }
    for path in &context.flows {
        let place = crate::report::slashed(path);
        let flow = match Flow::read(path) {
            Ok(flow) => flow,
            Err(error) => {
                report.add(Finding::error("flows.invalid", place, error));
                continue;
            }
        };
        let pictures = pictures(context, &flow);
        let mut ran = 0;
        let mut not = Vec::new();
        for variant in context.variants_per_profile() {
            if let Err(why) = flow.fits(variant.profile) {
                not.push(format!("{} ({why})", variant.profile.device.id));
                continue;
            }
            ran += 1;
            let mut session = start(context, &flow, &variant)?;
            let played = play(&flow, &mut session, &pictures);
            for file in &played.blessed {
                report.add(
                    Finding::info(
                        "flows.blessed",
                        place.clone(),
                        format!("Wrote the picture {}.", crate::report::slashed(file)),
                    )
                    .variant(variant.name()),
                );
            }
            match played.failure {
                Some(failure) => report.add(failed(context, &flow, &place, &variant, failure)?),
                None => report.add(
                    Finding::info(
                        "flows.passed",
                        place.clone(),
                        format!("All {} steps passed.", flow.lines.len()),
                    )
                    .variant(variant.name()),
                ),
            }
        }
        if ran == 0 {
            report.add(Finding::warning(
                "flows.skipped",
                place,
                format!(
                    "The flow fits none of the profiles looked at: {}.",
                    not.join(", ")
                ),
            ));
        }
    }
    Ok(())
}

fn failed(
    context: &Context,
    flow: &Flow,
    place: &str,
    variant: &Variant,
    failure: Failure,
) -> Result<Finding, String> {
    let picture = format!(
        "flows/{}/{}-line-{}.png",
        flow.name,
        variant.slug(),
        failure.line
    );
    failure.image.save_png(&context.path(&picture))?;
    let mut finding = Finding::error(
        "flows.failed",
        format!("{place}:{}", failure.line),
        format!(
            "`{}` expected {}, but found {}.",
            failure.text, failure.expected, failure.found
        ),
    )
    .variant(variant.name())
    .picture(picture);
    if let Some(area) = failure.area {
        finding = finding.area(area);
    }
    if let Some(plugin) = failure
        .screen
        .as_deref()
        .and_then(|s| context.target.owner(s))
    {
        finding = finding.plugin(plugin);
    }
    Ok(finding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flow_reads() {
        let flow = parse(
            "night",
            "# Night mode\n\
             profiles: xteink-x4, xteink-x4-classic\n\
             set: core.ui-corners = core.corners.square\n\
             \n\
             long:confirm\n\
             expect screen menu.list\n\
             expect focus \"Home\"\n\
             tap-on:Night mode, Inverts\n\
             wait:500\n\
             expect no text \"Battery\"\n\
             expect setting core.night-mode = on\n\
             expect no warnings\n\
             expect stack 2\n\
             expect off Night mode\n\
             expect picture menu\n",
        )
        .unwrap();
        assert_eq!(flow.profiles, ["xteink-x4", "xteink-x4-classic"]);
        assert_eq!(
            flow.settings,
            [("core.ui-corners".into(), "core.corners.square".into())]
        );
        let steps: Vec<&Step> = flow.lines.iter().map(|l| &l.step).collect();
        assert_eq!(
            steps,
            [
                &Step::Script("long:confirm".into()),
                &Step::Expect(Expect::Screen("menu.list".into())),
                &Step::Expect(Expect::Focus("Home".into())),
                &Step::TapOn("Night mode, Inverts".into()),
                &Step::Script("wait:500".into()),
                &Step::Expect(Expect::NoText("Battery".into())),
                &Step::Expect(Expect::Setting("core.night-mode".into(), "on".into())),
                &Step::Expect(Expect::NoWarnings),
                &Step::Expect(Expect::Stack(2)),
                &Step::Expect(Expect::Switch("Night mode".into(), false)),
                &Step::Expect(Expect::Picture("menu".into())),
            ]
        );
        assert_eq!(flow.lines[0].number, 5);
    }

    #[test]
    fn mistakes_name_their_line() {
        let error = |text: &str| parse("f", text).unwrap_err();
        assert!(error("confirm\nexpect nothing").starts_with("line 2:"));
        assert!(error("tap:1\n").contains("no step"));
        assert!(error("confirm,back\n").contains("one step a line"));
        assert!(error("confirm\nprofiles: xteink-x4\n").contains("before the first step"));
        assert!(error("expect stack two").contains("number"));
        assert!(error("# only a comment\n").contains("no steps"));
        assert!(error("expect no idea").contains("no text"));
    }

    #[test]
    fn flows_fit_profiles_by_buttons_and_touch() {
        let x4 = paperpaw_emulator::profiles::find("xteink-x4").unwrap();
        let boox = paperpaw_emulator::profiles::find("boox-go-7").unwrap();
        let buttons = parse("b", "long:confirm\nright\n").unwrap();
        assert!(buttons.fits(x4).is_ok());
        assert!(buttons.fits(boox).unwrap_err().contains("confirm"));
        let touch = parse("t", "tap-on:Tools\nback\n").unwrap();
        assert!(touch.fits(x4).unwrap_err().contains("touch"));
        assert!(touch.fits(boox).is_ok());
        let only = parse("o", "profiles: boox-go-7\nback\n").unwrap();
        assert!(only.fits(x4).is_err());
    }

    #[test]
    fn differences_are_boxed() {
        let a = Image::filled(10, 10, [255, 255, 255]);
        let mut b = a.clone();
        b.set_pixel(2, 3, [0, 0, 0]);
        b.set_pixel(5, 7, [0, 0, 0]);
        assert_eq!(difference(&a, &b), Some(Rect::new(2, 3, 4, 5)));
        assert_eq!(difference(&a, &a), None);
    }
}
