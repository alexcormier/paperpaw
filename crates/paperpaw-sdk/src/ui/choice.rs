//! An overlay at the bottom with a title and a few choices, e.g. what to
//! do with a file or whether to delete it.
//!
//! A plugin registers it as a screen of its own and opens it with
//! [`open`]; the choice comes back as the plugin's reply event, a
//! [`Picked`] with the context it was opened for and the option chosen.
//! Tapping outside, or *Back*, chooses nothing.
//!
//! ```ignore
//! r.add_screen(ScreenSpec {
//!     id: CHOICE,
//!     title: Cow::Borrowed("my.title"),
//!     factory: Box::new(|env, params| Box::new(Choice::new(env, params))),
//! })?;
//! cx.command(choice::open(CHOICE, CHOSEN, "Delete “a.epub”?", "books/a.epub", "my.option.", &["delete", "cancel"]));
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{actions, params};
use crate::ui::{self, ListChange, ListView, UiStyle};
use crate::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

/// The option chosen.
pub const OPTION: &str = "option";

crate::event_data! {
    /// What was chosen: the option, for the context the choice was
    /// opened for.
    pub struct Picked {
        /// The context ([`open`]'s `context`).
        pub path: alloc::string::String,
        pub option: alloc::string::String,
    }
}
const OPTIONS: &str = "options";
/// Translation keys of the options: this, then the option.
const LABELS: &str = "labels";
/// Lines of the title, at most.
const TITLE_LINES: usize = 3;

/// Opens the choice overlay `screen` with `title` and `options`, each
/// shown as the translation of `labels` followed by the option (e.g.
/// `file-manager.option.` and `delete`). The choice comes back as the
/// event `reply` ([`Picked`]).
pub fn open(
    screen: Id,
    reply: &crate::event::Event<Picked>,
    title: &str,
    context: &str,
    labels: &str,
    options: &[&str],
) -> Command {
    Command::OpenScreen(
        screen,
        Params::new()
            .with(params::TITLE, title.to_string())
            .with(params::PATH, context.to_string())
            .with(params::REPLY, reply.as_str())
            .with(LABELS, labels.to_string())
            .with(OPTIONS, options.join(",")),
    )
}

/// The choice overlay; see the [module documentation](self).
#[derive(Debug)]
pub struct Choice {
    style: UiStyle,
    region: Rect,
    title: Rect,
    list: ListView,
    /// Below the options when they take more than one page.
    pager: Option<ui::Pager>,
    title_text: String,
    path: String,
    reply: Option<Id>,
    labels: String,
    options: Vec<String>,
}

impl Choice {
    pub fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let options: Vec<String> = params
            .get(OPTIONS)
            .unwrap_or_default()
            .split(',')
            .filter(|o| !o.is_empty())
            .map(ToString::to_string)
            .collect();
        let title_text = params.get(params::TITLE).unwrap_or_default().to_string();
        let row = style.row as u16;
        // A long title takes up to three lines.
        let lines = style.font(env).map_or((1, 0.0), |font| {
            let width = f32::from(display.width) - 2.0 * style.margin as f32;
            let line = font.line_metrics(style.title).line_height;
            let count = font
                .wrap(style.title, &title_text, width)
                .len()
                .clamp(1, TITLE_LINES);
            (count, line)
        });
        let title_height = match lines {
            (1, _) => row,
            (count, line) => (count as f32 * line) as u16 + style.gap as u16,
        };
        // As many options as the screen holds below the title; more go on
        // pages, with a pager below them.
        let row = u32::from(row.max(1));
        let room =
            u32::from(display.height).saturating_sub(u32::from(title_height) + style.gap * 2);
        let count = options.len() as u32;
        let (shown, pager_height) = if count * row <= room {
            (count, 0)
        } else {
            let pager = u32::from(ui::Pager::height(&style));
            ((room.saturating_sub(pager) / row).max(1), pager)
        };
        let list_height = shown * row;
        let height = (u32::from(title_height) + list_height + pager_height + style.gap * 2)
            .min(u32::from(display.height)) as u16;
        let region = Rect::new(0, display.height - height, display.width, height);
        let title = Rect::new(0, region.y + style.gap as u16, display.width, title_height);
        let list_area = Rect::new(0, title.y + title_height, display.width, list_height as u16);
        let list = ListView::new(list_area, row).focus_on_keys(env);
        let pager = (pager_height > 0).then(|| {
            let area = Rect::new(
                0,
                list_area.y + list_area.height,
                display.width,
                pager_height as u16,
            );
            ui::Pager::new(env, area)
        });
        Self {
            style,
            region,
            title,
            list,
            pager,
            title_text,
            path: params.get(params::PATH).unwrap_or_default().to_string(),
            reply: params
                .get(params::REPLY)
                .and_then(|reply| Id::parse(reply).ok()),
            labels: params.get(LABELS).unwrap_or_default().to_string(),
            options,
        }
    }

    /// Draws the options again after the focus or the page changed.
    fn moved(&self, change: ListChange, cx: &mut Cx<'_, '_>) {
        match change {
            ListChange::None => {}
            ListChange::Focus => self.list.request(change, cx),
            ListChange::Page => {
                cx.request_redraw_area(self.region);
                cx.request_refresh(RefreshMode::Fast);
            }
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        cx.command(Command::Back);
        if let (Some(option), Some(reply)) = (self.options.get(index), &self.reply) {
            cx.command(Command::Emit(
                reply.clone(),
                Params::new()
                    .with(params::PATH, self.path.clone())
                    .with(OPTION, option.clone()),
            ));
        }
    }
}

