//! FNV-1a with 64 bits: short, stable ids of paths, addresses and
//! contents, e.g. in the names of cache files and of files kept per book.
//! Its values are part of those names, so of files on the card: it never
//! changes.

/// The FNV-1a hash of `data` (64 bits).
pub fn fnv1a64(data: impl AsRef<[u8]>) -> u64 {
    data.as_ref()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
}

#[cfg(test)]
mod tests {
    use super::fnv1a64;

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
}
