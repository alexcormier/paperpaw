//! The home screen: pages of widgets and tools in a grid, like a phone's.
//!
//! - **Pages**: each a grid of cells (4 columns by default, rows to fit
//!   the screen) in a setting of its own (see `pages`; the grid is the
//!   SDK's, `paperpaw_sdk::widgets::grid`): by default
//!   the clock, the book being read and the recent ones, then a page with
//!   all tools. The status line sits above the grid. Swipes and the page
//!   buttons go from page to page; dots below show where one is.
//! - **Items**: widgets (the home screen's own, see `blocks`, and any
//!   plugin's, `Registrar::add_widget`) and tools (actions, as icons),
//!   each taking a rectangle of cells; empty cells stay empty.
//! - **Targets**: what an item does when tapped (open a book, a tool).
//!   With buttons, left and right (or up and down) go from target to
//!   target and confirm runs it; the first press shows where the focus is.
//! - **Editing**: the pencil below the page (reached with the buttons too),
//!   a long press or *Edit home screen* in the main menu (`home.edit-pages`,
//!   also where the pencil is switched off) shows the grid: drag an item to move it, by its
//!   corner to size it; tap it for its options (move or size it with the
//!   buttons, its settings, remove it). *Add* lists what can be added,
//!   and adds or removes pages (see `add`).
//! - **Layouts**: ready-made ones to start from (calm, covers, a look at
//!   one's reading, a large clock, minimal), any plugin's too, chosen in
//!   the first-run setup, while editing or with `home.choose-layout`, each
//!   shown on the home screen itself before it is kept (see `layouts`).
//! - **Tools**: the entries of the `home.actions` menu (the library by
//!   default) and of the tools menu; plugins add their own with
//!   `Registrar::add_menu_entry`.
//!
//! The recently read books and their covers come from the library service
//! (`paperpaw_sdk::library`), which follows the standard reading events,
//! so any reader plugin works.
//! The home screen is bound to `core.home`; replace it with any other
//! screen by configuration or with a plugin of higher priority.

#![no_std]

extern crate alloc;

mod add;
mod blocks;
mod covers;
mod drawer;
mod edit;
mod layouts;
mod pages;
mod recent;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::widgets::grid::{self, Geometry, GridResponse, GridView, Item, What};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, Handled, Hint, Id, Manifest, MenuEntry, Params,
    Plugin, PointerEvent, Priority, Rect, RefreshMode, RegisterError, Registrar, Screen,
    ScreenSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("home");
pub const SCREEN: Id = Id::new_static("home.dashboard");
/// Shortcuts at the head of the home screen's grid of tools.
pub const ACTIONS: Id = Id::new_static("home.actions");
/// Editing a list of widgets (e.g. the sleep screen's).
pub(crate) const EDIT_SCREEN: Id = Id::new_static("home.edit");
/// Adding a widget or tool to a page.
pub(crate) const ADD_SCREEN: Id = Id::new_static("home.add-widget");
/// Adding a widget to a list of widgets.
pub(crate) const LIST_ADD_SCREEN: Id = Id::new_static("home.add-to-list");
/// Every tool at once (swipe up on the home screen).
pub const ALL_TOOLS: Id = Id::new_static("home.all-tools");
/// Edits the home screen's pages: in the main menu, so that buttons alone
/// reach it, also without the pencil.
pub const EDIT_PAGES: Id = Id::new_static("home.edit-pages");
const DRAWER_SCREEN: Id = Id::new_static("home.drawer");
const TIPS_SCREEN: Id = Id::new_static("home.tips");
/// An item's options were chosen.
const CHOSEN: Event<choice::Picked> = Event::new("home.item-chosen");

/// Whether the home screen tells how to swipe, until tried.
static TIPS: [paperpaw_sdk::SettingSpec; 1] = [paperpaw_sdk::SettingSpec::fixed_switch(
    drawer::TIPS_SETTING.static_str(),
    "home.setting.gesture-tips",
    true,
    paperpaw_sdk::builtin::groups::LOOK,
)
.advanced()
.described("home.setting.gesture-tips.help")];

