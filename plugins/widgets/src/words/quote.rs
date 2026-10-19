//! The quote of the day: a quote about reading, books and life, another
//! each day, in the reader's language, from those QuireInk brings (in
//! `data/quotes.<language>.txt`) and the reader's own (`quotes.txt` in
//! the plugin's folder, a quote per line: `text | author`). A tap shows
//! the next one.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use quireink_sdk::event::Event;
use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::ui::UiStyle;
use quireink_sdk::widgets::{self, Align, Frame, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::text::{self, FACES};
use super::{Turns, changed, changes, data, each_line, frame_height, of_the_day, today};

pub(super) const ID: Id = Id::new_static("widgets.quote");
/// Whose quotes: one of [`SOURCES`].
const SOURCE: Setting<Text> = Setting::new("widgets.quote.source");
/// QuireInk's, the reader's own, or both.
const SOURCES: [&str; 3] = [
    "widgets.quote.source.shipped",
    "widgets.quote.source.own",
    "widgets.quote.source.both",
];
/// When another quote shows: one of [`CHANGES`].
const CHANGE: Setting<Text> = Setting::new("widgets.quote.change");
/// Each day, or each time the widget shows.
pub(super) const CHANGES: [&str; 2] = ["widgets.change.daily", "widgets.change.shown"];
/// The font of the quote: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.quote.face");
/// Parts it shows: who said it, quotation marks around it, a large
/// quotation mark above it, "Quote of the day" above all.
const AUTHOR: &str = "author";
const MARKS: &str = "marks";
const MARK: &str = "mark";
const CAPTION: &str = "caption";
/// The next quote, asked for with a tap.
const NEXT: Event<()> = Event::new("widgets.quote.next");

const ENGLISH: &str = include_str!("../../data/quotes.en.txt");
const GERMAN: &str = include_str!("../../data/quotes.de.txt");

/// Where the reader's own quotes are.
pub(super) fn own_file() -> String {
    data("quotes.txt")
}

/// Where the turns of the quotes are kept (see [`Turns`]).
fn turns_file() -> String {
    data("quote.txt")
}

/// The quote's settings, kept in the program.
static SETTINGS: [SettingSpec; 10] = [
    SettingSpec::fixed_choice(
        "widgets.quote.source",
        "widgets.quote.setting.source",
        &SOURCES,
        SOURCES[2],
        ID,
    )
    .described("widgets.quote.setting.source.help"),
    SettingSpec::fixed_choice(
        "widgets.quote.change",
        "widgets.setting.change",
        &CHANGES,
        CHANGES[0],
        ID,
    ),
    widgets::part_spec(
        "widgets.quote.author",
        ID,
        "widgets.quote.setting.author",
        true,
    ),
    widgets::part_spec(
        "widgets.quote.marks",
        ID,
        "widgets.quote.setting.marks",
        true,
    ),
    widgets::part_spec(
        "widgets.quote.mark",
        ID,
        "widgets.quote.setting.mark",
        false,
    ),
    widgets::part_spec(
        "widgets.quote.caption",
        ID,
        "widgets.quote.setting.caption",
        false,
    ),
    SettingSpec::fixed_choice(
        "widgets.quote.face",
        "widgets.setting.font",
        &FACES,
        FACES[2],
        ID,
    ),
    widgets::size_spec("widgets.quote.size", ID, Size::Fit, true),
    widgets::align_spec("widgets.quote.align", ID, Align::Center),
    widgets::frame_spec("widgets.quote.frame", ID, Frame::None),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&ID, "widgets.quote"))?;
    r.add_settings(&SETTINGS)?;
    r.on(NEXT, |env, _, out| {
        Turns::turn(env.storage, &turns_file(), today(env), daily(env));
        changed(out, &ID);
    })?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.quote", |env| {
            // Another quote each time it shows: made anew each time.
            if !daily(env) {
                Turns::turn(env.storage, &turns_file(), today(env), false);
            }
            Box::new(QuoteWidget::default())
        })
        .category(widgets::categories::WORDS),
    )
}

