//! A to-do list: a few things to do, kept in `todo.txt` in the plugin's
//! folder (as todo.txt has them: an item a line, `x 2026-10-19 ` before
//! those done). The widget shows the open ones with a box each; a tap on
//! one ticks it off: it shows struck through for the rest of the day (or
//! goes at once), then it goes. The list's own screen adds, ticks, edits
//! and removes them.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use quireink_sdk::ask;
use quireink_sdk::builtin::actions;
use quireink_sdk::event::Event;
use quireink_sdk::replies::Typed;
use quireink_sdk::setting::{Setting, Text};
use quireink_sdk::time::LocalTime;
use quireink_sdk::ui::choice::{self, Choice};
use quireink_sdk::ui::{self, ListView, UiStyle};
use quireink_sdk::widgets::{self, Align, Frame, Look, Size, Widget, WidgetSpec, WidgetTarget};
use quireink_sdk::{
    ActionSpec, Arg, Canvas, Color, Command, Cx, Env, FontHandle, Gesture, GestureKind, Handled,
    Id, Params, Point, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec,
    SettingSpec, Storage,
};

use super::{changed, changes, data, frame_height, today};

pub(super) const ID: Id = Id::new_static("widgets.todo");
/// Opens the list's own screen.
pub const OPEN: Id = Id::new_static("widgets.todo.open");
/// Items shown at most.
const COUNT: Setting<i64> = Setting::new("widgets.todo.count");
/// What becomes of those ticked off: one of [`DONE`].
const DONE_SETTING: Setting<Text> = Setting::new("widgets.todo.done");
/// Struck through for the rest of the day, or gone at once.
const DONE: [&str; 2] = ["widgets.todo.done.struck", "widgets.todo.done.hidden"];
/// The boxes before the items: one of [`BOXES`].
const BOX: Setting<Text> = Setting::new("widgets.todo.box");
const BOXES: [&str; 3] = [
    "widgets.todo.box.circle",
    "widgets.todo.box.square",
    "widgets.todo.box.none",
];
/// "To do" above the items.
const CAPTION: &str = "caption";
const SCREEN: Id = Id::new_static("widgets.todo.list");
const CHOICE: Id = Id::new_static("widgets.todo.choice");
const CHOSEN: Event<choice::Picked> = Event::new("widgets.todo.chosen");
/// An item tapped in the widget: its text ([`TEXT`]) and whether it was
/// done ([`WAS_DONE`]).
const TICKED: Event<Params> = Event::new("widgets.todo.ticked");
const TEXT: &str = "text";
const WAS_DONE: &str = "done";
/// A new item typed, an item's text changed (its index in [`INDEX`]).
const ADDED: Event<Typed> = Event::new("widgets.todo.added");
const EDITED: Event<Typed> = Event::new("widgets.todo.edited");
const INDEX: &str = "index";
const OPTION_LABELS: &str = "widgets.todo.option.";
/// Items kept at most, and their length.
const MAX_ITEMS: usize = 100;
const MAX_TEXT: usize = 200;

