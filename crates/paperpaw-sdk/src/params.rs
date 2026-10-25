use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

/// Named string parameters passed with commands and events, e.g. the path
/// of the book a reader screen should open.
///
/// Parameters are plain strings so they can cross a message boundary
/// unchanged. An event may also carry its data as it is, unwritten (see
/// [`Params::carrying`]): data too large to copy, such as an HTTP
/// answer's body.
#[derive(Clone, Default)]
pub struct Params {
    values: Vec<(Cow<'static, str>, String)>,
    carried: Option<Rc<dyn Any>>,
}

/// As the named strings are, in any order: what is carried is not
/// compared.
impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl Eq for Params {}

impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Params").field(&self.values).finish()
    }
}

impl Params {
    pub const fn new() -> Self {
        Self {
            values: Vec::new(),
            carried: None,
        }
    }

    /// The same, carrying `value` as it is, unwritten: an event's data
    /// that a hearer of its type reads without a copy (see
    /// [`EventData::into_params`](crate::event::EventData::into_params)).
    #[must_use]
    pub fn carrying<T: Any>(mut self, value: T) -> Self {
        self.carried = Some(Rc::new(value));
        self
    }

    /// The value carried, if one of type `T` is.
    pub fn carried<T: Any>(&self) -> Option<&T> {
        self.carried.as_deref()?.downcast_ref()
    }

    /// Adds or replaces a parameter.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.set(key, value);
        self
    }

    pub fn set(&mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.values.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.values.push((key, value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_ref(), v.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_replace() {
        let params = Params::new().with("path", "a.txt").with("path", "b.txt");
        assert_eq!(params.get("path"), Some("b.txt"));
        assert_eq!(params.get("other"), None);
        assert_eq!(params.iter().count(), 1);
    }
}
