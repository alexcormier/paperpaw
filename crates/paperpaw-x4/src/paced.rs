//! Which frame goes to the panel when, for a screen that runs at a pace
//! (ADR 0030): the main loop goes on while a refresh runs, so a frame
//! asked for meanwhile must wait for the panel, and nothing may be sent
//! to the panel while it refreshes. The firmware asks this what to do;
//! tests drive it with a panel that refuses anything sent while busy.

use paperpaw_platform::{Rect, RefreshMode};

/// What to do with a frame asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Draw, send and start its refresh now: the panel is free.
    Start,
    /// It waits (merged with others waiting) until the panel is free.
    Wait,
}

/// The frames of a paced screen: the one on its way, what waits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Frames {
    in_flight: bool,
    waiting: Option<(Rect, RefreshMode)>,
}

impl Frames {
    /// Whether a frame is on its way to the panel.
    pub fn in_flight(&self) -> bool {
        self.in_flight
    }

    /// A frame of `area` in `mode` asked for.
    pub fn ask(&mut self, area: Rect, mode: RefreshMode) -> Ask {
        if !self.in_flight {
            self.in_flight = true;
            return Ask::Start;
        }
        self.waiting = Some(match self.waiting {
            Some((before, was)) => (before.union(&area), was.max(mode)),
            None => (area, mode),
        });
        Ask::Wait
    }

    /// The frame on its way is shown (its refresh over, both memories
    /// made the same): the one that waited, if any, to start now.
    pub fn shown(&mut self) -> Option<(Rect, RefreshMode)> {
        self.in_flight = false;
        let next = self.waiting.take();
        self.in_flight = next.is_some();
        next
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    /// A panel that refuses anything sent while it refreshes, and shows
    /// what it was sent.
    #[derive(Default)]
    struct Panel {
        busy: bool,
        shown: Vec<Rect>,
    }

    impl Panel {
        fn send_and_start(&mut self, area: Rect) {
            assert!(!self.busy, "sent while refreshing");
            self.busy = true;
            self.shown.push(area);
        }

        fn done(&mut self) {
            self.busy = false;
        }
    }

    /// The firmware's steps, as the main loop takes them.
    struct Loop {
        frames: Frames,
        panel: Panel,
    }

    impl Loop {
        fn ask(&mut self, area: Rect) {
            if self.frames.ask(area, RefreshMode::Fast) == Ask::Start {
                self.panel.send_and_start(area);
            }
        }

        /// The panel's refresh ends, the loop sees it.
        fn panel_done(&mut self) {
            self.panel.done();
            if let Some((area, _)) = self.frames.shown() {
                self.panel.send_and_start(area);
            }
        }

        /// Before sleep or anything else that uses the panel: waits for
        /// the frame on its way and the one that waited.
        fn settle(&mut self) {
            while self.frames.in_flight() {
                self.panel_done();
            }
        }
    }

    fn rect(x: u16) -> Rect {
        Rect::new(x, 0, 10, 10)
    }

    #[test]
    fn frames_asked_while_one_refreshes_wait_and_merge() {
        let mut l = Loop {
            frames: Frames::default(),
            panel: Panel::default(),
        };
        l.ask(rect(0));
        // Input and timers while it refreshes ask for more frames.
        l.ask(rect(20));
        l.ask(rect(40));
        assert_eq!(l.panel.shown, [rect(0)], "nothing sent while busy");
        l.panel_done();
        assert_eq!(l.panel.shown[1], rect(20).union(&rect(40)), "merged");
        l.panel_done();
        assert!(!l.frames.in_flight());
        assert_eq!(l.panel.shown.len(), 2);
    }

    #[test]
    fn a_sleep_meanwhile_shows_what_waited_first() {
        let mut l = Loop {
            frames: Frames::default(),
            panel: Panel::default(),
        };
        l.ask(rect(0));
        l.ask(rect(30));
        l.settle();
        assert_eq!(l.panel.shown, [rect(0), rect(30)], "no frame lost");
        assert!(!l.panel.busy && !l.frames.in_flight());
        // Any order of asks and ends never sends while busy.
        for pattern in 0u32..256 {
            let mut l = Loop {
                frames: Frames::default(),
                panel: Panel::default(),
            };
            let mut asked = 0;
            for bit in 0..8 {
                if pattern & (1 << bit) != 0 {
                    asked += 1;
                    l.ask(rect(bit));
                } else if l.frames.in_flight() {
                    l.panel_done();
                }
            }
            l.settle();
            assert!(l.panel.shown.len() <= asked);
            assert_eq!(asked > 0, !l.panel.shown.is_empty());
        }
    }
}
