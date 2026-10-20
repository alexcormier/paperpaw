//! The plugin manager: lists every plugin with its name, version and
//! description, and switches plugins on and off.
//!
//! - Each row shows a plugin and its state: *On*, *Off*, *Always on* (the
//!   kernel), *Required* (it alone provides e.g. the settings or the menu,
//!   so switching it off would leave no way back; once another plugin
//!   provides them too, it can be switched off) or *Error*.
//! - Confirming or tapping a row switches the plugin. The choice is saved
//!   to `paperpaw.ini` (`[plugins] id = off`) and applies after a restart:
//!   the bar at the bottom then offers *Restart now*. The bar is always
//!   there (with a hint before), so nothing moves when it changes.
//! - Below each name: what happens to the plugin (if anything), its
//!   description and version, readable without tapping it. Plugins with
//!   native code (ADR 0024) say so, and why one does not run (built for
//!   another PaperPaw, switched off by safe mode, …).
//! - After a start in safe mode (without code plugins), a notice says why
//!   and which plugin was loaded last; the plugins it switched off are
//!   switched on again here, one at a time.
//!
//! - Above the plugins: where more come from, e.g. the store
//!   ([`MORE`]): a row each, which opens it.
//!
//! The manager is itself a plugin and reached from the main settings page
//! (`core.settings-pages`); another plugin can replace it.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{CORE_PLUGIN, actions, events, menus};
use paperpaw_sdk::native::{NotInstalled, NotInstalledReason, SafeMode, SafeModeReason};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    ActionSpec, Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Manifest,
    MenuEntry, Params, Plugin, PluginInfo, PluginKind, PluginProblem, PluginState, Rect,
    RefreshMode, RegisterError, Registrar, SDK_VERSION, Screen, ScreenSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("plugin-manager");
/// The list of plugins.
pub const SCREEN: Id = Id::new_static("plugin-manager.list");
/// Opens [`SCREEN`].
pub const OPEN: Id = Id::new_static("plugin-manager.open");
/// The menu of places to get more plugins (a store): plugins add their
/// actions with [`MenuEntry`]; each is a row above the plugins.
pub const MORE: Id = Id::new_static("plugin-manager.more");

/// The plugin manager plugin.
#[derive(Debug, Default)]
pub struct PluginManager;

