//! The menu overlay.
//!
//! Shows the menu named by the `menu` parameter at the bottom of the screen,
//! above whatever is open. Entries come from every plugin (see
//! `Registrar::add_menu_entry`) or from the user's `[menus]` definitions.
//! Choosing an action closes the menu and runs it. Settings show their
//! current value and change in place, so the menu stays open: switches
//! toggle, numbers have − and + buttons, choices open a list. An entry
//! whose action is named like a menu with entries opens that menu as a
//! submenu above this one.
//!
//! Bound to the `core.menu` slot; replace it to get a different menu style
//! (radial, side bar, …) for every menu at once. The same plugin shows
//! menus as panels of controls (`core.panel`, see `panel`).

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, menus, params, slots};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest, Params, Plugin,
    Priority, Rect, RefreshMode, RegisterError, Registrar, Screen, ScreenSpec, SettingKind,
    SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("menu");
pub const SCREEN: Id = Id::new_static("menu.list");
/// A menu as a panel of controls.
pub const PANEL: Id = Id::new_static("menu.panel");
/// Shows short messages (`core.notice`).
pub const NOTICE: Id = Id::new_static("menu.notice");
/// How menus look: a list ([`STYLE_LIST`]) or tiles ([`STYLE_TILES`]).
pub const STYLE: Setting<Text> = Setting::new("menu.style");
pub const STYLE_LIST: &str = "menu.style.list";
pub const STYLE_TILES: &str = "menu.style.tiles";

mod bar;
mod notice;
mod panel;

/// The menu plugin.
#[derive(Debug, Default)]
pub struct Menu;

/// How menus look.
static SETTINGS: [SettingSpec; 1] = [
    SettingSpec::fixed_choice(
        STYLE.static_str(),
        "menu.setting.style",
        &[STYLE_LIST, STYLE_TILES],
        STYLE_LIST,
        paperpaw_sdk::builtin::groups::LOOK,
    )
    .described("menu.setting.style.help"),
];

impl Plugin for Menu {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Menu", Version::new(0, 1, 0))
            .description("The menu at the bottom of the screen, with actions and settings.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "menu.title",
            Box::new(|env, params| {
                if params.get(params::MODE) == Some(bar::MODE) || tiles(env, params) {
                    Box::new(bar::BarScreen::new(env, params))
                } else {
                    Box::new(MenuScreen::new(env, params))
                }
            }),
        ))?;
        r.bind_slot(slots::MENU, SCREEN, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            PANEL,
            "menu.title",
            Box::new(|env, params| Box::new(panel::PanelScreen::open(env, params))),
        ))?;
        r.bind_slot(slots::PANEL, PANEL, Priority::DEFAULT)?;
        r.add_screen(ScreenSpec::new(
            NOTICE,
            "menu.title",
            Box::new(|env, params| Box::new(notice::NoticeScreen::new(env, params))),
        ))?;
        r.bind_slot(slots::NOTICE, NOTICE, Priority::DEFAULT)?;
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// An entry of the menu, or a heading.
#[derive(Debug)]
struct Item {
    /// Action, setting or section.
    id: Id,
    /// Translation key of the label.
    title: String,
    icon: Option<String>,
    heading: bool,
    /// The line under it (a translation key), if it has one.
    description: Option<String>,
}

#[derive(Debug)]
struct MenuScreen {
    items: Vec<Item>,
    style: UiStyle,
    region: Rect,
    rows: ui::Rows,
    /// Shown when the items need more than one page.
    pager: Option<ui::Pager>,
    /// The focused number is being changed with buttons.
    editing: bool,
    /// The focus is shown: always with buttons only, on touch screens
    /// once buttons are used (a tap needs no focus).
    keys: bool,
    /// Room for icons left of the labels, if any entry has one.
    icon_width: u16,
    /// How each setting's row is laid out (its title never cut for its
    /// control), with the room of a number's value: none for others.
    lines: Vec<Option<(ui::RowLines, String)>>,
    /// How many menus are open below this one (submenus).
    depth: usize,
    /// What it was opened with, to make it anew (see `on_event`).
    params: Params,
}

/// Parameter: the depth of a submenu.
pub(crate) const DEPTH: &str = "depth";

