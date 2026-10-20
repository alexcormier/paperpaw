use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::collections::VecMap;
use crate::command::Command;
use crate::env::Env;
use crate::id::Id;
use crate::params::Params;
use crate::plugin::Priority;

/// Global handler of an action, called when the active screen does not
/// handle it. It reacts by emitting commands.
pub type ActionHandler = Box<dyn Fn(&Env<'_>, &mut Vec<Command>)>;

/// A handler of an action that gets the params it was run with
/// ([`Command::RunActionWith`](crate::Command::RunActionWith)), e.g. the
/// profile to apply.
pub type ActionParamsHandler = Box<dyn Fn(&Env<'_>, &Params, &mut Vec<Command>)>;

/// A handler put in place of an action's own
/// ([`Registrar::override_action`](crate::Registrar::override_action)).
#[non_exhaustive]
pub struct ActionOverride {
    pub action: Id,
    pub handler: ActionParamsHandler,
    pub priority: Priority,
}

impl fmt::Debug for ActionOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionOverride")
            .field("action", &self.action)
            .field("priority", &self.priority)
            .finish_non_exhaustive()
    }
}

/// Sees every action run, or every event emitted, with its params
/// (see [`ObserverSpec`]).
pub type Observer = Box<dyn Fn(&Env<'_>, &Id, &Params, &mut Vec<Command>)>;

/// Something that sees every action run
/// ([`Registrar::observe_actions`](crate::Registrar::observe_actions)) or
/// every event emitted
/// ([`Registrar::observe_events`](crate::Registrar::observe_events)): a
/// macro recorder, a rule on any event. It runs for each, so it does
/// little; an observer of events must not emit an event for every event
/// it sees.
#[non_exhaustive]
pub struct ObserverSpec {
    pub id: Id,
    pub observer: Observer,
}

impl ObserverSpec {
    pub fn new(
        id: Id,
        observer: impl Fn(&Env<'_>, &Id, &Params, &mut Vec<Command>) + 'static,
    ) -> Self {
        Self {
            id,
            observer: Box::new(observer),
        }
    }
}

impl fmt::Debug for ObserverSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObserverSpec")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

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
    /// What it does with the params it was run with, in place of
    /// [`handler`](Self::handler) ([`ActionSpec::handler_with`]).
    pub handler_with: Option<ActionParamsHandler>,
    /// How the row of hints shows it where a screen leaves that to the
    /// action ([`Hint::Default`](crate::Hint::Default)).
    pub hint: ActionHint,
    /// Translation key of what it does, in a sentence, for lists of
    /// actions to choose from.
    pub description: Option<Cow<'static, str>>,
    /// Translation key of words it is found by in a search of actions,
    /// separated by commas.
    pub keywords: Option<Cow<'static, str>>,
}

impl ActionSpec {
    /// A screen action `id`, called `title` (a translation key).
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            handler: None,
            handler_with: None,
            hint: ActionHint::Title,
            description: None,
            keywords: None,
        }
    }

    /// What it does where the screen shown does not take it.
    pub fn handler(mut self, handler: impl Fn(&Env<'_>, &mut Vec<Command>) + 'static) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// What it does where the screen shown does not take it, with the
    /// params it was run with
    /// ([`Command::RunActionWith`](crate::Command::RunActionWith); none for
    /// [`Command::RunAction`](crate::Command::RunAction) and inputs).
    pub fn handler_with(
        mut self,
        handler: impl Fn(&Env<'_>, &Params, &mut Vec<Command>) + 'static,
    ) -> Self {
        self.handler_with = Some(Box::new(handler));
        self
    }

    /// How the row of hints shows it.
    pub fn hint(mut self, hint: ActionHint) -> Self {
        self.hint = hint;
        self
    }

    /// What it does, in a sentence (a translation key).
    pub fn describe(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Words it is found by (a translation key; its text separated by
    /// commas).
    pub fn keywords(mut self, keywords: impl Into<Cow<'static, str>>) -> Self {
        self.keywords = Some(keywords.into());
        self
    }

    /// Whether it does something itself, without a screen that takes it.
    pub fn runs_itself(&self) -> bool {
        self.handler.is_some() || self.handler_with.is_some()
    }
}

impl fmt::Debug for ActionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("handler", &self.runs_itself())
            .field("hint", &self.hint)
            .field("description", &self.description)
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
#[derive(Default)]
pub struct Actions {
    map: VecMap<Id, ActionSpec>,
    /// Handlers plugins put in place of actions' own, the highest
    /// priority first: the action, the plugin, the handler.
    overrides: Vec<(Id, Priority, Id, ActionParamsHandler)>,
}

impl fmt::Debug for Actions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let overrides: Vec<&Id> = self.overrides.iter().map(|(action, ..)| action).collect();
        f.debug_struct("Actions")
            .field("map", &self.map)
            .field("overrides", &overrides)
            .finish()
    }
}

impl Actions {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
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

    /// Whether action `id` runs without a screen that takes it: it has a
    /// handler, or a plugin put one in place of its own. Else it is a
    /// screen action (or none), e.g. for a palette of actions to run.
    pub fn runnable(&self, id: &str) -> bool {
        self.override_of(id).is_some() || self.get(id).is_some_and(ActionSpec::runs_itself)
    }

    /// Puts `handler` of `plugin` in place of action `action`'s own,
    /// at `priority` (see
    /// [`Registrar::override_action`](crate::Registrar::override_action)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_override(
        &mut self,
        action: Id,
        priority: Priority,
        plugin: Id,
        handler: ActionParamsHandler,
    ) {
        let at = self
            .overrides
            .iter()
            .position(|(_, p, ..)| *p < priority)
            .unwrap_or(self.overrides.len());
        self.overrides
            .insert(at, (action, priority, plugin, handler));
    }

    /// The handler put in place of action `id`'s own with the highest
    /// priority, and the plugin whose it is.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn override_of(&self, id: &str) -> Option<(&Id, &ActionParamsHandler)> {
        self.overrides
            .iter()
            .find(|(action, ..)| action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    #[cfg(not(any(test, feature = "kernel")))]
    fn override_of(&self, id: &str) -> Option<(&Id, &ActionParamsHandler)> {
        self.overrides
            .iter()
            .find(|(action, ..)| action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.map.shrink_to_fit();
        self.overrides.shrink_to_fit();
    }
}
