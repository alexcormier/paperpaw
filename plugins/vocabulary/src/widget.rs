//! The vocabulary's widgets for the home screen (or any screen showing
//! widgets), each in the shared look of widgets (its frame, size and
//! alignment) with settings of its own for what it shows:
//!
//! - *Word of the day* (`vocabulary.widget.word`): a word to go over,
//!   another one each day (or the newest), with its sentence and book;
//! - *Words saved* (`vocabulary.widget.saved`): how many words are kept,
//!   from how many books and in how many languages, and the newest.
//!
//! A tap on either opens the list of words. They read the words when
//! they are updated (when shown and after events), never as they are
//! drawn: one pass over the file, keeping a word.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use quireink_sdk::ui::{self, UiStyle};
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{Arg, Canvas, Command, Env, FontHandle, Id, Rect, RegisterError, Registrar};

use crate::OPEN;
use crate::store::{self, Summary, Word};

/// The word of the day.
pub(crate) const WORD: Id = Id::new_static(WORD_NAME);
const WORD_NAME: &str = "vocabulary.widget.word";
/// The words saved.
pub(crate) const SAVED: Id = Id::new_static(SAVED_NAME);
const SAVED_NAME: &str = "vocabulary.widget.saved";
/// Which word the word of the day is: another each day, or the newest.
const PICKS: [&str; 2] = ["vocabulary.pick.daily", "vocabulary.pick.newest"];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    look_settings(r, &WORD, "vocabulary.widget.word")?;
    r.add_setting(widgets::choice(
        &WORD,
        "pick",
        "vocabulary.setting.pick",
        &PICKS,
        PICKS[0],
    ))?;
    for (key, title, default) in [
        ("caption", "vocabulary.part.caption", true),
        ("sentence", "vocabulary.part.sentence", true),
        ("book", "vocabulary.part.book", false),
    ] {
        r.add_setting(widgets::part_setting(&WORD, key, title, default))?;
    }
    r.add_widget(
        WidgetSpec::new(WORD, "vocabulary.widget.word", |_| {
            Box::new(WordWidget::default())
        })
        .category(widgets::categories::WORDS),
    )?;
    look_settings(r, &SAVED, "vocabulary.widget.saved")?;
    for (key, title, default) in [
        ("caption", "vocabulary.part.caption", true),
        ("newest", "vocabulary.part.newest", true),
        ("books", "vocabulary.part.books", false),
        ("languages", "vocabulary.part.languages", false),
    ] {
        r.add_setting(widgets::part_setting(&SAVED, key, title, default))?;
    }
    r.add_widget(
        WidgetSpec::new(SAVED, "vocabulary.widget.saved", |_| {
            Box::new(Saved::default())
        })
        .category(widgets::categories::WORDS),
    )
}

/// A widget's settings group with its frame, size and alignment.
fn look_settings(r: &mut Registrar, id: &Id, title: &'static str) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(id, title))?;
    r.add_setting(widgets::frame_setting(id, Frame::None))?;
    r.add_setting(widgets::size_setting(id, Size::Medium, true))?;
    r.add_setting(widgets::align_setting(id, Align::Left))
}

/// The height of a line of text `size` pixels large.
fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// The height `widget`'s frame takes around its content.
fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 1000, 1000);
    1000 - widgets::look::look(env, widget, probe).area.height
}

