//! FNV-1a with 64 bits: short, stable ids of paths, addresses and
//! contents, e.g. in the names of cache files and of files kept per book.
//! Its values are part of those names, so of files on the card: it never
//! changes.

/// The FNV-1a hash of `data` (64 bits).
pub fn fnv1a64(data: impl AsRef<[u8]>) -> u64 {
    let mut hash = Fnv1a64::new();
    hash.feed(data.as_ref());
    hash.value()
}

/// FNV-1a with 64 bits fed piece by piece, with nothing allocated: the
/// value [`fnv1a64`] gives of all the bytes fed. A [`Hasher`] for what
/// derives `Hash`, and a [`fmt::Write`] for what is written with
/// `write!`, e.g. a signature of what a screen shows.
///
/// [`Hasher`]: core::hash::Hasher
/// [`fmt::Write`]: core::fmt::Write
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fnv1a64(u64);

impl Fnv1a64 {
    /// Nothing fed yet.
    pub const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    /// Feeds `bytes`.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.0 = bytes.iter().fold(self.0, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    }

    /// The hash of what was fed.
    pub const fn value(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

impl core::hash::Hasher for Fnv1a64 {
    fn write(&mut self, bytes: &[u8]) {
        self.feed(bytes);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl core::fmt::Write for Fnv1a64 {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.feed(text.as_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Fnv1a64, fnv1a64};

    #[test]
    fn the_published_values() {
        // From the FNV reference's test vectors.
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64("foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(
            fnv1a64(b"foobar"),
            fnv1a64(alloc::string::String::from("foobar"))
        );
    }

    #[test]
    fn fed_in_pieces_it_is_the_same() {
        use core::fmt::Write;
        let mut hash = Fnv1a64::new();
        hash.feed(b"foo");
        hash.write_str("bar").unwrap();
        assert_eq!(hash.value(), fnv1a64("foobar"));
    }
}
