use alloc::borrow::Cow;
use core::fmt;

use crate::contribution::Registrar;
use crate::id::Id;
use crate::version::Version;

/// Static information about a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Plugin id; also the namespace of all its contributions.
    pub id: Id,
    /// Human-readable name in English. Plugins translate it with the key
    /// `<id>.plugin.name`.
    pub name: Cow<'static, str>,
    /// What the plugin does, one English sentence for plugin managers.
    /// Plugins translate it with the key `<id>.plugin.description`.
    pub description: Cow<'static, str>,
    pub version: Version,
    /// The SDK version the plugin was built against.
    pub sdk: Version,
}

/// A QuireInk plugin.
///
/// `register` is called once at start-up. It adds the plugin's contributions;
/// the kernel decides afterwards which ones are active (user configuration
/// and priorities may override them).
pub trait Plugin {
    fn manifest(&self) -> Manifest;

    /// Adds the plugin's contributions with `registrar` (see
    /// [`contribution`](crate::contribution)).
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError>;
}

/// Priority of a default binding. Higher wins; an explicit user setting
/// always wins over any priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(pub i16);

impl Priority {
    /// Used only when nothing else is available (diagnostics, placeholders).
    pub const FALLBACK: Priority = Priority(-100);
    /// The normal priority of first-party defaults.
    pub const DEFAULT: Priority = Priority(0);
    /// For plugins whose purpose is to replace a default.
    pub const PREFERRED: Priority = Priority(100);
}

/// A named UI role (home, library, reader, …) bound to a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSpec {
    pub id: Id,
    /// Translation key of the slot's display name.
    pub title: Cow<'static, str>,
}

/// Why a contribution was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterError {
    /// The id is not in the plugin's namespace.
    Namespace { id: Id },
    /// An id was registered twice.
    Duplicate { id: Id },
    /// A data contribution could not be parsed.
    Parse {
        line: u32,
        message: Cow<'static, str>,
    },
    /// Something else is wrong with the contribution.
    Invalid { message: Cow<'static, str> },
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::Namespace { id } => {
                write!(f, "`{id}` is outside the plugin's namespace")
            }
            RegisterError::Duplicate { id } => write!(f, "`{id}` is already registered"),
            RegisterError::Parse { line, message } => write!(f, "line {line}: {message}"),
            RegisterError::Invalid { message } => f.write_str(message),
        }
    }
}

impl core::error::Error for RegisterError {}
