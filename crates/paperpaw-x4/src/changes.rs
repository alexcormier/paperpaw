//! The presses and touches read in a timer's interrupt, waiting for the
//! main loop: a ring of a few. While the loop is busy (a refresh takes
//! most of a second on the X4 Pro), a finger moving on keeps reporting
//! where it is; a move that comes while the last one still waits takes
//! its place (see [`Queue::push`]), so the loop goes on from where the
//! finger is, not through every place it was, and the ring does not fill
//! up and drop the lift that ends it.
//!
//! Where the finger went farthest from where it came down stays: a move
//! back towards that place does not take the place of the farthest one
//! waiting, so that a finger that went past the distance a tap allows
//! and came back is not taken for a tap (or, held, a long press).

/// What a finger does, as far as merging its moves goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finger {
    Down(u16, u16),
    Move(u16, u16),
    Up,
}

/// A change waiting: a press or release, or what a finger does.
pub trait Reading: Copy {
    /// What a finger does, `None` for anything else (a button).
    fn finger(&self) -> Option<Finger>;
}

/// Changes waiting, oldest first, at most `N`.
#[derive(Debug)]
pub struct Queue<T, const N: usize> {
    items: [Option<T>; N],
    /// Where the oldest waits, and how many do.
    head: usize,
    len: usize,
    /// Where the finger came down, and how far from there it went (the
    /// square of the distance), of the changes pushed.
    down: Option<(u16, u16)>,
    farthest: u64,
}

impl<T: Reading, const N: usize> Default for Queue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Reading, const N: usize> Queue<T, N> {
    pub const fn new() -> Self {
        Self {
            items: [None; N],
            head: 0,
            len: 0,
            down: None,
            farthest: 0,
        }
    }

    /// Adds `change`; a finger's move takes the place of the move that
    /// waits last, unless that one is where the finger went farthest
    /// and this one comes back. `false` where it was dropped, all places
    /// taken.
    pub fn push(&mut self, change: T) -> bool {
        let finger = change.finger();
        if let Some(Finger::Move(x, y)) = finger {
            let away = self.away(x, y);
            let last = (self.head + self.len + N - 1) % N;
            let waiting = (self.len > 0)
                .then(|| self.items[last].and_then(|c| c.finger()))
                .flatten();
            if let Some(Finger::Move(wx, wy)) = waiting {
                let theirs = self.away(wx, wy);
                if !(theirs == self.farthest && away < theirs) {
                    self.items[last] = Some(change);
                    self.farthest = self.farthest.max(away);
                    return true;
                }
            }
        }
        if self.len == N {
            return false;
        }
        self.items[(self.head + self.len) % N] = Some(change);
        self.len += 1;
        // Of what is kept only: a change dropped did not happen.
        match finger {
            Some(Finger::Down(x, y)) => {
                self.down = Some((x, y));
                self.farthest = 0;
            }
            Some(Finger::Move(x, y)) => self.farthest = self.farthest.max(self.away(x, y)),
            Some(Finger::Up) => self.down = None,
            None => {}
        }
        true
    }

    /// How far `(x, y)` is from where the finger came down (squared).
    fn away(&self, x: u16, y: u16) -> u64 {
        let Some((dx, dy)) = self.down else {
            return 0;
        };
        let (a, b) = (u64::from(x.abs_diff(dx)), u64::from(y.abs_diff(dy)));
        a * a + b * b
    }

