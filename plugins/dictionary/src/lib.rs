//! The dictionary: looks up a word in the StarDict dictionaries of a
//! folder and shows the definitions in a panel over the lower part of the
//! screen.
//!
//! Bound to the `core.lookup` slot, which the reader opens for a word
//! (long press, or *Look up a word* in its menu). Dictionaries are folders
//! with `.ifo`, `.idx`, `.dict(.dz)` and optional `.syn` files, as used by
//! KOReader, GoldenDict and many e-readers; they are found anywhere below
//! the dictionary folder. The first lookup prepares a small index for each
//! dictionary in the background; lookups then take a few milliseconds and
//! little memory (see `paperpaw-stardict`).
//!
//! The panel also lists what other plugins' lookup providers find (see
//! `paperpaw_sdk::lookup`): those that work offline with every word,
//! those that go online when *Online* is chosen. The StarDict
//! dictionaries are a provider too, for a lookup screen of another
//! plugin.
//!
//! Buttons: next/previous page or up/down scroll long definitions, confirm
//! or back closes. Touch: tap the right part to scroll, the left part to go
//! back, outside to close.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, groups, params, slots};
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::lookup::{LookupAnswer, LookupEntry, LookupProviderSpec, LookupQuery};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest, Params,
    Plugin, Priority, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec, SettingSpec,
    Storage, StorageError, Version,
};
use paperpaw_stardict::{Builder, Dictionary};

pub const PLUGIN: Id = Id::new_static("dictionary");
/// The panel with the definitions; parameters `word` and `language`.
pub const POPUP: Id = Id::new_static("dictionary.popup");
/// The folder searched for dictionaries.
pub const FOLDER: Setting<Text> = Setting::new("dictionary.folder");
/// The StarDict dictionaries as a lookup provider.
pub const PROVIDER: Id = Id::new_static("dictionary.stardict");
/// The folder chooser of the `files` plugin, if installed.
const CHOOSE_FOLDER: Id = Id::new_static("files.choose-folder");

/// Bytes of an index read per background step.
const STEP_BYTES: usize = 64 * 1024;
/// Folders below the dictionary folder searched for dictionaries.
const MAX_DEPTH: usize = 3;

/// The dictionary plugin.
#[derive(Debug, Default)]
pub struct DictionaryPlugin;

/// The folder of the dictionaries, in their part of the reading settings,
/// chosen on the folder chooser.
static FOLDER_EXTRA: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new()
    .section("dictionary.section")
    .editor(CHOOSE_FOLDER);
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_text(
    FOLDER.static_str(),
    "dictionary.setting.folder",
    "dictionaries",
    groups::READING,
)
.advanced()
.with_extra(&FOLDER_EXTRA)
.described("dictionary.setting.folder.help")];

impl Plugin for DictionaryPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Dictionary", Version::new(0, 1, 0))
            .description("Looks up words in StarDict dictionaries.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            POPUP,
            "dictionary.title",
            Box::new(|env, params| Box::new(Popup::new(env, params))),
        ))?;
        r.bind_slot(slots::LOOKUP, POPUP, Priority::DEFAULT)?;
        r.add_lookup_provider(LookupProviderSpec::new(
            PROVIDER,
            "dictionary.title",
            |env, query| LookupAnswer::Ready(look_up_ready(env, &query.word)),
        ))?;
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The entries for `word` in the dictionaries whose indexes are ready
/// (preparing them is the panel's: a provider answers at once).
fn look_up_ready(env: &Env<'_>, word: &str) -> Vec<LookupEntry> {
    let folder = env.settings.read(&FOLDER);
    let found = find_dictionaries(env.storage, folder.trim_matches('/')).unwrap_or_default();
    let mut entries = Vec::new();
    for ifo in found {
        let Ok(dictionary) = Dictionary::open(env.storage, &ifo, &cache_of(&ifo)) else {
            continue;
        };
        if dictionary.builders(env.storage).is_empty() {
            entries.extend(entries_of(env, &dictionary, word));
        }
    }
    entries
}

/// The definitions of `word` in `dictionary`, one entry each.
fn entries_of(env: &Env<'_>, dictionary: &Dictionary, word: &str) -> Vec<LookupEntry> {
    dictionary
        .lookup(env.storage, word)
        .unwrap_or_default()
        .into_iter()
        .map(|definition| LookupEntry::new(dictionary.info.name.clone(), definition.text))
        .collect()
}