/// The caption's height, if `widget` shows it.
fn caption_room(env: &Env<'_>, widget: &str) -> u16 {
    if widgets::shows(env, widget, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// Places content `height` high with `widget`'s caption `text` above
/// it (if it shows), the two together in the middle of `look`'s area,
/// and draws the caption; where the content goes.
fn captioned(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &Look,
    text: &str,
    height: u16,
) -> Rect {
    let total = (caption_room(env, widget) + height).min(look.area.height);
    let area = look.area;
    let block = Rect::new(
        area.x,
        area.y + (area.height - total) / 2,
        area.width,
        total,
    );
    let below = if widgets::shows(env, widget, "caption") {
        let at = Look {
            area: block,
            ..*look
        };
        widgets::draw_caption(canvas, env, widget, &at, text)
    } else {
        block
    };
    Rect::new(below.x, below.y, below.width, height.min(below.height))
}

/// The size of `widget`'s main text: as its setting says, smaller where
/// a line of it is higher than `height`; "as large as fits" fits `text`
/// into `width` and `height` (up to three times a title's size).
fn text_size(
    env: &Env<'_>,
    font: &FontHandle<'_>,
    widget: &str,
    text: &str,
    (width, height): (u16, u16),
) -> f32 {
    let style = UiStyle::new(env);
    let base = style.title;
    let line = font.line_metrics(base).line_height.max(1.0);
    let by_height = base * f32::from(height) / line;
    match widgets::size(env, widget) {
        Size::Fit => {
            let wide = font.measure(base, text).max(1.0);
            let by_width = base * f32::from(width) / wide;
            by_width.min(by_height).clamp(style.small, base * 3.0)
        }
        size => size.text(&style).min(by_height.max(style.small)),
    }
}

/// A tap opens the list of words.
fn open_words(area: Rect) -> Vec<WidgetTarget> {
    alloc::vec![WidgetTarget {
        area,
        command: Command::RunAction(OPEN),
    }]
}

/// The word to show: another one every day, or the newest.
fn pick(env: &Env<'_>) -> Option<Word> {
    if env.settings.text(&format!("{WORD_NAME}.pick")) == PICKS[1] {
        return store::summary(env.storage).newest;
    }
    let count = store::count(env.storage);
    if count == 0 {
        return None;
    }
    let day = quireink_sdk::time::unix_seconds(env).map_or(0, |s| s / 86_400) as usize;
    store::range(env.storage, day % count, day % count + 1)
        .into_iter()
        .next()
        .map(|(_, word)| word)
}

/// The word of the day, read when updated.
#[derive(Default)]
struct WordWidget {
    word: Option<Word>,
    /// What it was read for: the day, the size of the file of words and
    /// the choice of word.
    read_for: Option<(i64, u64, bool)>,
}

impl Widget for WordWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read again only when the day, the words or the choice changed:
        // it is asked each minute, for the day.
        let day = quireink_sdk::time::unix_seconds(env).map_or(0, |s| s / 86_400);
        let size = env.storage.size(&store::file()).unwrap_or(0);
        let newest = env.settings.text(&format!("{WORD_NAME}.pick")) == PICKS[1];
        let read_for = Some((day, size, newest));
        if read_for == self.read_for {
            return false;
        }
        self.read_for = read_for;
        let word = pick(env);
        let changed = word != self.word;
        self.word = word;
        changed
    }

    fn live(&self) -> bool {
        // Another word at midnight.
        true
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = WORD_NAME;
        let size = widgets::size(env, id).text(&style);
        let small = line(&font, style.small);
        let sentence = if widgets::shows(env, id, "sentence") {
            small * 2
        } else {
            0
        };
        let book = if widgets::shows(env, id, "book") {
            small
        } else {
            0
        };
        frame_height(env, id) + caption_room(env, id) + line(&font, size) + sentence + book
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WORD_NAME, area);
        match &self.word {
            Some(word) => draw_word(canvas, env, &look, word),
            None => {
                widgets::draw_message(canvas, env, &look, env.i18n.tr("vocabulary.widget.empty"))
            }
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        open_words(area)
    }
}

/// The caption, the word, its sentence (as many lines as fit, three at
/// most) and its book.
fn draw_word(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, word: &Word) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = WORD_NAME;
    let small = line(&font, style.small);
    let area = look.area;
    let room = area.height.saturating_sub(caption_room(env, id));
    let width = f32::from(area.width);
    let book = if widgets::shows(env, id, "book") && !word.title.is_empty() {
        small
    } else {
        0
    };
    let size = text_size(
        env,
        &font,
        id,
        &word.word,
        (area.width, room.saturating_sub(book)),
    );
    let big = line(&font, size);
    // The sentence: the lines it needs, as far as they fit.
    let sentence = if widgets::shows(env, id, "sentence") && !word.sentence.is_empty() {
        let lines = font.wrap(style.small, &word.sentence, width).len().min(3) as u16;
        let fit = room.saturating_sub(big + book) / small.max(1);
        lines.min(fit) * small
    } else {
        0
    };
    let pick = env.settings.text(&format!("{id}.pick"));
    let caption = if pick == PICKS[1] {
        env.i18n.tr("vocabulary.widget.newest")
    } else {
        env.i18n.tr("vocabulary.widget.word")
    };
    let block = captioned(canvas, env, id, look, caption, big + sentence + book);
    let align = widgets::align(env, id);
    let shown = ui::fit(&font, size, &word.word, width);
    let x = align.x(area, font.measure(size, &shown));
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    ui::draw_label(canvas, &font, size, x, row(block.y, big), &shown, look.ink);
    let y = block.y + big;
    if sentence > 0 {
        let at = row(y, sentence);
        widgets::draw_wrapped(
            canvas,
            env,
            at,
            align,
            style.small,
            &word.sentence,
            look.faint,
        );
    }
    if book > 0 {
        let text = env.i18n.format(
            "vocabulary.widget.from",
            &[("title", Arg::Str(&word.title))],
        );
        let text = ui::fit(&font, style.small, &text, width);
        let x = align.x(area, font.measure(style.small, &text));
        ui::draw_label(
            canvas,
            &font,
            style.small,
            x,
            row(y + sentence, book),
            &text,
            look.faint,
        );
    }
}

