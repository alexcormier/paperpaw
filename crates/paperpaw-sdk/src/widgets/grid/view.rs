//! [`GridView`]: a grid of widgets and tools on a screen, edited in place.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt;

use super::{Cells, Geometry, Item, What, dragged, draw_tool};
use crate::builtin::{actions, params, slots};
use crate::event::Event;
use crate::ui::choice::{self, Picked};
use crate::ui::{self, UiStyle};
use crate::widgets::Widget;
use crate::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Point,
    PointerEvent, PointerPhase, Rect, RefreshMode,
};

/// Translation keys of an item's options: this, then the option.
const OPTIONS: &str = "core.grid.";

/// What a [`GridView`] did with an input, for its screen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GridResponse {
    /// Not the grid's: the screen may use it.
    Ignored,
    /// Done; nothing for the screen to do.
    Handled,
    /// The items were moved, sized or removed: keep them (every item
    /// with its place, those without room as they were).
    Changed(Vec<Item>),
    /// Editing began (`true`) or ended: lay the screen out again.
    Editing(bool),
    /// One of the screen's own buttons was chosen (see
    /// [`GridView::layout`]).
    Button(u16),
}

impl GridResponse {
    /// Whether the input was used.
    pub fn handled(&self) -> Handled {
        if *self == GridResponse::Ignored {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}

/// An item on the grid: where it is, and its widget, which keeps what it
/// shows.
struct Shown {
    item: Item,
    cells: Cells,
    area: Rect,
    widget: Option<Box<dyn Widget>>,
}

impl fmt::Debug for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shown")
            .field("item", &self.item)
            .field("cells", &self.cells)
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

/// What editing is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Choosing an item (tap, or the buttons' focus).
    Choose,
    /// The buttons move the item with the focus a cell at a time.
    Move,
    /// The buttons make it wider, narrower, taller or lower.
    Resize,
}

/// An item dragged with a finger: from where, and where it would go.
#[derive(Debug, Clone, Copy)]
struct Drag {
    index: usize,
    /// Its size is changed (dragged by the corner), not its place.
    resize: bool,
    start: Point,
    to: Cells,
    moved: bool,
}

/// Editing in place: the cells show, items are moved and sized by
/// dragging, or with the buttons through their options.
#[derive(Debug)]
struct Edit {
    mode: Mode,
    drag: Option<Drag>,
}

/// What a target does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Run(Command),
    /// An item, when editing: its index.
    Item(usize),
    /// One of the screen's own buttons.
    Button(u16),
}

/// A grid of widgets and tools on a screen of a plugin's own, edited in
/// place; see the [module documentation](super).
///
/// Its targets are the items' (the places in widgets that do something,
/// the tools; while editing, the items themselves), then the screen's
/// own buttons: taps and the buttons' focus reach them in that order.
/// An item's options are offered by a [`Choice`](crate::ui::choice::Choice)
/// screen the plugin registers; they come back as its event, which the
/// screen hands to [`on_event`](Self::on_event).
#[derive(Debug)]
pub struct GridView {
    style: UiStyle,
    geometry: Geometry,
    items: Vec<Shown>,
    /// Items without room, kept as they are.
    unplaced: Vec<Item>,
    buttons: Vec<(Rect, u16)>,
    /// Where taps and the buttons' focus go, in order.
    targets: Vec<(Rect, Target)>,
    /// The target with the focus, shown once buttons are used.
    focus: usize,
    focus_shown: bool,
    edit: Option<Edit>,
    labels: bool,
    choice: Id,
    chosen: Event<Picked>,
}

impl GridView {
    /// An empty grid, not edited; `choice` is the plugin's
    /// [`Choice`](crate::ui::choice::Choice) screen, `chosen` the event
    /// the options of an item come back with. Lay it out with
    /// [`layout`](Self::layout).
    pub fn new(env: &Env<'_>, choice: Id, chosen: Event<Picked>) -> Self {
        Self {
            style: UiStyle::new(env),
            geometry: Geometry::default(),
            items: Vec::new(),
            unplaced: Vec::new(),
            buttons: Vec::new(),
            targets: Vec::new(),
            focus: 0,
            focus_shown: !env.device.input.touch,
            edit: None,
            labels: true,
            choice,
            chosen,
        }
    }