/// The `.ifo` files below `folder`, sorted; `Err` where `folder` itself
/// cannot be listed (gone, or the card cannot be read).
fn find_dictionaries(storage: &dyn Storage, folder: &str) -> Result<Vec<String>, StorageError> {
    let mut found = Vec::new();
    let mut folders = alloc::vec![(folder.to_string(), 0usize)];
    let mut top = Ok(());
    while let Some((dir, depth)) = folders.pop() {
        let listed = storage.list(&dir, &mut |entry| {
            if entry.name.starts_with('.') {
                return;
            }
            let path = if dir.is_empty() {
                entry.name.to_string()
            } else {
                format!("{dir}/{}", entry.name)
            };
            if entry.is_dir && depth < MAX_DEPTH {
                folders.push((path, depth + 1));
            } else if !entry.is_dir && paperpaw_sdk::files::has_extension(entry.name, "ifo") {
                found.push(path);
            }
        });
        if depth == 0 {
            top = listed;
        }
    }
    top?;
    found.sort_unstable();
    Ok(found)
}

/// The folder in the plugin's own where the block indexes of the
/// dictionary `ifo` are kept: named by a hash of its path.
fn cache_of(ifo: &str) -> String {
    format!("{}/{:016x}", Env::plugin_dir(PLUGIN.as_str()), fnv1a64(ifo))
}

/// What a dictionary's block indexes are made from: the sizes of the
/// files still to be indexed. A dictionary that could not be indexed is
/// tried again only once they change.
fn stamp(builders: &[Builder]) -> String {
    let sizes: Vec<String> = builders
        .iter()
        .map(|b| b.progress().1.to_string())
        .collect();
    sizes.join(" ")
}

/// The file in a dictionary's cache that says it could not be indexed,
/// and from what ([`stamp`]).
fn failed_file(cache: &str) -> String {
    format!("{cache}/failed")
}

/// Whether the dictionary with cache `cache` and these builders could not
/// be indexed before, from the same files.
fn failed_before(storage: &dyn Storage, cache: &str, builders: &[Builder]) -> bool {
    paperpaw_sdk::read_file(storage, &failed_file(cache), 256)
        .is_ok_and(|kept| kept == stamp(builders).as_bytes())
}

/// The caches of dictionaries no longer in the folder go: all of them
/// are made again from their dictionaries when wanted.
fn forget_others(storage: &dyn Storage, found: &[String]) {
    let dir = Env::plugin_dir(PLUGIN.as_str());
    let kept: Vec<String> = found.iter().map(|ifo| cache_of(ifo)).collect();
    let mut gone = Vec::new();
    let _ = storage.list(&dir, &mut |entry| {
        let cache = format!("{dir}/{}", entry.name);
        let ours = entry.name.len() == 16 && entry.name.bytes().all(|b| b.is_ascii_hexdigit());
        if entry.is_dir && ours && !kept.contains(&cache) {
            gone.push(cache);
        }
    });
    for cache in gone {
        let _ = storage.remove_dir_all(&cache);
    }
}

/// What the panel shows.
#[derive(Debug)]
enum State {
    /// Building the dictionaries' indexes: the builders still to run (with
    /// the cache of their dictionary and its [`stamp`]), and the bytes done
    /// and in total.
    Preparing(Vec<(Builder, String, String)>, u64, u64),
    /// The text to show (definitions or a message).
    Ready,
}

struct Popup {
    word: String,
    /// The book's language, passed on to an online lookup.
    language: Option<String>,
    dictionaries: Vec<Dictionary>,
    state: State,
    /// What was found, from the dictionaries and the providers.
    entries: Vec<LookupEntry>,
    /// Online providers asked that have not answered yet, and whether
    /// they were asked (once a word).
    pending: Vec<Id>,
    asked_online: bool,
    /// The definitions (or a message), and its lines as byte ranges.
    text: String,
    lines: Vec<(usize, usize)>,
    /// First line shown.
    top: usize,
    style: UiStyle,
    region: Rect,
}

impl core::fmt::Debug for Popup {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Popup")
            .field("word", &self.word)
            .finish_non_exhaustive()
    }
}

