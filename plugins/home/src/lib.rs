//! The home screen: pages of widgets and tools in a grid, like a phone's.
//!
//! - **Pages**: each a grid of cells (4 columns by default, rows to fit
//!   the screen) in a setting of its own (see `pages`; the grid is the
//!   SDK's, `quireink_sdk::widgets::grid`): by default
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
//! - **Tools**: the entries of the `home.actions` menu (the library by
//!   default) and of the tools menu; plugins add their own with
//!   `Registrar::add_menu_entry`.
//!
//! The recently read books and their covers come from the library service
//! (`quireink_sdk::library`), which follows the standard reading events,
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
mod pages;
mod recent;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use quireink_sdk::builtin::{actions, events, params, slots};
use quireink_sdk::event::Event;
use quireink_sdk::ui::choice::{self, Choice};
use quireink_sdk::ui::{self, UiStyle};
use quireink_sdk::widgets::grid::{self, Geometry, GridResponse, GridView, Item};
use quireink_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, Handled, Hint, Id, Manifest, MenuEntry, Params,
    Plugin, PointerEvent, Priority, Rect, RefreshMode, RegisterError, Registrar, SDK_VERSION,
    Screen, ScreenSpec, Version,
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
pub(crate) const CHOICE: Id = Id::new_static("home.choice");
/// Every tool at once (swipe up on the home screen).
pub const ALL_TOOLS: Id = Id::new_static("home.all-tools");
/// Edits the home screen's pages: in the main menu, so that buttons alone
/// reach it, also without the pencil.
pub const EDIT_PAGES: Id = Id::new_static("home.edit-pages");
const DRAWER_SCREEN: Id = Id::new_static("home.drawer");
const TIPS_SCREEN: Id = Id::new_static("home.tips");
/// An item's options were chosen.
const CHOSEN: Event<choice::Picked> = Event::new("home.item-chosen");

/// The home screen plugin.
#[derive(Debug, Default)]
pub struct Home;

