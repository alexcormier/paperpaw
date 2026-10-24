//! Text spoken aloud for plugins ([`Io::Speech`], each text's end
//! reported with `Kernel::speech_done`).
//!
//! [`Io::Speech`]: super::Io::Speech

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Command, Id, Params};

/// What a shell does for speech.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechCall {
    /// Speak `text` instead of anything spoken now, or after it with
    /// `queue`, and report with `Kernel::speech_done` and `id` when it is
    /// done. Texts replaced are not reported.
    Say {
        id: u32,
        text: String,
        /// E.g. `de` or `en-GB`; empty for the system's language.
        language: String,
        /// In percent of the normal speed.
        rate: u16,
        queue: bool,
    },
    /// Stop speaking.
    Stop,
}

/// A text being spoken.
#[derive(Debug)]
struct Speaking {
    /// Its number for the shell.
    id: u32,
    reply: Id,
    params: Params,
    /// Screens open when it started: it stops once fewer are.
    screens: usize,
}

/// What is spoken, and what the shell is to do for it.
#[derive(Debug, Default)]
pub(crate) struct Speech {
    /// The shell speaks.
    pub system: bool,
    /// A plugin's voice speaking, and the screens open when it started.
    pub voice: Option<(Id, usize)>,
    calls: Vec<SpeechCall>,
    speaking: Vec<Speaking>,
    next: u32,
    /// What was said last of the focus (`core_settings::SPEAK_FOCUS`).
    pub focus: Option<String>,
}

impl Speech {
    /// Speech where the shell speaks (`system`).
    pub fn new(system: bool) -> Self {
        Self {
            system,
            ..Self::default()
        }
    }

    /// Starts speaking for a plugin, or tells it that the device cannot.
    pub fn speak(
        &mut self,
        speech: paperpaw_sdk::Speech,
        available: bool,
        screens: usize,
        out: &mut Vec<Command>,
    ) {
        if !available {
            return reply(speech.reply().clone(), speech.params().clone(), false, out);
        }
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        let queue = speech.is_queued();
        if !queue {
            self.speaking.clear();
        }
        self.speaking.push(Speaking {
            id,
            reply: speech.reply().clone(),
            params: speech.params().clone(),
            screens,
        });
        let (language, rate) = (String::from(speech.language()), speech.rate());
        self.calls.push(SpeechCall::Say {
            id,
            text: speech.into_text(),
            language,
            rate,
            queue,
        });
    }

    /// Stops speaking, if anything is spoken.
    pub fn stop(&mut self) {
        if !self.speaking.is_empty() {
            self.speaking.clear();
            self.calls.push(SpeechCall::Stop);
        }
    }

    /// Stops speaking what a screen closed since started.
    pub fn stop_for_closed_screens(&mut self, screens: usize) {
        if self.speaking.iter().any(|s| s.screens > screens) {
            self.stop();
        }
    }

    /// Text `id` was spoken (`spoken`), or could not be: its plugin hears.
    pub fn done(&mut self, id: u32, spoken: bool, out: &mut Vec<Command>) {
        if let Some(at) = self.speaking.iter().position(|s| s.id == id) {
            let speaking = self.speaking.remove(at);
            reply(speaking.reply, speaking.params, spoken, out);
        }
    }

    /// What the shell is to do.
    pub fn take(&mut self) -> Vec<SpeechCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether a text is being spoken.
    pub fn speaking(&self) -> bool {
        !self.speaking.is_empty()
    }
}

/// The reply to a [`Command::Speak`].
fn reply(reply: Id, params: Params, spoken: bool, out: &mut Vec<Command>) {
    use paperpaw_sdk::event::EventData;
    let spoken = paperpaw_sdk::replies::Spoken::new(spoken, params);
    out.push(Command::Emit(reply, spoken.into_params()));
}
