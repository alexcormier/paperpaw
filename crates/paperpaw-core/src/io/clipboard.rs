//! The system's clipboard, where the shell has one
//! (`KernelBuilder::system_clipboard`): text copied goes to it
//! ([`Io::Copy`]), and plugins that read the clipboard wait for its text
//! ([`Io::ReadClipboard`], answered with `Kernel::clipboard_text`).
//!
//! [`Io::Copy`]: super::Io::Copy
//! [`Io::ReadClipboard`]: super::Io::ReadClipboard

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Id, Params};

#[derive(Debug, Default)]
pub(crate) struct Clipboard {
    /// The shell has a system clipboard to read.
    pub system: bool,
    /// Text copied, for the shell to put on the system's clipboard.
    copied: Option<String>,
    /// Plugins waiting for the system clipboard's text: reply event and
    /// parameters.
    reads: Vec<(Id, Params)>,
    /// The shell was asked to read it, and has not answered yet.
    asked: bool,
}

impl Clipboard {
    /// With `system` whether the shell has a system clipboard to read.
    pub fn new(system: bool) -> Self {
        Self {
            system,
            ..Self::default()
        }
    }

    /// Text copied here, for the system's clipboard too.
    pub fn copy(&mut self, text: String) {
        self.copied = Some(text);
    }

    /// A plugin waits for the system clipboard's text.
    pub fn read(&mut self, reply: Id, params: Params) {
        self.reads.push((reply, params));
    }

    /// The text copied for the shell, if any.
    pub fn take_copied(&mut self) -> Option<String> {
        self.copied.take()
    }

    /// Whether the shell is to read the system's clipboard now: once for
    /// the plugins waiting.
    pub fn take_read(&mut self) -> bool {
        let ask = !self.reads.is_empty() && !self.asked;
        self.asked |= ask;
        ask
    }

    /// The plugins that waited, now that the text came.
    pub fn answered(&mut self) -> Vec<(Id, Params)> {
        self.asked = false;
        core::mem::take(&mut self.reads)
    }
}
