use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::env::Env;
use crate::id::Id;
use crate::params::Params;
use crate::plugin::Priority;

/// Global handler of an action, called when the active screen does not
/// handle it, with the params it was run with
/// ([`Command::RunAction`](crate::Command::RunAction); none from inputs),
/// e.g. the profile to apply. It reacts by queuing commands.
pub type ActionHandler = Box<dyn Fn(&Env<'_>, &Params, &mut crate::Commands)>;

/// A handler put in place of an action's own
/// ([`Registrar::override_action`](crate::Registrar::override_action)).
#[non_exhaustive]
pub struct ActionOverride {
    pub action: Id,
    pub handler: ActionHandler,
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
pub type Observer = Box<dyn Fn(&Env<'_>, &Id, &Params, &mut crate::Commands)>;

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
        id: impl Into<Id>,
        observer: impl Fn(&Env<'_>, &Id, &Params, &mut crate::Commands) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
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
///     .handler(|_, _, out| out.push(Command::Back))
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
    /// What few actions have, kept aside so that the others stay small.
    extra: Option<Box<ActionExtra>>,
}

/// What few actions have ([`ActionSpec`]).
#[derive(Default)]
struct ActionExtra {
    description: Option<Cow<'static, str>>,
    keywords: Option<Cow<'static, str>>,
    sealed: bool,
}

impl ActionSpec {
    /// A screen action `id`, called `title` (a translation key).
    pub fn new(id: impl Into<Id>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            handler: None,
            hint: ActionHint::Title,
            extra: None,
        }
    }

    fn extra(&mut self) -> &mut ActionExtra {
        self.extra.get_or_insert_with(Default::default)
    }

    /// What it does where the screen shown does not take it, with the
    /// params it was run with ([`Command::RunAction`](crate::Command::RunAction);
    /// none from inputs).
    pub fn handler(
        mut self,
        handler: impl Fn(&Env<'_>, &Params, &mut crate::Commands) + 'static,
    ) -> Self {
        self.handler = Some(Box::new(handler));
        self
    }

    /// How the row of hints shows it.
    pub fn hint(mut self, hint: ActionHint) -> Self {
        self.hint = hint;
        self
    }

    /// What it does, in a sentence (a translation key), for lists of
    /// actions to choose from.
    pub fn describe(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.extra().description = Some(description.into());
        self
    }

    /// Words it is found by in a search of actions (a translation key;
    /// its text separated by commas).
    pub fn keywords(mut self, keywords: impl Into<Cow<'static, str>>) -> Self {
        self.extra().keywords = Some(keywords.into());
        self
    }

    /// No handler of another plugin is put in its place
    /// ([`Registrar::override_action`](crate::Registrar::override_action)):
    /// for actions that keep the device safe, such as locking it.
    pub fn sealed(mut self) -> Self {
        self.extra().sealed = true;
        self
    }

    /// What it does ([`ActionSpec::describe`]).
    pub fn description(&self) -> Option<&str> {
        self.extra.as_ref()?.description.as_deref()
    }

    /// The words it is found by ([`ActionSpec::keywords`]).
    pub fn search_keywords(&self) -> Option<&str> {
        self.extra.as_ref()?.keywords.as_deref()
    }

    /// Whether no other handler is put in its place ([`ActionSpec::sealed`]).
    pub fn is_sealed(&self) -> bool {
        self.extra.as_ref().is_some_and(|extra| extra.sealed)
    }

    /// Whether it does something itself, without a screen that takes it.
    pub fn runs_itself(&self) -> bool {
        self.handler.is_some()
    }
}

impl fmt::Debug for ActionSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("handler", &self.runs_itself())
            .field("hint", &self.hint)
            .field("description", &self.description())
            .field("sealed", &self.is_sealed())
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
    /// Sorted by id; each spec's own id is the key (not kept twice: there
    /// are some hundred).
    specs: Vec<ActionSpec>,
    /// Handlers plugins put in place of actions' own, the highest
    /// priority first: the action, the plugin, the handler.
    overrides: Vec<(Id, Priority, Id, ActionHandler)>,
}

impl fmt::Debug for Actions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let overrides: Vec<&Id> = self.overrides.iter().map(|(action, ..)| action).collect();
        f.debug_struct("Actions")
            .field("specs", &self.specs)
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
        match self.find(action.id.as_str()) {
            Ok(at) => self.specs[at] = action,
            Err(at) => self.specs.insert(at, action),
        }
    }

    fn find(&self, id: &str) -> Result<usize, usize> {
        self.specs.binary_search_by(|spec| spec.id.as_str().cmp(id))
    }

    pub fn get(&self, id: &str) -> Option<&ActionSpec> {
        self.find(id).ok().map(|at| &self.specs[at])
    }

    pub fn contains(&self, id: &str) -> bool {
        self.find(id).is_ok()
    }

    pub fn iter(&self) -> impl Iterator<Item = &ActionSpec> {
        self.specs.iter()
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
        handler: ActionHandler,
    ) {
        let at = self
            .overrides
            .iter()
            .position(|(_, p, ..)| *p < priority)
            .unwrap_or(self.overrides.len());
        self.overrides
            .insert(at, (action, priority, plugin, handler));
    }

    /// Lets go of the handlers put in place of sealed actions (made before
    /// the sealed action was known); the actions they were for.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn drop_sealed_overrides(&mut self) -> Vec<Id> {
        let mut dropped = Vec::new();
        let specs = &self.specs;
        self.overrides.retain(|(action, ..)| {
            let sealed = specs
                .binary_search_by(|spec| spec.id.as_str().cmp(action.as_str()))
                .is_ok_and(|at| specs[at].is_sealed());
            if sealed {
                dropped.push(action.clone());
            }
            !sealed
        });
        dropped
    }

    /// The handler put in place of action `id`'s own with the highest
    /// priority, and the plugin whose it is.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn override_of(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        self.first_override(id)
    }

    #[cfg(not(any(test, feature = "kernel")))]
    fn override_of(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        self.first_override(id)
    }

    fn first_override(&self, id: &str) -> Option<(&Id, &ActionHandler)> {
        if self.get(id).is_some_and(ActionSpec::is_sealed) {
            return None;
        }
        self.overrides
            .iter()
            .find(|(action, ..)| action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    /// The handlers put in place of action `id`'s own, the highest
    /// priority first, with their plugins; none for a sealed action.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn overrides_of<'a>(
        &'a self,
        id: &'a str,
    ) -> impl Iterator<Item = (&'a Id, &'a ActionHandler)> + 'a {
        let sealed = self.get(id).is_some_and(ActionSpec::is_sealed);
        self.overrides
            .iter()
            .filter(move |(action, ..)| !sealed && action.as_str() == id)
            .map(|(_, _, plugin, handler)| (plugin, handler))
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.specs.shrink_to_fit();
        self.overrides.shrink_to_fit();
    }
}