    /// Whether tools show their names below their icons (they do at first).
    pub fn set_tool_labels(&mut self, on: bool) {
        self.labels = on;
    }

    /// Places `items` in `geometry` (those without room are kept, not
    /// shown), with the screen's own `buttons` after them (their area and
    /// a number the screen knows them by, see [`GridResponse::Button`]).
    /// The widgets are made anew, what they show read.
    pub fn layout(
        &mut self,
        env: &Env<'_>,
        geometry: Geometry,
        items: &[Item],
        buttons: &[(Rect, u16)],
    ) {
        self.style = UiStyle::new(env);
        let placed = super::place(env, &geometry, items);
        self.unplaced = items
            .iter()
            .filter(|i| !placed.iter().any(|(p, _)| p == *i))
            .cloned()
            .collect();
        self.items = placed
            .into_iter()
            .map(|(item, cells)| {
                let area = geometry.rect(cells);
                let widget = match &item.what {
                    What::Widget(id) => env.widgets.make(id, env).map(|mut widget| {
                        widget.place(env, area);
                        widget
                    }),
                    What::Tool(_) => None,
                };
                Shown {
                    widget,
                    area,
                    item,
                    cells,
                }
            })
            .collect();
        self.geometry = geometry;
        self.buttons = buttons.to_vec();
        self.target(env);
    }

    /// The targets of the items and the buttons as they are now.
    fn target(&mut self, env: &Env<'_>) {
        self.targets.clear();
        let editing = self.edit.is_some();
        for (index, shown) in self.items.iter().enumerate() {
            if editing {
                self.targets.push((shown.area, Target::Item(index)));
                continue;
            }
            match (&shown.item.what, &shown.widget) {
                (What::Widget(_), Some(widget)) => {
                    for target in widget.targets(env, shown.area) {
                        self.targets
                            .push((target.area, Target::Run(target.command)));
                    }
                }
                (What::Widget(_), None) => {}
                (What::Tool(action), _) => self
                    .targets
                    .push((shown.area, Target::Run(Command::RunAction(action.clone())))),
            }
        }
        for (area, button) in &self.buttons {
            self.targets.push((*area, Target::Button(*button)));
        }
        self.focus = self.focus.min(self.targets.len().saturating_sub(1));
    }

    /// The grid's cells and where they are.
    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// Lets go of the widgets and what they keep (e.g. covers made
    /// small), while the screen is hidden
    /// ([`Screen::on_hide`](crate::Screen::on_hide)): nothing of them is
    /// drawn until [`layout`](Self::layout) makes them anew.
    pub fn release(&mut self) {
        for shown in &mut self.items {
            shown.widget = None;
        }
    }

    /// Whether the grid is edited.
    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    /// Begins or ends editing (the focus to the first target); lay the
    /// screen out again after, its buttons may differ.
    pub fn set_editing(&mut self, on: bool) {
        self.edit = on.then_some(Edit {
            mode: Mode::Choose,
            drag: None,
        });
        self.focus = 0;
    }

    /// Whether the focus shows before the buttons are used (at first only
    /// on devices without touch); the first press shows it otherwise.
    pub fn set_focus_shown(&mut self, shown: bool) {
        self.focus_shown = shown;
    }

    /// Puts the focus on the first target (e.g. when another page shows).
    pub fn reset_focus(&mut self) {
        self.focus = 0;
    }

