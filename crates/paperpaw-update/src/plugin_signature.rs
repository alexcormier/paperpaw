//! Signatures of native plugin files (ADR 0024): an author signs their
//! own files with their own key, and a device remembers the key at the
//! first install and refuses, without a clear warning, an update signed
//! by another.
//!
//! The signature (Ed25519) covers [`PLUGIN_DOMAIN`] and then the whole
//! file, so that it cannot be taken for one of anything else (an update's
//! covers other bytes). It lies beside the file, as `<file>.sig`, in
//! [`PLUGIN_SIGNATURE_LEN`] bytes:
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..4 | `PPNS` |
//! | 4..6 | the format, 1 (little-endian) |
//! | 6..8 | zero |
//! | 8..40 | the signer's Ed25519 public key |
//! | 40..104 | the signature |
//!
//! A device checks it with [`PluginCheck`] as it reads the file, a piece
//! at a time; who signed it is for the device to judge
//! ([`PluginSignature::key`]).

use alloc::vec::Vec;
use core::fmt;

use ed25519_compact::{PublicKey, Signature, VerifyingState};

/// A plugin signature file's length.
pub const PLUGIN_SIGNATURE_LEN: usize = 104;
/// A plugin signature file's first bytes.
pub const PLUGIN_SIGNATURE_MAGIC: [u8; 4] = *b"PPNS";
/// The plugin signature format this crate reads and writes.
pub const PLUGIN_SIGNATURE_FORMAT: u16 = 1;
/// What the signature covers before the file.
pub const PLUGIN_DOMAIN: &[u8] = b"PaperPaw plugin file\0";

/// What is wrong with a plugin's signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureProblem {
    /// It is no plugin signature file.
    NotASignature,
    /// It is a signature file of a newer format than this crate reads.
    NewerFormat,
    /// The signature does not hold: the file was changed or damaged, or
    /// the signature is not one of it.
    Signature,
}

impl fmt::Display for SignatureProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SignatureProblem::NotASignature => "not a plugin signature",
            SignatureProblem::NewerFormat => "a plugin signature of a newer format",
            SignatureProblem::Signature => "the signature does not hold",
        })
    }
}

impl core::error::Error for SignatureProblem {}

/// A plugin file's signature: who signed it and how (see the
/// [module](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSignature {
    /// The signer's Ed25519 public key.
    pub key: [u8; 32],
    pub signature: [u8; 64],
}

impl PluginSignature {
    /// The signature in a signature file's bytes.
    pub fn read(bytes: &[u8]) -> Result<Self, SignatureProblem> {
        if bytes.len() != PLUGIN_SIGNATURE_LEN || bytes[..4] != PLUGIN_SIGNATURE_MAGIC {
            return Err(SignatureProblem::NotASignature);
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != PLUGIN_SIGNATURE_FORMAT {
            return Err(SignatureProblem::NewerFormat);
        }
        if bytes[6..8] != [0, 0] {
            return Err(SignatureProblem::NotASignature);
        }
        let mut key = [0; 32];
        key.copy_from_slice(&bytes[8..40]);
        let mut signature = [0; 64];
        signature.copy_from_slice(&bytes[40..104]);
        Ok(Self { key, signature })
    }

    /// The signature file's bytes.
    pub fn to_bytes(&self) -> [u8; PLUGIN_SIGNATURE_LEN] {
        let mut bytes = [0; PLUGIN_SIGNATURE_LEN];
        bytes[..4].copy_from_slice(&PLUGIN_SIGNATURE_MAGIC);
        bytes[4..6].copy_from_slice(&PLUGIN_SIGNATURE_FORMAT.to_le_bytes());
        bytes[8..40].copy_from_slice(&self.key);
        bytes[40..104].copy_from_slice(&self.signature);
        bytes
    }
}

/// A plugin file's signature, checked as the file is read:
/// [`PluginCheck::absorb`] every byte of it in order, then
/// [`PluginCheck::finish`].
pub struct PluginCheck {
    state: VerifyingState,
}

impl fmt::Debug for PluginCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginCheck").finish_non_exhaustive()
    }
}

impl PluginCheck {
    pub fn new(signature: &PluginSignature) -> Result<Self, SignatureProblem> {
        let key = PublicKey::new(signature.key);
        let mut state = key
            .verify_incremental(&Signature::new(signature.signature))
            .map_err(|_| SignatureProblem::Signature)?;
        state.absorb(PLUGIN_DOMAIN);
        Ok(Self { state })
    }

