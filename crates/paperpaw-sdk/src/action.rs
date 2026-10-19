use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::collections::VecMap;
use crate::command::Command;
use crate::env::Env;
use crate::id::Id;

/// Global handler of an action, called when the active screen does not
/// handle it. It reacts by emitting commands.
pub type ActionHandler = Box<dyn Fn(&Env<'_>, &mut Vec<Command>)>;

/// A command users can bind to buttons, taps and gestures.
///
/// Actions without a handler are *screen actions*: their meaning depends on
/// the active screen (e.g. `core.next-page` in a reader vs. in a list).
///
/// ```
/// use paperpaw_sdk::{ActionHint, ActionSpec, Command, Id};
///
/// const OPEN: Id = Id::new_static("demo.open");
/// let spec = ActionSpec::new(OPEN, "demo.open")
///     .handler(|_, out| out.push(Command::Back))
///     .hint(ActionHint::Label("demo.open.short".into()));
/// ```
#[non_exhaustive]
pub struct ActionSpec {
    pub id: Id,
    /// Translation key of the action's display name (for key map editors).
    pub title: Cow<'static, str>,
    pub handler: Option<ActionHandler>,
    /// How the row of hints shows it where a screen leaves that to the
    /// action ([`Hint::Default`](crate::Hint::Default)).
    pub hint: ActionHint,
}

impl ActionSpec {
    /// A screen action `id`, called `title` (a translation key).
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            handler: None,
            hint: ActionHint::Title,
        }
    }

    /// What it does where the screen shown does not take it.
    pub fn handler(mut self, handler: impl Fn(&Env<'_>, &mut Vec<Command>) + 'static) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// How the row of hints shows it.
    pub fn hint(mut self, hint: ActionHint) -> Self {
        self.hint = hint;
        self
    }
}

impl fmt::Debug for ActionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("handler", &self.handler.is_some())
            .field("hint", &self.hint)
            .finish()
    }
}

/// How the row of hints below the screen shows an action, unless the
/// screen says otherwise ([`Screen::hint`](crate::Screen::hint)).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionHint {
    /// Its title.
    #[default]
    Title,
    /// A word shorter than its title (a translation key), e.g. *Select*
    /// for confirming.
    Label(Cow<'static, str>),
    /// An icon ([`icons`](crate::icons)), e.g. an arrow for moving.
    Icon(&'static str),
    /// Nothing, e.g. for an action that does nothing.
    Hidden,
}

/// Every registered action, e.g. for menu and key map editors.
#[derive(Debug, Default)]
pub struct Actions {
    map: VecMap<Id, ActionSpec>,
}

impl Actions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, action: ActionSpec) {
        self.map.insert(action.id.clone(), action);
    }

    pub fn get(&self, id: &str) -> Option<&ActionSpec> {
        self.map.get(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ActionSpec> {
        self.map.values()
    }

    /// Releases unused capacity once registration is complete.
    pub fn shrink_to_fit(&mut self) {
        self.map.shrink_to_fit();
    }
}
