//! Decides how the panel is refreshed.

use paperpaw_platform::{Rect, RefreshMode};

/// Upgrades every n-th partial refresh to a full refresh of the whole
/// screen to remove ghosting. It counts the refreshes as they are done
/// ([`RefreshPolicy::done`]), after refresh filters changed them: a full
/// refresh a filter asked for starts the count anew.
#[derive(Debug, Default)]
pub(crate) struct RefreshPolicy {
    partials: u32,
}

impl RefreshPolicy {
    /// Returns the mode and region to refresh. `interval == 0` disables the
    /// periodic full refresh.
    pub(crate) fn decide(
        &self,
        requested: RefreshMode,
        dirty: Rect,
        screen: Rect,
        interval: u32,
    ) -> (RefreshMode, Rect) {
        match requested {
            RefreshMode::Partial if interval > 0 && self.partials + 1 >= interval => {
                (RefreshMode::Full, screen)
            }
            mode => (mode, dirty),
        }
    }

    /// A refresh in `mode` was done.
    pub(crate) fn done(&mut self, mode: RefreshMode) {
        match mode {
            RefreshMode::Full => self.partials = 0,
            RefreshMode::Partial => self.partials = self.partials.saturating_add(1),
            // Fast, and modes to come that leave ghosting as they find it.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_nth_partial_becomes_full() {
        let mut policy = RefreshPolicy::default();
        let dirty = Rect::new(1, 1, 2, 2);
        let screen = Rect::new(0, 0, 10, 10);
        let modes: [RefreshMode; 4] = core::array::from_fn(|_| {
            let (mode, _) = policy.decide(RefreshMode::Partial, dirty, screen, 3);
            policy.done(mode);
            mode
        });
        assert_eq!(
            modes,
            [
                RefreshMode::Partial,
                RefreshMode::Partial,
                RefreshMode::Full,
                RefreshMode::Partial
            ]
        );
    }

    #[test]
    fn fast_does_not_count_and_zero_disables() {
        let mut policy = RefreshPolicy::default();
        let r = Rect::new(0, 0, 1, 1);
        for _ in 0..10 {
            policy.done(RefreshMode::Fast);
            assert_eq!(
                policy.decide(RefreshMode::Fast, r, r, 2).0,
                RefreshMode::Fast
            );
            policy.done(RefreshMode::Partial);
            assert_eq!(
                policy.decide(RefreshMode::Partial, r, r, 0).0,
                RefreshMode::Partial
            );
        }
    }

    /// A full refresh done otherwise (asked for, or made so by a filter)
    /// starts the count anew; one that became partial does not.
    #[test]
    fn the_count_follows_the_refreshes_done() {
        let mut policy = RefreshPolicy::default();
        let r = Rect::new(0, 0, 1, 1);
        policy.done(RefreshMode::Partial);
        policy.done(RefreshMode::Full);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, 2).0,
            RefreshMode::Partial
        );
        policy.done(RefreshMode::Partial);
        // The full refresh the policy wanted became partial: still due.
        policy.done(RefreshMode::Partial);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, 2).0,
            RefreshMode::Full
        );
    }
}
