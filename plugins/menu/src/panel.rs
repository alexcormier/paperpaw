//! A menu shown as a panel of controls at the bottom of the screen, for
//! quick changes while reading: switches become buttons (several to a
//! row), numbers get `−` and `+` (those marked as sliders, such as the
//! frontlight, a slider between them), choices with few options become
//! segments,
//! other choices open their list, actions become buttons. Changes apply at
//! once and the panel stays open.
//!
//! Bound to the `core.panel` slot with the parameter `menu`; what the
//! panel shows is that menu, so users and plugins choose its entries like
//! any menu's. With the parameter `mode` set to `top` it hangs from the
//! top of the screen instead (the quick panel).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params, slots};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, MenuItem, Params, Rect,
    RefreshMode, Screen, SettingKind, SettingSpec, SettingValue,
};

use crate::{CHOOSE_SCREEN, is_menu};
use paperpaw_sdk::setting::Setting;

/// Most switches or buttons in one row.
const PER_ROW: usize = 3;
/// Most options shown as segments; more open a list.
const MAX_SEGMENTS: usize = 4;
/// Most of the screen the panel may cover.
const MAX_SHARE: f32 = 0.8;

/// What tapping a control does.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Control {
    /// Switches a setting on or off.
    Toggle(Id),
    /// Changes a number by a step down (`-1`) or up (`1`).
    Step(Id, i64),
    /// A number's slider: a tap sets the value where it lands.
    Slide(Id),
    /// Sets a choice to this option.
    Option(Id, String),
    /// Opens the setting's list or editor.
    Open(Id),
    /// Runs an action or opens a submenu.
    Run(Id),
}

/// A row: an optional label on the left, then its controls.
#[derive(Debug)]
struct Row {
    area: Rect,
    /// The setting the label names (a number's or a choice's).
    label: Option<Id>,
    controls: Vec<(Control, Rect)>,
}

pub(crate) struct PanelScreen {
    rows: Vec<Row>,
    /// It hangs from the top of the screen.
    top: bool,
    /// Where the time, the date and the battery are, hanging from the top.
    head: Rect,
    style: UiStyle,
    region: Rect,
    /// The control chosen with buttons: its place in reading order.
    focus: Option<usize>,
    /// Rows shown at a time; more go to further pages.
    per_page: usize,
    page: usize,
    pager: Option<ui::Pager>,
}

fn spec<'e>(env: &Env<'e>, id: &Id) -> Option<&'e SettingSpec> {
    env.settings.specs().find(|spec| spec.id == *id)
}

/// What a row of the panel shows, before it is placed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    /// Switches or buttons side by side.
    Buttons(Vec<Control>),
    /// A number with a slider between its `−` and `+`.
    Slider(Id),
    /// A number's `−` and `+`, placed by its value when drawn.
    Stepper(Id),
    /// A choice's options side by side.
    Segments(Id, Vec<String>),
    /// A setting opened in its list or editor.
    Open(Id),
}

/// The sizes the panel is laid out with.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    /// The screen's.
    width: u16,
    height: u16,
    margin: u16,
    gap: u16,
    row: u16,
    border: u16,
    /// The labels of settings, on the left of their rows.
    label: u16,
    /// The values of sliders, between the track and the `+`.
    value: u16,
    /// The head with the time, the date and the battery (hanging from
    /// the top only).
    head: u16,
    /// The bar that turns pages, where there are several.
    pager: u16,
    /// It hangs from the top of the screen.
    top: bool,
}

/// Where the panel's parts go.
#[derive(Debug)]
struct Layout {
    rows: Vec<Row>,
    region: Rect,
    head: Rect,
    per_page: usize,
    pager: Option<Rect>,
}

