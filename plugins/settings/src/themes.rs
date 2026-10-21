//! Choosing a theme: the presets of the kind `core.theme` (see
//! `paperpaw_sdk::presets::THEME`), offered at the top of the appearance
//! settings where any plugin brings one. A theme gives the appearance
//! settings its values and the others back their defaults; the user's
//! changes after it are theirs. *None* first gives them all their
//! defaults again. What the reader needs to read at all (the size of the
//! text, the contrast) no theme changes (`presets::theme_settings`).

use super::*;

use paperpaw_sdk::SettingValue;
use paperpaw_sdk::presets::{self, Preset};

/// The chooser of themes.
pub(super) const SCREEN: Id = Id::new_static("settings.themes");
/// No theme: the appearance as PaperPaw comes, first in the chooser.
const NONE: Id = Id::new_static("settings.theme-none");

/// What applying theme `id` (or [`NONE`]) makes of the settings.
fn changes(env: &Env<'_>, id: &Id) -> Option<Vec<(Id, Option<SettingValue>)>> {
    let covered = presets::theme_settings(env.settings);
    if *id == NONE {
        return Some(covered.into_iter().map(|id| (id, None)).collect());
    }
    let theme = themes(env).find(|theme| theme.id == *id)?;
    Some(theme.changes(env.settings, &covered))
}

/// The themes there are, in the order of the plugins.
pub(super) fn themes<'e>(env: &Env<'e>) -> impl Iterator<Item = &'e Preset> + 'e {
    env.settings.presets().of_kind(presets::THEME)
}

/// The theme in use: the one whose values the settings have, [`NONE`]
/// where they have their defaults.
fn applied(env: &Env<'_>) -> Option<Id> {
    let is = |id: &Id| changes(env, id).is_some_and(|c| presets::applied(env.settings, &c));
    if is(&NONE) {
        return Some(NONE);
    }
    themes(env).map(|theme| theme.id.clone()).find(|id| is(id))
}

/// What the theme row shows: the theme in use, none, or the user's own.
pub(super) fn applied_name(env: &Env<'_>) -> String {
    match applied(env) {
        Some(id) if id == NONE => env.i18n.tr("settings.theme.none").to_string(),
        Some(id) => themes(env)
            .find(|theme| theme.id == id)
            .map_or_else(String::new, |theme| env.i18n.tr(&theme.title).to_string()),
        None => env.i18n.tr("settings.theme.own").to_string(),
    }
}

#[derive(Debug)]
pub(super) struct Themes {
    /// The themes' ids, names and what each is.
    themes: Vec<(Id, String, String)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl Themes {
    pub(super) fn new(env: &Env<'_>) -> Self {
        let (style, header, mut list) = frame(env, true);
        let none = (
            NONE,
            env.i18n.tr("settings.theme.none").to_string(),
            env.i18n.tr("settings.theme.none.help").to_string(),
        );
        let themes: Vec<(Id, String, String)> = core::iter::once(none)
            .chain(themes(env).map(|theme| {
                let about = env.i18n.tr(&theme.description).to_string();
                (
                    theme.id.clone(),
                    env.i18n.tr(&theme.title).to_string(),
                    about,
                )
            }))
            .collect();
        let current = applied(env);
        list.set_focus(
            themes
                .iter()
                .position(|(id, ..)| Some(id) == current.as_ref())
                .unwrap_or(0),
        );
        Self {
            themes,
            style,
            header,
            list,
        }
    }

    /// Uses theme `index` and goes back to the settings.
    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((id, ..)) = self.themes.get(index) else {
            return;
        };
        let env = *cx.env();
        let Some(changes) = changes(&env, id) else {
            return;
        };
        cx.command(Command::Back);
        for command in presets::commands(env.settings, &changes) {
            cx.command(command);
        }
    }
}

impl Screen for Themes {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.themes.len();
        let title = env.i18n.tr("settings.theme");
        self.header
            .draw(canvas, env, &style, title, None, self.list.paging(len));
        let current = applied(env);
        let themes = &self.themes;
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                let (id, name, about) = &themes[index];
                let mark = if Some(id) == current.as_ref() {
                    "•"
                } else {
                    ""
                };
                ui::draw_noted_row(canvas, env, &style, rect, focused, name, mark, about);
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.themes.len();
        if let Some(change) = self.list.navigate(action, len) {
            change.request(cx);
        } else if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.themes.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.themes.len()) {
            Some(index) => {
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