    /// The items with their places as they are now, those without room
    /// after them.
    pub fn items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .items
            .iter()
            .map(|s| Item::placed(s.item.what.clone(), s.cells))
            .collect();
        items.extend(self.unplaced.iter().cloned());
        items
    }

    /// Has the widgets read what they show anew (see
    /// [`Widget::update`]; call it about each minute and after events
    /// that may change them) and draws those whose content changed again;
    /// `true` if any did.
    pub fn update_widgets(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        self.update(env, cx, |_| true)
    }

    /// The same for the widgets that change by themselves alone
    /// ([`Widget::live`]): each minute, or when the device's status
    /// changed.
    pub fn update_live(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        let mut changed = false;
        for shown in &mut self.items {
            let Some(widget) = &mut shown.widget else {
                continue;
            };
            if widget.live() && widget.update(env) {
                cx.request_redraw_area(shown.area);
                changed = true;
            }
        }
        if changed && self.edit.is_none() {
            self.target(env);
        }
        changed
    }

    /// The same for the items showing widget `id` alone, e.g. once
    /// something they show was looked up.
    pub fn update_widget(&mut self, id: &str, env: &Env<'_>, cx: &mut Cx<'_, '_>) -> bool {
        self.update(env, cx, |shown| {
            crate::widgets::base(shown) == id || shown == id
        })
    }

    fn update(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>, which: impl Fn(&str) -> bool) -> bool {
        let mut changed = false;
        for shown in &mut self.items {
            let (What::Widget(id), Some(widget)) = (&shown.item.what, &mut shown.widget) else {
                continue;
            };
            if which(id) && widget.update(env) {
                cx.request_redraw_area(shown.area);
                changed = true;
            }
        }
        // Their targets may have changed with them.
        if changed && self.edit.is_none() {
            self.target(env);
        }
        changed
    }

    /// Draws the items (while editing: the cells, the items' frames and
    /// the corners that size them, where a dragged item would go) and the
    /// focus once buttons are used. The screen draws its buttons itself.
    pub fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let editing = self.edit.is_some();
        if editing {
            self.draw_cells(canvas);
        }
        let line = self.style.line.max(1);
        let clip = canvas.clip();
        for shown in &self.items {
            // Those outside the part drawn are left (e.g. under a menu).
            if shown.area.intersection(&clip).is_none() {
                continue;
            }
            canvas.fill(shown.area, Color::WHITE);
            match (&shown.item.what, &shown.widget) {
                (What::Widget(_), Some(widget)) => widget.draw(canvas, env, shown.area),
                (What::Widget(_), None) => {}
                (What::Tool(action), _) => {
                    draw_tool(canvas, env, shown.area, action, self.labels);
                }
            }
            if editing {
                // In the gap round it, not over what it shows (and apart
                // from its neighbour's).
                let pad = (self.style.gap / 2).saturating_sub(line) as u16;
                let around = Rect::new(
                    shown.area.x.saturating_sub(pad),
                    shown.area.y.saturating_sub(pad),
                    shown.area.width + 2 * pad,
                    shown.area.height + 2 * pad,
                );
                let corner = self.style.corner;
                canvas.stroke_round(around, line, corner + u32::from(pad), Color::BLACK);
                canvas.fill_round(handle(shown.area, &self.style), corner, Color::BLACK);
            }
        }
        if let Some(Edit {
            drag: Some(drag), ..
        }) = &self.edit
            && drag.moved
        {
            let width = if self.free(drag.index, drag.to) {
                self.style.line * 4
            } else {
                self.style.line
            };
            let to = self.geometry.rect(drag.to);
            canvas.stroke_round(to, width, self.style.corner, Color::BLACK);
        }
        if self.focus_shown
            && let Some((area, _)) = self.targets.get(self.focus)
        {
            // Round as the style's corners, a little rounder outside.
            let out = self.style.line * 2;
            let frame = area.grow(out as u16);
            let radius = self.style.corner + out;
            canvas.stroke_round(frame, self.style.line * 3, radius, Color::BLACK);
        }
    }

    /// The grid's cells, lightly.
    fn draw_cells(&self, canvas: &mut Canvas<'_>) {
        let g = self.geometry;
        for row in 0..g.rows {
            for col in 0..g.columns {
                let cell = g.rect(Cells::new(col, row, 1, 1));
                canvas.stroke_round(cell, 1, self.style.corner, Color::gray(200));
            }
        }
    }

    /// While editing, *Back* says it ends editing.
    pub fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if *action == actions::BACK && self.edit.is_some() {
            return Hint::Text(env.i18n.tr("core.grid.done").to_string());
        }
        Hint::Default
    }

    /// The area around a target that its focus frame takes.
    fn around(&self, area: Rect) -> Rect {
        area.grow((self.style.line * 4) as u16)
    }

    /// Moves the focus to `to`, redrawing both targets.
    fn focus_on(&mut self, to: usize, cx: &mut Cx<'_, '_>) {
        if let Some((area, _)) = self.targets.get(self.focus) {
            cx.request_redraw_area(self.around(*area));
        }
        self.focus = to;
        if let Some((area, _)) = self.targets.get(to) {
            cx.request_redraw_area(self.around(*area));
        }
        cx.request_refresh(RefreshMode::Fast);
    }

    /// The target nearest below (or above) the focused one.
    fn vertical(&self, down: bool) -> Option<usize> {
        let (from, _) = self.targets.get(self.focus)?;
        let cx = i32::from(from.x) + i32::from(from.width) / 2;
        self.targets
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| {
                if down {
                    r.y >= from.y + from.height
                } else {
                    r.y + r.height <= from.y
                }
            })
            .min_by_key(|(_, (r, _))| {
                let dy = (i32::from(r.y) - i32::from(from.y)).abs();
                let dx = (i32::from(r.x) + i32::from(r.width) / 2 - cx).abs();
                dy * 4 + dx
            })
            .map(|(i, _)| i)
    }

    fn run(&mut self, index: usize, cx: &mut Cx<'_, '_>) -> GridResponse {
        let Some((_, target)) = self.targets.get(index).cloned() else {
            return GridResponse::Handled;
        };
        match target {
            Target::Run(command) => cx.command(command),
            Target::Item(item) => self.options(item, cx),
            Target::Button(button) => return GridResponse::Button(button),
        }
        GridResponse::Handled
    }

    /// A placement of widget `id` (`id@n`) no item here and no values set
    /// so far use.
    fn place(&self, env: &Env<'_>, id: &str) -> alloc::string::String {
        let base = Id::parse(id).unwrap_or_else(|_| Id::new_static("core"));
        (1..)
            .map(|n| base.placement(n))
            .find(|placed| {
                let used = self
                    .items
                    .iter()
                    .any(|i| matches!(&i.item.what, What::Widget(w) if w == placed.as_str()));
                !used && env.settings.placement_values(placed.as_str()).is_empty()
            })
            .map_or_else(|| id.into(), |placed| placed.as_str().into())
    }

    /// Offers what can be done with item `index`.
    fn options(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(shown) = self.items.get(index) else {
            return;
        };
        let mut options = alloc::vec!["move", "resize"];
        if let What::Widget(id) = &shown.item.what
            && crate::widgets::has_settings(&env, id)
        {
            options.push("settings");
        }
        options.push("remove");
        cx.command(choice::open(
            self.choice.clone(),
            &self.chosen,
            &shown.item.what.title(&env),
            &format!("{index}"),
            OPTIONS,
            &options,
        ));
    }

    /// Whether item `index` may take the cells `to`.
    fn free(&self, index: usize, to: Cells) -> bool {
        to.fits(self.geometry.columns, self.geometry.rows)
            && !self
                .items
                .iter()
                .enumerate()
                .any(|(i, s)| i != index && s.cells.overlaps(&to))
    }

    /// Puts item `index` at `to` if it may go there; `true` if it did.
    fn set_cells(&mut self, index: usize, to: Cells, cx: &mut Cx<'_, '_>) -> bool {
        if !self.free(index, to) {
            return false;
        }
        let Some(shown) = self.items.get(index) else {
            return false;
        };
        cx.request_redraw_area(self.around(shown.area));
        let area = self.geometry.rect(to);
        cx.request_redraw_area(self.around(area));
        let shown = &mut self.items[index];
        shown.cells = to;
        shown.area = area;
        if let Some(widget) = &mut shown.widget {
            let env = *cx.env();
            widget.place(&env, area);
        }
        if let Some(target) = self.targets.get_mut(index) {
            target.0 = area;
        }
        true
    }

    /// The item at a point, and whether at its corner (to size it).
    fn item_at(&self, point: Point) -> Option<(usize, bool)> {
        let index = self.items.iter().position(|s| s.area.contains(point))?;
        let corner = handle(self.items[index].area, &self.style).contains(point);
        Some((index, corner))
    }

    /// Ends moving or sizing with the buttons.
    fn end_mode(&mut self, cx: &mut Cx<'_, '_>) -> GridResponse {
        if let Some(edit) = &mut self.edit {
            edit.mode = Mode::Choose;
        }
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
        GridResponse::Changed(self.items())
    }

    /// An item's option chosen (the event given to the plugin's choice
    /// screen), or [`GridResponse::Ignored`] for other events.
    pub fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) -> GridResponse {
        let Some(picked) = self.chosen.read(event, params) else {
            return GridResponse::Ignored;
        };
        let Some(index) = picked
            .path
            .parse::<usize>()
            .ok()
            .filter(|i| *i < self.items.len())
        else {
            return GridResponse::Handled;
        };
        let env = *cx.env();
        match picked.option.as_str() {
            option @ ("move" | "resize") => {
                if let Some(edit) = &mut self.edit {
                    edit.mode = if option == "move" {
                        Mode::Move
                    } else {
                        Mode::Resize
                    };
                }
                self.focus = index;
                self.focus_shown = true;
                let message = env.i18n.tr(&format!("{OPTIONS}{option}.help")).to_string();
                cx.command(Command::Notify(message));
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            "settings" => {
                let What::Widget(id) = self.items[index].item.what.clone() else {
                    return GridResponse::Handled;
                };
                // Each placement has settings of its own: one placed before
                // there were placements gets its number first.
                let placed = if id.contains('@') {
                    id
                } else {
                    self.place(&env, &id)
                };
                let params = Params::new().with(params::ID, placed.clone());
                cx.command(Command::OpenSlot(slots::SETTINGS, params));
                if self.items[index].item.what != What::Widget(placed.clone()) {
                    self.items[index].item.what = What::Widget(placed);
                    return GridResponse::Changed(self.items());
                }
            }
            "remove" => {
                // What the placement set goes with it.
                if let What::Widget(id) = &self.items[index].item.what
                    && id.contains('@')
                {
                    for value in env.settings.placement_values(id) {
                        cx.command(Command::ResetSetting(value));
                    }
                }
                self.items.remove(index);
                self.target(&env);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                return GridResponse::Changed(self.items());
            }
            _ => {}
        }
        GridResponse::Handled
    }

    /// Buttons: the focus goes from target to target (left and right in
    /// order, up and down to the nearest), confirm runs it; the first
    /// press shows where the focus is. While an item is moved or sized,
    /// they move or size it. *Back* ends editing. The page buttons are
    /// left to the screen (except while an item is moved or sized, where
    /// they go up and down where there are no such buttons).
    pub fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> GridResponse {
        let a = action;
        if let Some(Edit { mode, .. }) = self.edit
            && mode != Mode::Choose
            && let Some(Target::Item(index)) = self.targets.get(self.focus).map(|t| t.1.clone())
        {
            return self.move_item(mode, index, a, cx);
        }
        if *a == actions::BACK && self.edit.is_some() {
            self.set_editing(false);
            return GridResponse::Editing(false);
        }
        let moves = [
            actions::UP,
            actions::DOWN,
            actions::LEFT,
            actions::RIGHT,
            actions::NEXT,
            actions::PREVIOUS,
            actions::CONFIRM,
        ];
        if !moves.contains(a) {
            return GridResponse::Ignored;
        }
        if !self.focus_shown {
            // The first press shows where the focus is.
            self.focus_shown = true;
            self.focus_on(self.focus, cx);
            return GridResponse::Handled;
        }
        let len = self.targets.len();
        let to = if *a == actions::CONFIRM {
            return self.run(self.focus, cx);
        } else if *a == actions::RIGHT || *a == actions::NEXT {
            ui::step_focus(self.focus, 1, len)
        } else if *a == actions::LEFT || *a == actions::PREVIOUS {
            ui::step_focus(self.focus, -1, len)
        } else {
            match self.vertical(*a == actions::DOWN) {
                Some(to) => to,
                None => return GridResponse::Handled,
            }
        };
        self.focus_on(to, cx);
        GridResponse::Handled
    }

    /// Moving or sizing item `index` with the buttons.
    fn move_item(&mut self, mode: Mode, index: usize, a: &Id, cx: &mut Cx<'_, '_>) -> GridResponse {
        if *a == actions::CONFIRM || *a == actions::BACK {
            return self.end_mode(cx);
        }
        let Some(c) = self.items.get(index).map(|s| s.cells) else {
            return GridResponse::Handled;
        };
        let step = |n: u8, more: bool| {
            if more {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1)
            }
        };
        let left = *a == actions::LEFT || *a == actions::PREVIOUS;
        let right = *a == actions::RIGHT || *a == actions::NEXT;
        // Up and down, or the page buttons where there are none (the
        // X4's side buttons).
        let up = *a == actions::UP || *a == actions::PREV_PAGE;
        let down = *a == actions::DOWN || *a == actions::NEXT_PAGE;
        let to = match mode {
            Mode::Move if left || right => Cells {
                col: step(c.col, right),
                ..c
            },
            Mode::Move if up || down => Cells {
                row: step(c.row, down),
                ..c
            },
            Mode::Resize if left || right => Cells {
                w: step(c.w, right).max(1),
                ..c
            },
            Mode::Resize if up || down => Cells {
                h: step(c.h, down).max(1),
                ..c
            },
            _ => return GridResponse::Ignored,
        };
        if to != c {
            if self.set_cells(index, to, cx) {
                cx.request_refresh(RefreshMode::Fast);
            } else if to.fits(self.geometry.columns, self.geometry.rows) {
                // Within the grid, but another item is in the way: said,
                // so the press does not seem lost.
                let env = *cx.env();
                cx.command(Command::Notify(
                    env.i18n.tr("core.grid.in-the-way").to_string(),
                ));
            }
        }
        GridResponse::Handled
    }

    /// While editing, the edges are the grid's: items at them are dragged
    /// and sized, not the screen left (for the screen's
    /// [`holds_edges`](crate::Screen::holds_edges)).
    pub fn holds_edges(&self) -> bool {
        self.edit.is_some()
    }

    /// While editing, a finger on an item drags it (by its corner: sizes
    /// it); lifted where it was, it offers the item's options.
    pub fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> GridResponse {
        if self.edit.is_none() {
            return GridResponse::Ignored;
        }
        match event.phase {
            PointerPhase::Down => {
                let Some((index, resize)) = self.item_at(event.point) else {
                    return GridResponse::Ignored;
                };
                let to = self.items[index].cells;
                if let Some(edit) = &mut self.edit {
                    edit.drag = Some(Drag {
                        index,
                        resize,
                        start: event.point,
                        to,
                        moved: false,
                    });
                }
                GridResponse::Handled
            }
            PointerPhase::Move => {
                let Some(mut drag) = self.edit.as_ref().and_then(|e| e.drag) else {
                    return GridResponse::Handled;
                };
                let Some(from) = self.items.get(drag.index).map(|s| s.cells) else {
                    return GridResponse::Handled;
                };
                let Some(to) = dragged(&self.geometry, from, drag.start, event.point, drag.resize)
                else {
                    return GridResponse::Handled;
                };
                if to != drag.to || !drag.moved {
                    cx.request_redraw_area(self.around(self.geometry.rect(drag.to)));
                    drag.to = to;
                    drag.moved |= to != from;
                    cx.request_redraw_area(self.around(self.geometry.rect(to)));
                    cx.request_refresh(RefreshMode::Fast);
                    if let Some(edit) = &mut self.edit {
                        edit.drag = Some(drag);
                    }
                }
                GridResponse::Handled
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                let Some(drag) = self.edit.as_mut().and_then(|e| e.drag.take()) else {
                    return GridResponse::Handled;
                };
                if !drag.moved {
                    // A tap: the item gets the focus and offers its options.
                    let target = self
                        .targets
                        .iter()
                        .position(|t| t.1 == Target::Item(drag.index));
                    if let Some(target) = target {
                        self.focus = target;
                        self.focus_shown = true;
                    }
                    if event.phase == PointerPhase::Up {
                        self.options(drag.index, cx);
                    }
                    cx.request_redraw();
                    cx.request_refresh(RefreshMode::Fast);
                    return GridResponse::Handled;
                }
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
                if event.phase == PointerPhase::Up && self.set_cells(drag.index, drag.to, cx) {
                    return GridResponse::Changed(self.items());
                }
                GridResponse::Handled
            }
        }
    }

    /// A tap runs the target there; a long press begins editing.
    pub fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> GridResponse {
        match gesture.kind {
            GestureKind::Tap => match self
                .targets
                .iter()
                .position(|(r, _)| r.contains(gesture.start))
            {
                Some(index) => {
                    self.focus = index;
                    self.run(index, cx)
                }
                None => GridResponse::Ignored,
            },
            GestureKind::LongPress if self.edit.is_none() => {
                self.set_editing(true);
                GridResponse::Editing(true)
            }
            _ => GridResponse::Ignored,
        }
    }
}

/// The corner of an item that sizes it when dragged.
fn handle(area: Rect, style: &UiStyle) -> Rect {
    let side = (style.row / 2).max(16) as u16;
    Rect::new(
        (area.x + area.width).saturating_sub(side),
        (area.y + area.height).saturating_sub(side),
        side,
        side,
    )
}
