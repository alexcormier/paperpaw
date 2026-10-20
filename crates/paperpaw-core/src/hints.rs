//! The row of hints over the buttons below the screen (where the device
//! has a row of buttons there): what each button does in the screen
//! shown, below a screen that keeps the row, or over its bottom for a
//! while after a press.

use alloc::borrow::Cow;
use alloc::string::ToString;
use alloc::vec::Vec;

use paperpaw_platform::{Instant, Rect, RefreshMode};
use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::{ActionHint, Hint, Hints, Id};

use crate::keymap::InputKey;
use crate::{ActiveScreen, HINTS_MS, Kernel};

/// What the row is, and what it shows.
#[derive(Debug)]
pub(crate) struct HintRow {
    /// The buttons in a row below the screen (see
    /// [`KernelBuilder::button_row`](crate::KernelBuilder::button_row)).
    pub buttons: &'static [&'static str],
    /// The height of the row, 0 while it is off.
    pub height: u16,
    /// The cell whose button was pressed in this call.
    pub pressed: Option<usize>,
    /// What the row shows, as it was last drawn.
    pub shown: Vec<Hint>,
    /// Whether holding each button does something too, as last drawn.
    pub held: Vec<bool>,
    /// Until when the row shows over a screen that has it only on a press.
    pub until: Option<Instant>,
}

impl HintRow {
    pub fn new(buttons: &'static [&'static str]) -> Self {
        Self {
            buttons,
            height: 0,
            pressed: None,
            shown: Vec::new(),
            held: Vec::new(),
            until: None,
        }
    }
}

impl Kernel {
    /// The row of hints, while it shows: below a screen that keeps it, or
    /// over the bottom of one that has it on a press, for a while.
    pub(crate) fn hint_row(&self) -> Option<Rect> {
        let panel = self.framebuffer.bounds();
        let top = self.shared.device.display.height;
        if top < panel.height {
            return Some(Rect::new(0, top, panel.width, panel.height - top));
        }
        let height = self.hint.height.min(panel.height / 2);
        (height > 0 && self.hint.until.is_some_and(|until| self.now < until))
            .then(|| Rect::new(0, panel.height - height, panel.width, height))
    }

    /// When the full screen shown has the row of hints.
    pub(crate) fn base_hints(&self) -> Hints {
        let env = self.shared.env(self.now);
        self.stack
            .iter()
            .rev()
            .find(|s| s.screen.region(&env).is_none())
            .map_or(Hints::Shown, |s| self.hints_of(s))
    }

    /// When open screen `active` shows the row: as it says, else as its
    /// plugin said for it.
    pub(crate) fn hints_of(&self, active: &ActiveScreen) -> Hints {
        active
            .screen
            .hints()
            .unwrap_or_else(|| self.registry.hints_of(&active.id))
    }

    /// Shows the row for a while over a screen that has it on a press.
    pub(crate) fn show_hints(&mut self) {
        if self.hint.height == 0 || self.base_hints() != Hints::OnPress {
            return;
        }
        self.hint.until = Some(self.now.add_millis(HINTS_MS));
        self.hint.shown.clear();
        self.output.request_refresh(RefreshMode::Fast);
    }

    /// Takes the row away again once its time is over: the screen below
    /// is drawn there again.
    pub(crate) fn hide_hints(&mut self) {
        if !self.hint.until.is_some_and(|until| until <= self.now) {
            return;
        }
        self.hint.until = None;
        let panel = self.framebuffer.bounds();
        let height = self.hint.height.min(panel.height / 2);
        self.invalidate(Rect::new(0, panel.height - height, panel.width, height));
        self.output.request_refresh(RefreshMode::Fast);
    }

    /// The height the row of hints takes: 0 where there is no row of
    /// buttons below the screen, it is switched off or the screen is
    /// turned (the buttons are no longer below it).
    pub(crate) fn wanted_hint_height(&self) -> u16 {
        let shown = !self.hint.buttons.is_empty()
            && self
                .shared
                .settings
                .bool(core_settings::BUTTON_HINTS.as_str())
            && self.rotation() == paperpaw_platform::Rotation::Deg0;
        if shown {
            paperpaw_sdk::ui::button_hints_height(&self.shared.env(self.now))
        } else {
            0
        }
    }

    /// Makes every open screen anew when the row of hints came, went or
    /// changed its height.
    pub(crate) fn apply_hint_height(&mut self) {
        let height = self.wanted_hint_height();
        if height == self.hint.height {
            return;
        }
        self.hint.height = height;
        self.remake_screens();
    }

    /// Whether holding each button in the row does something here: it is
    /// bound to an action the screen on top shows.
    pub(crate) fn current_holds(&self) -> Vec<bool> {
        let env = self.shared.env(self.now);
        let top = self.stack.last();
        self.hint
            .buttons
            .iter()
            .map(|button| {
                let Some(action) = self.resolve(&InputKey::LongButton(Cow::Borrowed(button)))
                else {
                    return false;
                };
                match top.map_or(Hint::Default, |t| t.screen.hint(action, &env)) {
                    Hint::Default => self.default_hint(action) != Hint::Hidden,
                    hint => hint != Hint::Hidden,
                }
            })
            .collect()
    }

    /// What the row of hints shows for each button in it: what the button
    /// runs here, as the screen on top names it.
    pub(crate) fn current_hints(&self) -> Vec<Hint> {
        use paperpaw_sdk::builtin::actions;
        let env = self.shared.env(self.now);
        let top = self.stack.last();
        self.hint
            .buttons
            .iter()
            .map(|button| {
                let Some(action) = self.resolve(&InputKey::Button(Cow::Borrowed(button))) else {
                    return Hint::Hidden;
                };
                match top.map_or(Hint::Default, |t| t.screen.hint(action, &env)) {
                    // Nowhere to go back to from the first screen.
                    Hint::Default if *action == actions::BACK && self.stack.len() < 2 => {
                        Hint::Hidden
                    }
                    Hint::Default => self.default_hint(action),
                    hint => hint,
                }
            })
            .collect()
    }

    /// How the row of hints shows an action, as the action says
    /// ([`ActionHint`]); nothing for an action no plugin knows.
    pub(crate) fn default_hint(&self, action: &Id) -> Hint {
        let Some(spec) = self.shared.actions.get(action.as_str()) else {
            return Hint::Hidden;
        };
        let translator = &self.shared.translator;
        match &spec.hint {
            ActionHint::Icon(icon) => Hint::Icon(icon),
            ActionHint::Label(key) => Hint::Text(translator.tr(key).to_string()),
            ActionHint::Hidden => Hint::Hidden,
            _ => Hint::Text(translator.tr(&spec.title).to_string()),
        }
    }
}