fn file() -> String {
    data("todo.txt")
}

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group(widgets::settings_group(&ID, "widgets.todo"))?;
    r.add_setting(SettingSpec::number(&COUNT, "widgets.todo.setting.count", 1, 12, 5).group(ID))?;
    r.add_setting(
        SettingSpec::choice(
            &DONE_SETTING,
            "widgets.todo.setting.done",
            DONE.iter().map(|s| (*s).into()).collect(),
            DONE[0],
        )
        .group(ID),
    )?;
    r.add_setting(
        SettingSpec::choice(
            &BOX,
            "widgets.todo.setting.box",
            BOXES.iter().map(|s| (*s).into()).collect(),
            BOXES[0],
        )
        .group(ID),
    )?;
    r.add_setting(widgets::part_setting(
        &ID,
        CAPTION,
        "widgets.todo.setting.caption",
        true,
    ))?;
    r.add_setting(widgets::size_setting(&ID, Size::Small, false))?;
    r.add_setting(widgets::align_setting(&ID, Align::Left))?;
    r.add_setting(widgets::frame_setting(&ID, Frame::None))?;
    r.on(TICKED, |env, params, out| {
        let Some(text) = params.get(TEXT) else {
            return;
        };
        let today = today(env);
        let mut items = load(env.storage, today);
        let was_done = params.get(WAS_DONE).is_some();
        if let Some(item) = items
            .iter_mut()
            .find(|i| i.text == text && i.done.is_some() == was_done)
        {
            item.done = if was_done { None } else { Some(today) };
            save(env.storage, &items);
        }
        changed(out, &ID);
    })?;
    r.add_screen(ScreenSpec {
        id: SCREEN,
        title: Cow::Borrowed("widgets.todo"),
        factory: Box::new(|env, _| Box::new(TodoScreen::new(env))),
    })?;
    r.add_screen(ScreenSpec {
        id: CHOICE,
        title: Cow::Borrowed("widgets.todo"),
        factory: Box::new(|env, params| Box::new(Choice::new(env, params))),
    })?;
    r.add_action(ActionSpec::new(OPEN, "widgets.todo").handler(
        |_: &Env<'_>, out: &mut Vec<Command>| {
            out.push(Command::OpenScreen(SCREEN, Params::new()));
        },
    ))?;
    r.add_icon(OPEN, Cow::Borrowed("list"))?;
    r.add_widget(
        WidgetSpec::new(ID, "widgets.todo", |_| Box::new(TodoWidget::default()))
            .category(widgets::categories::PERSONAL),
    )
}

// ---- The list -----------------------------------------------------------

/// A thing to do, and the day it was done (days since 1970).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    text: String,
    done: Option<i64>,
}