    /// The file's next bytes.
    pub fn absorb(&mut self, chunk: &[u8]) {
        self.state.absorb(chunk);
    }

    /// Whether the signature holds for all that was absorbed.
    pub fn finish(self) -> Result<(), SignatureProblem> {
        self.state.verify().map_err(|_| SignatureProblem::Signature)
    }
}

/// Signs the plugin file `file` with the Ed25519 key made from `seed`.
pub fn sign_plugin(file: &[u8], seed: &[u8; 32]) -> PluginSignature {
    let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(*seed));
    // Deterministic Ed25519 over the whole message, as for updates.
    let mut message = Vec::with_capacity(PLUGIN_DOMAIN.len() + file.len());
    message.extend_from_slice(PLUGIN_DOMAIN);
    message.extend_from_slice(file);
    PluginSignature {
        key: *pair.pk,
        signature: *pair.sk.sign(&message, None),
    }
}

/// Checks the plugin file `file` against the bytes of its signature file:
/// the signature if it holds.
pub fn check_plugin(file: &[u8], signature: &[u8]) -> Result<PluginSignature, SignatureProblem> {
    let signature = PluginSignature::read(signature)?;
    let mut check = PluginCheck::new(&signature)?;
    check.absorb(file);
    check.finish()?;
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TRAILER_LEN, public_key, sign};

    const SEED: [u8; 32] = [9; 32];

    fn file() -> Vec<u8> {
        (0..5000u32).map(|i| (i * 7) as u8).collect()
    }

    fn check_in_pieces(file: &[u8], signature: &PluginSignature, piece: usize) -> bool {
        let mut check = PluginCheck::new(signature).unwrap();
        for chunk in file.chunks(piece) {
            check.absorb(chunk);
        }
        check.finish().is_ok()
    }

    #[test]
    fn a_signed_file_holds_in_any_pieces() {
        let file = file();
        let signature = sign_plugin(&file, &SEED);
        assert_eq!(signature.key, public_key(&SEED));
        for piece in [1, 7, 4096, file.len()] {
            assert!(check_in_pieces(&file, &signature, piece), "{piece}");
        }
        let bytes = signature.to_bytes();
        assert_eq!(&bytes[..8], b"PPNS\x01\x00\x00\x00");
        assert_eq!(check_plugin(&file, &bytes), Ok(signature));
    }

    #[test]
    fn a_changed_byte_breaks_it() {
        let file = file();
        let signature = sign_plugin(&file, &SEED).to_bytes();
        for at in [0, 2500, file.len() - 1] {
            let mut changed = file.clone();
            changed[at] ^= 1;
            assert_eq!(
                check_plugin(&changed, &signature),
                Err(SignatureProblem::Signature),
                "{at}"
            );
        }
        assert_eq!(
            check_plugin(&file[1..], &signature),
            Err(SignatureProblem::Signature)
        );
        // Another key named.
        let mut other = signature;
        other[8] ^= 1;
        assert_eq!(
            check_plugin(&file, &other),
            Err(SignatureProblem::Signature)
        );
    }

    #[test]
    fn it_is_no_update_signature() {
        // The same key over the same bytes signs an update differently:
        // neither stands in for the other.
        let file = file();
        let update = sign(&file, &SEED).unwrap();
        let taken = PluginSignature {
            key: update.key,
            signature: update.signature,
        };
        assert_eq!(
            check_plugin(&file, &taken.to_bytes()),
            Err(SignatureProblem::Signature)
        );
        assert_eq!(
            check_plugin(&file, &update.to_bytes()[..TRAILER_LEN]),
            Err(SignatureProblem::NotASignature)
        );
    }

    #[test]
    fn what_is_not_a_signature_is_said() {
        let file = file();
        let signature = sign_plugin(&file, &SEED).to_bytes();
        assert_eq!(
            check_plugin(&file, &signature[..100]),
            Err(SignatureProblem::NotASignature)
        );
        let mut newer = signature;
        newer[4] = 2;
        assert_eq!(
            check_plugin(&file, &newer),
            Err(SignatureProblem::NewerFormat)
        );
        let mut reserved = signature;
        reserved[7] = 1;
        assert_eq!(
            check_plugin(&file, &reserved),
            Err(SignatureProblem::NotASignature)
        );
    }
}
