//! What the key map binds, as plugins see it, e.g. to show and change
//! the bindings ([`Command::Bind`](crate::Command::Bind)).
//!
//! Inputs are named as in `paperpaw.ini`: `button.<name>`, `long.<name>`
//! (held), `double.<name>` (pressed twice), `tap.<zone>`,
//! `long-tap.<zone>`, `double-tap.<zone>` (zones [`ZONES`], or a layer's
//! own: `[zones.<layer>] <name> = <x> <y> <width> <height>` in percent of
//! the screen), `tap2` (two fingers), `swipe.<direction>` ([`SWIPES`]),
//! `edge.<side>` ([`EDGES`]), `pinch.<out|in>` ([`PINCHES`]), a plugin's
//! `input.<id>` ([`InputSpec`]) and `gesture.<id>` ([`GestureSpec`]).
//! Layers are screen or slot ids, and
//! [`GLOBAL_KEYMAP_LAYER`](crate::builtin::GLOBAL_KEYMAP_LAYER).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_platform::{InputCaps, Instant, PointerEvent};

use crate::id::Id;
use crate::menus::MenuNeed;

/// The tap zones, a 3 by 3 grid over the screen.
pub const ZONES: [&str; 9] = [
    "top-left",
    "top",
    "top-right",
    "left",
    "center",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];
/// The directions of swipes.
pub const SWIPES: [&str; 4] = ["left", "right", "up", "down"];
/// The edges swipes in begin at.
pub const EDGES: [&str; 4] = ["top", "bottom", "left", "right"];
/// Two fingers moved apart or together.
pub const PINCHES: [&str; 2] = ["out", "in"];
/// How a screen takes gestures (`gestures.policy` in its layer): as
/// usual, its own (swipes in from an edge need a second), none.
pub const POLICIES: [&str; 3] = [
    "core.gestures.standard",
    "core.gestures.own",
    "core.gestures.off",
];

/// Every input a device with `caps` makes, as key maps name them: its
/// buttons pressed, held and pressed twice, and on touch screens taps,
/// long taps and double taps in each zone, two-finger taps, swipes,
/// swipes in from the edges and pinches. Plugins' inputs are in
/// [`Env::inputs`](crate::Env::inputs).
pub fn inputs(caps: &InputCaps) -> Vec<String> {
    let mut all = Vec::new();
    for key in caps.keys {
        for kind in ["button", "long", "double"] {
            all.push(format!("{kind}.{key}"));
        }
    }
    if caps.touch {
        for zone in ZONES {
            for kind in ["tap", "long-tap", "double-tap"] {
                all.push(format!("{kind}.{zone}"));
            }
        }
        all.push(String::from("tap2"));
        all.extend(SWIPES.iter().map(|d| format!("swipe.{d}")));
        all.extend(EDGES.iter().map(|e| format!("edge.{e}")));
        all.extend(PINCHES.iter().map(|p| format!("pinch.{p}")));
    }
    all
}

/// What the reader used last ([`Env::last_input`](crate::Env::last_input)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InputMode {
    #[default]
    Buttons,
    Touch,
    Pen,
}

/// An input of one's own (`input.<id>` in key maps), e.g. a chord a
/// plugin recognizes or a switch of a switch-access device: bound like
/// any other, raised with [`Command::Input`](crate::Command::Input).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputSpec {
    pub id: Id,
    /// Translation key of its name, for lists of inputs to bind.
    pub title: Cow<'static, str>,
    /// What the device must have for it.
    pub need: Option<MenuNeed>,
}

impl InputSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            need: None,
        }
    }

    pub fn need(mut self, need: MenuNeed) -> Self {
        self.need = Some(need);
        self
    }
}

/// What a recognizer of gestures made of what it saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Recognition {
    /// Nothing yet.
    Nothing,
    /// Its gesture: `gesture.<id>` runs what it is bound to.
    Recognized,
}

/// Recognizes a gesture of one's own from the fingers on the screen (a
/// circle, a zigzag, three fingers): every pointer sample comes to it,
/// in the screen's coordinates, beside what the kernel recognizes.
pub trait GestureRecognizer {
    /// A pointer sample at `now`.
    fn pointer(&mut self, event: &PointerEvent, now: Instant) -> Recognition;

    /// Time went on to `now` (asked with [`next_tick`](Self::next_tick)).
    fn tick(&mut self, now: Instant) -> Recognition {
        let _ = now;
        Recognition::Nothing
    }

    /// When it wants [`tick`](Self::tick), e.g. for a gesture of holding.
    fn next_tick(&self) -> Option<Instant> {
        None
    }
}

/// Makes a recognizer.
pub type RecognizerFactory = Box<dyn Fn() -> Box<dyn GestureRecognizer>>;

/// A recognizer of gestures a plugin adds (`gesture.<id>` in key maps).
#[non_exhaustive]
pub struct GestureSpec {
    pub id: Id,
    /// Translation key of its name, for lists of inputs to bind.
    pub title: Cow<'static, str>,
    pub factory: RecognizerFactory,
}

impl GestureSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, factory: RecognizerFactory) -> Self {
        Self {
            id,
            title: title.into(),
            factory,
        }
    }
}

impl fmt::Debug for GestureSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The bindings in effect.
pub trait KeyBindings {
    /// The action `input` runs, looked up in `layers` from the most
    /// specific (a screen, its slot) to `global`, as input is; `None` if
    /// it runs none.
    fn bound(&self, layers: &[&str], input: &str) -> Option<Id>;
}
