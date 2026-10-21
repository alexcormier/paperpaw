//! A group's page: its settings with their values, changed in place or
//! in a chooser.

use super::*;

/// The id of setting `id` of group `of` for its placement `placement`:
/// `widgets.clock.face` of `widgets.clock@2` is `widgets.clock@2.face`.
/// Settings of the group named otherwise stay as they are.
pub(super) fn placed(id: &Id, of: &Id, placement: &Id) -> Id {
    id.as_str()
        .strip_prefix(of.as_str())
        .filter(|rest| rest.starts_with('.'))
        .and_then(|rest| Id::parse(&alloc::format!("{placement}{rest}")).ok())
        .unwrap_or_else(|| id.clone())
}

/// Puts the focus of rebuilt `rows` back at `focus`, or the nearest entry.
pub(super) fn refocus(rows: &mut ui::Rows, focus: usize) {
    if rows.is_empty() {
        return;
    }
    rows.set_focus(focus.min(rows.len() - 1));
    if rows.is_heading(rows.focus()) && rows.step(true) == ListChange::None {
        rows.step(false);
    }
}

// ---- Group page: settings ----------------------------------------------

/// The most of the screen a group's preview takes.
const PREVIEW_SHARE: u16 = 3;

/// What a group's settings do, shown above them (a widget that previews
/// the group, e.g. a sample of text for the reading settings), under a
/// heading.
pub(super) struct Preview {
    widget: Box<dyn Widget>,
    heading: Rect,
    area: Rect,
}

impl core::fmt::Debug for Preview {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Preview")
            .field("heading", &self.heading)
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

impl Preview {
    /// The preview of `group`, if a widget previews it, at the top of
    /// `area`; `area` is left with the room below it.
    fn of(env: &Env<'_>, style: &UiStyle, group: &Id, area: &mut Rect) -> Option<Self> {
        let id = env.widgets.preview(group.as_str())?;
        let widget = env.widgets.make(id.as_str(), env)?;
        let heading = ui::heading_height(env, style);
        let most = (env.device.display.height / PREVIEW_SHARE).saturating_sub(heading);
        let height = widget.height(env, area.width).min(most);
        if height == 0 {
            return None;
        }
        let heading = Rect::new(area.x, area.y, area.width, heading);
        let shown = Rect::new(area.x, heading.y + heading.height, area.width, height);
        // A line below it, then the settings.
        let used = heading.height + height + style.gap as u16;
        *area = Rect::new(
            area.x,
            area.y + used,
            area.width,
            area.height.saturating_sub(used),
        );
        Some(Self {
            widget,
            heading,
            area: shown,
        })
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle) {
        let all = self.heading.union(&self.area);
        if all.intersection(&canvas.clip()).is_none() {
            return;
        }
        ui::draw_heading(
            canvas,
            env,
            style,
            self.heading,
            env.i18n.tr("settings.preview"),
        );
        self.widget.draw(canvas, env, self.area);
        // As the lines between rows.
        let margin = style.margin as u16;
        let line = Rect::new(
            self.area.x + margin,
            self.area.y + self.area.height + style.gap as u16 / 2,
            self.area.width.saturating_sub(2 * margin),
            style.line as u16,
        );
        canvas.fill_tint(line, Color::gray(170));
    }

