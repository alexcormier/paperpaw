//! *News headlines*: the newest headlines of the feeds fetched last, in
//! the shared look of widgets (its frame, size and alignment), with
//! settings of its own: how many, a line or two each, the feed's name
//! below each, lines between them. A tap on a headline opens its feed's
//! book; without feeds the widget says how to add them, and a tap opens
//! the list of feeds.
//!
//! The headlines are read when the widget is updated (when shown and
//! after events), never as they are drawn.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::builtin::{params, slots};
use quireink_sdk::ui::{self, UiStyle};
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{
    Canvas, Color, Command, Env, FontHandle, Id, Params, Rect, RegisterError, Registrar,
};

use crate::{OPEN, headlines};

pub(crate) const HEADLINES: Id = Id::new_static(NAME);
const NAME: &str = "news.widget.headlines";
/// Headlines shown at most (fewer where they do not fit).
const COUNTS: [&str; 5] = [
    "news.count.2",
    "news.count.3",
    "news.count.4",
    "news.count.6",
    "news.count.8",
];
/// The most of [`COUNTS`].
const MOST: usize = 8;
/// Lines a headline takes at most.
const LINES: [&str; 2] = ["news.lines.1", "news.lines.2"];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    let id = &HEADLINES;
    r.add_setting_group(widgets::settings_group(id, "news.widget.headlines"))?;
    r.add_setting(widgets::frame_setting(id, Frame::None))?;
    r.add_setting(widgets::size_setting(id, Size::Small, false))?;
    r.add_setting(widgets::align_setting(id, Align::Left))?;
    r.add_setting(widgets::choice(
        id,
        "count",
        "news.setting.count",
        &COUNTS,
        COUNTS[2],
    ))?;
    r.add_setting(widgets::choice(
        id,
        "lines",
        "news.setting.lines",
        &LINES,
        LINES[0],
    ))?;
    for (key, title, default) in [
        ("caption", "news.part.caption", true),
        ("source", "news.part.source", false),
        ("rules", "news.part.rules", true),
    ] {
        r.add_setting(widgets::part_setting(id, key, title, default))?;
    }
    r.add_widget(
        WidgetSpec::new(HEADLINES, "news.widget.headlines", |_| {
            Box::new(Headlines::default())
        })
        .category(widgets::categories::WORLD),
    )
}

/// How many headlines it shows at most, by its setting.
fn count(env: &Env<'_>) -> usize {
    let value = env.settings.text(&alloc::format!("{NAME}.count"));
    value
        .rsplit('.')
        .next()
        .and_then(|n| n.parse().ok())
        .map_or(4, |n: usize| n.clamp(1, MOST))
}

/// Lines a headline takes at most, by its setting.
fn lines_each(env: &Env<'_>) -> u16 {
    if env.settings.text(&alloc::format!("{NAME}.lines")) == LINES[1] {
        2
    } else {
        1
    }
}