/// Days since 1970 of a date (the proleptic Gregorian calendar).
fn days_of(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * i64::from((month + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_era_days - 719_468
}

/// `2026-10-19` of a day since 1970.
fn date_of(day: i64) -> String {
    let date = LocalTime::from_seconds(day * 86_400);
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

/// The day since 1970 of `2026-10-19`.
fn day_of(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then(|| days_of(year, month, day))
}

/// The items of a list as kept, but those done before `today`.
fn parse(text: &str, today: i64) -> Vec<Item> {
    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let item = match line.strip_prefix("x ") {
            Some(rest) => {
                let rest = rest.trim_start();
                let (date, text) = rest.split_once(' ').unwrap_or((rest, ""));
                match day_of(date) {
                    Some(day) => Item {
                        text: String::from(text.trim()),
                        done: Some(day),
                    },
                    // Done, but when is not said: long ago.
                    None => Item {
                        text: String::from(rest),
                        done: Some(i64::MIN),
                    },
                }
            }
            None => Item {
                text: String::from(line),
                done: None,
            },
        };
        let gone = item.done.is_some_and(|day| day < today);
        if !gone && !item.text.is_empty() && items.len() < MAX_ITEMS {
            items.push(item);
        }
    }
    items
}

/// The list as kept: an item a line, those done after `x` and the day.
fn format(items: &[Item]) -> String {
    let mut text = String::new();
    for item in items {
        let line = item.text.replace(['\n', '\r'], " ");
        match item.done {
            Some(day) => text.push_str(&format!("x {} {line}\n", date_of(day))),
            None => text.push_str(&format!("{line}\n")),
        }
    }
    text
}

fn load(storage: &dyn Storage, today: i64) -> Vec<Item> {
    quireink_sdk::read_file(storage, &file(), 32 * 1024)
        .map(|data| parse(&String::from_utf8_lossy(&data), today))
        .unwrap_or_default()
}

fn save(storage: &dyn Storage, items: &[Item]) {
    let _ = storage.write(&file(), format(items).as_bytes());
}

/// `text` as an item's: one line, not too long.
fn cleaned(text: &str) -> String {
    let text = text.replace(['\n', '\r', '\t'], " ");
    text.trim().chars().take(MAX_TEXT).collect()
}

// ---- The widget ---------------------------------------------------------

/// The list's first items, read when updated.
#[derive(Default)]
struct TodoWidget {
    /// The items shown.
    items: Vec<Item>,
    /// Open ones in all.
    open: usize,
    /// Items not shown for want of room or count.
    more: usize,
    /// The day and the changes it was read at.
    read: Option<(i64, u32)>,
}

impl Widget for TodoWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read each minute (a new day lets those done go), but read
        // anew only then or after a change.
        let day = today(env);
        let read = Some((day, changes()));
        if read == self.read {
            return false;
        }
        self.read = read;
        let hide = env.settings.read(&DONE_SETTING) == DONE[1];
        let all: Vec<Item> = load(env.storage, day)
            .into_iter()
            .filter(|i| !(hide && i.done.is_some()))
            .collect();
        let count = env.settings.read(&COUNT).clamp(1, 12) as usize;
        let open = all.iter().filter(|i| i.done.is_none()).count();
        let more = all.len().saturating_sub(count);
        let items: Vec<Item> = all.into_iter().take(count).collect();
        let changed = items != self.items || open != self.open || more != self.more;
        (self.items, self.open, self.more) = (items, open, more);
        changed
    }

    fn height(&self, env: &Env<'_>, _: u16) -> u16 {
        let rows = self.items.len().max(1) as u16 + u16::from(self.more > 0);
        let caption = if widgets::shows(env, ID.as_str(), CAPTION) {
            widgets::caption_height(env)
        } else {
            0
        };
        caption + row_height(env) * rows + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let align = widgets::align(env, ID.as_str());
        if self.items.is_empty() {
            // The whole place to say how to add some, small.
            let key = if env.device.input.touch {
                "widgets.todo.none.touch"
            } else {
                "widgets.todo.none"
            };
            let text = env.i18n.tr(key);
            super::draw_empty(canvas, env, &look, text);
            return;
        }
        let places = places(env, look.area, self);
        if places.caption.is_some() {
            let text = caption_text(env, self.open);
            widgets::draw_caption(canvas, env, ID.as_str(), &look, &text);
        }
        let size = text_size(env);
        let boxes = env.settings.read(&BOX);
        for (row, item) in places.rows.iter().zip(&self.items) {
            draw_item(canvas, env, &font, size, *row, item, boxes, align, &look);
        }
        if let Some((row, more)) = places.more {
            let text = env
                .i18n
                .format("widgets.todo.more", &[("count", Arg::Int(more as i64))]);
            let text = ui::fit(&font, style.small, &text, f32::from(row.width));
            let x = align.x(row, font.measure(style.small, &text));
            ui::draw_label(canvas, &font, style.small, x, row, &text, look.faint);
        }
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::look::look(env, ID.as_str(), area);
        let places = places(env, look.area, self);
        let open = Command::OpenScreen(SCREEN, Params::new());
        if self.items.is_empty() {
            return alloc::vec![WidgetTarget {
                area,
                command: open,
            }];
        }
        let mut targets = Vec::new();
        if let Some(caption) = places.caption {
            targets.push(WidgetTarget {
                area: caption,
                command: open.clone(),
            });
        }
        for (row, item) in places.rows.iter().zip(&self.items) {
            let mut params = Params::new().with(TEXT, item.text.clone());
            if item.done.is_some() {
                params.set(WAS_DONE, "1");
            }
            targets.push(WidgetTarget {
                area: *row,
                command: TICKED.emit(params),
            });
        }
        if let Some((row, _)) = places.more {
            targets.push(WidgetTarget {
                area: row,
                command: open,
            });
        }
        targets
    }
}

/// "To do", or "To do · 3" with open items.
fn caption_text(env: &Env<'_>, open: usize) -> String {
    let title = env.i18n.tr("widgets.todo");
    if open == 0 {
        String::from(title)
    } else {
        format!("{title} · {open}")
    }
}

/// The height of a row of the widget: its line with room around, at
/// least as high as a finger needs on touch screens.
fn row_height(env: &Env<'_>) -> u16 {
    let line = widgets::line_height(env, text_size(env));
    let least = if env.device.input.touch {
        env.metrics.mm(6) as u16
    } else {
        0
    };
    (line + line / 3).max(least)
}

/// The size of the items' text: a list is denser than a widget's one
/// line, so a step smaller than [`Size::text`] (small is the size of
/// notes, medium the body's).
fn text_size(env: &Env<'_>) -> f32 {
    let style = UiStyle::new(env);
    match widgets::size(env, ID.as_str()) {
        Size::Small => style.small,
        Size::Medium | Size::Fit => style.text,
        Size::Large => style.title,
    }
}