/// Whether another quote shows each day (else each time it shows).
fn daily(env: &Env<'_>) -> bool {
    env.settings.read(&CHANGE) != CHANGES[1]
}

/// A quote and who said it (empty if no one is known).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Quote {
    text: String,
    author: String,
}

/// A quote as a line of a file has it, `text | author`; `None` for
/// empty lines and comments (`#`).
fn parse(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (text, author) = line.rsplit_once('|').unwrap_or((line, ""));
    let text = text::unquoted(text);
    (!text.is_empty()).then_some((text, author.trim()))
}

/// The quotes QuireInk brings in `language`, English where it brings none.
fn shipped(language: &str) -> &'static str {
    if language.starts_with("de") {
        GERMAN
    } else {
        ENGLISH
    }
}

/// The quotes of `text`, the shipped ones' file.
fn quotes_in(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(parse)
}

/// The quote at `position` of the shipped ones (`shipped`) and then the
/// reader's own (in the file at `own`), and how many there are in all;
/// `None` without any.
fn quote_at(
    storage: &dyn quireink_sdk::Storage,
    shipped: Option<&str>,
    own: Option<&str>,
    position: impl Fn(usize) -> usize,
) -> Option<Quote> {
    let from_shipped = shipped.map_or(0, |s| quotes_in(s).count());
    let mut from_own = 0;
    if let Some(own) = own {
        each_line(storage, own, &mut |line| {
            from_own += usize::from(parse(line).is_some());
            true
        });
    }
    let total = from_shipped + from_own;
    if total == 0 {
        return None;
    }
    let at = position(total) % total;
    let quote = |(text, author): (&str, &str)| Quote {
        text: String::from(text),
        author: String::from(author),
    };
    if at < from_shipped {
        return shipped.and_then(|s| quotes_in(s).nth(at)).map(quote);
    }
    let mut found = None;
    let mut index = from_shipped;
    each_line(storage, own?, &mut |line| {
        let Some(parsed) = parse(line) else {
            return true;
        };
        if index == at {
            found = Some(quote(parsed));
            return false;
        }
        index += 1;
        true
    });
    found
}

/// Today's quote, turned on as often as tapped.
fn quote_of_the_day(env: &Env<'_>, day: i64) -> Option<Quote> {
    let source = env.settings.read(&SOURCE);
    let daily = daily(env);
    let turns = Turns::load(env.storage, &turns_file()).on(day, daily);
    let shipped = (source != SOURCES[1]).then(|| shipped(env.i18n.language()));
    let own = own_file();
    let own = (source != SOURCES[0]).then_some(own.as_str());
    quote_at(env.storage, shipped, own, |total| {
        let first = if daily { of_the_day(day, total) } else { 0 };
        first + turns as usize
    })
}

/// The quote of the day, read when updated.
#[derive(Default)]
struct QuoteWidget {
    quote: Option<Quote>,
    /// The day and the changes it was read at.
    read: Option<(i64, u32)>,
    /// The size it was last laid out at, in that area.
    fitted: Cell<Option<(Rect, f32)>>,
}

