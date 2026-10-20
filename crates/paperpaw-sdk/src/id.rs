use alloc::borrow::{Borrow, Cow};
use alloc::string::String;
use core::fmt;

/// A namespaced identifier such as `core.home` or `diagnostics.screen`.
///
/// Ids consist of one or more segments of lowercase ASCII letters, digits
/// and `-`, separated by `.`. The first segment is the namespace: the id of
/// the plugin that owns the contribution.
///
/// A segment may end in `@` and a number for one placement of something
/// placed more than once, e.g. `widgets.clock@2` for a clock's second
/// place and `widgets.clock@2.face` for that place's own setting (see
/// [`Id::placement`], [`Id::unplaced`]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub(crate) Cow<'static, str>);

/// Returned when parsing an invalid [`Id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ids are lowercase segments of [a-z0-9-] separated by '.'")
    }
}

impl core::error::Error for InvalidId {}

impl Id {
    /// Creates an id from a string literal.
    ///
    /// # Panics
    ///
    /// Panics if `id` is invalid. Use it in `const` items, where the check
    /// happens at compile time.
    pub const fn new_static(id: &'static str) -> Id {
        assert!(is_valid(id), "invalid id");
        Id(Cow::Borrowed(id))
    }

    /// Parses an id at runtime, e.g. from a configuration file.
    pub fn parse(id: &str) -> Result<Id, InvalidId> {
        if is_valid(id) {
            Ok(Id(Cow::Owned(String::from(id))))
        } else {
            Err(InvalidId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id as text kept in the program, for specs kept there too
    /// (e.g. `SettingSpec::fixed_number(MARGIN.static_str(), …)`, see
    /// [`SettingSpec::fixed`](crate::SettingSpec::fixed)): for ids made
    /// with [`Id::new_static`]; empty for ids parsed at run time. Takes
    /// the id, so it can be used in `const` and `static` items.
    pub const fn static_str(self) -> &'static str {
        let text = match &self.0 {
            Cow::Borrowed(id) => id,
            Cow::Owned(_) => "",
        };
        core::mem::forget(self);
        text
    }

    /// The first segment, i.e. the owning plugin's id.
    pub fn namespace(&self) -> &str {
        self.0.split('.').next().unwrap_or_default()
    }

    /// The id without its placement (`widgets.clock@2.face` is
    /// `widgets.clock.face`), if it names one; see [`Id::placement`].
    pub fn unplaced(&self) -> Option<Id> {
        unplaced(self.as_str()).map(|plain| Id(Cow::Owned(plain)))
    }

    /// [`Id::unplaced`] as text: its name before it had the one it has
    /// now, kept while plugins still call it.
    #[doc(hidden)]
    pub fn placed(&self) -> Option<String> {
        unplaced(self.as_str())
    }

    /// The id of placement `n` of this: `widgets.clock` and 2 make
    /// `widgets.clock@2`, and so does `widgets.clock@5` (another
    /// placement of the same).
    pub fn placement(&self, n: u32) -> Id {
        let last = self.0.rfind('.').map_or(0, |dot| dot + 1);
        let end = self.0[last..]
            .find('@')
            .map_or(self.0.len(), |at| last + at);
        Id(Cow::Owned(alloc::format!("{}@{n}", &self.0[..end])))
    }

    /// Whether this id belongs to the plugin `plugin`.
    pub fn is_owned_by(&self, plugin: &Id) -> bool {
        self.namespace() == plugin.as_str()
    }
}

impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

const fn is_valid(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut segment_start = true;
    // After `@`: digits only, at least one, to the segment's end.
    let mut placement = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            if segment_start {
                return false;
            }
            segment_start = true;
            placement = false;
        } else if b == b'@' {
            if segment_start || placement || i + 1 >= bytes.len() {
                return false;
            }
            placement = true;
            segment_start = true;
        } else if placement && !b.is_ascii_digit() {
            return false;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            segment_start = false;
        } else {
            return false;
        }
        i += 1;
    }
    !segment_start
}

/// Where `id` names a placement (`@` and its number), the id without it.
pub(crate) fn unplaced(id: &str) -> Option<String> {
    let at = id.find('@')?;
    let end = id[at..].find('.').map_or(id.len(), |dot| at + dot);
    let mut plain = String::with_capacity(id.len());
    plain.push_str(&id[..at]);
    plain.push_str(&id[end..]);
    Some(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_are_ids_of_their_own() {
        for valid in ["widgets.clock@2", "widgets.clock@12.face", "a@1"] {
            assert!(Id::parse(valid).is_ok(), "{valid}");
        }
        for invalid in ["widgets.clock@", "widgets.@2", "a@2b", "a@1@2", "@1"] {
            assert!(Id::parse(invalid).is_err(), "{invalid}");
        }
        let id = Id::parse("widgets.clock@2.face").unwrap();
        assert_eq!(id.unplaced(), Some(Id::new_static("widgets.clock.face")));
        assert_eq!(id.placed().as_deref(), Some("widgets.clock.face"));
        assert_eq!(Id::new_static("widgets.clock").unplaced(), None);
        let clock = Id::new_static("widgets.clock");
        assert_eq!(clock.placement(3).as_str(), "widgets.clock@3");
        // Another placement of a placed id, not one of it.
        let placed = clock.placement(2).placement(3);
        assert_eq!(placed.as_str(), "widgets.clock@3");
        assert!(Id::parse(placed.as_str()).is_ok());
        assert_eq!(id.placement(4).as_str(), "widgets.clock@2.face@4");
    }

    #[test]
    fn validates_ids() {
        for valid in ["core", "core.home", "my-plugin.screen-2", "a.b.c"] {
            assert!(Id::parse(valid).is_ok(), "{valid}");
        }
        for invalid in ["", ".", "a.", ".a", "a..b", "Core.home", "a b", "a_b"] {
            assert!(Id::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn namespace_and_ownership() {
        const ID: Id = Id::new_static("diagnostics.screen");
        assert_eq!(ID.namespace(), "diagnostics");
        assert!(ID.is_owned_by(&Id::new_static("diagnostics")));
        assert!(!ID.is_owned_by(&Id::new_static("diag")));
    }

    #[test]
    fn static_and_parsed_ids_are_equal() {
        assert_eq!(Id::new_static("core.home"), Id::parse("core.home").unwrap());
    }
}