    /// The oldest change waiting.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let change = self.items[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        change
    }

    /// Drops every change waiting.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// How many changes wait.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Change {
        Button(bool),
        Finger(Finger),
    }

    impl Reading for Change {
        fn finger(&self) -> Option<Finger> {
            match self {
                Change::Finger(f) => Some(*f),
                Change::Button(_) => None,
            }
        }
    }

    use Change::Finger as F;
    use Finger::{Down, Move, Up};

    fn drained<const N: usize>(queue: &mut Queue<Change, N>) -> Vec<Change> {
        core::iter::from_fn(|| queue.pop()).collect()
    }

    #[test]
    fn moves_waiting_are_one_where_the_finger_is() {
        let mut queue = Queue::<Change, 4>::new();
        assert!(queue.push(F(Down(0, 0))));
        for at in 1..=40 {
            assert!(queue.push(F(Move(at, 0))));
        }
        assert!(queue.push(F(Up)));
        assert_eq!(drained(&mut queue), [F(Down(0, 0)), F(Move(40, 0)), F(Up)]);
    }

    #[test]
    fn where_the_finger_went_farthest_stays() {
        let mut queue = Queue::<Change, 8>::new();
        queue.push(F(Down(100, 100)));
        // Out past a tap's distance and back while the loop was busy.
        for at in [104, 110, 130, 120, 104, 101] {
            queue.push(F(Move(at, 100)));
        }
        queue.push(F(Up));
        assert_eq!(
            drained(&mut queue),
            [
                F(Down(100, 100)),
                F(Move(130, 100)),
                F(Move(101, 100)),
                F(Up)
            ]
        );
    }

    #[test]
    fn farther_again_the_moves_are_one_again() {
        let mut queue = Queue::<Change, 8>::new();
        queue.push(F(Down(0, 0)));
        for at in [10, 5, 20, 30, 25, 40] {
            queue.push(F(Move(at, 0)));
        }
        // 10 farthest, then 5 back; 20 farther (takes 5's place) and
        // 30; 25 back from 30; 40 farther than all, in 25's place.
        assert_eq!(
            drained(&mut queue),
            [
                F(Down(0, 0)),
                F(Move(10, 0)),
                F(Move(30, 0)),
                F(Move(40, 0))
            ]
        );
    }

    #[test]
    fn a_change_dropped_leaves_where_the_finger_is() {
        let mut queue = Queue::<Change, 2>::new();
        queue.push(F(Down(0, 0)));
        queue.push(F(Move(50, 0)));
        // Full: the next finger's coming down is dropped, so the moves
        // still count from the first one's place.
        assert!(!queue.push(F(Down(1000, 1000))));
        assert_eq!(queue.pop(), Some(F(Down(0, 0))));
        assert!(queue.push(F(Move(40, 0))));
        assert_eq!(drained(&mut queue), [F(Move(50, 0)), F(Move(40, 0))]);
    }

    #[test]
    fn the_whole_screen_apart_is_measured() {
        let mut queue = Queue::<Change, 4>::new();
        queue.push(F(Down(0, 0)));
        queue.push(F(Move(u16::MAX, u16::MAX)));
        queue.push(F(Move(1, 1)));
        assert_eq!(
            drained(&mut queue),
            [F(Down(0, 0)), F(Move(u16::MAX, u16::MAX)), F(Move(1, 1))]
        );
    }

    #[test]
    fn a_move_taken_already_is_not_replaced() {
        let mut queue = Queue::<Change, 4>::new();
        queue.push(F(Down(0, 0)));
        queue.push(F(Move(1, 0)));
        assert_eq!(queue.pop(), Some(F(Down(0, 0))));
        assert_eq!(queue.pop(), Some(F(Move(1, 0))));
        queue.push(F(Move(2, 0)));
        queue.push(F(Move(3, 0)));
        assert_eq!(drained(&mut queue), [F(Move(3, 0))]);
    }

    #[test]
    fn full_it_drops_what_comes_and_keeps_its_order() {
        let mut queue = Queue::<Change, 2>::new();
        assert!(queue.push(Change::Button(true)));
        assert!(queue.push(Change::Button(false)));
        assert!(!queue.push(F(Down(1, 1))));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(Change::Button(true)));
        // Around the ring's end.
        assert!(queue.push(F(Down(3, 3))));
        assert!(!queue.push(F(Move(3, 4))), "full");
        assert_eq!(drained(&mut queue), [Change::Button(false), F(Down(3, 3))]);
        assert!(queue.is_empty());
    }
}
