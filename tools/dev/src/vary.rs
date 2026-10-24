//! Flows played again at the edges (see `flows`): what a plugin's actions
//! do when a step is done many times, when *Back*, *Home* or sleep and
//! waking come in the middle, when the network fails, is slow, answers
//! nothing or far too much, in the pseudo-language, with long data, and
//! with little room on the storage.
//!
//! Assertions are left out (they hold for the flow as written); each
//! variation passes when nothing panics, no allocation was refused (the
//! device would have run out of memory) and the home screen is reached
//! after (*Home*, else *Back* and *Home*). A variation that does not is
//! reported as `flows.varied`, an error, with what went wrong; one note
//! says how many held.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};

use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_emulator::{budget, headless, http};
use paperpaw_platform::{ButtonState, InputEvent};

use crate::cli::Context;
use crate::flows::{Flow, Step};
use crate::report::{Finding, Report};
use crate::session::{Session, Start};
use crate::variant::Variant;

/// How often each step is done in a row in [`Change::Repeat`].
const REPEATS: usize = 10;

/// Places in a flow an interruption comes at, at most.
const PLACES: usize = 4;

/// What the storage says it has free in [`Change::LittleStorage`].
const LITTLE: u64 = 64 * 1024;

/// The body of [`Answer::Huge`]: 4 MiB, cut as shells cut it at what
/// the call takes at most.
const HUGE: usize = 4 << 20;

/// Times the device asks for late answers before [`Answer::Slow`] gives
/// one.
const SLOW_POLLS: u32 = 40;

/// What the network answers in a variation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Nothing reached: status 0.
    Failing,
    /// Late: after many polls.
    Slow,
    /// 200 with an empty body.
    Empty,
    /// 200 with a body far larger than the device holds.
    Huge,
}

impl Answer {
    fn name(self) -> &'static str {
        match self {
            Answer::Failing => "the network failing",
            Answer::Slow => "the network slow",
            Answer::Empty => "empty answers",
            Answer::Huge => "huge answers",
        }
    }
}

/// What comes in the middle of a flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interruption {
    Back,
    Home,
    /// The device sleeps and is woken by its power button.
    Sleep,
}

/// A variation of a flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Every step done `REPEATS` times.
    Repeat,
    /// The interruption before the step at this index.
    Interrupt(Interruption, usize),
    Network(Answer),
    /// The pseudo-language: every text longer, with marks.
    Pseudo,
    /// A book with a very long name in the library.
    LongData,
    /// `LITTLE` bytes free on the storage.
    LittleStorage,
}

impl Change {
    fn name(&self, flow: &Flow) -> String {
        match self {
            Change::Repeat => format!("every step {REPEATS} times"),
            Change::Interrupt(what, at) => {
                let what = match what {
                    Interruption::Back => "Back",
                    Interruption::Home => "Home",
                    Interruption::Sleep => "sleep and waking",
                };
                let line = flow.lines.iter().filter(|l| doing(l)).nth(*at);
                match line {
                    Some(line) => format!("{what} before line {}", line.number),
                    None => format!("{what} at the end"),
                }
            }
            Change::Network(answer) => answer.name().to_string(),
            Change::Pseudo => String::from("the pseudo-language"),
            Change::LongData => String::from("long data"),
            Change::LittleStorage => format!("{} KiB free on the storage", LITTLE / 1024),
        }
    }
}

/// Whether a line does something (a step, not an assertion).
fn doing(line: &crate::flows::Line) -> bool {
    !matches!(line.step, Step::Expect(_))
}

