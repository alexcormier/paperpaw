//! A plugin's voice: chosen with `core.voice`, it speaks the texts of its
//! languages (the system's voice the rest), answers as the system would,
//! is stopped when asked or when the screen that spoke closes, and gives a
//! device without a voice of its own speech.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Tests panic on purpose when something is wrong.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{DEVICE, Harness, REPLY};
use paperpaw_core::sdk::voices::VoiceSpec;
use paperpaw_core::sdk::{Command, Id, Manifest, Params, Plugin, RegisterError, Registrar, Speech};
use paperpaw_core::{Io, Kernel, SpeechCall};
use paperpaw_platform::Instant;

/// What the voice was given: `say <text>` and `stop`.
type Heard = Rc<RefCell<Vec<String>>>;

/// A voice for English that answers at once.
struct Echo(Heard);

impl Plugin for Echo {
    fn manifest(&self) -> Manifest {
        common::manifest("echo")
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let (said, stopped) = (self.0.clone(), self.0.clone());
        r.add_voice(
            VoiceSpec::new(
                Id::new_static("echo.voice"),
                move |_, speech, out| {
                    said.borrow_mut().push(format!("say {}", speech.text));
                    out.push(speech.answer(true));
                },
                move |_, _| stopped.borrow_mut().push(String::from("stop")),
            )
            .languages(&["en"]),
        )
    }
}

fn kernel(harness: &Harness, heard: &Heard, config: &str, system: bool) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .plugin(Echo(heard.clone()))
        .speech(system)
        .user_config(config.to_string())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

fn say(text: &str, language: &str) -> Command {
    let speech = Speech::new(text, REPLY)
        .language(language)
        .params(Params::new().with("n", String::from(text)));
    Command::Speak(speech)
}

fn system_said(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Speech(SpeechCall::Say { text, .. }) => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn the_voice_chosen_speaks_its_languages() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "[core]\nvoice = echo.voice\n", true);
    harness.run(
        &mut kernel,
        10,
        [say("Hello", "en-GB"), say("Bonjour", "fr")],
    );
    assert_eq!(*heard.borrow(), ["say Hello"]);
    // The system's voice the rest.
    assert_eq!(system_said(&mut kernel), ["Bonjour"]);
    // The voice answered as the system does.
    let replies = harness.lines("event drv.reply");
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert!(replies[0].contains("n=Hello") && !replies[0].contains("failed"));
    // Stopped when asked.
    harness.run(&mut kernel, 20, [Command::StopSpeaking]);
    assert_eq!(heard.borrow().last().map(String::as_str), Some("stop"));
}

#[test]
fn the_systems_voice_speaks_until_another_is_chosen() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "", true);
    harness.run(&mut kernel, 10, [say("Hello", "en")]);
    assert!(heard.borrow().is_empty());
    assert_eq!(system_said(&mut kernel), ["Hello"]);
}

#[test]
fn a_plugins_voice_gives_a_device_speech() {
    let (harness, heard) = (Harness::new(), Heard::default());
    let mut kernel = kernel(&harness, &heard, "[core]\nvoice = echo.voice\n", false);
    harness.run(&mut kernel, 10, [say("Hello", "en"), say("Hallo", "de")]);
    assert_eq!(*heard.borrow(), ["say Hello"]);
    // German has no voice here: it fails at once, as without speech.
    let replies = harness.lines("event drv.reply");
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(
        replies
            .iter()
            .any(|r| r.contains("n=Hallo") && r.contains("failed"))
    );
    assert!(system_said(&mut kernel).is_empty());
}