/// The words saved, read when updated.
#[derive(Default)]
struct Saved {
    summary: Summary,
}

impl Widget for Saved {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let summary = store::summary(env.storage);
        let changed = summary != self.summary;
        self.summary = summary;
        changed
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return 0;
        };
        let id = SAVED_NAME;
        let size = widgets::size(env, id).text(&style);
        let notes = notes(env, &self.summary, |_| true).len() as u16;
        frame_height(env, id)
            + caption_room(env, id)
            + line(&font, size)
            + notes * line(&font, style.small)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, SAVED_NAME, area);
        if self.summary.count == 0 {
            widgets::draw_message(canvas, env, &look, env.i18n.tr("vocabulary.widget.empty"));
            return;
        }
        draw_saved(canvas, env, &look, &self.summary);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        open_words(area)
    }
}

/// The notes below the count: the newest word, where the words come
/// from (on one line where it `fits`, else a line each).
fn notes(env: &Env<'_>, summary: &Summary, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let id = SAVED_NAME;
    let mut notes = Vec::new();
    if let Some(newest) = summary
        .newest
        .as_ref()
        .filter(|_| widgets::shows(env, id, "newest"))
    {
        notes.push(env.i18n.format(
            "vocabulary.widget.latest",
            &[("word", Arg::Str(&newest.word))],
        ));
    }
    let mut from: Vec<String> = Vec::new();
    if widgets::shows(env, id, "books") && summary.books > 0 {
        from.push(
            env.i18n
                .plural("vocabulary.widget.books", summary.books as i64, &[]),
        );
    }
    if widgets::shows(env, id, "languages") && summary.languages > 0 {
        from.push(
            env.i18n
                .plural("vocabulary.widget.languages", summary.languages as i64, &[]),
        );
    }
    let joined = from.join(" · ");
    if fits(&joined) {
        notes.push(joined);
    } else {
        notes.extend(from);
    }
    notes
}

/// "128 words", and the notes below.
fn draw_saved(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &Look, summary: &Summary) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let id = SAVED_NAME;
    let small = line(&font, style.small);
    let area = look.area;
    let room = area.height.saturating_sub(caption_room(env, id));
    let headline = env
        .i18n
        .plural("vocabulary.widget.words", summary.count as i64, &[]);
    let width = f32::from(area.width);
    let fits = |text: &str| text.is_empty() || font.measure(style.small, text) <= width;
    let mut notes = notes(env, summary, fits);
    notes.retain(|n| !n.is_empty());
    let least = line(&font, style.text);
    while !notes.is_empty() && least + notes.len() as u16 * small > room {
        notes.pop();
    }
    let notes_h = notes.len() as u16 * small;
    let size = text_size(env, &font, id, &headline, (area.width, room - notes_h));
    let big = line(&font, size);
    let caption = env.i18n.tr("vocabulary.title");
    let block = captioned(canvas, env, id, look, caption, big + notes_h);
    let align = widgets::align(env, id);
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    let headline = ui::fit(&font, size, &headline, width);
    let x = align.x(area, font.measure(size, &headline));
    ui::draw_label(
        canvas,
        &font,
        size,
        x,
        row(block.y, big),
        &headline,
        look.ink,
    );
    for (n, note) in notes.iter().enumerate() {
        let note = ui::fit(&font, style.small, note, width);
        let x = align.x(area, font.measure(style.small, &note));
        let at = row(block.y + big + n as u16 * small, small);
        ui::draw_label(canvas, &font, style.small, x, at, &note, look.faint);
    }
}