impl Widget for QuoteWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read each minute, but read anew only on another day or after a
        // tap.
        let day = today(env);
        let read = Some((day, changes()));
        if read == self.read {
            return false;
        }
        self.read = read;
        let quote = quote_of_the_day(env, day);
        let changed = quote != self.quote;
        self.quote = quote;
        self.fitted.set(None);
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let style = UiStyle::new(env);
        let line = widgets::line_height(env, style.title);
        line * 3
            + widgets::line_height(env, style.small)
            + style.gap as u16
            + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let Some(quote) = &self.quote else {
            let message = env.i18n.tr("widgets.quote.none");
            super::draw_empty(canvas, env, &look, message);
            return;
        };
        let mut area = look.area;
        if widgets::shows(env, ID.as_str(), CAPTION) {
            let caption = env.i18n.tr("widgets.quote.caption");
            area = widgets::draw_caption(canvas, env, ID.as_str(), &look, caption);
        }
        let Some(font) = text::font(env, env.settings.read(&FACE)) else {
            return;
        };
        let style = UiStyle::new(env);
        let language = env.i18n.language();
        let shown = if widgets::shows(env, ID.as_str(), MARKS) {
            text::quoted(&quote.text, language)
        } else {
            String::from(text::unquoted(&quote.text))
        };
        let author = (widgets::shows(env, ID.as_str(), AUTHOR) && !quote.author.is_empty())
            .then_some(quote.author.as_str());
        let mut layout = Layout {
            mark: widgets::shows(env, ID.as_str(), MARK),
            author: author.map_or(0.0, |_| {
                f32::from(widgets::line_height(env, style.small)) + style.gap as f32 * 0.75
            }),
        };
        // Where a line of the quote would not fit with them, the large
        // mark and who said it go.
        let least = text::line_height(&font, style.small * 0.85);
        if layout.height(&font, style.small * 0.85, 1) > f32::from(area.height) {
            layout.mark = false;
        }
        let author = author.filter(|_| least + layout.author <= f32::from(area.height));
        if author.is_none() {
            layout.author = 0.0;
        }
        let width = f32::from(area.width);
        let size = match self.fitted.get() {
            Some((at, size)) if at == area => size,
            _ => {
                let size = match widgets::size(env, ID.as_str()) {
                    Size::Fit => text::largest(style.small * 0.85, style.title * 2.0, |size| {
                        let lines = text::count_lines(&font, size, &shown, width);
                        layout.height(&font, size, lines) <= f32::from(area.height)
                    }),
                    size => size.text(&style),
                };
                self.fitted.set(Some((area, size)));
                size
            }
        };
        let line = text::line_height(&font, size);
        let room = f32::from(area.height) - layout.height(&font, size, 0);
        let lines = text::lines(&font, size, &shown, width, ((room / line) as usize).max(1));
        let total = layout.height(&font, size, lines.len()) as u16;
        let mut top = area.y + area.height.saturating_sub(total) / 2;
        let align = widgets::align(env, ID.as_str());
        if layout.mark {
            // The opening mark large and faint above the quote, its top
            // where the block begins.
            let mark = ornament(language);
            let big = size * MARK_SCALE;
            let x = align.x(area, font.measure(big, mark));
            // Quotation marks are as high as capitals, a third of them.
            let baseline = f32::from(top) + big * 0.72;
            font.draw(canvas, big, x, baseline as i32, mark, look.faint);
            top += layout.mark_height(size) as u16;
        }
        top += text::draw_lines(canvas, &font, size, &lines, area, top, align, look.ink);
        if let (Some(author), Some(ui)) = (author, style.font(env)) {
            top += (style.gap as f32 * 0.75) as u16;
            let small = widgets::line_height(env, style.small);
            let row = Rect::new(area.x, top, area.width, small);
            let name = quireink_sdk::ui::fit(&ui, style.small, author, width);
            let x = align.x(area, ui.measure(style.small, &name));
            quireink_sdk::ui::draw_label(canvas, &ui, style.small, x, row, &name, look.faint);
        }
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget {
            area,
            command: NEXT.emit(()),
        }]
    }
}

/// How large the opening mark above the quote is, to the quote's size.
const MARK_SCALE: f32 = 2.4;

/// The large mark above a quote in `language`: a raised one, as low
/// opening marks look lost above the text; guillemets where the
/// language has them.
fn ornament(language: &str) -> &'static str {
    match text::marks(language).0.trim() {
        "«" | "»" => "«",
        _ => "“",
    }
}

/// What goes with the quote's lines.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// A large opening mark above.
    mark: bool,
    /// The height of who said it, below (0 if not shown).
    author: f32,
}

impl Layout {
    /// The room of the large mark above the quote at `size`: the upper
    /// part of a line, where quotation marks are.
    fn mark_height(&self, size: f32) -> f32 {
        if self.mark {
            size * MARK_SCALE * 0.42
        } else {
            0.0
        }
    }