/// Places `lines` on the screen: at its bottom (or top), at most
/// [`MAX_SHARE`] of it high, in pages if they need more.
fn layout(lines: &[Line], g: Geometry) -> Layout {
    let gap = g.gap;
    let room = (f32::from(g.height) * MAX_SHARE) as u16 - g.border - 2 * gap;
    let max_rows = usize::from(room / g.row.max(1)).max(1);
    // Too many rows: pages, with a bar to turn them.
    let paged = lines.len() > max_rows;
    let bar = if paged { g.pager } else { 0 };
    let per_page = if paged {
        usize::from((room - bar) / g.row.max(1)).max(1)
    } else {
        lines.len().max(1)
    };
    let head = if g.top { g.head } else { 0 };
    let height = g.border + 2 * gap + head + g.row * per_page as u16 + bar;
    let region = if g.top {
        Rect::new(0, 0, g.width, height)
    } else {
        Rect::new(0, g.height - height, g.width, height)
    };
    // The border on the side towards the rest of the screen.
    let inner_top = if g.top {
        region.y + head
    } else {
        region.y + g.border
    };
    let rows = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let y = inner_top + gap + g.row * (index % per_page) as u16;
            place(line, Rect::new(0, y, g.width, g.row), g)
        })
        .collect();
    let pager = paged.then(|| {
        let bottom = if g.top { g.border } else { 0 };
        Rect::new(0, region.y + region.height - bar - bottom, g.width, bar)
    });
    Layout {
        rows,
        region,
        head: Rect::new(
            g.margin,
            region.y + gap / 2,
            g.width.saturating_sub(2 * g.margin),
            head,
        ),
        per_page,
        pager,
    }
}

/// The controls of `line` in its row, `area`.
fn place(line: &Line, area: Rect, g: Geometry) -> Row {
    let inner = inset(area, g.margin);
    let rest = Rect::new(
        inner.x + g.label,
        inner.y,
        inner.width.saturating_sub(g.label),
        inner.height,
    );
    let (label, controls) = match line {
        Line::Buttons(controls) => {
            let cells = split(inner, controls.len(), g.gap / 2);
            (None, controls.iter().cloned().zip(cells).collect())
        }
        // A wide range: − slider value +.
        Line::Slider(id) => {
            let size = ui::button_height(area);
            let y = area.y + (area.height - size) / 2;
            let minus = Rect::new(rest.x, y, size, size);
            let plus = Rect::new(rest.x + rest.width - size, y, size, size);
            let value = if g.value > 0 { g.value + g.gap } else { 0 };
            let track = Rect::new(
                minus.x + size + g.gap,
                y,
                rest.width.saturating_sub(2 * (size + g.gap) + value),
                size,
            );
            let controls = alloc::vec![
                (Control::Step(id.clone(), -1), minus),
                (Control::Slide(id.clone()), track),
                (Control::Step(id.clone(), 1), plus),
            ];
            (Some(id.clone()), controls)
        }
        // The − and + are placed by the value, as in menus.
        Line::Stepper(id) => {
            let controls = alloc::vec![
                (Control::Step(id.clone(), -1), area),
                (Control::Step(id.clone(), 1), area),
            ];
            (Some(id.clone()), controls)
        }
        Line::Segments(id, options) => {
            let cells = split(rest, options.len(), g.gap / 2);
            let controls = options
                .iter()
                .zip(cells)
                .map(|(o, r)| (Control::Option(id.clone(), o.clone()), r))
                .collect();
            (Some(id.clone()), controls)
        }
        Line::Open(id) => (
            Some(id.clone()),
            alloc::vec![(Control::Open(id.clone()), inner)],
        ),
    };
    Row {
        area,
        label,
        controls,
    }
}

