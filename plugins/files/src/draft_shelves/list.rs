//! A shelf seen in full: its books as a calm list with covers, titles,
//! authors and how far each is read; a series by its numbers with the
//! next unread book marked; the series, authors or collections by name.
//!
//! Lists in alphabetical order jump from letter to letter with left and
//! right (the hints below the screen name the letters), so a book is
//! found with buttons alone without typing.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, MemoryClass, Params, Rect,
    RefreshMode, Screen,
};

use super::art::{self, Marks};
use super::gather::{self, Source, Tile};
use super::summary::{self, State, initial};
use super::{CHANGED, Covers};

/// Books listed at most where memory is small; the summary is read
/// again for more ([`Source::Search`] narrows it).
const MAX_SMALL: usize = 200;
const MAX_LARGE: usize = 2000;

pub(crate) struct ListScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    source: Source,
    title: String,
    tiles: Vec<Tile>,
    /// The next unread book of a series being read.
    next: Option<usize>,
    covers: Covers,
    /// The summary being made, where there is none yet.
    walk: Option<summary::Walk>,
    fresh: bool,
}

impl ListScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let source = Source::parse(
            params.get(params::MODE).unwrap_or_default(),
            params.get(params::TEXT).unwrap_or_default(),
        );
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            title: super::shelf_title(env, &source),
            source,
            style,
            header,
            tiles: Vec::new(),
            next: None,
            covers: Covers::default(),
            walk: None,
            fresh: true,
        };
        screen.gather(env);
        screen
    }

    fn gather(&mut self, env: &Env<'_>) {
        let cap = match env.device.memory {
            MemoryClass::Small => MAX_SMALL,
            MemoryClass::Large => MAX_LARGE,
        };
        let root = super::root(env);
        let gathered = gather::gather(env, &root, core::slice::from_ref(&self.source), cap);
        self.tiles = gathered.into_iter().next().unwrap_or_default().tiles;
        // In a series being read, the first book not opened yet.
        let started = self.tiles.iter().any(|t| t.item().state() != State::New);
        self.next = match self.source {
            Source::Series(_) if started => self
                .tiles
                .iter()
                .position(|t| t.item().state() == State::New),
            _ => None,
        };
        self.list.focus = self.list.focus.min(self.tiles.len().saturating_sub(1));
        self.show_covers(env);
    }

    /// The covers of the page shown, as large as they are drawn.
    fn show_covers(&mut self, env: &Env<'_>) {
        let rows = self.list.rows();
        let first = self.list.page() * rows;
        let place = crate::row_thumb(&self.style, self.list.row_rect(0));
        let wanted: Vec<(String, u16, u16)> = self
            .tiles
            .iter()
            .skip(first)
            .take(rows)
            .filter(|t| self.source != Source::FolderIndex || matches!(t, Tile::Book(_)))
            .map(|t| {
                let inner = art::cover_place(&self.style, t, place);
                (t.item().path.clone(), inner.width, inner.height)
            })
            .collect();
        self.covers.show(env, &wanted);
    }

    /// The first entry of the letter before or after the focused one's.
    fn letter_jump(&self, forward: bool) -> Option<usize> {
        let letter = |i: usize| initial(self.tiles[i].name());
        let focus = self.list.focus;
        if focus >= self.tiles.len() {
            return None;
        }
        let here = letter(focus);
        if forward {
            (focus + 1..self.tiles.len()).find(|&i| letter(i) != here)
        } else {
            // The start of this letter, then of the one before.
            let start = (0..focus)
                .rev()
                .find(|&i| letter(i) != here)
                .map(|i| i + 1)?;
            let before = letter(start - 1);
            Some(
                (0..start - 1)
                    .rev()
                    .find(|&i| letter(i) != before)
                    .map_or(0, |i| i + 1),
            )
        }
    }

    fn moved(&mut self, change: ui::ListChange, cx: &mut Cx<'_, '_>) {
        if change == ui::ListChange::Page {
            let env = *cx.env();
            self.show_covers(&env);
        }
        self.list.request(change, cx);
    }

    fn open(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(tile) = self.tiles.get(index) else {
            return;
        };
        if matches!(tile, Tile::Book(_)) {
            self.covers.clear();
        }
        cx.command(super::open_tile(tile, &self.source));
    }

    fn offer(&self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        match self.tiles.get(index) {
            Some(Tile::Book(item)) => {
                cx.command(super::open_sheet(&item.path));
                true
            }
            _ => false,
        }
    }

    /// The note below a book's title: its number in a series, its
    /// author, how far it is read.
    fn note(&self, env: &Env<'_>, index: usize) -> String {
        let tile = &self.tiles[index];
        let item = tile.item();
        if let Tile::Group(group) = tile {
            return env.i18n.plural("files.books", group.count as i64, &[]);
        }
        let mut parts: Vec<String> = Vec::new();
        if matches!(self.source, Source::Series(_)) && !item.number.is_empty() {
            let number = paperpaw_sdk::Arg::Str(&item.number);
            parts.push(
                env.i18n
                    .format("files.series-number", &[("number", number)]),
            );
        }
        if !item.author.is_empty() && !matches!(self.source, Source::Author(_)) {
            parts.push(item.author.clone());
        }
        let state = match item.state() {
            _ if self.next == Some(index) => env.i18n.tr("files.shelves.up-next").to_string(),
            State::Reading(p) => format!("{p} %"),
            State::Finished => env.i18n.tr("files.finished").to_string(),
            State::New => String::new(),
        };
        if !state.is_empty() {
            parts.push(state);
        }
        parts.join(" · ")
    }
}