/// The feed a book stands for: its file's name (`News/Tagesschau.html`
/// is "Tagesschau").
fn source(book: &str) -> &str {
    let file = book.rsplit('/').next().unwrap_or(book);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

/// The headlines widget: the last ones fetched, read when updated.
#[derive(Default)]
pub(crate) struct Headlines {
    /// Each one's title and its book.
    shown: Vec<(String, String)>,
}

impl Widget for Headlines {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let shown = headlines(env, count(env));
        let changed = shown != self.shown;
        self.shown = shown;
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
        let rows = self.shown.len().max(2) as u16;
        frame_height(env) + caption_room(env) + rows * row_height(env, &font, &style)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        if self.shown.is_empty() {
            widgets::draw_message(canvas, env, &look, env.i18n.tr("news.widget.empty"));
            return;
        }
        let rows = rows(env, &look, self.shown.len());
        let caption = env.i18n.tr("news.title");
        if widgets::shows(env, NAME, "caption") {
            let at = Look {
                area: Rect::new(look.area.x, rows.0, look.area.width, look.area.height),
                ..look
            };
            widgets::draw_caption(canvas, env, NAME, &at, caption);
        }
        for (n, row) in rows.1.iter().enumerate() {
            let (title, book) = &self.shown[n];
            draw_row(canvas, env, &look, *row, title, source(book), n > 0);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::look::look(env, NAME, area);
        if self.shown.is_empty() {
            return alloc::vec![WidgetTarget {
                area,
                command: Command::RunAction(OPEN),
            }];
        }
        rows(env, &look, self.shown.len())
            .1
            .into_iter()
            .zip(&self.shown)
            .map(|(row, (_, book))| WidgetTarget {
                area: row,
                command: Command::OpenSlot(
                    slots::READER,
                    Params::new().with(params::PATH, book.to_string()),
                ),
            })
            .collect()
    }
}

/// The height `widget`'s frame takes around its content.
fn frame_height(env: &Env<'_>) -> u16 {
    let probe = Rect::new(0, 0, 1000, 1000);
    1000 - widgets::look::look(env, NAME, probe).area.height
}

/// The caption's height, if it shows.
fn caption_room(env: &Env<'_>) -> u16 {
    if widgets::shows(env, NAME, "caption") {
        widgets::caption_height(env)
    } else {
        0
    }
}

/// The headlines' text size.
fn text_size(env: &Env<'_>, style: &UiStyle) -> f32 {
    widgets::size(env, NAME).text(style)
}

fn line(font: &FontHandle<'_>, size: f32) -> u16 {
    font.line_metrics(size).line_height as u16
}

/// The height of a headline's row: its lines, its source, and room
/// around a rule between rows.
fn row_height(env: &Env<'_>, font: &FontHandle<'_>, style: &UiStyle) -> u16 {
    let text = line(font, text_size(env, style)) * lines_each(env);
    let source = if widgets::shows(env, NAME, "source") {
        line(font, style.small)
    } else {
        0
    };
    let room = if widgets::shows(env, NAME, "rules") {
        style.gap as u16
    } else {
        style.gap as u16 / 3
    };
    text + source + room
}

/// Where the caption starts and the rows of as many of `count`
/// headlines as fit in `look`'s area, together in the middle.
fn rows(env: &Env<'_>, look: &Look, count: usize) -> (u16, Vec<Rect>) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return (look.area.y, Vec::new());
    };
    let area = look.area;
    let caption = caption_room(env);
    let row = row_height(env, &font, &style).max(1);
    let fit = usize::from(area.height.saturating_sub(caption) / row);
    let n = count.min(fit).max(1);
    let total = (caption + row * n as u16).min(area.height);
    let top = area.y + (area.height - total) / 2;
    let rows = (0..n)
        .map(|i| Rect::new(area.x, top + caption + row * i as u16, area.width, row))
        .collect();
    (top, rows)
}

/// A headline in its row: a rule above it (but for the first) if rules
/// show, its lines, its source below.
fn draw_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    look: &Look,
    row: Rect,
    title: &str,
    source: &str,
    rule: bool,
) {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return;
    };
    let size = text_size(env, &style);
    let small = line(&font, style.small);
    let rules = widgets::shows(env, NAME, "rules");
    let room = if rules {
        style.gap as u16
    } else {
        style.gap as u16 / 3
    };
    if rule && rules {
        // A thin rule, dithered light where the panel has few grays.
        let thin = style.line.max(1) as u16;
        let gray = if look.paper.luma() < 128 {
            Color::gray(110)
        } else {
            Color::gray(150)
        };
        canvas.fill_shaded(Rect::new(row.x, row.y, row.width, thin), gray);
    }
    let align = widgets::align(env, NAME);
    let source_h = if widgets::shows(env, NAME, "source") {
        small
    } else {
        0
    };
    let text = Rect::new(
        row.x,
        row.y + room / 2,
        row.width,
        row.height.saturating_sub(room + source_h),
    );
    if lines_each(env) > 1 {
        widgets::draw_wrapped(canvas, env, text, align, size, title, look.ink);
    } else {
        let shown = ui::fit(&font, size, title, f32::from(text.width));
        let x = align.x(text, font.measure(size, &shown));
        ui::draw_label(canvas, &font, size, x, text, &shown, look.ink);
    }
    if source_h > 0 {
        let at = Rect::new(row.x, text.y + text.height, row.width, small);
        let shown = ui::fit(&font, style.small, source, f32::from(at.width));
        let x = align.x(at, font.measure(style.small, &shown));
        ui::draw_label(canvas, &font, style.small, x, at, &shown, look.faint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_named_by_their_books() {
        assert_eq!(source("books/News/Tagesschau.html"), "Tagesschau");
        assert_eq!(source("News/The Guardian.html"), "The Guardian");
        assert_eq!(source("plain"), "plain");
    }

    #[test]
    fn counts_are_numbers() {
        let counts: Vec<usize> = COUNTS
            .iter()
            .filter_map(|c| c.rsplit('.').next()?.parse().ok())
            .collect();
        assert_eq!(counts.len(), COUNTS.len());
        assert!(counts.iter().all(|&c| (1..=MOST).contains(&c)));
    }
}
