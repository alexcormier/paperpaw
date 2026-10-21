//! The overview: today, the last seven days and the streak at a glance,
//! the last two weeks as bars, the totals, and the way to the calendar,
//! the books and the book being read.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::store::{self, Day};
use crate::{BOOK, BOOKS, CALENDAR, chart, fmt, totals};

/// Days in the chart.
const CHART_DAYS: i32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    ThisBook(u16),
    Calendar,
    Books,
    Sync,
}

pub(crate) struct Overview {
    style: UiStyle,
    header: ui::Header,
    tiles: Rect,
    chart: Rect,
    totals: Rect,
    list: ListView,
    rows: Vec<Row>,
    today: Option<i32>,
    days: Vec<Day>,
    books: usize,
    /// Distinct pages read today.
    today_pages: u32,
    /// The size the tiles' values are drawn at (the largest at which each
    /// fits), found once.
    value_size: f32,
}

impl Overview {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device.display;
        let current = params.get(params::ID).and_then(|id| id.parse::<u16>().ok());
        let mut rows = Vec::new();
        if let Some(book) = current {
            rows.push(Row::ThisBook(book));
        }
        rows.extend([Row::Calendar, Row::Books, Row::Sync]);
        let row = style.row as u16;
        let gap = style.gap as u16;
        let list_height = row * rows.len() as u16;
        // The lowest the rows may start.
        let lowest = display.height.saturating_sub(list_height + gap);
        let tiles = Rect::new(0, header.area.height + gap, display.width, row * 8 / 5);
        let chart_top = tiles.y + tiles.height + gap * 2;
        // The chart takes the room left, up to a third of the screen; the
        // totals right below it.
        let room = lowest.saturating_sub(chart_top + row + gap * 2);
        let chart = Rect::new(
            style.margin as u16,
            chart_top,
            display.width - 2 * style.margin as u16,
            room.min(display.height / 3),
        );
        let totals = Rect::new(0, chart.y + chart.height + gap, display.width, row);
        // The ways on right below.
        let list_top = (totals.y + totals.height + gap * 2).min(lowest);
        let today = fmt::today(env);
        let days = store::days(env.storage, |unix| fmt::day_of(env, unix));
        let today_pages = today.map_or(0, |day| distinct_pages(env, day));
        let mut overview = Self {
            list: ListView::new(
                Rect::new(0, list_top, display.width, list_height),
                style.row,
            )
            .focus_on_keys(env),
            rows,
            style,
            header,
            tiles,
            chart,
            totals,
            today,
            days,
            books: store::books(env.storage).len(),
            today_pages,
            value_size: style.title,
        };
        overview.value_size = overview.value_size(env);
        overview
    }

    /// The tiles: their names, values and notes; none without the time.
    fn tiles(&self, env: &Env<'_>) -> Option<[(&'static str, String, String); 3]> {
        let today = self.today?;
        let week: u32 = (0..7).map(|d| self.seconds_on(today - d)).sum();
        let streak = totals::streak(&self.days, today);
        let pages = env
            .i18n
            .plural("stats.pages", i64::from(self.today_pages), &[]);
        Some([
            (
                "stats.today",
                fmt::duration(env, u64::from(self.seconds_on(today))),
                pages,
            ),
            (
                "stats.week",
                fmt::duration(env, u64::from(week)),
                env.i18n.tr("stats.last-seven-days").into(),
            ),
            (
                "stats.streak",
                env.i18n.plural("stats.days", i64::from(streak), &[]),
                env.i18n.tr("stats.in-a-row").into(),
            ),
        ])
    }

    /// The room for the text of the tile `i`: up to the next tile's line,
    /// a gap before it; the last up to the margin.
    fn tile_room(&self, i: usize) -> f32 {
        let style = self.style;
        let right = if i + 1 < 3 { style.gap } else { style.margin };
        (f32::from(self.tiles.width / 3) - style.margin as f32 - right as f32).max(0.0)
    }

    /// The largest size at which each tile's value fits, down to the small
    /// one.
    fn value_size(&self, env: &Env<'_>) -> f32 {
        let style = self.style;
        let (Some(font), Some(tiles)) = (style.font(env), self.tiles(env)) else {
            return style.title;
        };
        tiles
            .iter()
            .enumerate()
            .map(|(i, (_, value, _))| {
                widgets::largest_size(style.title, style.small, |size| {
                    font.measure(size, value) <= self.tile_room(i)
                })
                .unwrap_or(style.small)
            })
            .fold(style.title, f32::min)
    }

    fn seconds_on(&self, day: i32) -> u32 {
        fmt::seconds_on(&self.days, day)
    }

    fn open(&self, row: Row, cx: &mut Cx<'_, '_>) {
        cx.command(match row {
            Row::ThisBook(book) => {
                Command::OpenScreen(BOOK, Params::new().with(params::ID, format!("{book}")))
            }
            Row::Calendar => Command::OpenScreen(CALENDAR, Params::new()),
            Row::Books => Command::OpenScreen(BOOKS, Params::new()),
            Row::Sync => Command::OpenScreen(crate::sync::SCREEN, Params::new()),
        });
    }

    fn draw_tiles(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let (Some(font), Some(tiles)) = (style.font(env), self.tiles(env)) else {
            return;
        };
        let t = self.tiles;
        let width = t.width / 3;
        let third = t.height / 3;
        let pad = style.margin as f32;
        let value_size = self.value_size;
        for (i, (label, value, note)) in tiles.iter().enumerate() {
            let x = t.x + width * i as u16;
            let left = f32::from(x) + pad;
            let room = self.tile_room(i);
            let cell = |row: u16| Rect::new(x, t.y + third * row, (pad + room) as u16, third);
            // A little smaller where a text does not fit whole (a longer
            // language), only then shortened.
            let least = style.small * 0.85;
            let lines = [
                (env.i18n.tr(label), style.small, least, style.faint(90)),
                (value.as_str(), value_size, value_size, Color::BLACK),
                (note.as_str(), style.small, least, style.faint(90)),
            ];
            for (row, (text, size, least, color)) in lines.into_iter().enumerate() {
                let (size, line) = widgets::fit_line(&font, size, least, text, room);
                ui::draw_label(canvas, &font, size, left, cell(row as u16), &line, color);
            }
            if i > 0 {
                canvas.fill_tint(
                    Rect::new(x, t.y, style.line.max(1) as u16, t.height),
                    Color::gray(190),
                );
            }
        }
    }

    fn draw_chart(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let c = self.chart;
        let title_h = (style.small * 1.6) as u16;
        let title = Rect::new(c.x, c.y, c.width, title_h);
        let label = env.i18n.format(
            "stats.last-days",
            &[("count", Arg::Int(i64::from(CHART_DAYS)))],
        );
        ui::draw_label(
            canvas,
            &font,
            style.small,
            f32::from(c.x),
            title,
            &label,
            style.faint(90),
        );
        let Some(today) = self.today else {
            return;
        };
        let first = today - CHART_DAYS + 1;
        let values: Vec<u32> = (first..=today).map(|d| self.seconds_on(d)).collect();
        let names: Vec<&str> = (first..=today)
            .map(|d| fmt::weekday_short(env, fmt::date_of(d).weekday))
            .collect();
        let max = values.iter().copied().max().unwrap_or(0);
        let top = if max > 0 {
            fmt::duration(env, u64::from(max))
        } else {
            String::new()
        };
        let area = Rect::new(
            c.x,
            c.y + title_h,
            c.width,
            c.height.saturating_sub(title_h),
        );
        chart::bars(
            canvas,
            env,
            &style,
            area,
            &values,
            &names,
            (Some(values.len() - 1), &top),
        );
    }

    /// All reading: the time, the books and the days.
    fn draw_totals(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let total: u64 = self.days.iter().map(|d| u64::from(d.seconds)).sum();
        let text = env.i18n.format(
            "stats.totals",
            &[
                ("time", Arg::Str(&fmt::duration(env, total))),
                (
                    "books",
                    Arg::Str(&env.i18n.plural("stats.books-count", self.books as i64, &[])),
                ),
                (
                    "days",
                    Arg::Str(
                        &env.i18n
                            .plural("stats.reading-days", self.days.len() as i64, &[]),
                    ),
                ),
            ],
        );
        let w = font.measure(style.small, &text);
        let x = (f32::from(self.totals.width) - w).max(0.0) / 2.0;
        ui::draw_label(
            canvas,
            &font,
            style.small,
            x,
            self.totals,
            &text,
            style.faint(60),
        );
    }
}