/// Where the widget's parts go in `area` (inside its frame).
struct Places {
    caption: Option<Rect>,
    rows: Vec<Rect>,
    /// "3 more", where not all are shown, and how many.
    more: Option<(Rect, usize)>,
}

fn places(env: &Env<'_>, area: Rect, widget: &TodoWidget) -> Places {
    let mut rest = area;
    let mut caption = None;
    let row = row_height(env).max(1);
    // The caption where an item fits below it.
    let height = widgets::caption_height(env);
    if widgets::shows(env, ID.as_str(), CAPTION) && rest.height >= height + row {
        caption = Some(Rect::new(rest.x, rest.y, rest.width, height));
        rest = Rect::new(rest.x, rest.y + height, rest.width, rest.height - height);
    }
    let fits = usize::from(rest.height / row).max(1);
    let row = row.min(rest.height.max(1));
    let mut shown = widget.items.len().min(fits);
    let mut more = widget.more;
    // Not all fit: the last row says how many more there are (where
    // two items show at least).
    if shown < widget.items.len() && shown > 2 {
        more += widget.items.len() - shown + 1;
        shown -= 1;
    }
    let rows: Vec<Rect> = (0..shown)
        .map(|n| Rect::new(rest.x, rest.y + row * n as u16, rest.width, row))
        .collect();
    let more = (more > 0 && rows.len() < fits).then(|| {
        let place = Rect::new(rest.x, rest.y + row * rows.len() as u16, rest.width, row);
        (place, more)
    });
    Places {
        caption,
        rows,
        more,
    }
}

/// Draws an item in `row`: its box, ticked if done, and its text, struck
/// through if done.
#[allow(clippy::too_many_arguments)]
fn draw_item(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    size: f32,
    row: Rect,
    item: &Item,
    boxes: &str,
    align: Align,
    look: &Look,
) {
    let style = UiStyle::new(env);
    let done = item.done.is_some();
    let side = (size * 0.8) as u16;
    let with_box = boxes != BOXES[2];
    let lead = if with_box {
        side + (size * 0.55) as u16
    } else {
        0
    };
    let room = f32::from(row.width.saturating_sub(lead));
    let text = ui::fit(font, size, &item.text, room);
    let width = font.measure(size, &text);
    let x = align.x(row, f32::from(lead) + width) as u16;
    let color = if done { look.faint } else { look.ink };
    if with_box {
        let mid = row.y + row.height / 2;
        let place = Rect::new(x, mid - side / 2, side, side);
        draw_box(canvas, env, place, boxes == BOXES[1], done, look);
    }
    let label = Rect::new(x + lead, row.y, row.width.saturating_sub(lead), row.height);
    ui::draw_label(canvas, font, size, f32::from(label.x), label, &text, color);
    if done {
        // A line through the middle of the small letters.
        let metrics = font.line_metrics(size);
        let baseline =
            f32::from(row.y) + (f32::from(row.height) + metrics.ascent - metrics.descent) / 2.0;
        let y = (baseline - size * 0.27) as u16;
        let line = style.line.max(1) as u16;
        canvas.fill(Rect::new(label.x, y, width as u16, line), color);
    }
}

/// A box in `place`: round or square, filled and ticked when done.
fn draw_box(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    place: Rect,
    square: bool,
    done: bool,
    look: &Look,
) {
    let style = UiStyle::new(env);
    let line = (u32::from(place.width) / 10).max(style.line).max(1);
    let r = u32::from(place.width / 2);
    let center = Point::new(i32::from(place.x) + r as i32, i32::from(place.y) + r as i32);
    match (square, done) {
        (false, false) => canvas.stroke_circle(center, r, line, look.ink),
        (false, true) => canvas.fill_circle(center, r, look.faint),
        (true, false) => canvas.stroke_round(place, line, r / 3, look.ink),
        (true, true) => canvas.fill_round(place, r / 3, look.faint),
    }
    if done {
        // A tick in the paper's colour.
        let s = place.width as i32;
        let (x, y) = (i32::from(place.x), i32::from(place.y));
        let width = (s as u32 / 9).max(2);
        let a = Point::new(x + s * 28 / 100, y + s * 50 / 100);
        let b = Point::new(x + s * 44 / 100, y + s * 66 / 100);
        let c = Point::new(x + s * 72 / 100, y + s * 36 / 100);
        canvas.draw_line(a, b, width, look.paper);
        canvas.draw_line(b, c, width, look.paper);
    }
}