impl PanelScreen {
    pub(crate) fn open(env: &Env<'_>, params: &Params) -> Self {
        let menu = params.get(params::MENU).unwrap_or_default();
        let top = params.get(params::MODE) == Some("top");
        let style = UiStyle::new(env);
        let display = env.device.display;
        let width = display.width;
        let margin = style.margin as u16;
        let items = env.menus.items(menu);
        // Labels as wide as the longest needs, at most half the row.
        let font = style.font(env);
        let measure = |text: &str| font.as_ref().map_or(0.0, |f| f.measure(style.text, text));
        let longest = items
            .iter()
            .filter_map(|item| spec(env, item.id))
            .filter(|s| !matches!(s.kind, SettingKind::Bool))
            .map(|s| measure(env.i18n.tr(&s.title)))
            .fold(0.0, f32::max);
        let label =
            ((longest as u16).saturating_add(style.gap as u16 * 2)).min((width - 2 * margin) / 2);
        // Sliders' values as wide as the widest of them needs.
        let sliders = items
            .iter()
            .filter_map(|item| spec(env, item.id))
            .filter(|s| ui::slider::shows(env, s));
        let value = measure(&ui::number_room(env, &style, sliders)) as u16;
        let head = if top {
            let size = env
                .metrics
                .pt(env.settings.read(&QUICK_SIZE).clamp(6, 24) as f32);
            (size * 1.6) as u16 + style.gap as u16
        } else {
            0
        };
        let geometry = Geometry {
            width,
            height: display.height,
            margin,
            gap: style.gap as u16,
            row: style.row as u16,
            border: (style.line * 2).max(2) as u16,
            label,
            value,
            head,
            pager: ui::Pager::height(&style),
            top,
        };
        let layout = layout(&lines(env, &style, &items, geometry), geometry);
        Self {
            rows: layout.rows,
            style,
            region: layout.region,
            top,
            head: layout.head,
            focus: (!env.device.input.touch).then_some(0),
            per_page: layout.per_page,
            page: 0,
            pager: layout.pager.map(|area| ui::Pager::new(env, area)),
        }
    }
}

/// What the panel shows of `items`, row by row: switches and buttons
/// several to a row (as many as stay readable; switches and buttons not
/// mixed), numbers with their `−` and `+` (a slider between for those
/// marked so, where it has room), choices with few options as segments,
/// anything else to open.
fn lines(env: &Env<'_>, style: &UiStyle, items: &[MenuItem<'_>], g: Geometry) -> Vec<Line> {
    let font = style.font(env);
    let label_of = |control: &Control| -> String {
        match control {
            Control::Toggle(id) => spec(env, id).map_or(String::new(), |s| {
                alloc::format!("• {}", env.i18n.tr(&s.title))
            }),
            Control::Run(id) => {
                let title = env
                    .actions
                    .get(id.as_str())
                    .map_or(id.as_str(), |a| a.title.as_ref());
                env.i18n.tr(title).to_string()
            }
            _ => String::new(),
        }
    };
    let small = |text: &str| font.as_ref().map_or(0.0, |f| f.measure(style.small, text));
    // As many buttons to a row as stay readable.
    let per_row = |group: &[Control]| -> usize {
        let inner = f32::from(g.width - 2 * g.margin);
        (1..=PER_ROW.min(group.len()))
            .rev()
            .find(|&n| {
                let cell = inner / n as f32 - 2.0 * g.gap as f32;
                group.iter().all(|c| small(&label_of(c)) <= cell)
            })
            .unwrap_or(1)
    };
    let flush = |group: &mut Vec<Control>, lines: &mut Vec<Line>| {
        if group.is_empty() {
            return;
        }
        let n = per_row(group);
        for chunk in core::mem::take(group).chunks(n) {
            lines.push(Line::Buttons(chunk.to_vec()));
        }
    };
    // Where a setting's controls go, beside its label.
    let inner = inset(Rect::new(0, 0, g.width, g.row), g.margin);
    let rest = Rect::new(
        inner.x + g.label,
        inner.y,
        inner.width.saturating_sub(g.label),
        inner.height,
    );
    let mut lines = Vec::new();
    let mut group: Vec<Control> = Vec::new();
    for item in items {
        let id = item.id.clone();
        let setting = spec(env, &id).filter(|s| s.extra().editor.is_none());
        let control = match setting.map(|s| &s.kind) {
            Some(SettingKind::Bool) => Some(Control::Toggle(id.clone())),
            None if spec(env, &id).is_none() => Some(Control::Run(id.clone())),
            _ => None,
        };
        if let Some(control) = control {
            // Switches and buttons share rows, but not with each other.
            let same = group.first().is_some_and(|first| {
                matches!(first, Control::Toggle(_)) == matches!(control, Control::Toggle(_))
            });
            if !group.is_empty() && (!same || group.len() == PER_ROW) {
                flush(&mut group, &mut lines);
            }
            group.push(control);
            continue;
        }
        flush(&mut group, &mut lines);
        let line = match spec(env, &id).map(|s| &s.kind) {
            Some(SettingKind::Int { .. })
                if setting.is_some_and(|s| ui::slider::shows(env, s)) && slider_fits(rest, g) =>
            {
                Line::Slider(id)
            }
            Some(SettingKind::Int { .. }) if setting.is_some() => Line::Stepper(id),
            Some(SettingKind::Choice(options))
                if setting.is_some() && fits_segments(env, style, options, rest) =>
            {
                Line::Segments(id, options.iter().map(|o| o.to_string()).collect())
            }
            _ => Line::Open(id),
        };
        lines.push(line);
    }
    flush(&mut group, &mut lines);
    lines
}