impl Screen for ListScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.tiles.len();
        self.header.draw(
            canvas,
            env,
            &style,
            &self.title,
            None,
            self.list.paging(len),
        );
        if let Some(walk) = &self.walk {
            let found = walk.found() as i64;
            let message = env.i18n.plural("files.collecting", found, &[]);
            ui::draw_message(canvas, env, &style, self.list.area, &message);
            return;
        }
        if len == 0 {
            let (icon, text) = match self.source {
                Source::Search(_) => ("search", "files.search.none"),
                Source::Favourites => ("star", "files.no-favourites"),
                Source::SeriesIndex => ("stack-2", "files.no-series"),
                _ => ("books", "files.no-books"),
            };
            let text = ui::tr_input(env, text);
            ui::draw_empty(canvas, env, &style, self.list.area, icon, "", text);
            return;
        }
        let folders = self.source == Source::FolderIndex;
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                let tile = &self.tiles[index];
                let place = crate::row_thumb(&style, rect);
                let ink = style.focus_ink(focused);
                if folders {
                    crate::draw_folder(canvas, &style, place, ink);
                } else {
                    let inner = art::cover_place(&style, tile, place);
                    let thumb = self
                        .covers
                        .get(&tile.item().path, inner.width, inner.height);
                    art::draw_tile(
                        canvas,
                        env,
                        &style,
                        place,
                        tile,
                        thumb,
                        focused,
                        Marks::Quiet,
                    );
                }
                // Title and note from a margin right of the cover to the edge's,
                // a group's chevron at the end.
                let Some(font) = style.font(env) else {
                    return;
                };
                let gap = style.gap as u16;
                let x = place.x + place.width + style.margin as u16;
                let mut end = rect.x + rect.width - style.margin as u16;
                if matches!(tile, Tile::Group(_)) {
                    let width = font.measure(style.text, "›") as u16;
                    end -= width;
                    let chevron = ui::title_area(&style, rect);
                    ui::draw_label(canvas, &font, style.text, f32::from(end), chevron, "›", ink);
                    end -= gap;
                }
                let width = end.saturating_sub(x);
                let title = ui::title_area(&style, Rect::new(x, rect.y, width, rect.height));
                ui::draw_label(
                    canvas,
                    &font,
                    style.text,
                    f32::from(x),
                    title,
                    tile.name(),
                    ink,
                );
                let small = font.line_metrics(style.small).line_height as u16;
                let note = Rect::new(x, title.y + title.height, width, small);
                let gray = style.focus_faint(focused, 80);
                let text = self.note(env, index);
                ui::draw_label(canvas, &font, style.small, f32::from(x), note, &text, gray);
                // The next book of a series: marked by a bar at the margin.
                if self.next == Some(index) {
                    let bar = Rect::new(
                        rect.x + style.margin as u16 / 4,
                        place.y,
                        (style.line as u16 * 3).max(3),
                        place.height,
                    );
                    canvas.fill(bar, ink);
                }
            });
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let _ = env;
        if self.source.alphabetical()
            && !self.tiles.is_empty()
            && (*action == actions::LEFT || *action == actions::RIGHT)
        {
            let forward = *action == actions::RIGHT;
            return match self.letter_jump(forward) {
                Some(i) => {
                    let letter = initial(self.tiles[i].name());
                    Hint::Text(if forward {
                        format!("{letter} ›")
                    } else {
                        format!("‹ {letter}")
                    })
                }
                None => Hint::Hidden,
            };
        }
        Hint::Default
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if !core::mem::take(&mut self.fresh) {
            // Back from a book: how far it is read changed.
            self.gather(&env);
            cx.request_redraw();
        }
        if self.walk.is_none() && !summary::exists(env.storage) {
            // Opened before the library ever showed: its summary first.
            self.walk = Some(summary::Walk::start(env.storage, &super::root(&env)));
            cx.wake_at(env.now);
        }
    }

    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(walk) = &mut self.walk else {
            return;
        };
        if walk.step(&env) {
            self.walk = None;
            self.gather(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        } else {
            cx.wake_at(env.now.add_millis(super::STEP_DELAY_MS));
        }
    }

    fn on_hide(&mut self) {
        self.covers.clear();
        self.walk = None;
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let len = self.tiles.len();
        if self.source.alphabetical() && (*a == actions::LEFT || *a == actions::RIGHT) {
            if let Some(to) = self.letter_jump(*a == actions::RIGHT) {
                let page = self.list.page();
                self.list.shown = true;
                self.list.focus = to;
                let change = if self.list.page() == page {
                    ui::ListChange::Focus
                } else {
                    ui::ListChange::Page
                };
                self.moved(change, cx);
            }
            return Handled::Yes;
        }
        if let Some(change) = self.list.navigate(a, len) {
            self.moved(change, cx);
        } else if *a == actions::CONFIRM {
            self.open(self.list.focus, cx);
        } else if *a == actions::MENU {
            if !self.offer(self.list.focus, cx) {
                cx.command(super::open_menu());
            }
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.tiles.len();
        match gesture.kind {
            GestureKind::Tap => {
                let paging = self.list.paging(len);
                let style = self.style;
                if let Some(action) = self.header.tap(cx.env(), &style, gesture.start, paging) {
                    cx.command(Command::RunAction(action));
                    return Handled::Yes;
                }
                match self.list.hit(gesture.start, len) {
                    Some(index) => {
                        self.list.focus = index;
                        self.open(index, cx);
                        Handled::Yes
                    }
                    None => Handled::No,
                }
            }
            GestureKind::LongPress => match self.list.hit(gesture.start, len) {
                Some(index) if self.offer(index, cx) => {
                    self.list.focus = index;
                    Handled::Yes
                }
                _ => Handled::No,
            },
            _ => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == CHANGED {
            let env = *cx.env();
            self.gather(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }
}
