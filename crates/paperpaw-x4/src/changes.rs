//! The presses and touches read in a timer's interrupt, waiting for the
//! main loop: a ring of a few. While the loop is busy (a refresh takes
//! most of a second on the X4 Pro), a finger moving on keeps reporting
//! where it is; a move that comes while the last one still waits takes
//! its place (see [`Queue::push`]), so the loop goes on from where the
//! finger is, not through every place it was, and the ring does not fill
//! up and drop the lift that ends it.

/// Changes waiting, oldest first, at most `N`.
#[derive(Debug)]
pub struct Queue<T, const N: usize> {
    items: [Option<T>; N],
    /// Where the oldest waits, and how many do.
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> Default for Queue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const N: usize> Queue<T, N> {
    pub const fn new() -> Self {
        Self {
            items: [None; N],
            head: 0,
            len: 0,
        }
    }

    /// Adds `change`, or puts it in the place of the last one waiting
    /// where `replaces(last, change)` says it stands for it (a finger
    /// moved again). `false` where it was dropped, all places taken.
    pub fn push(&mut self, change: T, replaces: impl Fn(&T, &T) -> bool) -> bool {
        if self.len > 0 {
            let last = (self.head + self.len - 1) % N;
            if let Some(waiting) = &mut self.items[last]
                && replaces(waiting, &change)
            {
                *waiting = change;
                return true;
            }
        }
        if self.len == N {
            return false;
        }
        self.items[(self.head + self.len) % N] = Some(change);
        self.len += 1;
        true
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
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Finger {
        Down(u16),
        Move(u16),
        Up(u16),
    }

    fn moved_again(last: &Finger, change: &Finger) -> bool {
        matches!((last, change), (Finger::Move(_), Finger::Move(_)))
    }

    fn drained<const N: usize>(queue: &mut Queue<Finger, N>) -> alloc::vec::Vec<Finger> {
        core::iter::from_fn(|| queue.pop()).collect()
    }

    #[test]
    fn moves_waiting_are_one_where_the_finger_is() {
        let mut queue = Queue::<Finger, 4>::new();
        assert!(queue.push(Finger::Down(0), moved_again));
        for at in 1..=40 {
            assert!(queue.push(Finger::Move(at), moved_again));
        }
        assert!(queue.push(Finger::Up(40), moved_again));
        assert_eq!(
            drained(&mut queue),
            [Finger::Down(0), Finger::Move(40), Finger::Up(40)]
        );
    }

    #[test]
    fn a_move_taken_already_is_not_replaced() {
        let mut queue = Queue::<Finger, 4>::new();
        queue.push(Finger::Down(0), moved_again);
        queue.push(Finger::Move(1), moved_again);
        assert_eq!(queue.pop(), Some(Finger::Down(0)));
        assert_eq!(queue.pop(), Some(Finger::Move(1)));
        queue.push(Finger::Move(2), moved_again);
        queue.push(Finger::Move(3), moved_again);
        assert_eq!(drained(&mut queue), [Finger::Move(3)]);
    }

    #[test]
    fn full_it_drops_what_comes_and_keeps_its_order() {
        let mut queue = Queue::<Finger, 2>::new();
        assert!(queue.push(Finger::Down(1), moved_again));
        assert!(queue.push(Finger::Up(1), moved_again));
        assert!(!queue.push(Finger::Down(2), moved_again));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some(Finger::Down(1)));
        // Around the ring's end.
        assert!(queue.push(Finger::Down(3), moved_again));
        assert!(!queue.push(Finger::Move(3), moved_again), "full");
        assert_eq!(drained(&mut queue), [Finger::Up(1), Finger::Down(3)]);
        assert!(queue.is_empty());
    }
}
