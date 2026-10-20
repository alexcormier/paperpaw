//! Random bytes from the platform's own source, for what others must not
//! guess: keys in addresses, tokens, nonces ([`Env::random`]).
//!
//! Each shell gives the kernel its source
//! (`KernelBuilder::random`): the operating system's on Android and on
//! desktops, the chip's random number generator on the X4. That one is
//! fully random while the radio is on; a key for the network is best
//! drawn once the network is up (e.g. when a server has started). Where a
//! shell gives none (tests), nothing is drawn: there is no weaker stand-in
//! to fall back to unnoticed.
//!
//! [`Env::random`]: crate::Env::random

use alloc::string::String;
use core::fmt;

/// The letters of [`Random::token`]: digits and lower case letters
/// without `i`, `l`, `o` and `u`, which are easily taken for others
/// (Crockford's base 32).
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The platform's source of random bytes; see the [module
/// documentation](self).
#[derive(Clone, Copy, Default)]
pub struct Random {
    fill: Option<fn(&mut [u8]) -> bool>,
}

impl Random {
    /// No source: nothing is drawn.
    pub const NONE: Self = Self { fill: None };

    /// The source `fill`, which fills a buffer with random bytes and says
    /// whether it could (for shells).
    pub const fn new(fill: fn(&mut [u8]) -> bool) -> Self {
        Self { fill: Some(fill) }
    }

    /// Whether there is a source.
    pub fn available(&self) -> bool {
        self.fill.is_some()
    }

    /// Fills `buf` with random bytes; `false` where there is no source or
    /// it failed, and `buf` is not to be used then.
    pub fn fill(&self, buf: &mut [u8]) -> bool {
        self.fill.is_some_and(|fill| fill(buf))
    }

    /// A random text of `chars` digits and lower case letters, 5 bits
    /// each (so 13 for at least 64 bits), easy to read and type; `None`
    /// where nothing can be drawn.
    pub fn token(&self, chars: usize) -> Option<String> {
        let mut bytes = alloc::vec![0u8; (chars * 5).div_ceil(8)];
        if !self.fill(&mut bytes) {
            return None;
        }
        let token = (0..chars)
            .map(|i| {
                // Bits 5i to 5i + 4, the first byte's highest first.
                let at = i * 5;
                let pair = u16::from(bytes[at / 8]) << 8
                    | u16::from(bytes.get(at / 8 + 1).copied().unwrap_or(0));
                let index = (pair >> (11 - at % 8)) & 0x1f;
                char::from(ALPHABET[usize::from(index)])
            })
            .collect();
        Some(token)
    }
}

impl fmt::Debug for Random {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Random")
            .field("available", &self.available())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_take_five_bits_a_letter() {
        let ones = Random::new(|buf| {
            buf.fill(0xff);
            true
        });
        assert_eq!(ones.token(13).as_deref(), Some("zzzzzzzzzzzzz"));
        // 0b00001_00010_00011_00100_00101_00110_00111_01000 …
        let counting = Random::new(|buf| {
            let bits = [0x08, 0x86, 0x42, 0x98, 0xe8];
            buf.copy_from_slice(&bits[..buf.len()]);
            true
        });
        assert_eq!(counting.token(8).as_deref(), Some("12345678"));
        assert_eq!(counting.token(1).as_deref(), Some("1"));
        assert_eq!(counting.token(0).as_deref(), Some(""));
    }

    #[test]
    fn nothing_is_drawn_without_a_source() {
        let mut buf = [0u8; 8];
        assert!(!Random::NONE.available());
        assert!(!Random::NONE.fill(&mut buf));
        assert_eq!(Random::NONE.token(13), None);
        let failing = Random::new(|_| false);
        assert!(failing.available());
        assert_eq!(failing.token(13), None);
    }
}
