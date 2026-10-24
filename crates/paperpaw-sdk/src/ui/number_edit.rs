//! Changing a number setting with the buttons, as the settings pages do
//! it, so that every screen that shows a number does it the same way:
//! Select starts changing it; the arrow and page keys move it, shown at
//! once (a preview); Select keeps the value shown, Back goes back to the
//! one before. The hint bar says so meanwhile.
//!
//! Touch screens change a number with its `−` and `+` at once and need
//! none of this.

use alloc::string::{String, ToString};
use alloc::vec;

use crate::builtin::actions;
use crate::id::Id;
use crate::screen::Hint;
use crate::settings::{SettingSpec, SettingValue};
use crate::ui::{Response, next_value, slider};
use crate::{Command, Cx, Env, RefreshMode};

/// A number being changed with the buttons, or none.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct NumberEdit {
    /// The value before it was changed, while it is.
    before: Option<SettingValue>,
}

impl NumberEdit {
    /// Whether a number is being changed.
    pub fn active(&self) -> bool {
        self.before.is_some()
    }

    /// Starts changing `spec` with the buttons.
    pub fn start(&mut self, env: &Env<'_>, spec: &SettingSpec, cx: &mut Cx<'_, '_>) {
        self.before = env.settings().get(spec.id.as_str()).cloned();
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    /// What `action` does while `spec` is being changed: moves it
    /// ([`Response::Changed`]), keeps it (Select) or goes back to the
    /// value before (Back), [`Response::Done`] with whether it was kept.
    /// Ignored while nothing is being changed, and for the actions it
    /// leaves to the screen. It asks for its own drawing.
    pub fn on_action(
        &mut self,
        env: &Env<'_>,
        spec: Option<&SettingSpec>,
        action: &Id,
        cx: &mut Cx<'_, '_>,
    ) -> Response<bool> {
        if !self.active() {
            return Response::Ignored;
        }
        if let Some(step) = slider::key_step(action) {
            if let Some(spec) = spec {
                preview_step(env, spec, step, cx);
            }
            Response::Changed
        } else if *action == actions::CONFIRM || *action == actions::BACK {
            let keep = *action == actions::CONFIRM;
            self.finish(env, spec, keep, cx);
            Response::Done(keep)
        } else {
            Response::Ignored
        }
    }

    /// Ends changing `spec`: keeps the value shown (`keep`), or shows the
    /// one before again.
    pub fn finish(
        &mut self,
        env: &Env<'_>,
        spec: Option<&SettingSpec>,
        keep: bool,
        cx: &mut Cx<'_, '_>,
    ) {
        let before = self.before.take();
        if keep
            && let Some(spec) = spec
            && let Some(shown) = env.settings().get(spec.id.as_str())
            && Some(shown) != before.as_ref()
        {
            cx.command(Command::SetSetting(spec.id.clone(), shown.clone()));
        }
        end_preview(env, cx);
        // Quick while it moved; now with its grays again.
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }

    /// Stops changing a number without keeping what was shown, e.g. for a
    /// tap elsewhere or another page.
    pub fn leave(&mut self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        if self.before.take().is_some() {
            end_preview(env, cx);
        }
    }

    /// The hint bar's text for `action` while a number is being changed:
    /// what each key does to it; [`Hint::Default`] otherwise.
    pub fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        if !self.active() {
            return Hint::Default;
        }
        editing_hint(action, env)
    }
}

/// The hint bar's text for `action` while the buttons change a number
/// (or move a slider): `−`, `+`, *Keep* for Select, *Cancel* for Back.
pub fn editing_hint(action: &Id, env: &Env<'_>) -> Hint {
    match slider::key_step(action) {
        Some(step) if step < 0 => Hint::Text(String::from("−")),
        Some(_) => Hint::Text(String::from("+")),
        None if *action == actions::CONFIRM => {
            Hint::Text(env.i18n().tr("core.hint.keep").to_string())
        }
        None if *action == actions::BACK => {
            Hint::Text(env.i18n().tr("core.hint.cancel").to_string())
        }
        None => Hint::Default,
    }
}

/// Moves `spec` by `step`: shown at once, kept when chosen.
fn preview_step(env: &Env<'_>, spec: &SettingSpec, step: i64, cx: &mut Cx<'_, '_>) {
    let current = env.settings().get(spec.id.as_str());
    if let Some(value) = next_value(env, spec, step).filter(|v| Some(v) != current) {
        cx.command(Command::PreviewSettings(vec![(
            spec.id.clone(),
            Some(value),
        )]));
    }
}

/// Ends showing settings changed but not kept.
fn end_preview(env: &Env<'_>, cx: &mut Cx<'_, '_>) {
    if env.settings().previewing() {
        cx.command(Command::PreviewSettings(alloc::vec::Vec::new()));
    }
}