impl Plugin for Home {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Home screen"),
            description: Cow::Borrowed(
                "The start page: pages of widgets, such as the book being read and the tools.",
            ),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("home.title"),
            factory: Box::new(|env, params| Box::new(Dashboard::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: EDIT_SCREEN,
            title: Cow::Borrowed("home.edit.screen"),
            factory: Box::new(|env, params| Box::new(edit::EditScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: ADD_SCREEN,
            title: Cow::Borrowed("home.edit.add"),
            factory: Box::new(|env, params| Box::new(add::AddScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: LIST_ADD_SCREEN,
            title: Cow::Borrowed("home.edit.add"),
            factory: Box::new(|env, params| Box::new(edit::AddScreen::new(env, params))),
        })?;
        r.add_screen(ScreenSpec {
            id: CHOICE,
            title: Cow::Borrowed("home.edit.screen"),
            factory: Box::new(|env, params| Box::new(Choice::new(env, params))),
        })?;
        r.bind_slot(slots::HOME, SCREEN, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec {
            id: DRAWER_SCREEN,
            title: Cow::Borrowed("home.drawer"),
            factory: Box::new(|env, _| Box::new(drawer::DrawerScreen::new(env))),
        })?;
        r.add_screen(ScreenSpec {
            id: TIPS_SCREEN,
            title: Cow::Borrowed("home.tips.title"),
            factory: Box::new(|env, _| Box::new(drawer::TipsScreen::new(env))),
        })?;
        r.add_action(
            quireink_sdk::ActionSpec::new(ALL_TOOLS, "home.drawer").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    out.push(Command::OpenScreen(DRAWER_SCREEN, Params::new()));
                },
            ),
        )?;
        r.add_icon(ALL_TOOLS, Cow::Borrowed("apps"))?;
        // Shown, the home screen edits itself; from elsewhere it opens
        // editing.
        r.add_action(
            quireink_sdk::ActionSpec::new(EDIT_PAGES, "home.edit-pages").handler(
                |_: &Env<'_>, out: &mut Vec<Command>| {
                    let params = Params::new().with(params::MODE, "edit");
                    out.push(Command::OpenSlot(slots::HOME, params));
                },
            ),
        )?;
        r.add_icon(EDIT_PAGES, Cow::Borrowed("pencil"))?;
        r.add_menu_entry(MenuEntry {
            menu: quireink_sdk::builtin::menus::MAIN,
            item: EDIT_PAGES,
            order: 40,
        })?;
        // As on phones: up on the home screen shows every tool.
        r.add_keymap(Cow::Borrowed(
            "[keymap.core.home]\nswipe.up = home.all-tools\n",
        ))?;
        r.add_setting(
            quireink_sdk::SettingSpec::switch(
                &drawer::TIPS_SETTING,
                "home.setting.gesture-tips",
                true,
            )
            .group(quireink_sdk::builtin::groups::LOOK)
            .advanced()
            .description("home.setting.gesture-tips.help"),
        )?;
        r.add_menu_entry(MenuEntry {
            menu: ACTIONS,
            item: actions::LIBRARY,
            order: 10,
        })?;
        pages::register(r)?;
        blocks::register(r)?;
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
}

/// Where the parts of the home screen go: the status line, the grid, the
/// footer.
fn frame(env: &Env<'_>) -> (Option<Rect>, Geometry, Rect) {
    let style = UiStyle::new(env);
    let display = env.device.display;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let footer_height = style.row as u16;
    let footer = Rect::new(
        0,
        display.height - footer_height,
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
    let area = Rect::new(
        margin - gap / 2,
        top,
        display.width.saturating_sub(2 * margin) + gap,
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
    fn new(env: &Env<'_>, params: &Params) -> Self {
        // Opened to edit a page (from its setting).
        let editing = params.get(params::ID).and_then(pages::page_of).or_else(|| {
            // Opened by *Edit home screen*.
            (params.get(params::MODE) == Some("edit")).then_some(0)
        });
        let mut grid = GridView::new(env, CHOICE, CHOSEN);
        grid.set_editing(editing.is_some());
        let (status, _, footer) = frame(env);
        let mut dashboard = Self {
            style: UiStyle::new(env),
            page: editing.unwrap_or(0),
            pages: 1,
            grid,
            status,
            footer,
            pencil: Rect::default(),
            plus: Rect::default(),
        };
        dashboard.layout(env);
        dashboard
    }

    /// Places the page's items in the grid, and the buttons below.
    fn layout(&mut self, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let (status, geometry, footer) = frame(env);
        let pages = pages::pages(env);
        self.pages = pages.len();
        self.page = self.page.min(self.pages - 1);
        let items = pages::expand(env, &pages[self.page]);
        // The pencil (done, when editing) at the right of the footer, if
        // it is wanted; add at the left when editing.
        let margin = style.margin as u16;
        let side = footer.height;
        self.pencil = Rect::new(display.width - margin - side, footer.y, side, side);
        self.plus = Rect::new(margin, footer.y, side, side);
        let editing = self.grid.editing();
        let mut buttons = Vec::new();
        if editing {
            buttons.push((self.plus, ADD));
        }
        if editing || env.settings.read(&pages::FOOTER_PENCIL) {
            buttons.push((self.pencil, PENCIL));
        }
        self.grid
            .set_tool_labels(env.settings.read(&blocks::TOOL_LABELS));
        self.grid.layout(env, geometry, &items, &buttons);
        self.status = status;
        self.footer = footer;
        self.style = style;
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
            GridResponse::Changed(items) => self.store(items, cx),
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
    fn store(&self, items: Vec<Item>, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let mut pages = pages::pages(&env);
        if let Some(page) = pages.get_mut(self.page) {
            *page = items;
        }
        for command in pages::store(&pages) {
            cx.command(command);
        }
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
                let line = quireink_sdk::status::StatusLine {
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
                if page == self.page {
                    canvas.fill_rect(x, y, dot, dot, Color::BLACK);
                } else {
                    canvas.stroke_rect(x, y, dot, dot, style.line.max(1), Color::BLACK);
                }
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
        if let Some(status) = self.status {
            blocks::draw_status(canvas, env, status);
        }
        self.draw_footer(canvas, env);
        self.grid.draw(canvas, env);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // The reader may have changed the recent books meanwhile.
        let env = *cx.env();
        // The first time on a touch screen: how the gestures go.
        if drawer::tips_due(&env) {
            cx.command(Command::OpenScreen(TIPS_SCREEN, Params::new()));
        }
        self.layout(&env);
        cx.request_redraw();
        cx.wake_at(env.now);
    }

    /// Redraws widgets whose content changed (the time each minute) and
    /// looks up the covers not known yet, one a time.
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        if let Some(status) = self.status {
            cx.request_redraw_area(status);
        }
        let mut changed = self.grid.refresh_widgets(&env, cx);
        let missing = blocks::wanted_covers(&env)
            .into_iter()
            .find(|path| covers::get(env.storage, path).is_none());
        if let Some(path) = missing {
            if covers::look_up(&env, &path).is_some() {
                changed |= self.grid.redraw_widget(pages::CONTINUE, cx);
                changed |= self.grid.redraw_widget(pages::RECENT, cx);
            }
            cx.wake_at(env.now.add_millis(20));
        } else {
            let next = quireink_sdk::time::next_minute(&env).unwrap_or(env.now.add_millis(60_000));
            cx.wake_at(next);
        }
        if changed {
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let response = self.grid.on_event(event, params, cx);
        if response != GridResponse::Ignored {
            self.respond(response, cx);
        } else if *event == events::SETTING_CHANGED || *event == events::STATUS_CHANGED {
            let env = *cx.env();
            self.layout(&env);
            cx.request_redraw();
        }
    }

    /// While the home screen is arranged, back ends it.
    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        self.grid.hint(action, env)
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
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
        let response = self.grid.on_pointer(event, cx);
        self.respond(response, cx)
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let response = self.grid.on_gesture(gesture, cx);
        self.respond(response, cx)
    }
}

#[cfg(test)]
mod tests {
    use quireink_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
