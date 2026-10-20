//! SHA-256, for checking that files came whole: native plugin files
//! (`paperpaw-native`) and what the plugin library lists by it. Signatures
//! are Ed25519's, with its own digest. RustCrypto's `sha2` computes it,
//! behind the same calls as the other digests here.

use alloc::string::String;

use sha2::Digest;

/// A SHA-256 digest computed piece by piece.
#[derive(Debug, Clone, Default)]
pub struct Sha256(sha2::Sha256);

impl Sha256 {
    pub fn new() -> Self {
        Self(sha2::Sha256::new())
    }

    /// Adds `data`.
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// The digest of everything added.
    pub fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }

    /// The digest as 64 lower-case hex digits.
    pub fn hex(self) -> String {
        crate::hex::encode(&self.finish())
    }
}

/// The SHA-256 of `data`.
pub fn digest(data: &[u8]) -> [u8; 32] {
    let mut sha = Sha256::new();
    sha.update(data);
    sha.finish()
}

/// The SHA-256 of `data` as hex.
pub fn hex(data: &[u8]) -> String {
    crate::hex::encode(&digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        // FIPS 180-4's examples and the empty message.
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn pieces_make_the_same_digest() {
        let data = alloc::vec![b'a'; 1_000_000];
        let mut sha = Sha256::new();
        for chunk in data.chunks(997) {
            sha.update(chunk);
        }
        // A million times "a" (FIPS 180-4's long example).
        assert_eq!(
            sha.hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(hex(&data[..1000]), hex(&alloc::vec![b'a'; 1000]));
    }
}
