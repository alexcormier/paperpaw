//! The list of setting groups, the first screen of the settings: a
//! group's page, or a plugin's settings, chosen from it.

use super::*;

// ---- Main page: groups -------------------------------------------------

/// What an entry of the main page opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Link {
    Group(Id),
    /// A page of another plugin: its action.
    Page(Id),
    /// The switch for advanced settings.
    Advanced,
    /// Searching the settings.
    Search,
}

#[derive(Debug)]
pub(super) struct Entry {
    /// `None` for a section's heading.
    pub(super) link: Option<Link>,
    pub(super) title: String,
    pub(super) icon: Option<String>,
}

#[derive(Debug)]
pub(super) struct GroupList {
    /// Groups with visible settings and pages of other plugins, by order
    /// under the headings of their sections, then the advanced switch.
    pub(super) entries: Vec<Entry>,
    pub(super) style: UiStyle,
    pub(super) header: ui::Header,
    pub(super) rows: ui::Rows,
    /// The focus shows (buttons were used).
    pub(super) keys: bool,
    /// Room for icons at the start of rows.
    pub(super) icon_room: u16,
}

impl GroupList {
    pub(super) fn new(env: &Env<'_>) -> Self {
        let (style, header, area) = page(env);
        let settings_menu = menus::SETTINGS;
        let menu = settings_menu.as_str();
        // (order, headings before entries of the same order, entry)
        let mut placed: Vec<(i32, bool, Entry)> = env
            .menus
            .items(menu)
            .iter()
            // Passwords and keys only once there are some.
            .filter(|item| *item.id != secrets::OPEN || !env.secrets.is_empty())
            .map(|item| {
                let order = env.menus.order_of(menu, item.id.as_str());
                (
                    order.map_or(i32::from(i16::MAX), i32::from),
                    !item.section,
                    Entry {
                        link: (!item.section).then(|| Link::Page(item.id.clone())),
                        title: env.i18n.tr(item.title).to_string(),
                        icon: item.icon.map(String::from),
                    },
                )
            })
            .collect();
        placed.extend(
            env.settings
                .groups()
                // A widget's own settings are opened where it is edited.
                .filter(|group| !env.widgets.has(group.id.as_str()))
                .filter(|group| {
                    env.settings
                        .specs()
                        .any(|spec| env.settings.group_of(spec) == group.id && visible(env, spec))
                })
                .map(|group| {
                    (
                        group.order,
                        true,
                        Entry {
                            link: Some(Link::Group(group.id.clone())),
                            title: env.i18n.tr(&group.title).to_string(),
                            icon: env.menus.icon(group.id.as_str()).map(String::from),
                        },
                    )
                }),
        );
        placed.push((
            i32::MIN,
            true,
            Entry {
                link: Some(Link::Search),
                title: env.i18n.tr("settings.search").to_string(),
                icon: Some(String::from("search")),
            },
        ));
        placed.push((
            i32::MAX,
            true,
            Entry {
                link: Some(Link::Advanced),
                title: env.i18n.tr("settings.advanced").to_string(),
                icon: Some(String::from("eye")),
            },
        ));
        placed.sort_by_key(|(order, entry, _)| (*order, *entry));
        let mut entries: Vec<Entry> = placed.into_iter().map(|(_, _, entry)| entry).collect();
        // Headings with nothing under them go.
        let empty: Vec<bool> = (0..entries.len())
            .map(|i| {
                entries[i].link.is_none()
                    && entries.get(i + 1).is_none_or(|next| next.link.is_none())
            })
            .collect();
        let mut index = 0;
        entries.retain(|_| {
            index += 1;
            !empty[index - 1]
        });
        let heading = ui::heading_height(env, &style);
        let heights = entries
            .iter()
            .map(|e| {
                if e.link.is_none() {
                    heading
                } else {
                    style.row as u16
                }
            })
            .collect();
        let headings = entries.iter().map(|e| e.link.is_none()).collect();
        let icon_room = ui::icon_room(
            env,
            &style,
            entries.iter().any(|e| e.link.is_some() && e.icon.is_some()),
        );
        Self {
            entries,
            style,
            header,
            rows: ui::Rows::new(area, heights, headings),
            keys: !env.device.input.touch,
            icon_room,
        }
    }

    pub(super) fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        match self.entries.get(index).and_then(|e| e.link.as_ref()) {
            Some(Link::Group(id)) => {
                // The group's own page, if its plugin brings one.
                let page = cx
                    .env()
                    .settings
                    .groups()
                    .find(|g| g.id == *id)
                    .and_then(|g| g.page.clone());
                cx.command(Command::OpenScreen(
                    page.unwrap_or(GROUP_SCREEN),
                    Params::new().with(ID, id.as_str().to_string()),
                ));
            }
            Some(Link::Page(action)) => cx.command(Command::RunAction(action.clone())),
            Some(Link::Advanced) => {
                let advanced = cx.env().settings.read(&ADVANCED);
                cx.command(Command::set(&ADVANCED, !advanced));
            }
            Some(Link::Search) => cx.command(search::ask(cx.env())),
            None => {}
        }
    }
}

impl Screen for GroupList {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        self.header.draw(
            canvas,
            env,
            &style,
            env.i18n.tr("settings.title"),
            None,
            paging(&self.rows),
        );
        let (entries, room) = (&self.entries, self.icon_room);
        self.rows
            .draw(canvas, &style, self.keys, |canvas, index, rect, focused| {
                let entry = &entries[index];
                let Some(link) = &entry.link else {
                    ui::draw_heading(canvas, env, &style, rect, &entry.title);
                    return;
                };
                let ink = if focused { Color::WHITE } else { Color::BLACK };
                let icon = entry.icon.as_deref();
                let rect = ui::draw_row_icon(canvas, env, &style, rect, room, icon, ink);
                if *link == Link::Advanced {
                    let on = env.settings.read(&ADVANCED);
                    ui::draw_switch_row(canvas, env, &style, rect, focused, &entry.title, "", on);
                } else if *link == Link::Search {
                    ui::draw_value_row(canvas, env, &style, rect, focused, &entry.title, "");
                } else {
                    ui::draw_value_row(canvas, env, &style, rect, focused, &entry.title, "›");
                }
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if let Some(change) = navigate_rows(&mut self.rows, &mut self.keys, action) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.rows.focus, cx);
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
        match self.rows.hit(gesture.start) {
            Some(index) => {
                self.rows.focus = index;
                self.keys = !cx.env().device.input.touch;
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(typed) = search::ENTERED.read(event, params) {
            let text = typed.text.trim();
            if !text.is_empty() {
                cx.command(Command::OpenScreen(
                    search::SCREEN,
                    Params::new().with(params::TEXT, text.to_string()),
                ));
            }
            return;
        }
        if *event == events::SETTING_CHANGED {
            // Language, UI size or the advanced switch may have changed.
            let env = *cx.env();
            let (focus, keys) = (self.rows.focus, self.keys);
            *self = Self::new(&env);
            self.keys = keys;
            refocus(&mut self.rows, focus);
            cx.request_redraw();
        }
    }
}
