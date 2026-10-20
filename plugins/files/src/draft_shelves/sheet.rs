//! What can be done with a book: a sheet over the bottom of the screen
//! with the book's cover, title, author and how far it is read, and its
//! actions as pills (Continue or Open, Next in series, Mark as finished
//! or unread, Favourite, Information). The first is the one most likely
//! wanted; buttons go from pill to pill, a tap does it at once.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
    library,
};

use super::art;
use super::gather::{self, Source};
use super::summary::{Item, State};
use super::{CHANGED, open_book};

/// What a pill does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Do {
    Open,
    Next,
    Finished,
    Unfinished,
    Favourite,
    Unfavourite,
    Info,
}

pub(crate) struct Sheet {
    style: UiStyle,
    region: Rect,
    item: Item,
    next: Option<Item>,
    thumb: Option<Thumbnail>,
    cover: Rect,
    /// The title's lines, the author's and the series' line.
    title: Vec<String>,
    series: String,
    text_x: u16,
    pills: Vec<(Do, String, Rect)>,
    focus: usize,
    shown: bool,
}

impl Sheet {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let path = params.get(params::PATH).unwrap_or_default();
        let book = library::book(env.storage, path).unwrap_or_default();
        let item = Item::of(path, &book);
        let favourite = library::favourites(env.storage).iter().any(|p| p == path);
        let next = next_in_series(env, &item);
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let font = style.font(env);

        // Laid out from the sheet's top, moved down to the bottom after.
        let line = |size: f32| {
            font.as_ref()
                .map_or(size * 1.3, |f| f.line_metrics(size).line_height) as u16
        };
        let mut y = gap * 2;
        let least = style.row as u16 * 2;
        let cover_width = least * 2 / 3;
        let text_x = margin + cover_width + gap * 2;
        let text_width = display.width.saturating_sub(text_x + margin);
        let title: Vec<String> = match &font {
            Some(font) => font
                .wrap(style.title, &item.title, f32::from(text_width))
                .into_iter()
                .take(2)
                .map(|r| item.title[r].to_string())
                .collect(),
            None => Vec::new(),
        };
        let series = if item.series.is_empty() {
            String::new()
        } else if item.number.is_empty() {
            item.series.clone()
        } else {
            let number = env
                .i18n
                .format("files.series-number", &[("number", Arg::Str(&item.number))]);
            alloc::format!("{} · {number}", item.series)
        };
        // Title, author and series, then how far it is read and its bar:
        // the cover as high as they are.
        let notes = [&item.author, &series]
            .iter()
            .filter(|t| !t.is_empty())
            .count() as u16;
        let text_height = title.len() as u16 * line(style.title)
            + notes * line(style.small)
            + gap
            + line(style.small)
            + bar_height(env, &style);
        let cover_height = text_height.max(least);
        let cover = Rect::new(margin, y, cover_width, cover_height);
        let format = display.format;
        let thumb = library::cover(env.storage, path, &book, cover_width, cover_height, format);
        y += cover_height + gap * 2;

        // The pills, as many in a row as fit.
        let mut wanted = alloc::vec![(
            Do::Open,
            match item.state() {
                State::Reading(_) => "files.shelves.continue-reading",
                _ => "files.option.open",
            }
        )];
        if next.is_some() {
            wanted.push((Do::Next, "files.shelves.next-in-series"));
        }
        wanted.push(match item.state() {
            State::Finished => (Do::Unfinished, "files.option.unfinished"),
            _ => (Do::Finished, "files.option.finished"),
        });
        wanted.push(if favourite {
            (Do::Unfavourite, "files.shelves.unfavourite")
        } else {
            (Do::Favourite, "files.shelves.favourite")
        });
        wanted.push((Do::Info, "files.shelves.information"));
        let height = ui::button_height(Rect::new(0, 0, 0, style.touch_row(style.row) as u16));
        let mut pills = Vec::new();
        let mut x = margin;
        for (what, key) in wanted {
            let label = env.i18n.tr(key).to_string();
            let words = font
                .as_ref()
                .map_or(0.0, |f| f.measure(style.small, &label)) as u16;
            let width = (words + gap * 3).max(height * 2);
            if x > margin && x + width > display.width - margin {
                x = margin;
                y += height + gap;
            }
            pills.push((what, label, Rect::new(x, y, width, height)));
            x += width + gap;
        }
        y += height + gap * 2;

        let top = display.height.saturating_sub(y);
        let shift = |r: Rect| Rect::new(r.x, r.y + top, r.width, r.height);
        for pill in &mut pills {
            pill.2 = shift(pill.2);
        }
        Self {
            region: Rect::new(0, top, display.width, y.min(display.height)),
            cover: shift(cover),
            item,
            next,
            thumb,
            title,
            series,
            text_x,
            pills,
            focus: 0,
            shown: !env.device.input.touch,
            style,
        }
    }

    fn run(&self, what: Do, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let path = self.item.path.clone();
        cx.command(Command::Back);
        match what {
            Do::Open => cx.command(open_book(&path)),
            Do::Next => {
                if let Some(next) = &self.next {
                    cx.command(open_book(&next.path));
                }
            }
            Do::Finished | Do::Unfinished => {
                library::set_finished(env.storage, &path, what == Do::Finished);
                cx.command(Command::Emit(CHANGED, Params::new()));
            }
            Do::Favourite | Do::Unfavourite => {
                let favourite = what == Do::Favourite;
                library::set_favourite(env.storage, &path, favourite);
                let key = if favourite {
                    "files.favourite-added"
                } else {
                    "files.favourite-removed"
                };
                cx.command(Command::Notify(env.i18n.tr(key).to_string()));
                cx.command(Command::Emit(CHANGED, Params::new()));
            }
            Do::Info => cx.command(Command::OpenScreen(
                crate::INFO_SCREEN,
                Params::new().with(params::PATH, path),
            )),
        }
    }
}

