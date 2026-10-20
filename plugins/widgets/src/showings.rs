//! How often a widget that shows another thing each time it shows (a
//! quote, a picture, a highlight) was shown ([`Showings`]).

use core::cell::Cell;

use paperpaw_sdk::Instant;

/// How often a widget was shown since the device started: counted in
/// memory as the widget is made, once for all made within
/// [`Showings::APART`], as a screen makes a widget several times as it
/// shows it (to measure it, to place it, anew after a change). Nothing is
/// written for it, and reading what it shows anew (each minute, after
/// events) does not count.
#[derive(Debug, Default)]
pub(crate) struct Showings {
    /// When the last was counted.
    at: Cell<Option<Instant>>,
    count: Cell<u32>,
}

impl Showings {
    /// Showings made within this many milliseconds of the last count as
    /// one.
    const APART: u64 = 60_000;

    /// Counts a showing at `now`, unless one was counted less than
    /// [`Self::APART`] before; the showings counted.
    pub(crate) fn show(&self, now: Instant) -> u32 {
        let apart = self
            .at
            .get()
            .is_none_or(|at| now.0.saturating_sub(at.0) >= Self::APART);
        if apart {
            self.at.set(Some(now));
            self.count.set(self.count.get().wrapping_add(1));
        }
        self.count.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showings_close_together_count_once() {
        let shown = Showings::default();
        // Made to measure, to place and to show: one showing.
        for millis in [0, 5, 400] {
            assert_eq!(shown.show(Instant(millis)), 1);
        }
        assert_eq!(shown.show(Instant(59_000)), 1);
        // Shown again later: another.
        assert_eq!(shown.show(Instant(61_000)), 2);
        assert_eq!(shown.show(Instant(61_500)), 2);
        assert_eq!(shown.show(Instant(3_600_000)), 3);
    }
}
