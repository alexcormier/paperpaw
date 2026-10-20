//! The wall's sheets over its bottom: what can be done with a book (its
//! cover, title, author and progress above a few pill buttons) and the
//! letters to find a title by.
//!
//! Both work with buttons (left and right move along the pills, confirm
//! chooses, back closes) and by touch (tap a pill; tap outside closes).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen, files,
};

use super::index::{initial, sort_key};
use super::{BOOK_CHANGED, LETTER_CHOSEN};
use crate::{ARCHIVE, INFO_SCREEN};

/// The pills of a sheet: in two columns, each as high as a finger is
/// wide on touch screens.
fn pill_height(style: &UiStyle) -> u16 {
    ((style.row * 7 / 10).max(style.finger)) as u16
}

/// The top edge of a sheet: a line across.
fn draw_edge(canvas: &mut Canvas<'_>, style: &UiStyle, region: Rect) {
    canvas.fill(region, Color::WHITE);
    canvas.fill_rect(
        i32::from(region.x),
        i32::from(region.y),
        u32::from(region.width),
        (style.line * 2).max(2),
        Color::BLACK,
    );
}

/// The focus among `len` pills moved by `delta`, staying within them.
fn moved(focus: usize, delta: isize, len: usize) -> usize {
    (focus as isize + delta).clamp(0, len.saturating_sub(1) as isize) as usize
}

/// What can be done with a book; parameter `path`.
pub(crate) struct BookSheet {
    style: UiStyle,
    region: Rect,
    path: String,
    /// The title's lines, at most two.
    title: Vec<String>,
    author: String,
    progress: Option<u8>,
    cover: Rect,
    thumb: Option<Thumbnail>,
    text: Rect,
    /// The options and where their pills are.
    options: Vec<(&'static str, Rect)>,
    /// The next book of its series to read, if any.
    next: Option<String>,
    focus: usize,
    shown: bool,
}

impl BookSheet {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let path = params.get(params::PATH).unwrap_or_default().to_string();
        let storage = env.storage;
        let book = library::book(storage, &path).unwrap_or_default();
        let progress = book.progress;
        let next = book
            .series
            .as_deref()
            .and_then(|series| next_in_series(env, series, &path, &book));
        let favourite = library::favourites(storage).contains(&path);

        let mut options = Vec::new();
        options.push(match progress {
            Some(p) if p > 0 && p < 100 => "continue",
            _ => "open",
        });
        if next.is_some() {
            options.push("next");
        }
        options.push(if progress == Some(100) {
            "unfinished"
        } else {
            "finished"
        });
        options.push(if favourite {
            "unfavourite"
        } else {
            "favourite"
        });
        options.push("info");
        let archive = env.settings.read(&ARCHIVE).trim_matches('/');
        if !archive.is_empty() && files::parent(&path) != archive {
            options.push("archive");
        }

