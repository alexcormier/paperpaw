//! Waiting for something, a bounded time: before the device starts again
//! or sleeps, the firmware leaves the network, but waits for the radio's
//! task going on (a join, a scan) only so long, then starts again
//! anyway. Never hanging matters more there than leaving cleanly.

/// Calls `step` (which says whether it is done, moving it on if not)
/// until it is done or `expired` says time is up; whether it was done.
pub fn wait(mut step: impl FnMut() -> bool, mut expired: impl FnMut() -> bool) -> bool {
    loop {
        if step() {
            return true;
        }
        if expired() {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    /// A task that takes `n` steps more.
    fn task(left: &Cell<u32>) -> bool {
        if left.get() == 0 {
            return true;
        }
        left.set(left.get() - 1);
        false
    }

    #[test]
    fn done_once_the_steps_finish_it() {
        let left = Cell::new(3);
        assert!(wait(|| task(&left), || false));
        assert_eq!(left.get(), 0);
    }

    #[test]
    fn never_longer_than_allowed() {
        let left = Cell::new(100);
        let checks = Cell::new(0);
        let done = wait(
            || task(&left),
            || {
                checks.set(checks.get() + 1);
                checks.get() >= 5
            },
        );
        assert!(!done);
        assert_eq!(left.get(), 95);
    }

    #[test]
    fn nothing_to_wait_for() {
        let left = Cell::new(0);
        assert!(wait(|| task(&left), || true));
    }
}