    /// Lays it out anew for the values shown; `true` if it changed.
    fn update(&mut self, env: &Env<'_>) -> bool {
        self.widget.update(env)
    }
}

/// A row of a group page.
#[derive(Debug)]
pub(super) enum Row {
    /// A section's heading: its title.
    Heading(String),
    Setting(Box<SettingSpec>),
    /// Resets the page's settings to their defaults, at its end.
    Reset,
    /// The theme in use, at the top of the appearance settings where
    /// there are themes (see [`themes`](super::themes)).
    Theme,
}

#[derive(Debug)]
pub(super) struct GroupPage {
    pub(super) group: Id,
    pub(super) title: String,
    pub(super) entries: Vec<Row>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) rows: ui::Rows,
    /// The focus shows (buttons were used, or a number was tapped).
    pub(super) keys: bool,
    /// The focused number is being changed with the buttons: the value it
    /// had before. What it is changed to shows without being kept
    /// (`Command::PreviewSettings`) until it is chosen.
    pub(super) editing: Option<SettingValue>,
    /// A slider's knob taken along by a finger: its row and the value it
    /// had before (shown, not kept, until the finger is lifted).
    pub(super) drag: ui::SliderDrag,
    pub(super) dragged: Option<(usize, i64)>,
    /// Rows have room for descriptions.
    pub(super) noted: bool,
    /// The widest value of the page's numbers, so their − and + line up.
    pub(super) number_room: String,
    /// What the settings do, shown above them, where the group has it.
    pub(super) preview: Option<Preview>,
    /// Covered by a full screen, and settings changed meanwhile.
    pub(super) covered: Covered,
}

impl GroupPage {
    pub(super) fn new(env: &Env<'_>, group: Id) -> Self {
        // A placement's group (`widgets.clock@2`): the widget's settings,
        // as that placement has them.
        let own = group.placed().and_then(|plain| Id::parse(&plain).ok());
        let of = own.as_ref().unwrap_or(&group);
        let title = env
            .settings
            .groups()
            .find(|g| g.id == *of)
            .map_or_else(String::new, |g| env.i18n.tr(&g.title).to_string());
        let specs = env
            .settings
            .specs()
            .filter(|spec| env.settings.group_of(spec) == *of && visible(env, spec))
            .cloned()
            .map(|mut spec| {
                if own.is_some() {
                    spec.id = placed(&spec.id, of, &group);
                }
                spec
            })
            .collect::<Vec<SettingSpec>>();
        // Sections: settings without one first, then the standard ones,
        // then the others as they come.
        let mut sections: Vec<Option<&str>> = Vec::new();
        for spec in &specs {
            let section = spec.extra().section.as_deref();
            if !sections.contains(&section) {
                sections.push(section);
            }
        }
        let rank = |section: &Option<&str>| match section {
            None => 0,
            Some(key) => setting_sections::ORDER
                .iter()
                .position(|s| s == key)
                .map_or(setting_sections::ORDER.len() + 1, |p| p + 1),
        };
        sections.sort_by_key(rank);
        // Headings only where there is more than one part.
        let headed = sections.len() > 1;
        let mut entries = Vec::new();
        if *of == groups::LOOK && own.is_none() && super::themes::themes(env).next().is_some() {
            entries.push(Row::Theme);
        }
        for section in &sections {
            if let Some(key) = section.filter(|_| headed) {
                entries.push(Row::Heading(env.i18n.tr(key).to_string()));
            }
            entries.extend(
                specs
                    .iter()
                    .filter(|spec| spec.extra().section.as_deref() == *section)
                    .cloned()
                    .map(|spec| Row::Setting(Box::new(spec))),
            );
        }
        if !specs.is_empty() {
            entries.push(Row::Reset);
        }
        let noted = specs.iter().any(|spec| spec.description.is_some());
        let (style, header, mut area) = page(env);
        // A placement's settings have none (they are the widget's).
        let preview = own
            .is_none()
            .then(|| Preview::of(env, &style, &group, &mut area))
            .flatten();
        let heading = ui::heading_height(env, &style);
        let row = if noted { style.noted_row() } else { style.row } as u16;
        let slider_row = ui::slider::row_height(&style, noted);
        let heights = entries
            .iter()
            .map(|e| match e {
                Row::Heading(_) => heading,
                Row::Setting(spec) if ui::slider::shows(env, spec) => slider_row,
                Row::Setting(spec) => ui::setting_row(&style, spec, u32::from(row)) as u16,
                Row::Reset | Row::Theme => style.row as u16,
            })
            .collect();
        let headings = entries
            .iter()
            .map(|e| matches!(e, Row::Heading(_)))
            .collect();
        let number_room = ui::number_room(
            env,
            &style,
            entries.iter().filter_map(|e| match e {
                Row::Setting(spec) => Some(&**spec),
                Row::Heading(_) | Row::Reset | Row::Theme => None,
            }),
        );
        Self {
            group,
            title,
            entries,
            style,
            header,
            rows: ui::Rows::new(area, heights, headings),
            keys: !env.device.input.touch,
            editing: None,
            drag: ui::SliderDrag::default(),
            dragged: None,
            noted,
            number_room,
            preview,
            covered: Covered::default(),
        }
    }

