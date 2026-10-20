//! Building blocks of reading, for readers and for the plugins around
//! them.
//!
//! The words of the page shown: any plugin asks the reader with
//! [`ask_page_words`] and hears them as [`PageWords`] (a dictionary that
//! lists the page's hard words, a summary, a word count). The reader
//! answers while a book is open; nothing comes otherwise. Decorators get
//! the words with every page without asking (see
//! [`decorations`](crate::decorations)).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::events;
use crate::document::Locator;
use crate::event::{Event, EventData};
use crate::{Command, Params};

/// A word of the page: its text and its place in the book. A word broken
/// over two lines is two words, each with the whole word as its text and
/// its own part's place (as [`PageWord`](crate::decorations::PageWord)).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Word {
    pub text: String,
    pub start: Locator,
    /// Just after its last character.
    pub end: Locator,
}

impl Word {
    pub fn new(text: impl Into<String>, start: Locator, end: Locator) -> Self {
        Self {
            text: text.into(),
            start,
            end,
        }
    }
}

/// The words of the page a reader shows, in reading order, the answer to
/// [`ask_page_words`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PageWords {
    /// The book.
    pub path: String,
    /// Its language, e.g. `de` (empty where unknown).
    pub language: String,
    /// Where the page starts and ends.
    pub start: Locator,
    pub end: Locator,
    pub words: Vec<Word>,
}

impl PageWords {
    pub fn new(
        path: impl Into<String>,
        language: impl Into<String>,
        start: Locator,
        end: Locator,
    ) -> Self {
        Self {
            path: path.into(),
            language: language.into(),
            start,
            end,
            words: Vec::new(),
        }
    }

    /// The same with `words`.
    pub fn words(mut self, words: Vec<Word>) -> Self {
        self.words = words;
        self
    }

    /// The page's text: its words with a space between.
    pub fn text(&self) -> String {
        let mut text = String::new();
        let mut last: Option<&Word> = None;
        for word in &self.words {
            // A broken word's second part: the whole word is there.
            if last.is_some_and(|l| l.text == word.text && l.end == word.start) {
                continue;
            }
            if last.is_some() {
                text.push(' ');
            }
            text.push_str(&word.text);
            last = Some(word);
        }
        text
    }
}

fn locator(text: &str) -> Option<Locator> {
    let (section, offset) = text.split_once(':')?;
    Some(Locator::new(section.parse().ok()?, offset.parse().ok()?))
}

impl EventData for PageWords {
    /// A word a line, `section:start:end:text` (words hold no line breaks).
    fn to_params(&self) -> Params {
        let mut words = String::new();
        for word in &self.words {
            let (start, end) = (word.start, word.end);
            words.push_str(&format!(
                "{}:{}:{}:{}:{}\n",
                start.section, start.offset, end.section, end.offset, word.text
            ));
        }
        let at = |l: Locator| format!("{}:{}", l.section, l.offset);
        Params::new()
            .with(crate::builtin::params::PATH, self.path.clone())
            .with(crate::builtin::params::LANGUAGE, self.language.clone())
            .with("start", at(self.start))
            .with("end", at(self.end))
            .with("words", words)
    }

    fn from_params(params: &Params) -> Option<Self> {
        let mut words = Vec::new();
        for line in params.get("words")?.lines() {
            let mut parts = line.splitn(5, ':');
            let mut number = || parts.next()?.parse::<u32>().ok();
            let start = Locator::new(number()?, number()?);
            let end = Locator::new(number()?, number()?);
            words.push(Word::new(parts.next()?.to_string(), start, end));
        }
        Some(
            PageWords::new(
                params.get(crate::builtin::params::PATH)?,
                params
                    .get(crate::builtin::params::LANGUAGE)
                    .unwrap_or_default(),
                locator(params.get("start")?)?,
                locator(params.get("end")?)?,
            )
            .words(words),
        )
    }
}

/// Asks the reader for the words of the page it shows: it answers with
/// `reply` (nothing comes where no book is open).
pub fn ask_page_words(reply: &Event<PageWords>) -> Command {
    events::PAGE_WORDS_WANTED.emit(events::PageWordsWanted::new(reply.id().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_words_go_through_an_event_whole() {
        let at = |o| Locator::new(3, o);
        let words = PageWords::new("books/a.epub", "en", at(0), at(30)).words(alloc::vec![
            Word::new("The", at(0), at(3)),
            Word::new("rabbit-hole:", at(4), at(16)),
            Word::new("Kaninchenbau", at(17), at(26)),
            Word::new("Kaninchenbau", at(26), at(29)),
        ]);
        let back = PageWords::from_params(&words.to_params()).unwrap();
        assert_eq!(back, words);
        assert_eq!(back.text(), "The rabbit-hole: Kaninchenbau");
        assert!(PageWords::from_params(&Params::new()).is_none());
    }

    #[test]
    fn asking_names_the_reply() {
        const MINE: Event<PageWords> = Event::new("my.words");
        let Command::Emit(event, params) = ask_page_words(&MINE) else {
            panic!("an event");
        };
        assert_eq!(event, *events::PAGE_WORDS_WANTED.id());
        let wanted = events::PAGE_WORDS_WANTED.read(&event, &params).unwrap();
        assert_eq!(wanted.reply, *MINE.id());
    }
}