// ---- The list's screen --------------------------------------------------

/// The list: *Add* first, then the items; a tap ticks one off or on,
/// holding it (or the menu button) offers to edit or remove it.
struct TodoScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    items: Vec<Item>,
}

impl TodoScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area.height + style.gap as u16;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        Self {
            list: ListView::new(area, style.row).focus_on_keys(env),
            style,
            header,
            items: load(env.storage, today(env)),
        }
    }

    fn len(&self) -> usize {
        self.items.len() + 1
    }

    fn changed(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        save(env.storage, &self.items);
        let mut out = Vec::new();
        changed(&mut out, &ID);
        for command in out {
            cx.command(command);
        }
        self.list.focus = self.list.focus.min(self.len() - 1);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Row `row` chosen: *Add* asks for an item, an item is ticked off or
    /// on.
    fn open(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if row == 0 {
            cx.command(ask::text(env.i18n.tr("widgets.todo.add"), &ADDED).command());
            return;
        }
        let today = today(&env);
        if let Some(item) = self.items.get_mut(row - 1) {
            item.done = match item.done {
                Some(_) => None,
                None => Some(today),
            };
            self.changed(cx);
        }
    }

    /// What can be done with item `row`.
    fn offer(&mut self, row: usize, cx: &mut Cx<'_, '_>) {
        let Some(item) = row.checked_sub(1).and_then(|i| self.items.get(i)) else {
            return;
        };
        let tick = if item.done.is_some() {
            "untick"
        } else {
            "tick"
        };
        cx.command(choice::open(
            CHOICE,
            &CHOSEN,
            &item.text,
            &format!("{}", row - 1),
            OPTION_LABELS,
            &[tick, "edit", "up", "remove"],
        ));
    }
}