/// The home screen plugin.
#[derive(Debug, Default)]
pub struct Home;

impl Plugin for Home {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Home screen", Version::new(0, 1, 0))
            .description(
                "The start page: pages of widgets, such as the book being read and the tools.",
            )
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "home.title",
            Box::new(|env, params| Box::new(Dashboard::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            EDIT_SCREEN,
            "home.edit.screen",
            Box::new(|env, params| Box::new(edit::EditScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            ADD_SCREEN,
            "home.edit.add",
            Box::new(|env, params| Box::new(add::AddScreen::new(env, params))),
        ))?;
        r.add_screen(ScreenSpec::new(
            LIST_ADD_SCREEN,
            "home.edit.add",
            Box::new(|env, params| Box::new(edit::AddScreen::new(env, params))),
        ))?;
        r.bind_slot(slots::HOME, SCREEN, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            layouts::SCREEN,
            "home.layouts.title",
            Box::new(|env, _| Box::new(Dashboard::chooser(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            DRAWER_SCREEN,
            "home.drawer",
            Box::new(|env, _| Box::new(drawer::DrawerScreen::new(env))),
        ))?;
        r.add_screen(ScreenSpec::new(
            TIPS_SCREEN,
            "home.tips.title",
            Box::new(|env, _| Box::new(drawer::TipsScreen::new(env))),
        ))?;
        r.add_action(
            paperpaw_sdk::ActionSpec::new(ALL_TOOLS, "home.drawer").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    out.push(Command::OpenScreen(DRAWER_SCREEN, Params::new()));
                },
            ),
        )?;
        r.add_icon(ALL_TOOLS, Cow::Borrowed("apps"))?;
        // Shown, the home screen edits itself; from elsewhere it opens
        // editing.
        r.add_action(
            paperpaw_sdk::ActionSpec::new(EDIT_PAGES, "home.edit-pages").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    let params = Params::new().with(params::MODE, "edit");
                    out.push(Command::OpenSlot(slots::HOME, params));
                },
            ),
        )?;
        r.add_icon(EDIT_PAGES, Cow::Borrowed("pencil"))?;
        r.add_menu_entry(MenuEntry::new(paperpaw_sdk::builtin::menus::MAIN, EDIT_PAGES).order(40))?;
        // As on phones: up on the home screen shows every tool.
        r.add_keymap(Cow::Borrowed(
            "[keymap.core.home]\nswipe.up = home.all-tools\n",
        ))?;
        r.add_settings(&TIPS)?;
        r.add_menu_entry(MenuEntry::new(ACTIONS, actions::LIBRARY).order(10))?;
        pages::register(r)?;
        blocks::register(r)?;
        layouts::register(r)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The home screen's own buttons below the grid.
const PENCIL: u16 = 0;
const ADD: u16 = 1;

#[derive(Debug)]
struct Dashboard {
    style: UiStyle,
    page: usize,
    pages: usize,
    grid: GridView,
    /// The status line above the grid, if shown.
    status: Option<Rect>,
    /// The dots and the pencil (when editing: add and done) below.
    footer: Rect,
    pencil: Rect,
    plus: Rect,
    /// Recent books whose covers are to be looked up, one a tick.
    missing: Vec<String>,
    /// How many of the recent books the page's widgets show.
    books: usize,
    /// The widgets on the page, by their ids (`home.recent@1`).
    shown: Vec<String>,
    /// Choosing a layout: each shown here, a sheet below (see `layouts`).
    chooser: Option<layouts::Chooser>,
    /// What the page shows changed (a setting, the library): laid out
    /// anew before it is drawn next, once for many changes.
    stale: bool,
}

/// The next full minute, when the clock shows another time.
fn next_minute(env: &Env<'_>) -> paperpaw_sdk::Instant {
    paperpaw_sdk::time::next_minute(env).unwrap_or(env.now.add_millis(60_000))
}

/// Where the parts of the home screen go: the status line, the grid, the
/// footer; with `sheet`, a footer that high, the grid's cells as many as
/// without it.
fn frame(env: &Env<'_>, sheet: Option<u16>) -> (Option<Rect>, Geometry, Rect) {
    let (status, geometry, footer) = frame_of(env, UiStyle::new(env).row as u16);
    let Some(height) = sheet else {
        return (status, geometry, footer);
    };
    let (_, shorter, footer) = frame_of(env, height);
    let gap = UiStyle::new(env).gap as u16;
    let geometry = Geometry::of(shorter.area, geometry.columns, geometry.rows, gap);
    (status, geometry, footer)
}

/// [`frame`] with a footer `footer_height` high.
fn frame_of(env: &Env<'_>, footer_height: u16) -> (Option<Rect>, Geometry, Rect) {
    let style = UiStyle::new(env);
    let display = env.device.display;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let footer = Rect::new(
        0,
        display.height.saturating_sub(footer_height),
        display.width,
        footer_height,
    );
    let mut top = gap / 2;
    let status = env.settings.read(&pages::STATUS_BAR).then(|| {
        let height = blocks::status_height(env, display.width);
        let r = Rect::new(
            margin,
            top,
            display.width.saturating_sub(2 * margin),
            height,
        );
        top += height;
        r
    });
    // Half a gap wider than the margins on either side, as the grid
    // leaves half a gap round its cells: they line up with the margins.
    // A margin narrower than that is as wide as that, inside the screen.
    let outer = margin.max(gap / 2);
    let area = Rect::new(
        outer - gap / 2,
        top,
        display.width.saturating_sub(2 * outer) + gap,
        footer.y.saturating_sub(top),
    );
    let columns = env
        .settings
        .read(&pages::COLUMNS)
        .clamp(2, i64::from(grid::MAX_COLUMNS)) as u8;
    let rows = env
        .settings
        .read(&pages::ROWS)
        .clamp(0, i64::from(grid::MAX_ROWS)) as u8;
    (status, Geometry::new(env, area, columns, rows), footer)
}

impl Dashboard {
    /// The home screen, laid out when it shows (or is drawn first).
    fn new(env: &Env<'_>, params: &Params) -> Self {
        // Opened to edit a page (from its setting): that page, where it
        // shows.
        let page_set = params.get(params::ID).and_then(pages::slot_of).map(|slot| {
            pages::pages(env)
                .iter()
                .position(|p| p.slot == slot)
                .unwrap_or(0)
        });
        // Opened by *Edit home screen*.
        let editing = page_set.or_else(|| (params.get(params::MODE) == Some("edit")).then_some(0));
        let mut grid = GridView::asking(env, CHOSEN);
        grid.set_editing(editing.is_some());
        let (status, _, footer) = frame(env, None);
        Self {
            style: UiStyle::new(env),
            page: editing.unwrap_or(0),
            pages: 1,
            grid,
            status,
            footer,
            pencil: Rect::default(),
            plus: Rect::default(),
            missing: Vec::new(),
            books: 0,
            shown: Vec::new(),
            chooser: None,
            stale: true,
        }
    }

