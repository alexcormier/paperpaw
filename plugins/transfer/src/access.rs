//! Who is answered: requests that carry the key of the address shown on
//! the device. The key is drawn anew for every transfer from the
//! platform's random source ([`Env::random`](paperpaw_sdk::Env::random));
//! wrong keys are counted, and after a few of them no request is answered
//! for a while, so the key cannot be found by trying.

use alloc::string::String;

use paperpaw_sdk::Instant;
use paperpaw_sdk::random::Random;

/// Letters of a key, 5 bits each: 65 bits.
pub const KEY_LETTERS: usize = 13;
// At least 64 bits.
const _: () = assert!(KEY_LETTERS * 5 >= 64);
/// Wrong keys taken before requests are turned away.
pub const WRONG_KEYS: u32 = 5;
/// How long requests are turned away then, in milliseconds.
pub const LOCKED_FOR: u64 = 60_000;

/// A key for one transfer from `random`, the platform's source; `None`
/// where it has none.
pub fn key(random: &Random) -> Option<String> {
    random.token(KEY_LETTERS)
}

/// Whether a request is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Granted,
    /// Without the key, or with a wrong one.
    Denied,
    /// Too many wrong keys were tried: no request is answered for now.
    Locked,
}

/// The key of a transfer, and the wrong keys tried.
#[derive(Debug)]
pub struct Guard {
    key: String,
    wrong: u32,
    locked_until: Option<Instant>,
}

impl Guard {
    pub fn new(key: String) -> Self {
        Self {
            key,
            wrong: 0,
            locked_until: None,
        }
    }

    /// Whether a request carrying `given` (`None`: no key at all) is
    /// answered at `now`. After [`WRONG_KEYS`] wrong keys, none is for
    /// [`LOCKED_FOR`], not even one with the right key. Requests without
    /// a key (a browser asking for an icon) are turned away uncounted:
    /// they guess nothing.
    pub fn check(&mut self, given: Option<&str>, now: Instant) -> Access {
        if let Some(until) = self.locked_until {
            if now < until {
                return Access::Locked;
            }
            self.locked_until = None;
            self.wrong = 0;
        }
        match given {
            Some(given) if same(given, &self.key) => Access::Granted,
            Some(_) => {
                self.wrong += 1;
                if self.wrong >= WRONG_KEYS {
                    self.locked_until = Some(now.add_millis(LOCKED_FOR));
                }
                Access::Denied
            }
            None => Access::Denied,
        }
    }
}

/// Whether `a` and `b` are equal, compared in a time that does not tell
/// how much of them is.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |differ, (x, y)| differ | (x ^ y))
            == 0
}