impl PanelScreen {
    fn pages(&self) -> usize {
        self.rows.len().div_ceil(self.per_page).max(1)
    }

    /// The rows of the page shown, with the number of controls before them.
    fn visible_rows(&self) -> impl Iterator<Item = (usize, &Row)> {
        let first = self.page * self.per_page;
        let before: usize = self.rows[..first.min(self.rows.len())]
            .iter()
            .map(|r| r.controls.len())
            .sum();
        self.rows
            .iter()
            .skip(first)
            .take(self.per_page)
            .scan(before, |count, row| {
                let at = *count;
                *count += row.controls.len();
                Some((at, row))
            })
    }

    /// The page with control `index`.
    fn page_of(&self, index: usize) -> usize {
        let mut count = 0;
        for (row_index, row) in self.rows.iter().enumerate() {
            count += row.controls.len();
            if index < count {
                return row_index / self.per_page;
            }
        }
        0
    }

    /// Every control in reading order.
    fn controls(&self) -> impl Iterator<Item = &(Control, Rect)> {
        self.rows.iter().flat_map(|row| row.controls.iter())
    }

    /// Where the controls of `row` are now: a number's `−` and `+` follow
    /// its value's width.
    fn placed(&self, env: &Env<'_>, row: &Row) -> Vec<(Control, Rect)> {
        let steps = row.label.as_ref().and_then(|id| spec(env, id)).filter(|s| {
            matches!(s.kind, SettingKind::Int { .. })
                && matches!(row.controls.first(), Some((Control::Step(..), _)))
                && row.controls.len() == 2
        });
        match steps {
            Some(spec) => {
                let value = ui::setting_value(env, spec);
                match ui::stepper(env, &self.style, row.area, &value) {
                    Some((minus, plus)) => alloc::vec![
                        (row.controls[0].0.clone(), minus),
                        (row.controls[1].0.clone(), plus),
                    ],
                    None => Vec::new(),
                }
            }
            None => row.controls.clone(),
        }
    }