    /// The home screen choosing a layout.
    fn chooser(env: &Env<'_>) -> Self {
        let mut dashboard = Self::new(env, &Params::new());
        dashboard.chooser = Some(layouts::Chooser::new(env));
        dashboard
    }

    /// Places the page's items in the grid, and the buttons below.
    fn layout(&mut self, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let sheet = self.chooser.as_ref().map(|c| c.height(env));
        let (status, geometry, footer) = frame(env, sheet);
        let pages = pages::pages(env);
        self.pages = pages.len();
        self.page = self.page.min(self.pages - 1);
        let mut items = pages::expand(env, &pages[self.page].items);
        self.shown = items
            .iter()
            .filter_map(|item| match &item.what {
                What::Widget(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        if sheet.is_some() {
            // Placed as on the home screen, then shown in the lower
            // preview cell for cell.
            let (_, full, _) = frame(env, None);
            items = grid::place(env, &full, &items)
                .into_iter()
                .map(|(item, cells)| Item::placed(item.what, cells))
                .collect();
        }
        // The pencil (done, when editing) at the right of the footer, if
        // it is wanted; add at the left when editing.
        let margin = style.margin as u16;
        let side = footer.height;
        self.pencil = Rect::new(display.width - margin - side, footer.y, side, side);
        self.plus = Rect::new(margin, footer.y, side, side);
        let editing = self.grid.editing();
        let mut buttons = Vec::new();
        if let Some(chooser) = &mut self.chooser {
            // Shown, not used: no focus, nothing to tap but the sheet.
            self.page = 0;
            self.grid.set_focus_shown(false);
            chooser.layout(env, footer);
        } else {
            if editing {
                buttons.push((self.plus, ADD));
            }
            if editing || env.settings.read(&pages::FOOTER_PENCIL) {
                buttons.push((self.pencil, PENCIL));
            }
        }
        self.grid
            .set_tool_labels(env.settings.read(&blocks::TOOL_LABELS));
        self.grid.layout(env, geometry, &items, &buttons);
        self.books = blocks::books_shown(env, &items);
        self.status = status;
        self.footer = footer;
        self.style = style;
        self.stale = false;
    }

    /// Lays the page out anew before it is drawn next.
    fn mark_stale(&mut self, cx: &mut Cx<'_, '_>) {
        self.stale = true;
        cx.request_redraw();
    }

    /// Does what was asked on the chooser's sheet.
    fn choose(&mut self, tap: layouts::Tap, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(chooser) = &mut self.chooser else {
            return;
        };
        match tap {
            layouts::Tap::Use => {
                for command in chooser.choose(&env) {
                    cx.command(command);
                }
            }
            layouts::Tap::Next | layouts::Tap::Previous => {
                if chooser.turn(tap == layouts::Tap::Next) {
                    cx.command(chooser.preview(&env));
                }
            }
        }
    }

    /// Lays the screen out again and draws it.
    fn relayout(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.layout(&env);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Does what the grid asks for.
    fn respond(&mut self, response: GridResponse, cx: &mut Cx<'_, '_>) -> Handled {
        let handled = response.handled();
        match response {
            GridResponse::Changed(items) => self.store(&items, cx),
            GridResponse::Editing(_) => self.relayout(cx),
            GridResponse::Button(PENCIL) => self.set_editing(!self.grid.editing(), cx),
            GridResponse::Button(ADD) => cx.command(Command::OpenScreen(
                ADD_SCREEN,
                Params::new().with(add::PAGE, format!("{}", self.page)),
            )),
            _ => {}
        }
        handled
    }

    fn turn(&mut self, forward: bool, cx: &mut Cx<'_, '_>) {
        let to = if forward {
            (self.page + 1 < self.pages).then_some(self.page + 1)
        } else {
            self.page.checked_sub(1)
        };
        if let Some(to) = to {
            self.page = to;
            self.grid.reset_focus();
            self.relayout(cx);
        }
    }

    /// Begins or ends editing the page.
    fn set_editing(&mut self, on: bool, cx: &mut Cx<'_, '_>) {
        self.grid.set_editing(on);
        self.relayout(cx);
    }

    /// Keeps the page as `items`.
    fn store(&self, items: &[Item], cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(page) = pages::pages(&env).get(self.page) {
            cx.command(pages::store(page.slot, items));
        }
    }

    /// Whether a change of setting `id` changes how the page is made: the
    /// home screen's own settings, the look of all (`core.`), and those
    /// of the widgets on the page. What other settings change, the
    /// widgets read anew without being made anew.
    fn remakes(&self, id: &str) -> bool {
        let of = |widget: &str| {
            id.strip_prefix(widget)
                .is_some_and(|rest| rest.starts_with('.'))
        };
        of("home")
            || of("core")
            || self
                .shown
                .iter()
                .any(|widget| of(widget) || of(widgets::base(widget)))
    }

    fn draw_footer(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let f = self.footer;
        let editing = self.grid.editing();
        // Status items at the left, where adding goes while editing.
        if !editing {
            let font = style.font(env);
            if let Some(font) = font {
                let margin = style.margin as u16;
                let width = (f.width / 2).saturating_sub(margin + style.gap as u16 * 3);
                let line = paperpaw_sdk::status::StatusLine {
                    area: Rect::new(margin, f.y, width, f.height),
                    lists: [env.settings.read(&pages::FOOTER_ITEMS), "", ""],
                    size: style.small,
                    color: style.faint(60),
                    percent: true,
                };
                line.draw(canvas, env, &font, &Params::new());
            }
        }
        // A dot a page, the current one filled.
        if self.pages > 1 && (editing || env.settings.read(&pages::FOOTER_DOTS)) {
            let dot = (style.gap * 2 / 3).max(6);
            let spacing = dot * 2;
            let total = dot * self.pages as u32 + (spacing - dot) * (self.pages as u32 - 1);
            let mut x = i32::from(f.x) + (i32::from(f.width) - total as i32) / 2;
            let y = i32::from(f.y) + (i32::from(f.height) - dot as i32) / 2;
            for page in 0..self.pages {
                pages::draw_dot(canvas, env, &style, (x, y), dot, page == self.page);
                x += spacing as i32;
            }
        }
        let size = style.title;
        let mut icon = |name: &str, area: Rect, ink: Color| {
            let w = ui::icon_width(env, name, size);
            let x = f32::from(area.x) + (f32::from(area.width) - w) / 2.0;
            ui::draw_icon(canvas, env, name, size, x, area, ink);
        };
        if editing {
            icon("plus", self.plus, Color::BLACK);
            icon("check", self.pencil, Color::BLACK);
        } else if env.settings.read(&pages::FOOTER_PENCIL) {
            icon("pencil", self.pencil, style.faint(60));
        }
    }
}

impl Screen for Dashboard {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.stale {
            self.layout(env);
        }
        if let Some(status) = self.status {
            blocks::draw_status(canvas, env, status);
        }
        match &self.chooser {
            Some(chooser) => chooser.draw(canvas, env),
            None => self.draw_footer(canvas, env),
        }
        self.grid.draw(canvas, env);
    }

    /// Hidden by a book or another screen: the widgets go with what they
    /// keep (covers made small), made anew when it shows again (or is
    /// drawn below an overlay left open).
    fn on_hide(&mut self) {
        self.grid.release();
        self.stale = true;
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // The reader may have changed the recent books meanwhile.
        let env = *cx.env();
        if let Some(chooser) = &self.chooser {
            // The layout chosen, shown.
            cx.command(chooser.preview(&env));
        } else if drawer::tips_due(&env) {
            // The first time on a touch screen: how the gestures go.
            cx.command(Command::OpenScreen(TIPS_SCREEN, Params::new()));
        }
        // Each time it shows, the page as it is: made anew after it was
        // hidden (its widgets went) or changed, else what they show read
        // again. The focus where wanted.
        self.grid
            .set_focus_shown(!env.device.input.touch && env.settings.read(&blocks::FOCUS_AT_ONCE));
        if self.stale {
            self.layout(&env);
        } else {
            self.grid.update_widgets(&env, cx);
        }
        cx.request_redraw();
        // The covers not known yet are looked up first, one a tick.
        self.missing = blocks::missing_covers(&env, self.books);
        self.missing.reverse();
        if self.missing.is_empty() {
            cx.wake_at(next_minute(&env));
        } else {
            cx.wake_at(env.now);
        }
    }

    /// Looks up the covers not known yet, one a tick; then, each minute,
    /// updates the widgets (the time) and draws those that changed.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut changed = false;
        if let Some(path) = self.missing.pop() {
            if covers::look_up(&env, &path).is_some() {
                changed |= self.grid.update_widget(pages::CONTINUE, &env, cx);
                changed |= self.grid.update_widget(pages::RECENT, &env, cx);
            }
            if self.missing.is_empty() {
                cx.wake_at(next_minute(&env));
            } else {
                cx.wake_at(env.now.add_millis(20));
            }
        } else {
            if let Some(status) = self.status {
                cx.request_redraw_area(status);
            }
            changed = self.grid.update_live(&env, cx);
            cx.wake_at(next_minute(&env));
        }
        if changed {
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let response = self.grid.on_event(event, params, cx);
        let env = *cx.env();
        let setting = events::SETTING_CHANGED.read(event, params);
        if response != GridResponse::Ignored {
            self.respond(response, cx);
        } else if (self.chooser.is_some() && (layouts::shows_anew(event) || setting.is_some()))
            || setting
                .as_ref()
                .is_some_and(|changed| self.remakes(changed.id.as_str()))
        {
            // The widgets made anew, what they show read again.
            self.mark_stale(cx);
            if self.chooser.is_some() {
                cx.request_refresh(RefreshMode::Partial);
            }
        } else if setting.is_some()
            || *event == events::LIBRARY_CHANGED
            || *event == events::BOOK_MOVED
        {
            // What the widgets show read again, where they are made (when
            // hidden, they are made anew as it shows).
            if !self.stale && self.grid.update_widgets(&env, cx) {
                cx.request_refresh(RefreshMode::Fast);
            }
        } else if let Some(changed) = events::WIDGET_CHANGED.read(event, params) {
            if self.grid.update_widget(changed.id.as_str(), &env, cx) {
                cx.request_refresh(RefreshMode::Fast);
            }
        } else if *event == events::STATUS_CHANGED {
            if let Some(status) = self.status {
                cx.request_redraw_area(status);
            }
            self.grid.update_live(&env, cx);
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// While the home screen is arranged, back ends it.
    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if self.chooser.is_some() {
            return if *action == actions::CONFIRM {
                Hint::Text(env.i18n.tr("home.layouts.use-short").into())
            } else {
                Hint::Default
            };
        }
        self.grid.hint(action, env)
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if self.chooser.is_some() {
            return match layouts::action(action) {
                Some(tap) => {
                    self.choose(tap, cx);
                    Handled::Yes
                }
                None => Handled::No,
            };
        }
        if *action == EDIT_PAGES {
            self.set_editing(!self.grid.editing(), cx);
            return Handled::Yes;
        }
        let response = self.grid.on_action(action, cx);
        if response != GridResponse::Ignored {
            return self.respond(response, cx);
        }
        if *action == actions::NEXT_PAGE || *action == actions::PREV_PAGE {
            self.turn(*action == actions::NEXT_PAGE, cx);
            return Handled::Yes;
        }
        Handled::No
    }

    /// While editing, the edges are the grid's.
    fn holds_edges(&self) -> bool {
        self.grid.holds_edges()
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        if self.chooser.is_some() {
            return Handled::No;
        }
        let response = self.grid.on_pointer(event, cx);
        self.respond(response, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(chooser) = &self.chooser {
            // The layout is only shown: taps on it do nothing.
            let on_sheet = chooser.contains(gesture.start);
            return match layouts::gesture(chooser, gesture) {
                Some(tap) => {
                    self.choose(tap, cx);
                    Handled::Yes
                }
                None if gesture.kind == paperpaw_sdk::GestureKind::Tap && !on_sheet => Handled::Yes,
                None => Handled::No,
            };
        }
        let response = self.grid.on_gesture(gesture, cx);
        self.respond(response, cx)
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
