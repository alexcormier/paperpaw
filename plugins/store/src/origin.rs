//! Which installed plugins the store updates: only those it installed
//! itself (see [`install::origin`]), never one put in place otherwise.

use paperpaw_sdk::install::{self, Origin};
use paperpaw_sdk::{Env, Version};

use crate::catalog::Entry;

impl Entry {
    /// Whether a newer version than the one installed is offered: only
    /// for a plugin this store installed, never over one put in place
    /// otherwise (see [`install::origin`]).
    pub(crate) fn newer(&self, env: &Env<'_>) -> bool {
        match (self.installed(env), Version::parse(&self.version)) {
            (Some(installed), Some(offered)) => {
                offered > installed && self.origin(env, installed) == Origin::Store
            }
            _ => false,
        }
    }

    /// Whether the plugin is installed, but not by this store (the Wi-Fi
    /// transfer, a memory card, one's own build): its version here may be
    /// used in its place, when the reader says so, never as an update.
    pub(crate) fn local(&self, env: &Env<'_>) -> bool {
        self.installed(env)
            .is_some_and(|installed| self.origin(env, installed) == Origin::Local)
    }

    fn origin(&self, env: &Env<'_>, installed: Version) -> Origin {
        install::origin(env.storage, &self.id, installed)
    }
}
