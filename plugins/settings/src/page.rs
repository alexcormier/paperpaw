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
    rows.focus = focus.min(rows.len() - 1);
    if rows.is_heading(rows.focus) && rows.step(true) == ListChange::None {
        rows.step(false);
    }
}

// ---- Group page: settings ----------------------------------------------

/// A row of a group page.
#[derive(Debug)]
pub(super) enum Row {
    /// A section's heading: its title.
    Heading(String),
    Setting(Box<SettingSpec>),
    /// Resets the page's settings to their defaults, at its end.
    Reset,
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
    /// The focused number is being edited.
    pub(super) editing: bool,
    /// Rows have room for descriptions.
    pub(super) noted: bool,
    /// The widest value of the page's numbers, so their − and + line up.
    pub(super) number_room: String,
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
        let (style, header, area) = page(env);
        let heading = ui::heading_height(env, &style);
        let row = if noted { style.noted_row() } else { style.row } as u16;
        let heights = entries
            .iter()
            .map(|e| match e {
                Row::Heading(_) => heading,
                Row::Setting(_) => row,
                Row::Reset => style.row as u16,
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
                Row::Heading(_) | Row::Reset => None,
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
            editing: false,
            noted,
            number_room,
        }
    }

    /// Focuses setting `id`, shown so (e.g. found by a search).
    pub(super) fn focus_on(&mut self, _: &Env<'_>, id: &str) {
        let at = self
            .entries
            .iter()
            .position(|e| matches!(e, Row::Setting(spec) if spec.id.as_str() == id));
        if let Some(at) = at {
            self.rows.focus = at;
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
        self.spec(self.rows.focus)
    }

    /// Changes a number by `step`, or toggles a switch or a choice of two.
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
            // Quick while editing with buttons; else with its grays.
            cx.request_refresh(if self.editing {
                RefreshMode::Fast
            } else {
                RefreshMode::Partial
            });
        }
    }

    /// What confirming (or tapping) the focused setting does.
    pub(super) fn activate(&mut self, env: &Env<'_>, step: i64, cx: &mut Cx<'_, '_>) {
        if matches!(self.entries.get(self.rows.focus), Some(Row::Reset)) {
            cx.command(ask_reset(env, Some((&self.group, &self.title))));
            return;
        }
        let Some(spec) = self.focused() else {
            return;
        };
        if let Some(editor) = &spec.extra().editor {
            cx.command(Command::OpenScreen(
                editor.clone(),
                Params::new().with(ID, spec.id.as_str().to_string()),
            ));
            return;
        }
        match spec.kind {
            SettingKind::Bool => self.change(env, 1, cx),
            SettingKind::Int { .. } if self.editing => self.change(env, step, cx),
            SettingKind::Int { .. } => {
                self.editing = true;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            SettingKind::Choice(_) | SettingKind::Language if ui::two_options(env, spec) => {
                self.change(env, 1, cx);
            }
            SettingKind::Choice(_) | SettingKind::Language => {
                cx.command(Command::OpenScreen(
                    CHOOSE_SCREEN,
                    Params::new().with(ID, spec.id.as_str().to_string()),
                ));
            }
            SettingKind::Text => {
                let question = ask::text(env.i18n.tr(&spec.title), &TEXT_ENTERED)
                    .with(ID, spec.id.as_str().to_string());
                let question = if spec.extra().secret {
                    question.password()
                } else {
                    question.initial(ui::setting_value(env, spec))
                };
                cx.command(question.command());
            }
            _ => {}
        }
    }
}

impl Screen for GroupPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        self.header
            .draw(canvas, env, &style, &self.title, None, paging(&self.rows));
        let entries = &self.entries;
        let editing = self.editing;
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
                    Row::Setting(spec) => spec,
                };
                let title = env.i18n.tr(&spec.title);
                let note = spec.description.as_deref().map_or("", |d| env.i18n.tr(d));
                let note = if noted { note } else { "" };
                let mut value = ui::setting_value(env, spec);
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
        if self.editing {
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
            if decrease.contains(action) {
                self.change(&env, -1, cx);
            } else if increase.contains(action) {
                self.change(&env, 1, cx);
            } else if *action == actions::CONFIRM || *action == actions::BACK {
                self.editing = false;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else {
                return Handled::No;
            }
            return Handled::Yes;
        }
        if let Some(change) = navigate_rows(&mut self.rows, &mut self.keys, action) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.activate(&env, 1, cx);
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
            self.rows.focus = index;
            self.keys = false;
            self.editing = false;
            let (Some(spec), Some(rect)) = (self.focused(), self.rows.rect_of(index)) else {
                return Handled::Yes;
            };
            let rect = self.title_area(rect);
            let value = ui::setting_value(&env, spec);
            let room = if self.number_room.len() < value.len() {
                &value
            } else {
                &self.number_room
            };
            if let Some((minus, plus)) = ui::stepper(&env, &self.style, rect, room) {
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
            if index != self.rows.focus || !self.keys {
                self.rows.focus = index;
                self.keys = true;
                self.editing = false;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
                return Handled::Yes;
            }
            let (Some(spec), Some(rect)) = (self.focused(), self.rows.rect_of(index)) else {
                return Handled::Yes;
            };
            let rect = self.title_area(rect);
            let value = ui::setting_value(&env, spec);
            let room = if self.number_room.len() < value.len() {
                &value
            } else {
                &self.number_room
            };
            if let Some((minus, plus)) = ui::stepper(&env, &self.style, rect, room) {
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
        if index != self.rows.focus || keys != self.keys {
            self.rows.focus = index;
            self.keys = keys;
            self.editing = false;
            cx.request_redraw();
        }
        self.activate(&env, 1, cx);
        Handled::Yes
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
            let env = *cx.env();
            let (focus, keys, editing) = (self.rows.focus, self.keys, self.editing);
            *self = Self::new(&env, self.group.clone());
            refocus(&mut self.rows, focus);
            self.keys = keys;
            self.editing = editing;
            cx.request_redraw();
        }
    }
}
