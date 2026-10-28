//! Decides how the panel is refreshed.

use paperpaw_platform::{Rect, RefreshMode};

/// Upgrades every n-th partial refresh of a page (most of the screen, as
/// a page turned) to a full refresh of the whole screen to remove
/// ghosting, as the setting says ("after this many page turns"): smaller
/// ones (a line of progress, a focus moved, the clock) neither count nor
/// flash. It counts the refreshes as they are done
/// ([`RefreshPolicy::done`]), after refresh filters changed them: a full
/// refresh a filter asked for starts the count anew.
#[derive(Debug, Default)]
pub(crate) struct RefreshPolicy {
    partials: u32,
    /// What the areas asked for since the last refresh add up to (they
    /// may overlap, and the region refreshed is the one rectangle around
    /// them): two items far apart are a region of most of the screen, but
    /// no page. `None`: the region is taken as it is.
    weight: Option<u32>,
}

impl RefreshPolicy {
    /// Says what the areas drawn for the refresh that is decided and done
    /// next add up to (see the field), or `None` for what lies in the
    /// region.
    pub(crate) fn weigh(&mut self, weight: Option<u32>) {
        self.weight = weight;
    }

    /// Whether `region` is most of `screen` (half or more) as far as what
    /// was asked for in it goes: a page.
    fn is_page(&self, region: Rect, screen: Rect) -> bool {
        let area = |r: Rect| u32::from(r.width) * u32::from(r.height);
        let drawn = self.weight.map_or(area(region), |w| w.min(area(region)));
        drawn * 2 >= area(screen)
    }

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
            RefreshMode::Partial | RefreshMode::Gray
                if interval > 0 && self.is_page(dirty, screen) && self.partials + 1 >= interval =>
            {
                (RefreshMode::Full, screen)
            }
            mode => (mode, dirty),
        }
    }

    /// How many refreshes that leave ghosting were made since the last
    /// that cleaned the panel.
    pub(crate) fn since_full(&self) -> u32 {
        self.partials
    }

    /// A refresh in `mode` of `region` (of `screen`) was done.
    pub(crate) fn done(&mut self, mode: RefreshMode, region: Rect, screen: Rect) {
        match mode {
            RefreshMode::Full | RefreshMode::Clear => self.partials = 0,
            RefreshMode::Partial | RefreshMode::Gray if self.is_page(region, screen) => {
                self.partials = self.partials.saturating_add(1)
            }
            // Smaller ones, fast and black and white leave ghosting as
            // they find it.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_partials_never_flash_nor_count() {
        let mut policy = RefreshPolicy::default();
        let screen = Rect::new(0, 0, 480, 800);
        let line = Rect::new(0, 740, 480, 40);
        for _ in 0..50 {
            let (mode, region) = policy.decide(RefreshMode::Partial, line, screen, 6);
            assert_eq!((mode, region), (RefreshMode::Partial, line));
            policy.done(mode, region, screen);
        }
        assert_eq!(policy.since_full(), 0);
        // Pages still flash at the interval: the sixth.
        let page = Rect::new(0, 0, 480, 760);
        let modes: alloc::vec::Vec<RefreshMode> = (0..6)
            .map(|_| {
                let (mode, region) = policy.decide(RefreshMode::Partial, page, screen, 6);
                policy.done(mode, region, screen);
                mode
            })
            .collect();
        assert_eq!(modes[..5], [RefreshMode::Partial; 5]);
        assert_eq!(modes[5], RefreshMode::Full);
    }

    #[test]
    fn every_nth_partial_becomes_full() {
        let mut policy = RefreshPolicy::default();
        let dirty = Rect::new(0, 0, 10, 6);
        let screen = Rect::new(0, 0, 10, 10);
        let modes: [RefreshMode; 4] = core::array::from_fn(|_| {
            let (mode, _) = policy.decide(RefreshMode::Partial, dirty, screen, 3);
            policy.done(mode, screen, screen);
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
            policy.done(RefreshMode::Fast, r, r);
            assert_eq!(
                policy.decide(RefreshMode::Fast, r, r, 2).0,
                RefreshMode::Fast
            );
            policy.done(RefreshMode::Partial, r, r);
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
        policy.done(RefreshMode::Partial, r, r);
        policy.done(RefreshMode::Full, r, r);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, 2).0,
            RefreshMode::Partial
        );
        policy.done(RefreshMode::Partial, r, r);
        // The full refresh the policy wanted became partial: still due.
        policy.done(RefreshMode::Partial, r, r);
        assert_eq!(
            policy.decide(RefreshMode::Partial, r, r, 2).0,
            RefreshMode::Full
        );
    }
}
