//! Asking the reader for text or a folder, with the answer as a typed
//! event ([`Typed`], [`Chosen`]) the plugin hears with
//! [`Registrar::on`](crate::Registrar::on):
//!
//! ```
//! use quireink_sdk::ask;
//! use quireink_sdk::event::Event;
//! use quireink_sdk::replies::Typed;
//!
//! const NAMED: Event<Typed> = Event::new("demo.named");
//!
//! // In an action or a screen:
//! let command = ask::text("Name of the shelf", &NAMED)
//!     .initial("Novels")
//!     .with("shelf", "3")
//!     .command();
//! // r.on(NAMED, |env, typed, out| { typed.text, typed.context.get("shelf") … })
//! # let _ = command;
//! ```
//!
//! The keyboard (slot [`KEYBOARD`](crate::builtin::slots::KEYBOARD)) and
//! the folder chooser ([`CHOOSE_FOLDER`](crate::builtin::slots::CHOOSE_FOLDER))
//! are plugins another can replace: they answer every question asked this
//! way.

use alloc::borrow::Cow;
use alloc::string::String;

use crate::builtin::{params, slots};
use crate::command::Command;
use crate::event::Event;
use crate::params::Params;
use crate::replies::{Chosen, Typed};

/// A question for text (see [`text`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct TextQuestion {
    params: Params,
}

/// Asks for text, under `title` (already translated); the answer comes as
/// `reply`.
pub fn text(title: impl Into<String>, reply: &Event<Typed>) -> TextQuestion {
    TextQuestion {
        params: Params::new()
            .with(params::TITLE, title)
            .with(params::REPLY, reply.as_str()),
    }
}

impl TextQuestion {
    /// The text to start from.
    pub fn initial(mut self, text: impl Into<String>) -> Self {
        self.params.set(params::TEXT, text);
        self
    }

    /// Shown as dots, e.g. a password.
    pub fn password(mut self) -> Self {
        self.params.set(params::MODE, "password");
        self
    }

    /// Digits only.
    pub fn number(mut self) -> Self {
        self.params.set(params::MODE, "number");
        self
    }

    /// Passes `value` along: it comes back in the answer's context.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// Passes all of `context` along (see [`TextQuestion::with`]).
    pub fn context(mut self, context: &Params) -> Self {
        for (key, value) in context.iter() {
            self.params.set(String::from(key), value);
        }
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::KEYBOARD, self.params)
    }

    /// The command that asks on another device, e.g. a phone (slot
    /// [`REMOTE_KEYBOARD`](crate::builtin::slots::REMOTE_KEYBOARD)).
    pub fn remote_command(self) -> Command {
        Command::OpenSlot(slots::REMOTE_KEYBOARD, self.params)
    }
}

/// A question for a folder (see [`folder`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct FolderQuestion {
    params: Params,
}

/// Asks for a folder; the answer comes as `reply`.
pub fn folder(reply: &Event<Chosen>) -> FolderQuestion {
    FolderQuestion {
        params: Params::new().with(params::REPLY, reply.as_str()),
    }
}

impl FolderQuestion {
    /// Where to start browsing.
    pub fn start(mut self, path: impl Into<String>) -> Self {
        self.params.set(params::TEXT, path);
        self
    }

    /// Passes `value` along: it comes back in the answer's context.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::CHOOSE_FOLDER, self.params)
    }
}