/// Distinct pages read on local day `day`.
pub(crate) fn distinct_pages(env: &Env<'_>, day: i32) -> u32 {
    let since = fmt::unix_near(day).saturating_sub(14 * 3600).max(0) as u32;
    let mut pages: Vec<(u16, u32)> = Vec::new();
    store::scan_recent(env.storage, since, |stat| {
        if fmt::day_of(env, stat.start) == day {
            pages.push((stat.book, stat.page));
        }
    });
    pages.sort_unstable();
    pages.dedup();
    pages.len() as u32
}

impl Screen for Overview {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n.tr("stats.title");
        self.header.draw(canvas, env, &style, title, None, None);
        if self.today.is_some() {
            self.draw_tiles(canvas, env);
            self.draw_chart(canvas, env);
        } else {
            // Without the time no day is today: what needs it gives way to
            // a hint (and the totals, if anything was read).
            let area = Rect::new(
                0,
                self.tiles.y,
                self.tiles.width,
                self.totals.y - self.tiles.y,
            );
            ui::draw_message(canvas, env, &style, area, env.i18n.tr("stats.no-clock"));
        }
        if self.today.is_some() || !self.days.is_empty() {
            self.draw_totals(canvas, env);
        }
        let rows = &self.rows;
        self.list.draw(
            canvas,
            &style,
            rows.len(),
            |canvas, index, rect, focused| {
                let key = match rows[index] {
                    Row::ThisBook(_) => "stats.this-book",
                    Row::Calendar => "stats.calendar",
                    Row::Books => "stats.books",
                    Row::Sync => "stats.sync.title",
                };
                ui::draw_value_row(canvas, env, &style, rect, focused, env.i18n.tr(key), "›");
            },
        );
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let delta = if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            -1
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            1
        } else if *a == actions::CONFIRM {
            self.open(self.rows[self.list.focus().min(self.rows.len() - 1)], cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        if self.list.move_focus(delta, self.rows.len()) != ListChange::None {
            cx.request_redraw_area(self.list.area());
            cx.request_refresh(RefreshMode::Fast);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.rows.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        cx.request_redraw_area(self.list.area());
        self.open(self.rows[index], cx);
        Handled::Yes
    }
}