impl Plugin for PluginManager {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Plugin manager"),
            description: Cow::Borrowed("Lists the plugins and switches them on and off."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec {
            id: SCREEN,
            title: Cow::Borrowed("plugin-manager.title"),
            factory: Box::new(|env, _| Box::new(PluginList::new(env))),
        })?;
        r.add_action(ActionSpec::new(OPEN, "plugin-manager.title").handler(
            |_: &Env<'_>, out: &mut Vec<Command>| {
                out.push(Command::OpenScreen(SCREEN, Params::new()));
            },
        ))?;
        r.add_menu_entry(MenuEntry {
            menu: menus::SETTINGS,
            item: OPEN,
            order: 220,
        })?;
        r.add_icon(OPEN, Cow::Borrowed("puzzle"))?;
        r.on(events::STARTED, |env, _, out| {
            let Some(native) = env.native else {
                return;
            };
            // One notice shows at a time: safe mode's, the more
            // important, last.
            if let Some(notice) = not_installed_notice(env, &native.not_installed) {
                out.push(Command::Notify(notice));
            }
            if let Some(safe_mode) = &native.safe_mode {
                out.push(Command::Notify(safe_mode_notice(env, safe_mode)));
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// Where the focus is, for buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    /// The *Restart now* button of the bar.
    Restart,
}

#[derive(Debug)]
struct PluginList {
    /// The actions of [`MORE`], above the plugins.
    more: Vec<Id>,
    /// Plugin ids: the kernel first, then by name.
    plugins: Vec<Id>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The bar at the bottom: a hint, or *Restart now*.
    bar: Rect,
    focus: Focus,
}

impl PluginList {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device.display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area.height + style.gap as u16;
        let bar_height = style.row as u16 + style.gap as u16;
        let bar = Rect::new(
            0,
            display.height.saturating_sub(bar_height),
            display.width,
            bar_height,
        );
        let height = display.height.saturating_sub(top + bar_height);
        let area = Rect::new(0, top, display.width, height);
        let list = ListView::new(area, style.noted_row()).focus_on_keys(env);
        let mut manager = Self {
            more: Vec::new(),
            plugins: Vec::new(),
            style,
            header,
            list,
            bar,
            focus: Focus::List,
        };
        manager.read_rows(env);
        manager
    }

    /// The rows as they are now: plugins come and go while the manager is
    /// open (e.g. installed from a store opened from it).
    fn read_rows(&mut self, env: &Env<'_>) {
        self.more = env
            .menus
            .items(MORE.as_str())
            .into_iter()
            .filter(|item| !item.section && env.actions.contains(item.id.as_str()))
            .map(|item| item.id.clone())
            .collect();
        let mut plugins: Vec<&PluginInfo> = env.plugins.iter().collect();
        plugins.sort_by_cached_key(|p| {
            (
                p.manifest.id != CORE_PLUGIN,
                p.name(env.i18n).to_lowercase(),
            )
        });
        self.plugins = plugins.iter().map(|p| p.manifest.id.clone()).collect();
    }

    /// The *Restart now* button, while a change waits for a restart.
    fn restart_button(&self, env: &Env<'_>) -> Option<Rect> {
        if !env.plugins.restart_needed() {
            return None;
        }
        let font = self.style.font(env)?;
        let label = env.i18n.tr("plugin-manager.restart");
        let pad = self.style.margin as u16;
        let width = font.measure(self.style.text, label) as u16 + 2 * pad;
        let height = ui::button_height(self.bar);
        Some(Rect::new(
            self.bar.width.saturating_sub(width + pad),
            self.bar.y + (self.bar.height - height) / 2,
            width,
            height,
        ))
    }

    /// The rows: the places to get more, then the plugins.
    fn len(&self) -> usize {
        self.more.len() + self.plugins.len()
    }

    /// Opens a place to get more, or switches a plugin: what row `index`
    /// does.
    fn activate(&mut self, env: &Env<'_>, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some(action) = self.more.get(index) {
            cx.command(Command::RunAction(action.clone()));
        } else if let Some(id) = self.plugins.get(index - self.more.len()).cloned() {
            self.switch(env, &id, cx);
        }
    }

    /// Switches the plugin `id`.
    fn switch(&mut self, env: &Env<'_>, id: &Id, cx: &mut Cx<'_, '_>) {
        let Some(info) = env.plugins.get(id.as_str()) else {
            return;
        };
        if info.enabled && locked(info) {
            // The description below says why.
            return;
        }
        cx.command(Command::SetPluginEnabled(id.clone(), !info.enabled));
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }
}

/// Whether the plugin must stay on: the kernel says so, or it is this
/// manager (without it, plugins could not be switched on again).
fn locked(info: &PluginInfo) -> bool {
    !info.can_switch_off() || info.manifest.id == PLUGIN
}

/// The state shown on the right of a row.
fn state(info: &PluginInfo) -> &'static str {
    if info.manifest.id == CORE_PLUGIN {
        "plugin-manager.always-on"
    } else if info.state == PluginState::Failed && info.enabled {
        "plugin-manager.error"
    } else if locked(info) {
        "plugin-manager.required"
    } else if info.enabled {
        "plugin-manager.on"
    } else {
        "plugin-manager.off"
    }
}

