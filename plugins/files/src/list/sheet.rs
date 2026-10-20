//! What can be done with a book: a sheet from the bottom with its cover,
//! title, author and how far it is read, then the actions (go on reading
//! or open, its series, finished or unread, favourite, information).
//!
//! The book's details and small cover are read when the sheet opens.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::library;
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen,
};

use super::index::{Line, hash};
use super::rows::{self, Loaded, Sizes};
use super::shelf::{self, Kind};

/// An action of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Open,
    Series,
    Finished,
    Unfinished,
    Favourite,
    Unfavourite,
    Info,
    Archive,
    Menu,
}

pub(crate) struct BookSheet {
    style: UiStyle,
    sizes: Sizes,
    region: Rect,
    /// The part with the cover, title, author and progress.
    head: Rect,
    list: ListView,
    book: Loaded,
    actions: Vec<(Action, String)>,
}

impl BookSheet {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let path = params.get(params::PATH).unwrap_or_default().to_string();
        let storage = env.storage;
        let details = library::book(storage, &path).unwrap_or_default();
        let opened = library::opened_books(storage)
            .iter()
            .any(|p| hash(p) == hash(&path));
        let line = Line {
            title: details.shown_title(&path).into_owned(),
            author: details.author.clone().unwrap_or_default(),
            series: details.series.clone().unwrap_or_default(),
            number: details.series_index.clone().unwrap_or_default(),
            tags: String::new(),
            path: path.clone(),
        };
        let mut book = Loaded::load(env, line, opened, sizes.cover, false);
        let note = book.author.clone();
        let width = env.device.display.width;
        rows::prepare_row(env, &style, &sizes, width, &mut book, note, false);
        let favourite = library::favourites(storage).contains(&path);
        let tr = |key: &str| env.i18n.tr(key).to_string();
        let mut actions = Vec::new();
        let open = match book.progress {
            1..=99 => "files.list.continue",
            _ => "files.option.open",
        };
        actions.push((Action::Open, tr(open)));
        if !book.series.is_empty() {
            let label = env.i18n.format(
                "files.list.option.series",
                &[("series", Arg::Str(&book.series))],
            );
            actions.push((Action::Series, label));
        }
        if book.progress == 100 {
            actions.push((Action::Unfinished, tr("files.option.unfinished")));
        } else {
            actions.push((Action::Finished, tr("files.option.finished")));
        }
        if favourite {
            actions.push((Action::Unfavourite, tr("files.option.unfavourite")));
        } else {
            actions.push((Action::Favourite, tr("files.option.favourite")));
        }
        actions.push((Action::Info, tr("files.option.info")));
        if crate::can_archive(env, &path) {
            actions.push((Action::Archive, tr("files.option.archive")));
        }
        if !env.device.input.touch {
            actions.push((Action::Menu, tr("files.option.menu")));
        }

        let display = env.device.display;
        let gap = style.gap as u16;
        let head_h = sizes.cover.1 + 2 * gap;
        let row = style.touch_row(style.row) as u16;
        let rows_h = row * actions.len() as u16;
        let height = (head_h + rows_h + gap).min(display.height);
        let region = Rect::new(0, display.height - height, display.width, height);
        let head = Rect::new(0, region.y + gap / 2, display.width, head_h);
        let list = ListView::new(
            Rect::new(0, head.y + head.height, display.width, rows_h),
            u32::from(row),
        )
        .focus_on_keys(env);
        Self {
            style,
            sizes,
            region,
            head,
            list,
            book,
            actions,
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((action, _)) = self.actions.get(index) else {
            return;
        };
        let env = *cx.env();
        let path = self.book.path.as_str();
        cx.command(Command::Back);
        match action {
            Action::Open => super::read(path, cx),
            Action::Series => cx.command(shelf::open(Kind::Series, &self.book.series)),
            Action::Finished | Action::Unfinished => {
                library::set_finished(env.storage, path, *action == Action::Finished);
                cx.command(super::touched(path));
            }
            Action::Favourite | Action::Unfavourite => {
                let favourite = *action == Action::Favourite;
                library::set_favourite(env.storage, path, favourite);
                cx.command(super::touched(path));
                let key = if favourite {
                    "files.favourite-added"
                } else {
                    "files.favourite-removed"
                };
                cx.command(Command::Notify(env.i18n.tr(key).to_string()));
            }
            Action::Info => cx.command(Command::OpenScreen(
                crate::INFO_SCREEN,
                Params::new().with(params::PATH, path.to_string()),
            )),
            Action::Archive => crate::archive(&env, path, cx),
            Action::Menu => super::open_menu(cx),
        }
    }
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
        let r = self.region;
        canvas.fill(r, Color::WHITE);
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (style.line * 2).max(2),
            Color::BLACK,
        );
        // The book as a row of the list, but larger text: its cover, title,
        // author and state, never focused.
        rows::draw_book_row(
            canvas,
            env,
            &style,
            &self.sizes,
            self.head,
            false,
            &self.book,
            false,
        );
        ui::draw_separator(canvas, &style, self.head, 0);
        let actions = &self.actions;
        self.list.draw(
            canvas,
            &style,
            actions.len(),
            |canvas, index, rect, focused| {
                let label = &actions[index].1;
                ui::draw_value_row(canvas, env, &style, rect, focused, label, "");
            },
        );
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let delta = if [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            -1
        } else if [
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            1
        } else if *a == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if self.list.move_focus(delta, self.actions.len()) != ListChange::None {
            cx.request_redraw_area(self.list.area());
            cx.request_refresh(RefreshMode::Fast);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self.list.hit(gesture.start, self.actions.len()) {
            self.choose(index, cx);
        } else if self.head.contains(gesture.start) {
            // The book itself: open it.
            self.choose(0, cx);
        }
        Handled::Yes
    }
}