impl Popup {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let top = display.height * 9 / 20;
        let mut popup = Self {
            word: params.get(params::WORD).unwrap_or_default().to_string(),
            language: params.get(params::LANGUAGE).map(ToString::to_string),
            dictionaries: Vec::new(),
            state: State::Ready,
            entries: Vec::new(),
            pending: Vec::new(),
            asked_online: false,
            text: String::new(),
            lines: Vec::new(),
            top: 0,
            style,
            region: Rect::new(0, top, display.width, display.height - top),
        };
        let folder = env.settings.read(&FOLDER).trim_matches('/');
        let found = find_dictionaries(env.storage, folder);
        if let Ok(found) = &found {
            forget_others(env.storage, found);
        }
        let mut builders = Vec::new();
        for ifo in found.unwrap_or_default() {
            let cache = cache_of(&ifo);
            let Ok(dictionary) = Dictionary::open(env.storage, &ifo, &cache) else {
                continue;
            };
            let missing = dictionary.builders(env.storage);
            if missing.is_empty() {
                popup.dictionaries.push(dictionary);
            } else if !failed_before(env.storage, &cache, &missing) {
                // Damaged ones are not read again with every word.
                let stamp = stamp(&missing);
                builders.extend(
                    missing
                        .into_iter()
                        .map(|b| (b, cache.clone(), stamp.clone())),
                );
                popup.dictionaries.push(dictionary);
            }
        }
        let others = env.lookup.fitting(env, &popup.query(), false).count() > 1;
        if popup.dictionaries.is_empty() && !others {
            let message = env
                .i18n
                .format("dictionary.none", &[("folder", Arg::Str(folder))]);
            popup.set_text(env, message);
            return popup;
        }
        if builders.is_empty() {
            popup.look_up(env);
        } else {
            let total = builders.iter().map(|(b, _, _)| b.progress().1).sum();
            popup.state = State::Preparing(builders, 0, total);
        }
        popup
    }

    fn query(&self) -> LookupQuery {
        LookupQuery::new(self.word.clone(), self.language.clone())
    }

    /// Looks the word up in every dictionary, and asks the providers that
    /// work offline.
    fn look_up(&mut self, env: &Env<'_>) {
        self.entries = self
            .dictionaries
            .iter()
            .flat_map(|dictionary| entries_of(env, dictionary, &self.word))
            .collect();
        // Their own commands are none: offline providers answer at once.
        let asked = env.lookup.ask(env, &self.query(), false, &[PROVIDER]);
        self.entries.extend(asked.entries);
        self.pending = asked.pending;
        self.state = State::Ready;
        self.show_entries(env);
    }

    /// Shows the entries, each source named where there are several.
    fn show_entries(&mut self, env: &Env<'_>) {
        let several = self
            .entries
            .first()
            .is_some_and(|first| self.entries.iter().any(|e| e.source != first.source));
        let mut text = String::new();
        let mut last: Option<&str> = None;
        for entry in &self.entries {
            if several && last != Some(entry.source.as_str()) {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                text.push_str(&format!("({})", entry.source));
                last = Some(&entry.source);
            }
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(entry.text.trim_end());
        }
        if text.is_empty() && self.pending.is_empty() {
            text = env
                .i18n
                .format("dictionary.not-found", &[("word", Arg::Str(&self.word))]);
        }
        let top = self.top;
        self.set_text(env, text);
        self.top = top.min(self.lines.len().saturating_sub(1));
    }

    /// Takes `text` and wraps it into lines.
    fn set_text(&mut self, env: &Env<'_>, text: String) {
        self.lines.clear();
        self.top = 0;
        if let Some(font) = self.style.font(env) {
            let width = f32::from(self.region.width) - 2.0 * self.style.margin as f32;
            let mut start = 0;
            for paragraph in text.split('\n') {
                if paragraph.is_empty() {
                    self.lines.push((start, start));
                }
                for range in font.wrap(self.style.text, paragraph, width) {
                    self.lines.push((start + range.start, start + range.end));
                }
                start += paragraph.len() + 1;
            }
        }
        self.text = text;
    }

    /// Where *Online* is in the header, if the word can be looked up
    /// online: left of the page note.
    fn online_area(&self, env: &Env<'_>, note: &str) -> Option<Rect> {
        if !self.can_go_online(env) {
            return None;
        }
        let font = self.style.font(env)?;
        let header = self.header();
        let margin = self.style.margin as f32;
        let note_width = font.measure(self.style.small, note);
        let gap = if note.is_empty() { 0.0 } else { margin };
        let label = font.measure(self.style.small, env.i18n.tr("dictionary.online"));
        let right = f32::from(header.width) - margin - note_width - gap;
        let x = (right - label).max(0.0);
        Some(Rect::new(
            x as u16,
            header.y,
            (label + margin) as u16,
            header.height,
        ))
    }

    /// Whether the word can be looked up online: providers that go
    /// online, or a screen bound for it; with a network, not yet asked.
    fn can_go_online(&self, env: &Env<'_>) -> bool {
        let providers = env
            .lookup
            .fitting(env, &self.query(), true)
            .next()
            .is_some();
        env.caps.network && !self.asked_online && (providers || env.has_slot(&slots::LOOKUP_ONLINE))
    }

    /// Asks the providers that go online, their entries shown here as
    /// they come; without any, opens the online lookup instead.
    fn online(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let query = self.query();
        if env.lookup.fitting(&env, &query, true).next().is_some() {
            let asked = env.lookup.ask(&env, &query, true, &[]);
            self.asked_online = true;
            self.entries.extend(asked.entries);
            self.pending = asked.pending;
            for command in asked.commands {
                cx.command(command);
            }
            self.show_entries(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
            return;
        }
        let mut params = Params::new().with(params::WORD, self.word.clone());
        if let Some(language) = &self.language {
            params = params.with(params::LANGUAGE, language.clone());
        }
        cx.command(Command::Back);
        cx.command(Command::OpenSlot(slots::LOOKUP_ONLINE, params));
    }

    /// The note in the header: preparing, waiting for answers, or the
    /// page.
    fn note(&self, env: &Env<'_>) -> String {
        let rows = self.rows(env);
        match &self.state {
            State::Ready if !self.pending.is_empty() => {
                env.i18n.tr("dictionary.asking").to_string()
            }
            State::Preparing(_, done, total) => env.i18n.format(
                "dictionary.preparing",
                &[("percent", Arg::Int((done * 100 / (*total).max(1)) as i64))],
            ),
            State::Ready if self.lines.len() > rows => env.i18n.format(
                "dictionary.page",
                &[
                    ("page", Arg::Int((self.top / rows + 1) as i64)),
                    ("pages", Arg::Int(self.lines.len().div_ceil(rows) as i64)),
                ],
            ),
            State::Ready => String::new(),
        }
    }

    fn header(&self) -> Rect {
        Rect::new(
            self.region.x,
            self.region.y,
            self.region.width,
            self.style.row as u16,
        )
    }

    fn line_height(&self, env: &Env<'_>) -> f32 {
        self.style.font(env).map_or(self.style.text * 1.3, |f| {
            f.line_metrics(self.style.text).line_height
        })
    }

    /// Lines that fit below the header.
    fn rows(&self, env: &Env<'_>) -> usize {
        let header = self.header();
        let height =
            f32::from(self.region.height) - f32::from(header.height) - self.style.gap as f32;
        ((height / self.line_height(env)) as usize).max(1)
    }

    /// Scrolls by a page; `false` if there is nothing more that way.
    fn scroll(&mut self, env: &Env<'_>, forward: bool, cx: &mut Cx<'_, '_>) -> bool {
        let rows = self.rows(env);
        let top = if forward {
            let next = self.top + rows;
            if next >= self.lines.len() {
                return false;
            }
            next
        } else {
            if self.top == 0 {
                return false;
            }
            self.top.saturating_sub(rows)
        };
        self.top = top;
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        true
    }
}

