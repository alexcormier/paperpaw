//! Events: notifications the kernel and plugins emit, which every
//! subscriber hears in registration order, then every open screen.
//!
//! An [`Event<P>`] names an event and the type of what it carries, so
//! emitting and hearing it are checked as the plugin is built:
//!
//! ```
//! use quireink_sdk::event::Event;
//!
//! quireink_sdk::event_data! {
//!     /// A word was saved.
//!     pub struct Saved {
//!         /// The word.
//!         pub word: String,
//!         /// How often it was saved before.
//!         pub count: u32,
//!     }
//! }
//!
//! pub const SAVED: Event<Saved> = Event::new("demo.saved");
//!
//! // Emitting: out.push(SAVED.emit(Saved { word, count }));
//! // Hearing:  r.on(SAVED, |env, saved, out| { /* saved.word */ })?;
//! // On a screen: if let Some(saved) = SAVED.read(event, params) { … }
//! ```
//!
//! Underneath, what an event carries travels as [`Params`] (named
//! strings), so it can cross to plugins in other languages and be
//! scheduled ([`Command::Schedule`]); events of other plugins, known only
//! at run time, are heard by id ([`Registrar::subscribe`](crate::Registrar::subscribe)).

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;

use crate::command::Command;
use crate::env::Env;
use crate::id::Id;
use crate::params::Params;

/// Reacts to an event by emitting commands.
///
/// Events are notifications: the kernel and plugins emit them with
/// [`Command::Emit`], and every subscriber is called in registration order.
/// Any plugin can define its own events (namespaced ids such as
/// `reader.page-turned`), which other plugins may subscribe to.
pub type EventHandler = Box<dyn Fn(&Env<'_>, &Params, &mut Vec<Command>)>;

/// What an event carries, written as and read from [`Params`]; made with
/// [`event_data!`](crate::event_data) for structs of named values.
pub trait EventData: Sized + 'static {
    fn to_params(&self) -> Params;
    /// `None` if a value it needs is missing or unreadable.
    fn from_params(params: &Params) -> Option<Self>;
}

/// Nothing: for events that only say that something happened.
impl EventData for () {
    fn to_params(&self) -> Params {
        Params::new()
    }

    fn from_params(_: &Params) -> Option<Self> {
        Some(())
    }
}

/// Named strings as they are, for events whose values vary.
impl EventData for Params {
    fn to_params(&self) -> Params {
        self.clone()
    }

    fn from_params(params: &Params) -> Option<Self> {
        Some(params.clone())
    }
}

/// A value an event carries, as text.
pub trait ParamValue: Sized {
    fn to_param(&self) -> String;
    fn from_param(text: &str) -> Option<Self>;
    /// The value when the event does not carry it: `None` makes it
    /// required (the event is not heard without it).
    fn missing() -> Option<Self> {
        None
    }
}

impl ParamValue for String {
    fn to_param(&self) -> String {
        self.clone()
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(String::from(text))
    }
}

impl ParamValue for Id {
    fn to_param(&self) -> String {
        self.as_str().to_string()
    }

    fn from_param(text: &str) -> Option<Self> {
        Id::parse(text).ok()
    }
}

impl ParamValue for bool {
    fn to_param(&self) -> String {
        String::from(if *self { "true" } else { "false" })
    }

    fn from_param(text: &str) -> Option<Self> {
        match text {
            "true" | "1" | "on" | "yes" => Some(true),
            "false" | "0" | "off" | "no" => Some(false),
            _ => None,
        }
    }
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl ParamValue for $t {
            fn to_param(&self) -> String {
                self.to_string()
            }

            fn from_param(text: &str) -> Option<Self> {
                text.trim().parse().ok()
            }
        }
    )*};
}
numbers!(u8, u16, u32, u64, usize, i32, i64, f32);

/// An optional value: `None` when the event does not carry it.
impl<T: ParamValue> ParamValue for Option<T> {
    fn to_param(&self) -> String {
        self.as_ref().map(T::to_param).unwrap_or_default()
    }

    fn from_param(text: &str) -> Option<Self> {
        Some(T::from_param(text))
    }

    fn missing() -> Option<Self> {
        Some(None)
    }
}