        // The cover beside title, author and progress; the pills below.
        let display = env.device.display;
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let room = display.width.saturating_sub(2 * margin);
        let cover_h = (style.row * 2) as u16;
        let cover_w = cover_h * 2 / 3;
        let text_x = margin + cover_w + gap;
        let text_w = display.width.saturating_sub(text_x + margin);
        let (mut title, mut text_h) = (Vec::new(), 0);
        let shown_title = book.shown_title(&path).into_owned();
        if let Some(font) = style.font(env) {
            let line = font.line_metrics(style.title).line_height as u16;
            let wrapped = font.wrap(style.title, &shown_title, f32::from(text_w));
            let more = wrapped.len() > 2;
            for (n, range) in wrapped.into_iter().take(2).enumerate() {
                let text = if more && n == 1 {
                    ui::fit(
                        &font,
                        style.title,
                        &shown_title[range.start..],
                        f32::from(text_w),
                    )
                } else {
                    shown_title[range].to_string()
                };
                title.push(text);
            }
            let small = font.line_metrics(style.small).line_height as u16;
            text_h = line * title.len() as u16 + small * 2 + gap;
        }
        let head = cover_h.max(text_h);
        let pill_h = pill_height(&style);
        let rows = options.len().div_ceil(2) as u16;
        let height = gap * 2 + head + gap * 2 + rows * (pill_h + gap) + gap;
        let region = Rect::new(
            0,
            display.height.saturating_sub(height),
            display.width,
            height,
        );
        let top = region.y + gap * 2;
        let cover = Rect::new(margin, top, cover_w, cover_h);
        let thumb = library::cover(storage, &path, &book, cover_w, cover_h, display.format);
        let pills_top = top + head + gap * 2;
        let pill_w = room.saturating_sub(gap) / 2;
        let options = options
            .into_iter()
            .enumerate()
            .map(|(i, option)| {
                let (row, column) = ((i / 2) as u16, (i % 2) as u16);
                let at = Rect::new(
                    margin + column * (pill_w + gap),
                    pills_top + row * (pill_h + gap),
                    pill_w,
                    pill_h,
                );
                (option, at)
            })
            .collect();
        Self {
            region,
            path,
            title,
            author: book.author.clone().unwrap_or_default(),
            progress,
            cover,
            thumb,
            text: Rect::new(text_x, top, text_w, head),
            options,
            next,
            focus: 0,
            shown: !env.device.input.touch,
            style,
        }
    }

    /// Does what option `index` says, and closes.
    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((option, _)) = self.options.get(index) else {
            return;
        };
        let env = *cx.env();
        let path = self.path.clone();
        cx.command(Command::Back);
        let changed = |cx: &mut Cx<'_, '_>, path: &str| {
            cx.command(BOOK_CHANGED.emit(ui::choice::Picked {
                path: path.to_string(),
                option: String::new(),
            }));
        };
        match *option {
            "open" | "continue" => cx.command(Command::OpenSlot(
                slots::READER,
                Params::new().with(params::PATH, path),
            )),
            "next" => {
                if let Some(next) = &self.next {
                    cx.command(Command::OpenSlot(
                        slots::READER,
                        Params::new().with(params::PATH, next.clone()),
                    ));
                }
            }
            "finished" | "unfinished" => {
                library::set_finished(env.storage, &path, *option == "finished");
                changed(cx, &path);
            }
            "favourite" | "unfavourite" => {
                let favourite = *option == "favourite";
                library::set_favourite(env.storage, &path, favourite);
                let key = if favourite {
                    "files.favourite-added"
                } else {
                    "files.favourite-removed"
                };
                cx.command(Command::Notify(env.i18n.tr(key).to_string()));
                changed(cx, &path);
            }
            "info" => cx.command(Command::OpenScreen(
                INFO_SCREEN,
                Params::new().with(params::PATH, path),
            )),
            "archive" => {
                let archive = env.settings.read(&ARCHIVE).trim_matches('/').to_string();
                let _ = env.storage.create_dir(&archive);
                let target = files::free_path(env.storage, &archive, files::name(&path));
                match files::move_to(env.storage, &path, &target) {
                    Ok(()) => {
                        cx.command(events::BOOK_MOVED.emit(events::BookMoved {
                            from: path,
                            path: target,
                        }));
                        cx.command(Command::Notify(env.i18n.tr("files.archived").to_string()));
                        cx.command(library::changed());
                    }
                    Err(_) => cx.command(Command::Notify(
                        env.i18n.tr("files.archive-failed").to_string(),
                    )),
                }
            }
            _ => {}
        }
    }
}