/// The height of the bar that shows how far a book is read.
fn bar_height(env: &Env<'_>, style: &UiStyle) -> u16 {
    (env.metrics.tenth_mm(9) as u16).max(3) + 2 * style.line.max(1) as u16
}

/// The book after `item` in its series not read to the end yet.
fn next_in_series(env: &Env<'_>, item: &Item) -> Option<Item> {
    if item.series.is_empty() {
        return None;
    }
    let root = super::root(env);
    let source = Source::Series(item.series.clone());
    let gathered = gather::gather(env, &root, &[source], 100);
    let number = item.series_number();
    gathered
        .into_iter()
        .next()?
        .tiles
        .into_iter()
        .map(|t| t.item().clone())
        .find(|b| b.path != item.path && b.series_number() > number && b.state() != State::Finished)
}

impl Screen for Sheet {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let r = self.region;
        canvas.fill(r, Color::WHITE);
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (style.line * 2).max(2),
            Color::BLACK,
        );
        let drawn = art::draw_cover(
            canvas,
            env,
            &style,
            self.cover,
            &self.item,
            self.thumb.as_ref(),
            false,
        );
        if let State::Finished = self.item.state() {
            art::draw_state(
                canvas,
                env,
                &style,
                drawn,
                State::Finished,
                art::Marks::Quiet,
            );
        }
        let Some(font) = style.font(env) else {
            return;
        };
        let margin = style.margin as u16;
        let x = self.text_x;
        let width = r.width.saturating_sub(x + margin);
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let mut y = self.cover.y;
        for text in &self.title {
            let row = Rect::new(x, y, width, line(style.title));
            ui::draw_label(
                canvas,
                &font,
                style.title,
                f32::from(x),
                row,
                text,
                Color::BLACK,
            );
            y += line(style.title);
        }
        let gray = style.faint(80);
        for text in [&self.item.author, &self.series] {
            if text.is_empty() {
                continue;
            }
            let row = Rect::new(x, y, width, line(style.small));
            ui::draw_label(canvas, &font, style.small, f32::from(x), row, text, gray);
            y += line(style.small);
        }
        // How far it is read, at the bottom beside the cover.
        let state = match self.item.state() {
            State::New => env.i18n.tr("files.shelves.not-started").to_string(),
            State::Reading(p) => env
                .i18n
                .format("files.shelves.read", &[("percent", Arg::Int(i64::from(p)))]),
            State::Finished => env.i18n.tr("files.finished").to_string(),
        };
        let bottom = self.cover.y + self.cover.height;
        let small = line(style.small);
        let border = style.line.max(1) as u16;
        let bar = bar_height(env, &style);
        let row = Rect::new(x, bottom.saturating_sub(small + bar), width, small);
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(x),
            row,
            &state,
            Color::BLACK,
        );
        if let State::Reading(p) = self.item.state() {
            let area = Rect::new(x, bottom.saturating_sub(bar), width, bar);
            art::progress_in(canvas, area, border, p, false);
        }
        for (i, (_, label, rect)) in self.pills.iter().enumerate() {
            let focused = self.shown && self.focus == i;
            ui::draw_button(canvas, env, &style, *rect, label, i == 0, focused);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let len = self.pills.len();
        let step = |delta: isize| ui::step_focus(self.focus, delta, len);
        if [
            actions::LEFT,
            actions::UP,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            self.focus = if self.shown { step(-1) } else { self.focus };
        } else if [
            actions::RIGHT,
            actions::DOWN,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            self.focus = if self.shown { step(1) } else { self.focus };
        } else if *a == actions::CONFIRM {
            if let Some((what, _, _)) = self.pills.get(self.focus) {
                self.run(*what, cx);
            }
            return Handled::Yes;
        } else if *a == actions::BACK || *a == actions::MENU {
            cx.command(Command::Back);
            return Handled::Yes;
        } else {
            return Handled::No;
        }
        self.shown = true;
        cx.request_redraw_area(self.region);
        cx.request_refresh(paperpaw_sdk::RefreshMode::Fast);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let tapped = self
            .pills
            .iter()
            .find(|(_, _, rect)| rect.contains(gesture.start))
            .map(|(what, _, _)| *what);
        match tapped {
            Some(what) => {
                self.run(what, cx);
                Handled::Yes
            }
            None if self.region.contains(gesture.start) => Handled::Yes,
            None => Handled::No,
        }
    }
}