impl Screen for TodoScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        self.header.draw(
            canvas,
            env,
            &style,
            env.i18n.tr("widgets.todo"),
            None,
            self.list.paging(len),
        );
        let Some(font) = style.font(env) else {
            return;
        };
        let items = &self.items;
        self.list
            .draw(canvas, &style, len, |canvas, row, rect, focused| {
                let (ink, paper) = if focused {
                    (Color::WHITE, Color::BLACK)
                } else {
                    (Color::BLACK, Color::WHITE)
                };
                let margin = style.margin as u16;
                let inner = Rect::new(
                    rect.x + margin,
                    rect.y,
                    rect.width.saturating_sub(2 * margin),
                    rect.height,
                );
                let look = Look {
                    area: inner,
                    ink,
                    faint: if focused {
                        Color::gray(200)
                    } else {
                        style.faint(90)
                    },
                    paper,
                    frame: widgets::Frame::None,
                };
                if row == 0 {
                    let icon = style.text * 1.1;
                    let width =
                        ui::draw_icon(canvas, env, "plus", icon, f32::from(inner.x), inner, ink);
                    let x = f32::from(inner.x)
                        + if width > 0.0 {
                            width + style.gap as f32
                        } else {
                            0.0
                        };
                    let text = env.i18n.tr("widgets.todo.add");
                    ui::draw_label(canvas, &font, style.text, x, inner, text, ink);
                    return;
                }
                if let Some(item) = items.get(row - 1) {
                    let boxes = BOXES[0];
                    draw_item(
                        canvas,
                        env,
                        &font,
                        style.text,
                        inner,
                        item,
                        boxes,
                        Align::Left,
                        &look,
                    );
                }
            });
        if self.items.is_empty() {
            let row = self.list.row_rect(1);
            let area = Rect::new(
                row.x + style.margin as u16,
                row.y,
                row.width.saturating_sub(2 * style.margin as u16),
                style.row as u16 * 2,
            );
            let look = Look {
                area,
                ink: style.faint(90),
                faint: style.faint(90),
                paper: Color::WHITE,
                frame: widgets::Frame::None,
            };
            widgets::draw_message(canvas, env, &look, env.i18n.tr("widgets.todo.empty"));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(action, rows) {
            delta
        } else if *action == actions::CONFIRM {
            self.open(self.list.focus, cx);
            return Handled::Yes;
        } else if *action == actions::MENU {
            self.offer(self.list.focus, cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus(delta, len);
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.len();
        if gesture.kind == GestureKind::Tap
            && let Some(action) =
                self.header
                    .tap(&env, &self.style, gesture.start, self.list.paging(len))
        {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(row) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.focus = row;
        match gesture.kind {
            GestureKind::Tap => self.open(row, cx),
            GestureKind::LongPress => self.offer(row, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(typed) = ADDED.read(event, params) {
            let text = cleaned(&typed.text);
            if text.is_empty() || self.items.len() >= MAX_ITEMS {
                return;
            }
            self.items.push(Item { text, done: None });
            // The new item in sight.
            self.list.focus = self.items.len();
            self.changed(cx);
        } else if let Some(typed) = EDITED.read(event, params) {
            let index = typed
                .context
                .get(INDEX)
                .and_then(|i| i.parse::<usize>().ok());
            let text = cleaned(&typed.text);
            if let Some(item) = index.and_then(|i| self.items.get_mut(i))
                && !text.is_empty()
            {
                item.text = text;
                self.changed(cx);
            }
        } else if let Some(picked) = CHOSEN.read(event, params) {
            let Some(index) = picked
                .path
                .parse::<usize>()
                .ok()
                .filter(|i| *i < self.items.len())
            else {
                return;
            };
            match picked.option.as_str() {
                "tick" | "untick" => self.open(index + 1, cx),
                "edit" => {
                    let question = ask::text(env.i18n.tr("widgets.todo.edit"), &EDITED)
                        .initial(self.items[index].text.clone())
                        .with(INDEX, index.to_string());
                    cx.command(question.command());
                }
                "up" if index > 0 => {
                    self.items.swap(index, index - 1);
                    self.list.focus = index;
                    self.changed(cx);
                }
                "remove" => {
                    self.items.remove(index);
                    self.changed(cx);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quireink_sdk::MemoryStorage;

    #[test]
    fn days_and_dates_agree() {
        assert_eq!(days_of(1970, 1, 1), 0);
        assert_eq!(days_of(2000, 3, 1), 11_017);
        let day = days_of(2026, 10, 19);
        assert_eq!(date_of(day), "2026-10-19");
        assert_eq!(day_of("2026-10-19"), Some(day));
        assert_eq!(
            day_of("2024-02-29").map(date_of).as_deref(),
            Some("2024-02-29")
        );
        assert_eq!(day_of("2026-13-01"), None);
        assert_eq!(day_of("yesterday"), None);
    }

    #[test]
    fn the_list_is_kept() {
        let storage = MemoryStorage::new();
        let today = days_of(2026, 10, 19);
        let items = [
            Item {
                text: String::from("Return the library book"),
                done: None,
            },
            Item {
                text: String::from("Call Anna\nback"),
                done: Some(today),
            },
        ];
        save(&storage, &items);
        let text = String::from_utf8(quireink_sdk::read_file(&storage, &file(), 1024).unwrap());
        assert_eq!(
            text.unwrap(),
            "Return the library book\nx 2026-10-19 Call Anna back\n"
        );
        let loaded = load(&storage, today);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], items[0]);
        assert_eq!(loaded[1].text, "Call Anna back");
        assert_eq!(loaded[1].done, Some(today));
        // The next day, what was done is gone.
        assert_eq!(load(&storage, today + 1), alloc::vec![items[0].clone()]);
    }

    /// What version 1 wrote stays readable (see `docs/data-files.md`).
    #[test]
    fn the_list_of_version_1_is_read() {
        let today = days_of(2026, 10, 19);
        let items = parse(include_str!("../../testdata/todo.txt.v1"), today);
        let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, ["Buy bread", "Water the plants", "Read chapter 3"]);
        assert_eq!(items[1].done, Some(today));
        assert_eq!(items[0].done, None);
    }

    #[test]
    fn items_are_cleaned() {
        assert_eq!(cleaned("  a\tb\nc  "), "a b c");
        assert_eq!(cleaned(&"x".repeat(300)).len(), MAX_TEXT);
    }
}