/// The variations of `flow`: repeated, interrupted at a few places, the
/// network's four ways, the pseudo-language, long data, little storage.
pub fn changes(flow: &Flow) -> Vec<Change> {
    let steps = flow.lines.iter().filter(|l| doing(l)).count();
    let mut changes = vec![Change::Repeat];
    // A few places spread over the flow, the start left out (nothing
    // has happened yet) and the end in.
    let places: Vec<usize> = if steps <= PLACES {
        (1..=steps).collect()
    } else {
        (1..=PLACES).map(|n| n * steps / PLACES).collect()
    };
    for what in [Interruption::Back, Interruption::Home, Interruption::Sleep] {
        changes.extend(places.iter().map(|&at| Change::Interrupt(what, at)));
    }
    changes
        .extend([Answer::Failing, Answer::Slow, Answer::Empty, Answer::Huge].map(Change::Network));
    changes.extend([Change::Pseudo, Change::LongData, Change::LittleStorage]);
    changes
}

/// The network as a variation has it (see [`http::Route`]).
struct Answers {
    answer: Answer,
    waiting: VecDeque<HttpResponse>,
    polls: u32,
}

impl http::Route for Answers {
    fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
        let ok = |body: Vec<u8>| HttpResponse {
            id: call.id,
            status: 200,
            body,
            ..HttpResponse::default()
        };
        match self.answer {
            Answer::Failing => Some(HttpResponse {
                id: call.id,
                status: 0,
                body: b"no network (paperpaw-dev)".to_vec(),
                ..HttpResponse::default()
            }),
            Answer::Empty => Some(ok(Vec::new())),
            Answer::Huge => {
                let most = if call.max_response > 0 {
                    call.max_response
                } else {
                    HUGE
                };
                Some(ok(vec![b'x'; HUGE.min(most)]))
            }
            Answer::Slow => {
                self.waiting.push_back(ok(Vec::new()));
                None
            }
        }
    }

    fn later(&mut self) -> Option<HttpResponse> {
        if self.waiting.is_empty() {
            return None;
        }
        self.polls += 1;
        if self.polls < SLOW_POLLS {
            return None;
        }
        self.polls = 0;
        self.waiting.pop_front()
    }
}

/// Plays `flow`'s variations on `variant` and reports those that did not
/// hold.
pub fn run(
    context: &Context,
    flow: &Flow,
    place: &str,
    variant: &Variant,
    report: &mut Report,
) -> Result<(), String> {
    let changes = changes(flow);
    let mut held = 0;
    for change in &changes {
        match play(context, flow, variant, change) {
            Ok(()) => held += 1,
            Err(what) => report.add(
                Finding::error(
                    "flows.varied",
                    place,
                    format!("With {}: {what}.", change.name(flow)),
                )
                .variant(variant.name()),
            ),
        }
    }
    report.add(
        Finding::info(
            "flows.varied",
            place,
            format!("{held} of {} variations held.", changes.len()),
        )
        .variant(variant.name()),
    );
    Ok(())
}

/// Plays one variation; what went wrong, if anything.
fn play(context: &Context, flow: &Flow, variant: &Variant, change: &Change) -> Result<(), String> {
    let mut settings = context.settings.clone();
    settings.extend(flow.settings.iter().cloned());
    let mut start = Start {
        settings,
        ..Start::default()
    };
    let mut variant = variant.clone();
    match change {
        Change::Pseudo => variant.language = crate::lint::PSEUDO.to_string(),
        Change::LongData => start.files.push((
            format!(
                "books/{}.txt",
                "A title far too long to fit anywhere ".repeat(6)
            ),
            b"A book with a very long name.".to_vec(),
        )),
        Change::LittleStorage => start.free_space = Some(LITTLE),
        _ => {}
    }
    let route: Option<Box<dyn http::Route>> = match change {
        Change::Network(answer) => Some(Box::new(Answers {
            answer: *answer,
            waiting: VecDeque::new(),
            polls: 0,
        })),
        _ => None,
    };
    http::set_route(route);
    let refused = budget::stats().failed;
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let mut session = Session::start(context, &variant, &start)?;
        steps(&mut session, flow, change)?;
        // Home reached after, whatever was left open.
        session.go_home()
    }));
    http::set_route(None);
    let result = match result {
        Ok(result) => result,
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(ToString::to_string))
                .unwrap_or_else(|| String::from("a panic"));
            Err(format!("it panicked: {message}"))
        }
    };
    result?;
    let now = budget::stats().failed;
    if now > refused {
        return Err(format!(
            "{} allocations were refused: the device would have run out of memory",
            now - refused
        ));
    }
    Ok(())
}

