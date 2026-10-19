//! Samples of fonts before they are downloaded: for each, a font pack of
//! one size with only the characters of its name and of a sentence in
//! the reader's language, made by the font library on request (a few
//! kilobytes), kept on the storage and drawn as any text. One is fetched
//! at a time; those of the fonts shown are held in memory.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::{Command, Env, FontHandle, FontSet, HttpRequest, Id, Params};

use crate::PLUGIN;
use crate::catalog::Font;

/// The answer with a sample.
pub(crate) const SAMPLE: Event<HttpReply> = Event::new("fonts-browser.sample");

/// Parameter of a sample's request: the font's id.
const FONT: &str = "font";
/// Largest sample read.
const MAX_SAMPLE: usize = 64 * 1024;
/// Memory for the glyphs of the samples shown.
const CACHE: usize = 4 * 1024;
/// Samples held at most before the oldest go.
const HELD: usize = 12;

fn dir() -> String {
    format!("{}/samples", Env::plugin_dir(PLUGIN.as_str()))
}

/// The sentence a font is shown with, in the reader's language.
pub(crate) fn sentence<'a>(env: &Env<'a>) -> &'a str {
    env.i18n.tr("fonts-browser.sample")
}

/// Its characters and the font's name, each once, and `…` for a name
/// cut short.
fn characters(env: &Env<'_>, font: &Font) -> String {
    let mut chars: Vec<char> = font
        .name
        .chars()
        .chain(sentence(env).chars())
        .chain(['…'])
        .collect();
    chars.sort_unstable();
    chars.dedup();
    chars.into_iter().filter(|c| !c.is_control()).collect()
}

/// The samples of fonts shown on a screen.
pub(crate) struct Samples {
    set: FontSet,
    /// Font ids and their sample in the set (`None`: none to be had).
    held: Vec<(String, Option<usize>)>,
    /// The font whose sample is on its way.
    fetching: Option<String>,
    /// The size samples are made in, in pixels.
    pub size: u32,
}

impl Samples {
    pub(crate) fn new(style: &UiStyle) -> Self {
        Self {
            set: FontSet::new(CACHE),
            held: Vec::new(),
            fetching: None,
            // A little larger than menu text, for the letters to show.
            size: (style.text * 1.3 + 0.5) as u32,
        }
    }

    /// Where the sample of `font` is kept: one for each version, size and
    /// language.
    fn path(&self, env: &Env<'_>, font: &Font) -> String {
        let version: String = font
            .version
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        format!(
            "{}/{}-{version}-{}-{}.ppfp",
            dir(),
            font.id,
            self.size,
            env.i18n.language()
        )
    }

    /// The sample of font `id`, if it is held.
    pub(crate) fn get(&self, id: &str) -> Option<FontHandle<'_>> {
        let index = self.held.iter().find(|(i, _)| i == id)?.1?;
        self.set.get(index)
    }

    /// Holds the samples of `fonts` kept on the storage and asks for the
    /// first one missing; `true` if one more is held.
    pub(crate) fn want(&mut self, env: &Env<'_>, fonts: &[&Font], out: &mut Vec<Command>) -> bool {
        if self.held.len() + fonts.len() > HELD {
            self.set = FontSet::new(CACHE);
            self.held.clear();
        }
        let mut more = false;
        for font in fonts {
            if self.held.iter().any(|(id, _)| *id == font.id) {
                continue;
            }
            let path = self.path(env, font);
            if !env.storage.exists(&path) {
                continue;
            }
            let loaded = paperpaw_sdk::read_file(env.storage, &path, MAX_SAMPLE)
                .ok()
                .and_then(|data| paperpaw_sdk::Font::from_data(data, &[]).ok());
            let index = loaded.map(|f| self.set.add(Id::new_static("fonts-browser.sample"), f));
            if index.is_none() {
                // Not a pack after all: fetched again.
                let _ = env.storage.remove(&path);
                continue;
            }
            self.held.push((font.id.clone(), index));
            more = true;
        }
        if self.fetching.is_none()
            && let Some(font) = fonts
                .iter()
                .find(|f| !f.sample.is_empty() && !self.held.iter().any(|(id, _)| *id == f.id))
        {
            let separator = if font.sample.contains('?') { '&' } else { '?' };
            let url = format!(
                "{}{separator}size={}&text={}",
                font.sample,
                self.size,
                paperpaw_sdk::web::encode(&characters(env, font))
            );
            let mut request = HttpRequest::new("GET", &url, SAMPLE);
            let _ = env.storage.create_dir(&dir());
            request.save_to = Some(self.path(env, font));
            request.params = Params::new().with(FONT, font.id.clone());
            out.push(Command::Http(request));
            self.fetching = Some(font.id.clone());
        }
        more
    }

    /// A sample arrived (or not): the next can be fetched.
    pub(crate) fn received(&mut self, env: &Env<'_>, reply: &HttpReply, fonts: &[&Font]) {
        let id = reply.context.get(FONT).unwrap_or_default();
        if self.fetching.as_deref() != Some(id) {
            return;
        }
        self.fetching = None;
        if !(200..300).contains(&reply.status)
            && let Some(font) = fonts.iter().find(|f| f.id == id)
        {
            let _ = env.storage.remove(&self.path(env, font));
            // Not asked for again while the screen is open.
            self.held.push((font.id.clone(), None));
        }
    }
}

/// Removes the samples kept of fonts or versions no longer offered.
pub(crate) fn prune(env: &Env<'_>, samples: &Samples, fonts: &[Font]) {
    let keep: Vec<String> = fonts.iter().map(|f| samples.path(env, f)).collect();
    let mut gone: Vec<String> = Vec::new();
    let dir = dir();
    let _ = env.storage.list(&dir, &mut |entry| {
        let path = format!("{dir}/{}", entry.name);
        if !keep.contains(&path) {
            gone.push(path);
        }
    });
    for path in gone {
        let _ = env.storage.remove(&path);
    }
}