    /// Makes the page anew for the settings now, keeping its focus and a
    /// number being changed.
    fn rebuild(&mut self, env: &Env<'_>) {
        let focus = self.rows.focus();
        let (keys, drag, dragged) = (self.keys, self.drag, self.dragged);
        let editing = self.editing.take();
        *self = Self::new(env, self.group.clone());
        refocus(&mut self.rows, focus);
        self.keys = keys;
        self.editing = editing;
        self.drag = drag;
        self.dragged = dragged;
    }

    /// Focuses setting `id`, shown so (e.g. found by a search).
    pub(super) fn focus_on(&mut self, _: &Env<'_>, id: &str) {
        let at = self
            .entries
            .iter()
            .position(|e| matches!(e, Row::Setting(spec) if spec.id.as_str() == id));
        if let Some(at) = at {
            self.rows.set_focus(at);
            self.keys = true;
        }
    }

    /// Where a row's title and value (or stepper) are drawn.
    pub(super) fn title_area(&self, rect: Rect) -> Rect {
        if self.noted {
            ui::title_area(&self.style, rect)
        } else {
            rect
        }
    }

    pub(super) fn spec(&self, index: usize) -> Option<&SettingSpec> {
        match self.entries.get(index) {
            Some(Row::Setting(spec)) => Some(spec),
            _ => None,
        }
    }

    pub(super) fn focused(&self) -> Option<&SettingSpec> {
        self.spec(self.rows.focus())
    }