impl Screen for Popup {
    /// *Select* closes it, *Menu* looks the word up online where it can;
    /// the arrows scroll.
    fn hint(&self, action: &Id, env: &Env<'_>) -> paperpaw_sdk::Hint {
        if *action == actions::CONFIRM {
            paperpaw_sdk::Hint::Text(env.i18n.tr("core.hint.close").to_string())
        } else if *action == actions::MENU && self.online_area(env, "").is_some() {
            paperpaw_sdk::Hint::Text(env.i18n.tr("dictionary.online").to_string())
        } else {
            paperpaw_sdk::Hint::Default
        }
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let r = self.region;
        canvas.fill(r, Color::WHITE);
        let line = (style.line * 2).max(2);
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            line,
            Color::BLACK,
        );
        let Some(font) = style.font(env) else {
            return;
        };
        // The word, and where in the definitions the page is.
        let header = self.header();
        let rows = self.rows(env);
        let note = self.note(env);
        let margin = style.margin as f32;
        let note_width = font.measure(style.small, &note);
        // The page at the right edge, *Online* left of it, the word left
        // of both.
        let note_x = f32::from(header.width) - margin - note_width;
        let mut right = note_x;
        if let Some(online) = self.online_area(env, &note) {
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(online.x),
                header,
                env.i18n.tr("dictionary.online"),
                Color::BLACK,
            );
            right = f32::from(online.x);
        }
        ui::draw_label(
            canvas,
            &font,
            style.small,
            note_x,
            header,
            &note,
            style.faint(80),
        );
        let word = ui::fit(&font, style.title, &self.word, right - margin * 2.0);
        ui::draw_label(
            canvas,
            &font,
            style.title,
            margin,
            header,
            &word,
            Color::BLACK,
        );
        if matches!(self.state, State::Preparing(..)) {
            return;
        }
        let height = self.line_height(env);
        let ascent = font.line_metrics(style.text).ascent;
        let top = f32::from(header.y + header.height) + style.gap as f32 / 2.0;
        for (n, &(start, end)) in self.lines.iter().skip(self.top).take(rows).enumerate() {
            let baseline = top + height * n as f32 + ascent;
            font.draw(
                canvas,
                style.text,
                margin,
                baseline as i32,
                &self.text[start..end],
                Color::BLACK,
            );
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if matches!(self.state, State::Preparing(..)) {
            let now = cx.env().now;
            cx.wake_at(now);
        }
    }

    /// Prepares the dictionaries a step at a time, then looks up the word.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let State::Preparing(builders, done, _) = &mut self.state else {
            return;
        };
        let finished = match builders.last_mut() {
            Some((builder, cache, stamp)) => {
                let before = builder.progress().0;
                let finished = builder.step(env.storage, STEP_BYTES);
                *done += builder.progress().0 - before;
                match finished {
                    Ok(false) => false,
                    Ok(true) => {
                        builders.pop();
                        builders.is_empty()
                    }
                    // A damaged dictionary: skipped, now and until its
                    // files change (a card that could not be read is
                    // tried again).
                    Err(e) => {
                        if matches!(e, paperpaw_stardict::Error::Malformed(_)) {
                            let _ = env.storage.write(&failed_file(cache), stamp.as_bytes());
                        }
                        let cache = cache.clone();
                        builders.retain(|(_, c, _)| *c != cache);
                        builders.is_empty()
                    }
                }
            }
            None => true,
        };
        if finished {
            self.look_up(&env);
        } else {
            cx.wake_at(env.now.add_millis(1));
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let a = action;
        if [
            actions::NEXT_PAGE,
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
        ]
        .contains(a)
        {
            self.scroll(&env, true, cx);
        } else if [
            actions::PREV_PAGE,
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
        ]
        .contains(a)
        {
            self.scroll(&env, false, cx);
        } else if *a == actions::CONFIRM {
            cx.command(Command::Back);
        } else if *a == actions::MENU && self.online_area(&env, "").is_some() {
            self.online(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    /// An online provider's answer: shown with the others.
    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let Some(answer) = events::LOOKUP_ANSWERED.read(event, params) else {
            return;
        };
        let Some(at) = self.pending.iter().position(|p| *p == answer.provider) else {
            return;
        };
        if answer.word != self.word {
            return;
        }
        self.pending.remove(at);
        if !answer.text.trim().is_empty() {
            self.entries
                .push(LookupEntry::new(answer.source, answer.text));
        }
        let env = *cx.env();
        self.show_entries(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap || !self.region.contains(gesture.start) {
            return Handled::No;
        }
        let env = *cx.env();
        let note = self.note(&env);
        if self
            .online_area(&env, &note)
            .is_some_and(|area| area.contains(gesture.start))
        {
            self.online(cx);
            return Handled::Yes;
        }
        let forward = gesture.start.x > i32::from(self.region.width) / 3;
        // At the end, a tap closes the panel.
        if !self.scroll(&env, forward, cx) && forward {
            cx.command(Command::Back);
        }
        Handled::Yes
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

    /// Dictionaries are found whatever the case of their extension (FAT
    /// and Windows keep `.IFO`); a folder that cannot be listed says so.
    #[test]
    fn dictionaries_are_found_in_any_case() {
        let storage = paperpaw_sdk::MemoryStorage::new()
            .with("dict/a/a.ifo", "")
            .with("dict/B/B.IFO", "")
            .with("dict/c.idx", "");
        assert_eq!(
            super::find_dictionaries(&storage, "dict"),
            Ok(alloc::vec!["dict/B/B.IFO".into(), "dict/a/a.ifo".into()])
        );
        assert!(super::find_dictionaries(&storage, "gone").is_err());
    }
}