/// The note below a plugin's name: what happens to it, its description
/// and version.
fn details(env: &Env<'_>, info: &PluginInfo) -> String {
    let i18n = env.i18n;
    let version = i18n.format(
        "plugin-manager.version",
        &[("version", Arg::Str(&info.manifest.version.to_string()))],
    );
    let note = if info.manifest.id == CORE_PLUGIN {
        None
    } else if let Some(problem) = info.problem {
        // Switched off by safe mode, or only held back at this start.
        let key = match problem {
            PluginProblem::SafeMode if !info.enabled => "plugin-manager.code.safe-mode-off",
            _ => problem_key(problem),
        };
        Some(i18n.tr(key).to_string())
    } else if info.manifest.id == PLUGIN && info.required_for.is_empty() {
        Some(i18n.tr("plugin-manager.self").to_string())
    } else if !info.required_for.is_empty() {
        let what: Vec<&str> = info
            .required_for
            .iter()
            .map(|slot| i18n.tr(slot_key(slot)))
            .collect();
        Some(i18n.format(
            "plugin-manager.needed-for",
            &[("what", Arg::Str(&what.join(", ")))],
        ))
    } else if let Some(update) = info.update {
        Some(i18n.format(
            "plugin-manager.updates-after-restart",
            &[("version", Arg::Str(&update.to_string()))],
        ))
    } else if info.restart_needed() {
        Some(
            i18n.tr(if info.enabled {
                "plugin-manager.starts-after-restart"
            } else {
                "plugin-manager.stops-after-restart"
            })
            .to_string(),
        )
    } else if info.state == PluginState::Failed {
        Some(i18n.tr("plugin-manager.failed").to_string())
    } else {
        None
    };
    let description = info.description(i18n);
    let code = if info.kind == PluginKind::Code {
        i18n.tr("plugin-manager.code")
    } else {
        ""
    };
    [
        note.as_deref().unwrap_or_default(),
        description,
        code,
        &version,
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// What a start in safe mode says: why, the plugin loaded last, what it
/// switched off.
fn safe_mode_notice(env: &Env<'_>, safe_mode: &SafeMode) -> String {
    let i18n = env.i18n;
    let name = |id: &str| {
        env.plugins
            .get(id)
            .map_or_else(|| String::from(id), |p| String::from(p.name(i18n)))
    };
    // Short: a notice has a few lines. The plugin manager says the rest.
    match (safe_mode.reason, &safe_mode.crashed, &safe_mode.last_loaded) {
        (SafeModeReason::KeyHeld, ..) => String::from(i18n.tr("plugin-manager.safe-mode.key")),
        (_, Some(id), _) => i18n.format(
            "plugin-manager.safe-mode.crashed",
            &[("name", Arg::Str(&name(id)))],
        ),
        (_, None, Some(id)) => i18n.format(
            "plugin-manager.safe-mode.last",
            &[("name", Arg::Str(&name(id)))],
        ),
        (_, None, None) => String::from(i18n.tr("plugin-manager.safe-mode.failed")),
    }
}

/// What is said of the plugins the shell did not install at the start:
/// their names (those of the versions installed, or their ids), and why.
fn not_installed_notice(env: &Env<'_>, not_installed: &[NotInstalled]) -> Option<String> {
    let name = |n: &NotInstalled| {
        let id = n.plugin.as_str();
        env.plugins
            .get(id)
            .map_or_else(|| String::from(id), |p| String::from(p.name(env.i18n)))
    };
    let (key, names) = match not_installed {
        [] => return None,
        [one] => (
            match one.reason {
                NotInstalledReason::OtherAuthor => "plugin-manager.not-installed.other-author",
                _ => "plugin-manager.not-installed.damaged",
            },
            name(one),
        ),
        several => (
            "plugin-manager.not-installed.several",
            several.iter().map(name).collect::<Vec<_>>().join(", "),
        ),
    };
    Some(env.i18n.format(key, &[("name", Arg::Str(&names))]))
}

/// Why a code plugin does not run, as said below its name.
fn problem_key(problem: PluginProblem) -> &'static str {
    match problem {
        PluginProblem::OtherBuild => "plugin-manager.code.other-build",
        PluginProblem::Damaged => "plugin-manager.code.damaged",
        PluginProblem::Mismatch => "plugin-manager.code.mismatch",
        PluginProblem::NoRoom => "plugin-manager.code.no-room",
        PluginProblem::Unsupported => "plugin-manager.code.unsupported",
        PluginProblem::SafeMode => "plugin-manager.code.safe-mode",
        PluginProblem::Crashed => "plugin-manager.code.crashed",
        PluginProblem::NotInstalled => "plugin-manager.code.not-installed",
        PluginProblem::Changed => "plugin-manager.code.changed",
        _ => "plugin-manager.failed",
    }
}

/// Translation key of a standard slot's name.
fn slot_key(slot: &Id) -> &'static str {
    match slot.as_str() {
        "core.home" => "plugin-manager.slot.home",
        "core.reader" => "plugin-manager.slot.reader",
        "core.library" => "plugin-manager.slot.library",
        "core.menu" => "plugin-manager.slot.menu",
        "core.settings" => "plugin-manager.slot.settings",
        "core.font-ui" => "plugin-manager.slot.fonts",
        _ => "plugin-manager.slot.other",
    }
}