/// The next value of choice or language setting `spec`, where no screen
/// lists its values (see `ui::setting_opens`).
pub(crate) fn choose(env: &Env<'_>, spec: &SettingSpec, cx: &mut Cx<'_, '_>) {
    if let Some(value) = ui::next_value(env, spec, 1) {
        cx.command(Command::SetSetting(spec.id.clone(), value));
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }
}

/// Which of `items` the focus passes over: those shown faint now.
fn passed(env: &Env<'_>, items: &[Item]) -> Vec<bool> {
    items
        .iter()
        .map(|item| !item.heading && !env.menus.state(env, item.id.as_str()).enabled)
        .collect()
}

impl MenuScreen {
    fn new(env: &Env<'_>, params: &Params) -> Self {
        let main = menus::MAIN;
        let menu = params.get(params::MENU).unwrap_or(main.as_str());
        let mut items: Vec<Item> = env
            .menus
            .shown_items(env, menu)
            .into_iter()
            .filter(|item| !paperpaw_sdk::Menus::hidden(params, item.id.as_str()))
            // A setting shown only while another has a value.
            .filter(|item| {
                env.settings
                    .spec(item.id.as_str())
                    .is_none_or(|s| env.settings.shows(s))
            })
            .map(|item| Item {
                id: item.id.clone(),
                title: String::from(item.title),
                icon: item.icon.map(String::from),
                heading: item.section,
                description: env.menus.description(item.id.as_str()).map(String::from),
            })
            .collect();
        // Headings with nothing below them are left out.
        let keep: Vec<bool> = (0..items.len())
            .map(|i| !items[i].heading || items.get(i + 1).is_some_and(|next| !next.heading))
            .collect();
        let mut keep = keep.into_iter();
        items.retain(|_| keep.next().unwrap_or(true));
        let style = UiStyle::new(env);
        let display = env.device.display;
        let border = style.line * 2;
        let heading = ui::heading_height(env, &style);
        let icon_width = ui::icon_room(
            env,
            &style,
            items.iter().any(|i| i.icon.is_some() && !i.heading),
        );
        // A setting's row as the settings pages lay it out: its control
        // below its title where both do not fit side by side.
        let width = display.width.saturating_sub(icon_width);
        let lines: Vec<Option<(ui::RowLines, String)>> = items
            .iter()
            .map(|item| {
                let spec = Self::setting(env, &item.id).filter(|_| !item.heading)?;
                let room = ui::number_room(env, &style, core::iter::once(spec));
                let lines = ui::row_lines(env, &style, spec, width, &room, style.row, false);
                Some((lines, room))
            })
            .collect();
        let heights: Vec<u16> = items
            .iter()
            .zip(&lines)
            .map(|(item, lines)| {
                // A number's − and + grow, and its row with them.
                let row = lines.as_ref().map_or(style.row as u16, |(l, _)| l.height);
                if item.heading {
                    heading
                } else if item.description.is_some() {
                    // A line more, below the label.
                    row + description_height(env, &style)
                } else {
                    row
                }
            })
            .collect();
        let total: u32 = heights
            .iter()
            .map(|&h| u32::from(h))
            .sum::<u32>()
            .max(style.row);
        let wanted = total + 2 * style.gap + border;
        // Up to the header of the screen below, which stays in sight.
        let most = u32::from(display.height).saturating_sub(style.header);
        let height = wanted.min(most) as u16;
        let region = Rect::new(0, display.height - height, display.width, height);
        let mut list_area = Rect::new(
            0,
            region.y + (border + style.gap) as u16,
            display.width,
            height.saturating_sub((border + 2 * style.gap) as u16),
        );
        // Too many items: a bar at the bottom shows that there are more
        // pages and turns them.
        let mut pager = None;
        if wanted > u32::from(height) {
            let bar = ui::Pager::height(&style);
            list_area.height = list_area.height.saturating_sub(bar);
            let area = Rect::new(0, region.y + region.height - bar, display.width, bar);
            pager = Some(ui::Pager::new(env, area));
        }
        let headings = items.iter().map(|i| i.heading).collect();
        let mut rows = ui::Rows::new(list_area, heights, headings);
        rows.set_passed(passed(env, &items));
        // With buttons, on the first entry that can be chosen.
        if passed(env, &items).get(rows.focus()) == Some(&true) {
            rows.step(true);
        }
        Self {
            items,
            region,
            rows,
            pager,
            editing: false,
            keys: !env.device.input.touch,
            icon_width,
            lines,
            depth: params.get(DEPTH).and_then(|d| d.parse().ok()).unwrap_or(0),
            style,
            params: params.clone(),
        }
    }