    /// The height of it all with `lines` lines at `size`.
    fn height(&self, font: &quireink_sdk::FontHandle<'_>, size: f32, lines: usize) -> f32 {
        self.mark_height(size) + text::line_height(font, size) * lines as f32 + self.author
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::SOURCE.as_str(),
            super::CHANGE.as_str(),
            super::FACE.as_str(),
            "widgets.quote.author",
            "widgets.quote.marks",
            "widgets.quote.mark",
            "widgets.quote.caption",
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["size", "align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
    use super::*;
    use quireink_sdk::MemoryStorage;

    #[test]
    fn enough_quotes_come_with_quireink() {
        for (text, language) in [(ENGLISH, "en"), (GERMAN, "de")] {
            let quotes: Vec<_> = quotes_in(text).collect();
            assert!(quotes.len() >= 60, "{language}: {}", quotes.len());
            for (quote, author) in &quotes {
                assert!(!author.is_empty(), "{quote}");
                // Marks come from the widget, as the language has them;
                // dashes are not used.
                assert!(!quote.starts_with(['"', '“', '„']), "{quote}");
                assert!(!quote.contains(['\u{2013}', '\u{2014}']), "{quote}");
            }
        }
        assert_eq!(shipped("de-AT"), GERMAN);
        assert_eq!(shipped("fr"), ENGLISH);
    }

    #[test]
    fn own_quotes_are_parsed() {
        assert_eq!(parse("Read on. | Me"), Some(("Read on.", "Me")));
        assert_eq!(
            parse("  \"Quoted\" |  Someone  "),
            Some(("Quoted", "Someone"))
        );
        assert_eq!(parse("No one said it"), Some(("No one said it", "")));
        // The last bar parts the author.
        assert_eq!(parse("a | b | c"), Some(("a | b", "c")));
        assert_eq!(parse("# a comment | x"), None);
        assert_eq!(parse("   "), None);
        assert_eq!(parse(" | Nobody"), None);
    }

    #[test]
    fn quotes_come_from_the_sources_chosen() {
        let own = "# Mine\nFirst own | A\n\nSecond own\n";
        let storage = MemoryStorage::new().with(&own_file(), own);
        let path = own_file();
        let shipped = "One | X\nTwo | Y\n";
        let at = |shipped: Option<&str>, own: Option<&str>, n: usize| {
            quote_at(&storage, shipped, own, |_| n).map(|q| q.text)
        };
        // Shipped first, then the own ones, round again.
        let both: Vec<_> = (0..5)
            .map(|n| at(Some(shipped), Some(&path), n).unwrap())
            .collect();
        assert_eq!(
            both,
            ["One", "Two", "First own", "Second own", "One"].map(String::from)
        );
        assert_eq!(at(None, Some(&path), 1).as_deref(), Some("Second own"));
        assert_eq!(at(Some(shipped), None, 3).as_deref(), Some("Two"));
        let author = quote_at(&storage, None, Some(&path), |_| 0).unwrap().author;
        assert_eq!(author, "A");
        // None at all: none shown.
        assert_eq!(at(None, Some("missing.txt"), 0), None);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn own_quotes_of_version_1_are_read() {
        let storage =
            MemoryStorage::new().with(&own_file(), include_str!("../../testdata/quotes.txt.v1"));
        let path = own_file();
        let all: Vec<Quote> = (0..3)
            .map(|n| quote_at(&storage, None, Some(&path), |_| n).unwrap())
            .collect();
        assert_eq!(all[0].author, "Someone");
        assert_eq!(all[1].text, "Read on.");
        assert_eq!(all[2].text, "No one said this");
        assert_eq!(all[2].author, "");
    }

    #[test]
    fn a_quote_a_day() {
        let storage = MemoryStorage::new();
        let pick = |day: i64| {
            quote_at(&storage, Some(ENGLISH), None, |total| {
                of_the_day(day, total)
            })
            .unwrap()
        };
        assert_eq!(pick(20_375), pick(20_375));
        assert_ne!(pick(20_375), pick(20_376));
    }
}