impl Screen for Choice {
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
        if let Some(font) = style.font(env) {
            let margin = style.margin as f32;
            let width = f32::from(self.title.width) - 2.0 * margin;
            let mut lines = font.wrap(style.title, &self.title_text, width);
            if lines.len() <= 1 {
                ui::draw_label(
                    canvas,
                    &font,
                    style.title,
                    margin,
                    self.title,
                    &self.title_text,
                    Color::BLACK,
                );
            } else {
                let more = lines.len() > TITLE_LINES;
                lines.truncate(TITLE_LINES);
                let line = font.line_metrics(style.title).line_height;
                let count = lines.len();
                for (n, range) in lines.into_iter().enumerate() {
                    let text = if more && n + 1 == count {
                        // The rest, shortened.
                        alloc::borrow::Cow::Owned(ui::fit(
                            &font,
                            style.title,
                            &self.title_text[range.start..],
                            width,
                        ))
                    } else {
                        alloc::borrow::Cow::Borrowed(&self.title_text[range])
                    };
                    let y = self.title.y + (n as f32 * line) as u16;
                    let area = Rect::new(0, y, self.title.width, line as u16);
                    ui::draw_label(
                        canvas,
                        &font,
                        style.title,
                        margin,
                        area,
                        &text,
                        Color::BLACK,
                    );
                }
            }
        }
        let (options, labels) = (&self.options, &self.labels);
        self.list.draw(
            canvas,
            &style,
            options.len(),
            |canvas, index, rect, focused| {
                let key = alloc::format!("{labels}{}", options[index]);
                ui::draw_value_row(canvas, env, &style, rect, focused, env.i18n.tr(&key), "");
            },
        );
        if let Some(pager) = &self.pager {
            let page = (self.list.page(), self.list.pages(options.len()));
            pager.draw(canvas, env, &style, page);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            // Nothing to choose: closed at once.
            if self.options.is_empty() {
                self.choose(0, cx);
                return Handled::Yes;
            }
            match self.list.confirm() {
                Some(index) => self.choose(index, cx),
                None => self.moved(ListChange::Focus, cx),
            }
            return Handled::Yes;
        }
        // While the focus shows (buttons are used, always on the X4, whose
        // buttons beside the screen are the page buttons) the page buttons
        // step a row as the others do; a swipe on a touch screen, the
        // focus hidden, turns a page.
        let page = if self.list.shown {
            1
        } else {
            self.list.rows() as isize
        };
        match ui::list_step(action, page) {
            Some(delta) => {
                let change = self.list.move_focus(delta, self.options.len());
                self.moved(change, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self.list.hit(gesture.start, self.options.len()) {
            self.choose(index, cx);
        } else if let Some(action) = self.pager.and_then(|p| p.tap(gesture.start)) {
            // A page on, or back: where its first option is.
            let rows = self.list.rows();
            let page = self.list.page();
            let to = if action == actions::NEXT_PAGE {
                (page + 1) * rows
            } else {
                page.saturating_sub(1) * rows
            };
            if to < self.options.len() && to / rows != page {
                self.list.focus = to;
                self.moved(ListChange::Page, cx);
            }
        }
        Handled::Yes
    }
}