    /// The setting behind item `index`, if it is one.
    fn setting<'e>(env: &Env<'e>, id: &Id) -> Option<&'e SettingSpec> {
        // Found by its id, not by going through every setting.
        env.settings.spec(id.as_str()).filter(|spec| spec.id == *id)
    }

    /// Changes a setting by one step.
    fn step(&self, env: &Env<'_>, spec: &SettingSpec, step: i64, cx: &mut Cx<'_, '_>) {
        if let Some(value) = ui::next_value(env, spec, step)
            && Some(&value) != env.settings.get(spec.id.as_str())
        {
            cx.command(Command::SetSetting(spec.id.clone(), value));
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// Chooses item `index`: runs an action, or changes a setting.
    fn run(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(item) = self.items.get(index).filter(|i| !i.heading) else {
            return;
        };
        // Shown faint by its plugin: nothing to do now.
        if !env.menus.state(&env, item.id.as_str()).enabled {
            return;
        }
        let id = &item.id;
        if is_menu(&env, id) {
            cx.command(Command::OpenScreen(
                SCREEN,
                Params::new()
                    .with(params::MENU, id.as_str().to_string())
                    .with(DEPTH, alloc::format!("{}", self.depth + 1)),
            ));
            return;
        }
        let Some(spec) = Self::setting(&env, id) else {
            // Closes this menu and those below it, so the action reaches
            // the screen the menu was opened on.
            for _ in 0..=self.depth {
                cx.command(Command::Back);
            }
            cx.command(Command::RunAction(id.clone()));
            return;
        };
        // Its editor, the keyboard for text, the list of its values.
        if let Some(command) = ui::setting_opens(&env, spec) {
            cx.command(command);
            return;
        }
        match spec.kind {
            SettingKind::Int { .. } => {
                self.editing = !self.editing;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            _ => self.step(&env, spec, 1, cx),
        }
    }

    /// Draws entry `item` in `rect`: its icon, label and value.
    fn draw_entry(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        (item, laid): (&Item, Option<&(ui::RowLines, String)>),
        rect: Rect,
        focused: bool,
    ) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let state = env.menus.state(env, item.id.as_str());
        let ink = match state.enabled {
            true => style.focus_ink(focused),
            false => style.faint(60),
        };
        // The line under it, below the row.
        let rect = match &item.description {
            Some(key) => {
                let row = laid.map_or(style.row as u16, |(l, _)| l.height);
                let row = row.min(rect.height);
                let below = Rect::new(rect.x, rect.y + row, rect.width, rect.height - row);
                let x = f32::from(below.x) + style.margin as f32 + f32::from(self.icon_width);
                let width = f32::from(below.width) - x - style.margin as f32;
                let text = ui::fit(&font, style.small, env.i18n.tr(key), width);
                ui::draw_label(canvas, &font, style.small, x, below, &text, style.faint(80));
                Rect::new(rect.x, rect.y, rect.width, row)
            }
            None => rect,
        };
        // The label and value right of the icons.
        let rect = ui::draw_row_icon(
            canvas,
            env,
            &style,
            rect,
            self.icon_width,
            item.icon.as_deref(),
            ink,
        );
        let label = state
            .label
            .as_deref()
            .unwrap_or_else(|| env.i18n.tr(&item.title));
        // As its plugin says it is now: a switch, a badge.
        if let Some(on) = state.checked {
            ui::draw_switch_row(canvas, env, &style, rect, focused, label, "", on);
            return;
        }
        if let Some(badge) = &state.badge {
            ui::draw_value_row(canvas, env, &style, rect, focused, label, badge);
            return;
        }
        let (lines, room) = match laid {
            Some((lines, room)) => (lines.at(rect), room.as_str()),
            None => (ui::RowLines::one(rect), ""),
        };
        let number = |spec: &SettingSpec| matches!(spec.kind, SettingKind::Int { .. });
        match Self::setting(env, &item.id) {
            Some(spec) if matches!(spec.kind, SettingKind::Bool) => {
                let on = env.settings.bool(spec.id.as_str());
                ui::draw_switch_lines(canvas, env, &style, rect, lines, focused, label, "", on);
            }
            Some(spec) if focused && number(spec) => {
                let value = ui::setting_value(env, spec);
                let editing = self.editing;
                ui::draw_stepper_lines(canvas, env, &style, lines, label, &value, room, editing);
            }
            // On touch screens − and + are always there.
            Some(spec) if env.device.input.touch && number(spec) => {
                let value = ui::setting_value(env, spec);
                ui::draw_number_lines(canvas, env, &style, lines, label, &value, room, None);
            }
            Some(spec) => {
                let mut value = ui::setting_value(env, spec);
                if ui::opens_screen(env, spec) {
                    value.push_str(" ›");
                }
                ui::draw_setting_row(canvas, env, &style, rect, lines, focused, label, &value, "");
            }
            None if is_menu(env, &item.id) => {
                ui::draw_value_row(canvas, env, &style, rect, focused, label, "›");
            }
            None => {
                let x = f32::from(rect.x) + style.margin as f32;
                let width = f32::from(rect.width) - 2.0 * style.margin as f32;
                let label = ui::fit(&font, style.text, label, width);
                ui::draw_label(canvas, &font, style.text, x, rect, &label, ink);
            }
        }
    }
}

/// The height of the line under an entry.
fn description_height(env: &Env<'_>, style: &UiStyle) -> u16 {
    style.font(env).map_or(style.small as u16, |font| {
        font.line_metrics(style.small).line_height as u16
    })
}

/// Whether the menu in `params` opens as tiles: the user wants them, it
/// is a menu of its own (not a submenu) and every entry has an icon.
fn tiles(env: &Env<'_>, params: &Params) -> bool {
    if env.settings.read(&STYLE) != STYLE_TILES || params.get(DEPTH).is_some() {
        return false;
    }
    let items = env
        .menus
        .items(params.get(params::MENU).unwrap_or_default());
    let entries: Vec<_> = items.iter().filter(|i| !i.section).collect();
    !entries.is_empty() && entries.iter().all(|i| i.icon.is_some())
}

/// Whether `id` names a menu with entries (a submenu).
pub(crate) fn is_menu(env: &Env<'_>, id: &Id) -> bool {
    !env.menus.items(id.as_str()).is_empty()
}

impl Screen for MenuScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        canvas.fill(self.region, Color::WHITE);
        let border = Rect::new(
            self.region.x,
            self.region.y,
            self.region.width,
            style.line.max(1) as u16,
        );
        canvas.fill(border, Color::BLACK);
        if style.font(env).is_none() {
            return;
        }
        if self.items.is_empty() {
            ui::draw_message(
                canvas,
                env,
                &style,
                self.rows.area(),
                env.i18n.tr("menu.empty"),
            );
            return;
        }
        let items = &self.items;
        self.rows
            .draw(canvas, &style, self.keys, |canvas, index, rect, focused| {
                let item = &items[index];
                if item.heading {
                    ui::draw_heading(canvas, env, &style, rect, env.i18n.tr(&item.title));
                } else {
                    let laid = self.lines.get(index).and_then(Option::as_ref);
                    self.draw_entry(canvas, env, (item, laid), rect, focused);
                }
            });
        if let Some(pager) = &self.pager {
            pager.draw(canvas, env, &style, self.rows.paging());
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if self.editing {
            let env = *cx.env();
            let spec = self
                .items
                .get(self.rows.focus())
                .and_then(|item| Self::setting(&env, &item.id));
            let decrease = [
                actions::LEFT,
                actions::DOWN,
                actions::PREV_PAGE,
                actions::PREVIOUS,
            ];
            let increase = [
                actions::RIGHT,
                actions::UP,
                actions::NEXT_PAGE,
                actions::NEXT,
            ];
            match spec {
                Some(spec) if decrease.contains(a) => self.step(&env, spec, -1, cx),
                Some(spec) if increase.contains(a) => self.step(&env, spec, 1, cx),
                _ if *a == actions::CONFIRM || *a == actions::BACK => {
                    self.editing = false;
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                }
                _ => return Handled::No,
            }
            return Handled::Yes;
        }
        // Directions, not pages: turning pages (also by swiping) keeps the
        // focus hidden on touch screens.
        let navigation = [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
        ];
        if navigation.contains(a) && !self.keys {
            // The first button press shows where the focus is.
            self.keys = true;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return Handled::Yes;
        }
        // Entries faint now are passed.
        self.rows.set_passed(passed(cx.env(), &self.items));
        // The focus changes nothing but its rows: only they are drawn.
        if *a == actions::UP || *a == actions::LEFT || *a == actions::PREVIOUS {
            let change = self.rows.step(false);
            self.rows.request(change, cx);
        } else if *a == actions::DOWN || *a == actions::RIGHT || *a == actions::NEXT {
            let change = self.rows.step(true);
            self.rows.request(change, cx);
        } else if *a == actions::NEXT_PAGE {
            let change = self.rows.turn(true);
            self.rows.request(change, cx);
        } else if *a == actions::PREV_PAGE {
            let change = self.rows.turn(false);
            self.rows.request(change, cx);
        } else if *a == actions::CONFIRM {
            self.run(self.rows.focus(), cx);
        } else if *a == actions::MENU {
            // The menu button toggles the menu.
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.pager.and_then(|pager| pager.tap(gesture.start)) {
            let forward = action == actions::NEXT_PAGE;
            self.rows.turn(forward).request(cx);
            return Handled::Yes;
        }
        let Some(index) = self.rows.hit(gesture.start) else {
            // Taps between the items do nothing; outside, they close it.
            return Handled::Yes;
        };
        let env = *cx.env();
        let number = self.items.get(index).and_then(|item| {
            Self::setting(&env, &item.id).filter(|s| matches!(s.kind, SettingKind::Int { .. }))
        });
        let Some(spec) = number else {
            self.rows.set_focus(index);
            self.editing = false;
            self.run(index, cx);
            return Handled::Yes;
        };
        // On touch screens − and + are always there and act at once;
        // with buttons the first tap selects a number.
        let touch = env.device.input.touch;
        if !touch && (index != self.rows.focus() || !self.keys) {
            self.rows.set_focus(index);
            self.keys = true;
            self.editing = false;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return Handled::Yes;
        }
        let Some(rect) = self.rows.rect_of(index) else {
            return Handled::Yes;
        };
        let rect = Rect::new(
            rect.x + self.icon_width,
            rect.y,
            rect.width.saturating_sub(self.icon_width),
            rect.height,
        );
        let value = ui::setting_value(&env, spec);
        if touch {
            self.rows.set_focus(index);
            self.editing = false;
        }
        let (line, room) = match self.lines.get(index).and_then(Option::as_ref) {
            Some((lines, room)) => (lines.at(rect).control, room.as_str()),
            None => (rect, ""),
        };
        if let Some((minus, plus)) = ui::value_stepper(&env, &self.style, line, room, &value) {
            let x = gesture.start.x;
            // A little room around the buttons: easier to hit.
            let slack = if touch {
                i32::from(self.style.gap as u16)
            } else {
                0
            };
            let within = |r: Rect| x >= i32::from(r.x) - slack && x < r.right() as i32 + slack;
            if within(minus) {
                self.step(&env, spec, -1, cx);
            } else if within(plus) {
                self.step(&env, spec, 1, cx);
            }
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        // Values shown in the menu may have changed; entries shown only
        // while a setting has a value come or go with it.
        if let Some(changed) = events::SETTING_CHANGED.read(event, params) {
            let env = *cx.env();
            let before = self.region;
            if env.settings.governs(changed.id.as_str()) {
                // Anew, the focus on the entry it was on (where it stays).
                let focused = self.items.get(self.rows.focus()).map(|i| i.id.clone());
                let (focus, keys) = (self.rows.focus(), self.keys);
                let opened = self.params.clone();
                *self = MenuScreen::new(&env, &opened);
                let at = focused
                    .and_then(|id| self.items.iter().position(|i| i.id == id))
                    .unwrap_or(focus.min(self.items.len().saturating_sub(1)));
                self.rows.set_focus(at);
                self.keys = keys;
            }
            // What it covered before too, should it have shrunk.
            cx.request_redraw_area(before.union(&self.region));
        }
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