/// The flow's steps as the variation has them.
fn steps(session: &mut Session, flow: &Flow, change: &Change) -> Result<(), String> {
    let doing: Vec<&crate::flows::Line> = flow.lines.iter().filter(|l| doing(l)).collect();
    let times = if *change == Change::Repeat {
        REPEATS
    } else {
        1
    };
    for (index, line) in doing.iter().enumerate() {
        if let Change::Interrupt(what, at) = change
            && *at == index
        {
            interrupt(session, *what)?;
        }
        for _ in 0..times {
            // A step that cannot be done now (its text gone after an
            // interruption) is passed: the variation is about what
            // happens, not the flow's own path.
            let _ = step(session, line);
        }
    }
    if let Change::Interrupt(what, at) = change
        && *at >= doing.len()
    {
        interrupt(session, *what)?;
    }
    session.settle();
    Ok(())
}

fn step(session: &mut Session, line: &crate::flows::Line) -> Result<(), String> {
    match &line.step {
        Step::Script(step) => headless::run_script(&mut session.device, step),
        Step::TapOn(label) => {
            let parts = session.parts();
            let part = paperpaw_core::sdk::semantics::find(&parts, label)
                .ok_or_else(|| format!("no `{label}`"))?;
            let a = part.area;
            let tap = format!(
                "tap:{}:{}",
                i32::from(a.x) + i32::from(a.width) / 2,
                i32::from(a.y) + i32::from(a.height) / 2
            );
            headless::run_script(&mut session.device, &tap)
        }
        Step::Expect(_) => Ok(()),
    }
}

fn interrupt(session: &mut Session, what: Interruption) -> Result<(), String> {
    use paperpaw_core::sdk::builtin::actions;
    match what {
        Interruption::Back => session.run(actions::BACK.as_str()),
        Interruption::Home => session.run(actions::HOME.as_str()),
        Interruption::Sleep => {
            session.run(actions::SLEEP.as_str())?;
            for state in [ButtonState::Pressed, ButtonState::Released] {
                session.device.input(InputEvent::Button {
                    name: "power",
                    state,
                });
            }
            session.settle();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flow_is_varied_at_a_few_places() {
        let flow = crate::flows::parse(
            "f",
            "confirm\nexpect screen menu.list\nright\nright\nright\nconfirm\nback\n",
        )
        .unwrap();
        let changes = changes(&flow);
        assert_eq!(changes[0], Change::Repeat);
        // Six steps: four places each for Back, Home and sleep.
        let interrupts = changes
            .iter()
            .filter(|c| matches!(c, Change::Interrupt(..)))
            .count();
        assert_eq!(interrupts, 3 * PLACES);
        assert!(changes.contains(&Change::Network(Answer::Huge)));
        assert!(changes.contains(&Change::LittleStorage));
        assert_eq!(
            Change::Interrupt(Interruption::Back, 1).name(&flow),
            "Back before line 3"
        );
    }

    #[test]
    fn slow_answers_come_late() {
        let mut route = Answers {
            answer: Answer::Slow,
            waiting: VecDeque::new(),
            polls: 0,
        };
        let call = HttpCall {
            id: 7,
            method: String::from("GET"),
            url: String::from("https://example.org/"),
            headers: Vec::new(),
            body: None,
            body_from: None,
            max_response: 1024,
            timeout_ms: 1000,
            want_headers: Vec::new(),
            save_to: None,
            progress: false,
        };
        assert!(http::Route::start(&mut route, call).is_none());
        let polls = (0..100)
            .position(|_| http::Route::later(&mut route).is_some())
            .unwrap();
        assert_eq!(polls + 1, SLOW_POLLS as usize);
    }
}