/// The book of `series` to read after the one at `path` (`book`): the
/// first not finished that follows it by number, else the first not
/// finished at all.
fn next_in_series(env: &Env<'_>, series: &str, path: &str, book: &library::Book) -> Option<String> {
    let number = |n: &str| n.trim().parse::<f32>().unwrap_or(f32::MAX);
    let mut others: Vec<(f32, String)> = Vec::new();
    catalog::each(env.storage, &mut |record| {
        if record.series == series && record.path != path {
            others.push((number(&record.number), record.path));
        }
    });
    others.sort_by(|a, b| a.0.total_cmp(&b.0));
    let unfinished = |p: &str| {
        env.storage.size(p).is_ok()
            && library::book(env.storage, p).and_then(|b| b.progress) != Some(100)
    };
    let own = book.series_index.as_deref().map_or(f32::MAX, number);
    others
        .iter()
        .find(|(n, p)| *n > own && unfinished(p))
        .or_else(|| others.iter().find(|(_, p)| unfinished(p)))
        .map(|(_, p)| p.clone())
}

impl Screen for BookSheet {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        draw_edge(canvas, &style, self.region);
        let Some(font) = style.font(env) else {
            return;
        };
        match &self.thumb {
            Some(thumb) => {
                let y = self.cover.y + self.cover.height.saturating_sub(thumb.height());
                let radius = crate::picture_radius(&style, self.cover);
                canvas.small_pictures(|canvas| thumb.draw_round(canvas, self.cover.x, y, radius));
            }
            None => super::draw_placeholder(
                canvas,
                env,
                &style,
                self.cover,
                self.title.first().map_or("", String::as_str),
                &self.author,
            ),
        }
        let t = self.text;
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let mut y = t.y;
        for text in &self.title {
            let row = Rect::new(t.x, y, t.width, line(style.title));
            ui::draw_label(
                canvas,
                &font,
                style.title,
                f32::from(t.x),
                row,
                text,
                Color::BLACK,
            );
            y += line(style.title);
        }
        if !self.author.is_empty() {
            let row = Rect::new(t.x, y, t.width, line(style.small));
            let faint = style.faint(80);
            ui::draw_label(
                canvas,
                &font,
                style.small,
                f32::from(t.x),
                row,
                &self.author,
                faint,
            );
            y += line(style.small);
        }
        let row = Rect::new(t.x, y + style.gap as u16 / 2, t.width, line(style.small));
        super::draw_progress_line(canvas, env, &style, row, self.progress, Color::BLACK);

