//! A request whose connection stalls without a byte of the answer. Seen
//! on the X4 and the X4 Pro: the connection up, the request (or TLS's
//! first message) sent and acknowledged, then nothing at all, the same
//! again on a fresh connection, until the device started anew. So such a
//! request is not failed at once: the radio starts anew (its driver made
//! again, the network joined again), and the request is made once more.
//! A connection that stalls after part of the answer came fails as it
//! did: the network works, the other side is slow.

/// What becomes of a request whose connection stalled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stalled {
    /// It fails ("timed out").
    Fail,
    /// The radio starts anew, and the request is made once more.
    Again,
}

/// What becomes of a request that stalled: `received`, whether any byte
/// of the answer came over its connection; `again`, whether it is made
/// once more already.
pub fn stalled(received: bool, again: bool) -> Stalled {
    if received || again {
        Stalled::Fail
    } else {
        Stalled::Again
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_stall_is_tried_once_more_with_the_radio_anew() {
        assert_eq!(stalled(false, false), Stalled::Again);
        // Once only: a second silent stall fails.
        assert_eq!(stalled(false, true), Stalled::Fail);
    }

    #[test]
    fn a_stall_after_part_of_the_answer_fails() {
        assert_eq!(stalled(true, false), Stalled::Fail);
        assert_eq!(stalled(true, true), Stalled::Fail);
    }
}