    /// Changes a number by `step` at once (a tap on its − or +), or
    /// toggles a switch or a choice of two.
    pub(super) fn change(&self, env: &Env<'_>, step: i64, cx: &mut Cx<'_, '_>) {
        let Some(spec) = self.focused() else {
            return;
        };
        let current = env.settings.get(spec.id.as_str());
        let Some(value) = ui::next_value(env, spec, step) else {
            return;
        };
        if Some(&value) != current {
            cx.command(Command::SetSetting(spec.id.clone(), value));
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    /// Moves the number being changed with the buttons by `step`: shown
    /// at once, kept when chosen ([`GroupPage::finish`]).
    fn preview_step(&self, env: &Env<'_>, step: i64, cx: &mut Cx<'_, '_>) {
        let Some(spec) = self.focused() else {
            return;
        };
        let current = env.settings.get(spec.id.as_str());
        if let Some(value) = ui::next_value(env, spec, step).filter(|v| Some(v) != current) {
            cx.command(Command::PreviewSettings(alloc::vec![(
                spec.id.clone(),
                Some(value)
            )]));
        }
    }

    /// Ends changing the focused number with the buttons: keeps the value
    /// shown (`keep`), or shows the one before again.
    fn finish(&mut self, env: &Env<'_>, keep: bool, cx: &mut Cx<'_, '_>) {
        let before = self.editing.take();
        if keep
            && let Some(spec) = self.focused()
            && let Some(shown) = env.settings.get(spec.id.as_str())
            && Some(shown) != before.as_ref()
        {
            cx.command(Command::SetSetting(spec.id.clone(), shown.clone()));
        }
        end_preview(env, cx);
        // Quick while it moved; now with its grays again.
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Stops changing a number with the buttons without keeping what was
    /// shown, e.g. for a tap elsewhere.
    fn leave_editing(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        if self.editing.take().is_some() {
            end_preview(env, cx);
        }
    }

    /// The − and + of number row `index` in `rect`, where it shows them:
    /// a slider's in its band, else beside the value.
    fn steps(&self, env: &Env<'_>, index: usize, rect: Rect) -> Option<(Rect, Rect)> {
        let spec = self.spec(index)?;
        if ui::slider::shows(env, spec) {
            let parts = ui::slider::parts(&self.style, ui::slider::band(&self.style, rect));
            return Some((parts.minus, parts.plus));
        }
        let value = ui::setting_value(env, spec);
        let room = if self.number_room.len() < value.len() {
            &value
        } else {
            &self.number_room
        };
        ui::stepper(env, &self.style, self.title_area(rect), room)
    }

    /// Redraws row `index` where it is (a value shown that changed).
    fn redraw_row(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(rect) = self.rows.rect_of(index) {
            cx.request_redraw_area(rect);
            cx.request_refresh(RefreshMode::Fast);
        }
    }

    /// What confirming (or tapping) the focused setting does.
    pub(super) fn activate(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        match self.entries.get(self.rows.focus()) {
            Some(Row::Reset) => {
                cx.command(ask_reset(env, Some((&self.group, &self.title))));
                return;
            }
            Some(Row::Theme) => {
                cx.command(Command::OpenScreen(super::themes::SCREEN, Params::new()));
                return;
            }
            _ => {}
        }
        let Some(spec) = self.focused() else {
            return;
        };
        // Its editor, the keyboard, the list of its values.
        if let Some(command) = ui::setting_opens(env, spec) {
            cx.command(command);
            return;
        }
        match spec.kind {
            SettingKind::Int { .. } => {
                self.editing = env.settings.get(spec.id.as_str()).cloned();
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            _ => self.change(env, 1, cx),
        }
    }
}

impl Screen for GroupPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        self.header
            .draw(canvas, env, &style, &self.title, None, paging(&self.rows));
        if let Some(preview) = &self.preview {
            preview.draw(canvas, env, &style);
        }
        let entries = &self.entries;
        let editing = self.editing.is_some();
        let noted = self.noted;
        let room = self.number_room.as_str();
        let touch = env.device.input.touch;
        self.rows
            .draw(canvas, &style, self.keys, |canvas, index, rect, focused| {
                let spec = match &entries[index] {
                    Row::Heading(title) => {
                        ui::draw_heading(canvas, env, &style, rect, title);
                        return;
                    }
                    Row::Reset => {
                        let title = env.i18n.tr("settings.reset-page");
                        let rect = Rect::new(rect.x, rect.y, rect.width, style.row as u16);
                        ui::draw_value_row(canvas, env, &style, rect, focused, title, "");
                        return;
                    }
                    Row::Theme => {
                        let title = env.i18n.tr("settings.theme");
                        let rect = Rect::new(rect.x, rect.y, rect.width, style.row as u16);
                        let name = super::themes::applied_name(env);
                        ui::draw_value_row(canvas, env, &style, rect, focused, title, &name);
                        return;
                    }
                    Row::Setting(spec) => spec,
                };
                let title = env.i18n.tr(&spec.title);
                let note = spec.description.as_deref().map_or("", |d| env.i18n.tr(d));
                let note = if noted { note } else { "" };
                let mut value = ui::setting_value(env, spec);
                if let Some(slider) = ui::Slider::of(spec).filter(|_| ui::slider::shows(env, spec))
                {
                    let focus = match (focused, editing) {
                        (false, _) => ui::SliderFocus::None,
                        (true, false) => ui::SliderFocus::Focused,
                        (true, true) => ui::SliderFocus::Editing,
                    };
                    let view =
                        ui::SliderView::new(rect, env.settings.int(spec.id.as_str()), &value)
                            .title(title)
                            .note(note)
                            .focus(focus);
                    slider.draw_row(canvas, env, &style, &view);
                    return;
                }
                match spec.kind {
                    SettingKind::Int { .. } if focused => {
                        let area = if noted {
                            ui::title_area(&style, rect)
                        } else {
                            rect
                        };
                        ui::draw_stepper(canvas, env, &style, area, title, &value, room, editing);
                        if !note.is_empty() {
                            ui::draw_note(canvas, env, &style, rect, true, note);
                        }
                        return;
                    }
                    // On touch screens − and + are always there.
                    SettingKind::Int { .. } if touch => {
                        let area = if noted {
                            ui::title_area(&style, rect)
                        } else {
                            rect
                        };
                        ui::draw_number(canvas, env, &style, area, title, &value, room, None);
                        if !note.is_empty() {
                            ui::draw_note(canvas, env, &style, rect, false, note);
                        }
                        return;
                    }
                    SettingKind::Bool if spec.extra().editor.is_none() => {
                        let on = env.settings.bool(spec.id.as_str());
                        ui::draw_switch_row(canvas, env, &style, rect, focused, title, note, on);
                        return;
                    }
                    _ => {}
                }
                if ui::opens_screen(env, spec) {
                    value.push_str(" ›");
                }
                ui::draw_noted_row(canvas, env, &style, rect, focused, title, &value, note);
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if self.editing.is_some() {
            if let Some(step) = ui::slider::key_step(action) {
                self.preview_step(&env, step, cx);
            } else if *action == actions::CONFIRM || *action == actions::BACK {
                self.finish(&env, *action == actions::CONFIRM, cx);
            } else {
                return Handled::No;
            }
            return Handled::Yes;
        }
        if let Some(change) = navigate_rows(&mut self.rows, &mut self.keys, action) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.activate(&env, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging(&self.rows));
        if let Some(action) = tapped {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let Some(index) = self.rows.hit(gesture.start) else {
            return Handled::No;
        };
        let env = *cx.env();
        let is_number = matches!(
            self.spec(index).map(|s| &s.kind),
            Some(SettingKind::Int { .. })
        );
        if is_number && env.device.input.touch {
            // − and + are always there: a tap on one changes the number,
            // elsewhere on the row nothing happens.
            self.rows.set_focus(index);
            self.keys = false;
            self.leave_editing(&env, cx);
            let Some(rect) = self.rows.rect_of(index) else {
                return Handled::Yes;
            };
            if let Some((minus, plus)) = self.steps(&env, index, rect) {
                let x = gesture.start.x;
                // A little room around the buttons: easier to hit.
                let slack = i32::from(self.style.gap as u16);
                let within = |r: Rect| x >= i32::from(r.x) - slack && x < r.right() as i32 + slack;
                if within(minus) {
                    self.change(&env, -1, cx);
                } else if within(plus) {
                    self.change(&env, 1, cx);
                }
            }
            return Handled::Yes;
        }
        if is_number {
            // The first tap selects a number; then its − and + buttons
            // change it, so a value never changes by accident.
            if index != self.rows.focus() || !self.keys {
                self.rows.set_focus(index);
                self.keys = true;
                self.leave_editing(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
                return Handled::Yes;
            }
            let Some(rect) = self.rows.rect_of(index) else {
                return Handled::Yes;
            };
            if let Some((minus, plus)) = self.steps(&env, index, rect) {
                let x = gesture.start.x;
                let within = |r: Rect| x >= i32::from(r.x) && x < r.right() as i32;
                if within(minus) {
                    self.change(&env, -1, cx);
                } else if within(plus) {
                    self.change(&env, 1, cx);
                }
            }
            return Handled::Yes;
        }
        // Anything else acts at once; the focus hides again.
        let keys = !env.device.input.touch;
        if index != self.rows.focus() || keys != self.keys {
            self.rows.set_focus(index);
            self.keys = keys;
            self.leave_editing(&env, cx);
            cx.request_redraw();
        }
        self.activate(&env, cx);
        Handled::Yes
    }

    fn on_pointer(&mut self, event: &PointerEvent, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        if !env.device.input.touch {
            return Handled::No;
        }
        // The row of the knob taken, or of a finger coming down.
        let index = match self.dragged {
            Some((index, _)) => Some(index),
            None if event.phase == PointerPhase::Down => self.rows.hit(event.point),
            None => None,
        };
        let Some((index, slider, id, rect)) = index.and_then(|i| {
            let spec = self.spec(i).filter(|s| ui::slider::shows(&env, s))?;
            let slider = ui::Slider::of(spec)?;
            Some((i, slider, spec.id.clone(), self.rows.rect_of(i)?))
        }) else {
            return Handled::No;
        };
        let band = ui::slider::band(&self.style, rect);
        let value = env.settings.int(id.as_str());
        let Some(step) = self
            .drag
            .on_pointer(event, &self.style, &slider, band, value)
        else {
            return Handled::No;
        };
        let before = match self.dragged {
            Some((_, before)) => before,
            None => {
                let before = env.settings.int(id.as_str());
                self.dragged = Some((index, before));
                // The focus hides, as after other taps.
                self.rows.set_focus(index);
                self.keys = false;
                self.leave_editing(&env, cx);
                cx.request_redraw();
                before
            }
        };
        match step {
            ui::SliderStep::Moved(value) => {
                cx.command(Command::PreviewSettings(alloc::vec![(
                    id,
                    Some(SettingValue::Int(value))
                )]));
            }
            ui::SliderStep::Same => {}
            ui::SliderStep::Chosen(value) => {
                self.dragged = None;
                if value != before {
                    cx.command(Command::SetSetting(id, SettingValue::Int(value)));
                }
                end_preview(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            ui::SliderStep::Cancelled => {
                self.dragged = None;
                end_preview(&env, cx);
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Partial);
            }
            _ => {}
        }
        Handled::Yes
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        // While the buttons change a number: what each does to it.
        if self.editing.is_none() {
            return Hint::Default;
        }
        match ui::slider::key_step(action) {
            Some(step) if step < 0 => Hint::Text(String::from("−")),
            Some(_) => Hint::Text(String::from("+")),
            None if *action == actions::CONFIRM => {
                Hint::Text(env.i18n.tr("settings.hint.keep").to_string())
            }
            None if *action == actions::BACK => {
                Hint::Text(env.i18n.tr("settings.hint.cancel").to_string())
            }
            None => Hint::Default,
        }
    }

    fn on_hide(&mut self) {
        self.covered.hide();
    }

    /// Asked for while covered: it comes once the page shows again, also
    /// below an overlay left open above it (where `on_show` does not).
    fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        self.on_show(cx);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.covered.shown() {
            let env = *cx.env();
            self.rebuild(&env);
            cx.request_redraw();
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(picked) = CHOSEN.read(event, params) {
            if picked.path == self.group.as_str() {
                let env = *cx.env();
                reset(&env, &picked, cx);
            }
            return;
        }
        if *event == events::SETTING_CHANGED {
            let covered = self.covered.changed(cx);
            let env = *cx.env();
            // A setting shown only while this one has a value.
            let shows_others = events::SETTING_CHANGED
                .read(event, params)
                .is_some_and(|changed| env.settings.governs(changed.id.as_str()));
            if covered || shows_others {
                self.rebuild(&env);
                cx.request_redraw();
            }
        } else if *event == events::PREVIEW_CHANGED {
            // A value being chosen, shown: its row and what it does.
            let env = *cx.env();
            let index = self.dragged.map_or(self.rows.focus(), |(index, _)| index);
            self.redraw_row(index, cx);
            if let Some(preview) = self.preview.as_mut()
                && preview.update(&env)
            {
                cx.request_redraw_area(preview.area);
            }
        }
    }
}