    fn activate(&mut self, control: &Control, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let set = |cx: &mut Cx<'_, '_>, id: &Id, value: SettingValue| {
            if env.settings.get(id.as_str()) != Some(&value) {
                cx.command(Command::SetSetting(id.clone(), value));
            }
        };
        match control {
            Control::Toggle(id) => {
                let on = env.settings.bool(id.as_str());
                set(cx, id, SettingValue::Bool(!on));
            }
            Control::Step(id, step) => {
                if let Some(value) = spec(&env, id).and_then(|s| ui::next_value(&env, s, *step)) {
                    set(cx, id, value);
                }
            }
            Control::Option(id, option) => {
                set(cx, id, SettingValue::Text(option.clone().into()));
            }
            // Set by where it is tapped (see `on_gesture`).
            Control::Slide(_) => {}
            Control::Open(id) => {
                let screen = spec(&env, id)
                    .and_then(|s| s.extra().editor.clone())
                    .unwrap_or(CHOOSE_SCREEN);
                cx.command(Command::OpenScreen(
                    screen,
                    Params::new().with("id", id.as_str().to_string()),
                ));
            }
            Control::Run(id) if is_menu(&env, id) => {
                cx.command(Command::Back);
                cx.command(Command::OpenSlot(
                    slots::MENU,
                    Params::new().with(params::MENU, id.as_str().to_string()),
                ));
            }
            Control::Run(id) => {
                cx.command(Command::Back);
                cx.command(Command::RunAction(id.clone()));
            }
        }
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn draw_control(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        control: &Control,
        rect: Rect,
        focused: bool,
    ) {
        let style = self.style;
        let Some(font) = style.font(env) else {
            return;
        };
        let (on, label): (bool, String) = match control {
            Control::Toggle(id) => {
                let name = spec(env, id).map_or("", |s| env.i18n.tr(&s.title));
                let mark = if env.settings.is_scoped(id.as_str()) {
                    "• "
                } else {
                    ""
                };
                (
                    env.settings.bool(id.as_str()),
                    alloc::format!("{mark}{name}"),
                )
            }
            Control::Step(_, step) => (false, String::from(if *step < 0 { "−" } else { "+" })),
            Control::Slide(id) => {
                return draw_slider(canvas, env, &style, id, rect, focused);
            }
            Control::Option(id, option) => (
                env.settings.text(id.as_str()) == option,
                env.i18n.tr(option).to_string(),
            ),
            Control::Open(id) => {
                // A row: the setting's name, its value and an arrow.
                let Some(spec) = spec(env, id) else {
                    return;
                };
                if focused {
                    ui::draw_focus(canvas, &style, rect);
                }
                let color = if focused { Color::WHITE } else { Color::BLACK };
                let value = alloc::format!("{} ›", ui::setting_value(env, spec));
                let title = env.i18n.tr(&spec.title);
                let gap = style.gap as f32;
                let width = f32::from(rect.width);
                let (name, value, value_width) =
                    ui::fit_pair(&font, style.text, title, &value, width, gap);
                let right = f32::from(rect.x) + f32::from(rect.width);
                ui::draw_label(
                    canvas,
                    &font,
                    style.text,
                    right - value_width,
                    rect,
                    &value,
                    color,
                );
                ui::draw_label(
                    canvas,
                    &font,
                    style.text,
                    f32::from(rect.x),
                    rect,
                    &name,
                    color,
                );
                return;
            }
            Control::Run(id) => {
                let title = env
                    .actions
                    .get(id.as_str())
                    .map_or(id.as_str(), |a| a.title.as_ref());
                (false, env.i18n.tr(title).to_string())
            }
        };
        ui::draw_button(canvas, env, &style, rect, &label, on, focused);
    }
}

/// A slider's track (its − and + are controls of their own), framed
/// when focused.
fn draw_slider(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    id: &Id,
    rect: Rect,
    focused: bool,
) {
    let Some(slider) = spec(env, id).and_then(ui::Slider::of) else {
        return;
    };
    let value = env.settings.int(id.as_str());
    slider.draw_track(canvas, style, rect, value, Color::BLACK);
    if focused {
        canvas.stroke(rect, ui::button_border(style), Color::BLACK);
    }
}

/// Whether a slider's track has room in `rest` (beside its label) to be
/// set by eye; where it does not (a narrow screen), the number gets its
/// `−` and `+` alone.
fn slider_fits(rest: Rect, g: Geometry) -> bool {
    let size = ui::button_height(Rect::new(0, 0, g.width, g.row));
    let value = if g.value > 0 { g.value + g.gap } else { 0 };
    let track = rest.width.saturating_sub(2 * (size + g.gap) + value);
    track >= 3 * size
}

/// Whether a choice's options fit side by side in `area`, readably.
fn fits_segments(
    env: &Env<'_>,
    style: &UiStyle,
    options: &paperpaw_sdk::Choices,
    area: Rect,
) -> bool {
    let Some(font) = style.font(env) else {
        return false;
    };
    if options.is_empty() || options.len() > MAX_SEGMENTS {
        return false;
    }
    let cell = f32::from(area.width) / options.len() as f32 - 2.0 * style.gap as f32;
    options
        .iter()
        .all(|o| font.measure(style.small, env.i18n.tr(o)) <= cell)
}

/// `area` without `margin` left and right and a little above and below.
fn inset(area: Rect, margin: u16) -> Rect {
    let pad = (area.height - ui::button_height(area)) / 2;
    area.inset(margin, pad)
}

/// `count` equal parts of `area` side by side, `gap` apart.
fn split(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    let count = count.max(1) as u16;
    let width = area.width.saturating_sub(gap * (count - 1)) / count;
    (0..count)
        .map(|i| Rect::new(area.x + i * (width + gap), area.y, width, area.height))
        .collect()
}

impl Screen for PanelScreen {
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
        let border = (style.line * 2).max(2);
        let y = if self.top {
            i32::from(r.y + r.height) - border as i32
        } else {
            i32::from(r.y)
        };
        canvas.fill_rect(i32::from(r.x), y, u32::from(r.width), border, Color::BLACK);
        let Some(font) = style.font(env) else {
            return;
        };
        if self.top && self.head.height > 0 {
            draw_head(canvas, env, &style, &font, self.head);
        }
        if self.rows.is_empty() {
            ui::draw_message(canvas, env, &style, r, env.i18n.tr("menu.empty"));
            return;
        }
        for (first, row) in self.visible_rows() {
            let label = row.label.as_ref().and_then(|id| spec(env, id));
            // A number with − and + by its value (a slider row has three).
            let number = label.filter(|_| {
                matches!(row.controls.first(), Some((Control::Step(..), _)))
                    && row.controls.len() == 2
            });
            if let Some(spec) = number {
                // As in menus: title, then [−] value [+].
                let focus = match self.focus {
                    Some(f) if f == first => Some(false),
                    Some(f) if f == first + 1 => Some(true),
                    _ => None,
                };
                let title = env.i18n.tr(&spec.title);
                let value = ui::setting_value(env, spec);
                ui::draw_number(canvas, env, &style, row.area, title, &value, &value, focus);
                continue;
            }
            if let Some(spec) = label
                && !matches!(row.controls.first(), Some((Control::Open(_), _)))
            {
                let x = style.margin as f32;
                let first = row.controls.first().map_or(r.width, |(_, c)| c.x);
                let room = f32::from(first) - x - style.gap as f32;
                let name = ui::fit(&font, style.text, env.i18n.tr(&spec.title), room);
                ui::draw_label(canvas, &font, style.text, x, row.area, &name, Color::BLACK);
            }
            for (index, (control, rect)) in (first..).zip(row.controls.iter()) {
                self.draw_control(canvas, env, control, *rect, self.focus == Some(index));
            }
            // A slider's value, between its track and its +.
            if let (Some(spec), [_, (Control::Slide(_), track), (_, plus)]) =
                (label, row.controls.as_slice())
            {
                let start = track.x + track.width;
                let room = Rect::new(start, track.y, plus.x.saturating_sub(start), track.height);
                let value = ui::setting_value(env, spec);
                let x = f32::from(room.x)
                    + (f32::from(room.width) - font.measure(style.text, &value)) / 2.0;
                ui::draw_label(canvas, &font, style.text, x, room, &value, Color::BLACK);
            }
        }
        if let Some(pager) = &self.pager {
            pager.draw(canvas, env, &style, (self.page, self.pages()));
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let count = self.controls().count();
        let a = action;
        if *a == actions::CONFIRM {
            let control = self
                .focus
                .and_then(|f| self.controls().nth(f))
                .map(|(c, _)| c.clone());
            match control {
                Some(control) => self.activate(&control, cx),
                None => {
                    self.focus = Some(0);
                    self.redraw(cx);
                }
            }
            return Handled::Yes;
        }
        if *a == actions::MENU || *a == actions::BACK {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        let back = [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ];
        let forward = [
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
            actions::NEXT_PAGE,
        ];
        let delta: isize = if back.contains(a) {
            -1
        } else if forward.contains(a) {
            1
        } else {
            return Handled::No;
        };
        if count > 0 {
            let current = self.focus.map_or(-1, |f| f as isize);
            let focus = (current + delta).rem_euclid(count as isize) as usize;
            self.focus = Some(focus);
            self.page = self.page_of(focus);
            self.redraw(cx);
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.region.contains(gesture.start) {
            return Handled::No;
        }
        if let Some(action) = self.pager.and_then(|p| p.tap(gesture.start)) {
            let pages = self.pages();
            self.page = if action == actions::NEXT_PAGE {
                (self.page + 1).min(pages - 1)
            } else {
                self.page.saturating_sub(1)
            };
            self.focus = None;
            self.redraw(cx);
            return Handled::Yes;
        }
        if gesture.kind == GestureKind::Tap {
            let env = *cx.env();
            let control = self
                .visible_rows()
                .flat_map(|(_, row)| self.placed(&env, row))
                .find(|(_, rect)| rect.contains(gesture.start))
                .map(|(c, _)| c);
            if let Some(control) = control {
                self.focus = None;
                if let Control::Slide(id) = &control
                    && let Some((slider, rect)) = spec(&env, id).and_then(ui::Slider::of).zip(
                        self.controls()
                            .find(|(c, _)| *c == control)
                            .map(|(_, r)| *r),
                    )
                {
                    let value = SettingValue::Int(slider.value_at(rect, gesture.start.x));
                    if env.settings.get(id.as_str()) != Some(&value) {
                        cx.command(Command::SetSetting(id.clone(), value));
                    }
                } else {
                    self.activate(&control, cx);
                }
            }
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        // Values shown may have changed.
        if *event == events::SETTING_CHANGED {
            self.redraw(cx);
        }
    }
}

/// The quick panel's head: a status line of the items chosen for its
/// left, centre and right ([`QUICK_PARTS`]), as large as set.
fn draw_head(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    _: &UiStyle,
    font: &paperpaw_sdk::FontHandle<'_>,
    area: Rect,
) {
    let lists = QUICK_PARTS.map(|id| env.settings.text(id.as_str()));
    let line = paperpaw_sdk::status::StatusLine {
        area,
        lists,
        size: env
            .metrics
            .pt(env.settings.read(&QUICK_SIZE).clamp(6, 24) as f32),
        color: Color::BLACK,
        percent: true,
    };
    line.draw(canvas, env, font, &Params::new());
}

/// What the quick panel's head shows at its left, centre and right.
pub(crate) const QUICK_PARTS: [Id; 3] = [QUICK_LEFT, QUICK_CENTER, QUICK_RIGHT];
pub(crate) const QUICK_LEFT: Id = Id::new_static("menu.quick.left");
pub(crate) const QUICK_CENTER: Id = Id::new_static("menu.quick.center");
pub(crate) const QUICK_RIGHT: Id = Id::new_static("menu.quick.right");
/// The size of the quick panel's head, in points.
pub(crate) const QUICK_SIZE: Setting<i64> = Setting::new("menu.quick.size");

#[cfg(test)]
mod layout_tests {
    use super::*;

    const GEOMETRY: Geometry = Geometry {
        width: 480,
        height: 800,
        margin: 20,
        gap: 10,
        row: 60,
        border: 2,
        label: 150,
        value: 60,
        head: 40,
        pager: 45,
        top: false,
    };

    fn id(name: &'static str) -> Id {
        Id::new_static(name)
    }

    #[test]
    fn at_the_bottom_rows_follow_each_other() {
        let lines = [Line::Stepper(id("a.size")), Line::Open(id("a.font"))];
        let layout = layout(&lines, GEOMETRY);
        assert_eq!(layout.region.y + layout.region.height, 800);
        assert_eq!(layout.per_page, 2);
        assert!(layout.pager.is_none());
        let [first, second] = layout.rows.as_slice() else {
            panic!("two rows");
        };
        assert_eq!(second.area.y, first.area.y + 60);
        assert!(first.area.y >= layout.region.y + 2);
        assert_eq!(first.label, Some(id("a.size")));
    }

    #[test]
    fn from_the_top_the_head_comes_first() {
        let g = Geometry {
            top: true,
            ..GEOMETRY
        };
        let layout = layout(&[Line::Open(id("a.font"))], g);
        assert_eq!(layout.region.y, 0);
        assert_eq!(layout.head.height, 40);
        assert!(layout.rows[0].area.y >= layout.head.y + layout.head.height);
    }

    #[test]
    fn too_many_rows_go_to_pages() {
        let lines: Vec<Line> = (0..20).map(|_| Line::Open(id("a.font"))).collect();
        let layout = layout(&lines, GEOMETRY);
        assert!(layout.per_page < 20);
        assert!(layout.region.height <= (800.0 * MAX_SHARE) as u16);
        let pager = layout.pager.expect("a pager");
        assert_eq!(pager.y + pager.height, 800);
        // A row of the second page stands where the first page's first one
        // does.
        let second_page = &layout.rows[layout.per_page];
        assert_eq!(second_page.area.y, layout.rows[0].area.y);
    }

    #[test]
    fn controls_share_their_row() {
        let buttons = Line::Buttons(alloc::vec![
            Control::Toggle(id("a.one")),
            Control::Toggle(id("a.two")),
            Control::Toggle(id("a.three")),
        ]);
        let row = place(&buttons, Rect::new(0, 100, 480, 60), GEOMETRY);
        assert_eq!(row.controls.len(), 3);
        let widths: Vec<u16> = row.controls.iter().map(|(_, r)| r.width).collect();
        assert!(widths.iter().all(|w| *w == widths[0]));
        assert!(row.controls[2].1.x + row.controls[2].1.width <= 480 - 20);

        let slider = place(
            &Line::Slider(id("a.light")),
            Rect::new(0, 0, 480, 60),
            GEOMETRY,
        );
        let [(minus, m), (track, t), (plus, p)] = slider.controls.as_slice() else {
            panic!("three controls");
        };
        assert_eq!(*minus, Control::Step(id("a.light"), -1));
        assert_eq!(*track, Control::Slide(id("a.light")));
        assert_eq!(*plus, Control::Step(id("a.light"), 1));
        // Beside the label, the track between the two, with room for the
        // value before the +.
        assert!(m.x >= 20 + 150);
        assert!(m.x + m.width < t.x && t.x + t.width + 60 < p.x);

        let segments = Line::Segments(
            id("a.align"),
            alloc::vec![String::from("l"), String::from("r")],
        );
        let row = place(&segments, Rect::new(0, 0, 480, 60), GEOMETRY);
        assert_eq!(row.controls.len(), 2);
        assert_eq!(
            row.controls[1].0,
            Control::Option(id("a.align"), String::from("r"))
        );
    }

    #[test]
    fn sliders_need_room_for_their_track() {
        let rest = |width| Rect::new(0, 0, width, 42);
        // Buttons 42 wide: − and + with their gaps, the value, and a track
        // three buttons long.
        assert!(slider_fits(rest(2 * 52 + 70 + 126), GEOMETRY));
        assert!(!slider_fits(rest(2 * 52 + 70 + 125), GEOMETRY));
    }
}