impl Screen for PluginList {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        self.list.focus = self.list.focus.min(len.saturating_sub(1));
        if self.focus == Focus::Restart && self.restart_button(env).is_none() {
            self.focus = Focus::List;
        }
        self.list.active = self.focus == Focus::List;
        self.header.draw(
            canvas,
            env,
            &style,
            env.i18n.tr("plugin-manager.title"),
            None,
            self.list.paging(len),
        );
        let (more, plugins) = (&self.more, &self.plugins);
        self.list
            .draw(canvas, &style, len, |canvas, index, rect, focused| {
                if let Some(action) = more.get(index) {
                    // Named by its action, told by the plugin that added it.
                    let name = env.actions.get(action.as_str()).map_or("", |a| &a.title);
                    let note = env
                        .plugins
                        .owner(action)
                        .map_or("", |p| p.description(env.i18n));
                    let name = env.i18n.tr(name);
                    ui::draw_noted_row(canvas, env, &style, rect, focused, name, "›", note);
                } else if let Some(info) = env.plugins.get(plugins[index - more.len()].as_str()) {
                    let value = env.i18n.tr(state(info));
                    let name = info.name(env.i18n);
                    let note = details(env, info);
                    ui::draw_noted_row(canvas, env, &style, rect, focused, name, value, &note);
                }
            });
        // The bar: a line above it, the hint or what a restart does, and
        // the button.
        let line = Rect::new(0, self.bar.y, self.bar.width, (style.line as u16).max(1));
        canvas.fill(line, paperpaw_sdk::Color::gray(150));
        let button = self.restart_button(env);
        if let Some(button) = button {
            let label = env.i18n.tr("plugin-manager.restart");
            let focused = self.focus == Focus::Restart;
            ui::draw_button(canvas, env, &style, button, label, true, focused);
        }
        if let Some(font) = style.font(env) {
            let key = if button.is_some() {
                "plugin-manager.restart.short"
            } else {
                "plugin-manager.hint"
            };
            let right = button.map_or(self.bar.width, |b| b.x);
            let area = Rect::new(
                0,
                self.bar.y,
                right.saturating_sub(style.gap as u16),
                self.bar.height,
            );
            ui::draw_label(
                canvas,
                &font,
                style.small,
                style.margin as f32,
                area,
                ui::tr_input(env, key),
                style.faint(60),
            );
        }
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.read_rows(&env);
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        if *event == events::PLUGINS_CHANGED {
            let env = *cx.env();
            self.read_rows(&env);
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Partial);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let len = self.len();
        let page = self.list.rows() as isize;
        let a = action;
        // Down from the last plugin reaches the bar's button, up comes back.
        let restart = self.restart_button(&env).is_some();
        if self.focus == Focus::Restart {
            if *a == actions::CONFIRM {
                cx.command(Command::Restart);
            } else if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
                self.focus = Focus::List;
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            } else if *a != actions::DOWN && *a != actions::RIGHT && *a != actions::NEXT {
                return Handled::No;
            }
            return Handled::Yes;
        }
        let down = [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a);
        if down && restart && self.list.focus + 1 >= len {
            self.focus = Focus::Restart;
            cx.request_redraw();
            cx.request_refresh(RefreshMode::Fast);
            return Handled::Yes;
        }
        let delta = if *a == actions::UP || *a == actions::LEFT || *a == actions::PREVIOUS {
            -1
        } else if *a == actions::DOWN || *a == actions::RIGHT || *a == actions::NEXT {
            1
        } else if *a == actions::NEXT_PAGE {
            page
        } else if *a == actions::PREV_PAGE {
            -page
        } else if *a == actions::CONFIRM {
            self.activate(&env, self.list.focus, cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.len());
        let tapped = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging);
        if let Some(action) = tapped {
            cx.command(Command::RunAction(action));
            return Handled::Yes;
        }
        let env = *cx.env();
        if self
            .restart_button(&env)
            .is_some_and(|b| b.contains(gesture.start))
        {
            cx.command(Command::Restart);
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.len()) else {
            return Handled::No;
        };
        self.list.focus = index;
        self.focus = Focus::List;
        self.activate(&env, index, cx);
        Handled::Yes
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    use super::*;

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn standard_slots_have_names() {
        for slot in [
            "core.home",
            "core.reader",
            "core.library",
            "core.menu",
            "core.settings",
            "core.font-ui",
        ] {
            assert_ne!(slot_key(&Id::new_static(slot)), "plugin-manager.slot.other");
        }
    }
}