/// Writes `value` under `key` unless it is an absent optional one (for
/// [`event_data!`](crate::event_data)).
#[doc(hidden)]
pub fn put<T: ParamValue>(params: &mut Params, key: &'static str, value: &T) {
    let text = value.to_param();
    if !(text.is_empty() && T::missing().is_some()) {
        params.set(key, text);
    }
}

/// Reads the value under `key` (for [`event_data!`](crate::event_data)).
#[doc(hidden)]
pub fn take<T: ParamValue>(params: &Params, key: &str) -> Option<T> {
    match params.get(key) {
        Some(text) => T::from_param(text),
        None => T::missing(),
    }
}

/// A struct of named values an event carries, each a [`ParamValue`]
/// (`String`, `Id`, numbers, `bool`, or an `Option` of one); each is
/// carried under its field's name. See the [module documentation](self).
#[macro_export]
macro_rules! event_data {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $type:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        $vis struct $name {
            $(
                $(#[$field_meta])*
                $field_vis $field: $type,
            )*
        }

        impl $crate::event::EventData for $name {
            fn to_params(&self) -> $crate::Params {
                let mut params = $crate::Params::new();
                $( $crate::event::put(&mut params, stringify!($field), &self.$field); )*
                params
            }

            fn from_params(params: &$crate::Params) -> Option<Self> {
                Some(Self {
                    $( $field: $crate::event::take(params, stringify!($field))?, )*
                })
            }
        }
    };
}

/// An event: its id and what it carries (see the [module
/// documentation](self)).
pub struct Event<P: EventData = ()> {
    id: Id,
    data: PhantomData<fn() -> P>,
}

impl<P: EventData> Event<P> {
    /// The event `id`, e.g. `reader.page-turned`; checked as for
    /// [`Id::new_static`].
    pub const fn new(id: &'static str) -> Self {
        Self {
            id: Id::new_static(id),
            data: PhantomData,
        }
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }

    pub fn as_str(&self) -> &str {
        self.id.as_str()
    }

    /// The command that emits the event with `data`.
    pub fn emit(&self, data: P) -> Command {
        Command::Emit(self.id.clone(), data.to_params())
    }

    /// The command that emits it with `data` at `at` (see
    /// [`Command::Schedule`]).
    pub fn schedule(&self, data: P, at: quireink_platform::Instant) -> Command {
        Command::Schedule(self.id.clone(), data.to_params(), at)
    }

    /// What it carries, if `event` is this event (for screens'
    /// [`on_event`](crate::Screen::on_event)).
    pub fn read(&self, event: &Id, params: &Params) -> Option<P> {
        (*event == self.id)
            .then(|| P::from_params(params))
            .flatten()
    }
}

impl<P: EventData> Clone for Event<P> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            data: PhantomData,
        }
    }
}

impl<P: EventData> fmt::Debug for Event<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Event({})", self.id)
    }
}

impl<P: EventData> From<Event<P>> for Id {
    fn from(event: Event<P>) -> Id {
        event.id
    }
}

impl<P: EventData> PartialEq<Id> for Event<P> {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl<P: EventData> PartialEq for Event<P> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<P: EventData> Eq for Event<P> {}

impl<P: EventData> PartialEq<Event<P>> for Id {
    fn eq(&self, other: &Event<P>) -> bool {
        *self == other.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::event_data! {
        struct Moved {
            from: String,
            to: String,
            count: u32,
            note: Option<String>,
            done: bool,
        }
    }

    const MOVED: Event<Moved> = Event::new("demo.moved");

    #[test]
    fn data_goes_through_params_and_back() {
        let moved = Moved {
            from: "a b.epub".into(),
            to: "c.epub".into(),
            count: 3,
            note: None,
            done: true,
        };
        let Command::Emit(id, params) = MOVED.emit(moved.clone()) else {
            panic!("an emit");
        };
        assert_eq!(id, MOVED);
        assert_eq!(params.get("from"), Some("a b.epub"));
        assert_eq!(params.get("note"), None, "absent, not empty");
        assert_eq!(MOVED.read(&id, &params), Some(moved));
        // Another event, or one without what is required: not this one.
        assert_eq!(MOVED.read(&Id::new_static("demo.other"), &params), None);
        assert_eq!(MOVED.read(&id, &Params::new().with("from", "x")), None);
        let with_note = params.clone().with("note", "hi");
        assert_eq!(
            MOVED.read(&id, &with_note).and_then(|m| m.note),
            Some(String::from("hi"))
        );
    }
}