        for (i, (option, at)) in self.options.iter().enumerate() {
            let key = format!("files.wall.option.{option}");
            let label = env.i18n.tr(&key);
            let focused = self.shown && i == self.focus;
            // The first is what is done most: filled.
            ui::draw_button(canvas, env, &style, *at, label, i == 0, focused);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let delta = if [actions::LEFT, actions::PREVIOUS].contains(a) {
            -1
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            1
        } else if [actions::UP, actions::PREV_PAGE].contains(a) {
            -2
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(a) {
            2
        } else if *a == actions::CONFIRM {
            self.choose(self.focus, cx);
            return Handled::Yes;
        } else if [actions::BACK, actions::MENU].contains(a) {
            cx.command(Command::Back);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if self.shown {
            self.focus = moved(self.focus, delta, self.options.len());
        }
        self.shown = true;
        cx.request_redraw_area(self.region);
        cx.request_refresh(RefreshMode::Fast);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self
            .options
            .iter()
            .position(|(_, at)| at.contains(gesture.start))
        {
            self.choose(index, cx);
        }
        Handled::Yes
    }
}

/// The letters titles start with, as keys; those no title starts with
/// are faint and skipped.
pub(crate) struct LetterSheet {
    style: UiStyle,
    region: Rect,
    title: Rect,
    /// The letter, whether a title starts with it, and its key.
    keys: Vec<(u8, bool, Rect)>,
    focus: usize,
    shown: bool,
}

/// Keys in a row of the letters.
const LETTER_COLUMNS: u16 = 7;

impl LetterSheet {
    pub(crate) fn new(env: &Env<'_>, _: &Params) -> Self {
        let style = UiStyle::new(env);
        let mut found = [false; 27];
        catalog::each(
            env.storage,
            &mut |record| match initial(sort_key(&record.title)) {
                b'#' => found[26] = true,
                c => found[usize::from(c - b'A')] = true,
            },
        );
        let display = env.device.display;
        let margin = style.margin as u16;
        let gap = style.gap as u16;
        let room = display.width.saturating_sub(2 * margin);
        let between = gap / 2;
        let key_w = room.saturating_sub(between * (LETTER_COLUMNS - 1)) / LETTER_COLUMNS;
        let key_h = pill_height(&style).max(key_w * 4 / 5);
        let title_h = style.row as u16;
        let rows = 27u16.div_ceil(LETTER_COLUMNS);
        let height = gap + title_h + rows * (key_h + between) + gap * 2;
        let region = Rect::new(
            0,
            display.height.saturating_sub(height),
            display.width,
            height,
        );
        let title = Rect::new(0, region.y + gap, display.width, title_h);
        let top = title.y + title_h;
        let keys: Vec<(u8, bool, Rect)> = (b'A'..=b'Z')
            .chain(core::iter::once(b'#'))
            .zip(found)
            .enumerate()
            .map(|(i, (letter, found))| {
                let (row, column) = (i as u16 / LETTER_COLUMNS, i as u16 % LETTER_COLUMNS);
                let at = Rect::new(
                    margin + column * (key_w + between),
                    top + row * (key_h + between),
                    key_w,
                    key_h,
                );
                (letter, found, at)
            })
            .collect();
        let focus = keys.iter().position(|k| k.1).unwrap_or(0);
        Self {
            style,
            region,
            title,
            keys,
            focus,
            shown: !env.device.input.touch,
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((letter, true, _)) = self.keys.get(index) else {
            return;
        };
        cx.command(Command::Back);
        cx.command(LETTER_CHOSEN.emit(ui::choice::Picked {
            path: String::from(*letter as char),
            option: String::new(),
        }));
    }

    /// The next letter a title starts with, `delta` keys on.
    fn step(&self, delta: isize) -> usize {
        let mut at = self.focus as isize;
        loop {
            at += delta;
            match self.keys.get(at.max(-1) as usize) {
                Some((_, true, _)) if at >= 0 => return at as usize,
                Some(_) if at >= 0 => {}
                _ => return self.focus,
            }
        }
    }
}

impl Screen for LetterSheet {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        draw_edge(canvas, &style, self.region);
        let Some(font) = style.font(env) else {
            return;
        };
        let title = env.i18n.tr("files.wall.letter");
        let x = style.margin as f32;
        ui::draw_label(
            canvas,
            &font,
            style.title,
            x,
            self.title,
            title,
            Color::BLACK,
        );
        for (i, (letter, found, at)) in self.keys.iter().enumerate() {
            let mut text = [0u8; 4];
            let label = (*letter as char).encode_utf8(&mut text);
            if *found {
                let focused = self.shown && i == self.focus;
                ui::draw_button(canvas, env, &style, *at, label, false, focused);
            } else {
                // No title starts with it: a faint letter, no key.
                let w = font.measure(style.text, label);
                let x = f32::from(at.x) + (f32::from(at.width) - w) / 2.0;
                ui::draw_label(canvas, &font, style.text, x, *at, label, style.faint(170));
            }
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let columns = LETTER_COLUMNS as isize;
        let delta = if [actions::LEFT, actions::PREVIOUS].contains(a) {
            -1
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            1
        } else if [actions::UP, actions::PREV_PAGE].contains(a) {
            -columns
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(a) {
            columns
        } else if *a == actions::CONFIRM {
            self.choose(self.focus, cx);
            return Handled::Yes;
        } else if [actions::BACK, actions::MENU].contains(a) {
            cx.command(Command::Back);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if self.shown {
            self.focus = self.step(delta);
        }
        self.shown = true;
        cx.request_redraw_area(self.region);
        cx.request_refresh(RefreshMode::Fast);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self
            .keys
            .iter()
            .position(|(_, _, at)| at.contains(gesture.start))
        {
            self.choose(index, cx);
        }
        Handled::Yes
    }
}
