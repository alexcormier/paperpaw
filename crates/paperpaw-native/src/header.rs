//! A plugin file's header: what it is for, how large its parts are, and
//! the digest of what follows (see the [crate documentation](crate)).

use core::fmt;

use paperpaw_digest::sha256::Sha256;

use crate::interface::ENTRY_LEN;
use crate::signature::Signature;

/// A plugin file's first bytes.
pub const MAGIC: [u8; 4] = *b"PPNP";
/// The format this crate reads and writes: 2 named the compiler, 3 adds
/// hot code and the patches that move it.
pub const FORMAT: u16 = 3;
/// The header's length.
pub const HEADER_LEN: usize = 92;
/// The bytes of the header its digest covers (all before the digest).
const DIGESTED_LEN: usize = 60;
/// The largest image taken: more than either device has room for.
pub const MAX_IMAGE_LEN: u32 = 4 << 20;
/// Where `paperpaw-plugin.x` links a plugin's read-only data.
pub const LINK_BASE: u32 = 0x3C00_0000;
/// How far the instruction bus's view of memory lies from the data bus's
/// on both chips: memory seen at data address A runs at A + this.
pub const CODE_OFFSET: u32 = 0x0600_0000;

/// The chips plugin files are made for, by the ids ESP-IDF's images
/// give them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Chip {
    /// The Xteink X4's (RISC-V).
    Esp32C3,
    /// The Xteink X4 Pro's (Xtensa).
    Esp32S3,
}

impl Chip {
    /// The chip's id in files and images.
    pub const fn id(self) -> u16 {
        match self {
            Chip::Esp32C3 => 5,
            Chip::Esp32S3 => 9,
        }
    }

    pub const fn from_id(id: u16) -> Option<Chip> {
        match id {
            5 => Some(Chip::Esp32C3),
            9 => Some(Chip::Esp32S3),
            _ => None,
        }
    }

    /// The Rust target plugins for it are built for.
    pub const fn target(self) -> &'static str {
        match self {
            Chip::Esp32C3 => "riscv32imc-unknown-none-elf",
            Chip::Esp32S3 => "xtensa-esp32s3-none-elf",
        }
    }
}

impl fmt::Display for Chip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Chip::Esp32C3 => "ESP32-C3",
            Chip::Esp32S3 => "ESP32-S3",
        })
    }
}

/// What is wrong with a plugin file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// It is no plugin file.
    NotAPlugin,
    /// It is a plugin file of a newer format than this one reads.
    NewerFormat,
    /// It is a plugin file of an older format: it needs a new build.
    OlderFormat,
    /// It is made for a chip this crate does not know.
    UnknownChip,
    /// Its header does not hold together.
    Malformed,
    /// It is not as long as its header says.
    Length,
    /// It was changed or damaged: its digest does not hold.
    Damaged,
    /// A word it lists to relocate is not one, or a patch is not right.
    Relocation,
    /// It ends with an author's signature that does not hold.
    Signature,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Problem::NotAPlugin => "not a plugin file",
            Problem::NewerFormat => "a plugin file of a newer format",
            Problem::OlderFormat => "a plugin file of an older format, to be built anew",
            Problem::UnknownChip => "made for an unknown chip",
            Problem::Malformed => "its header does not hold together",
            Problem::Length => "not as long as it says",
            Problem::Damaged => "changed or damaged",
            Problem::Relocation => "a word to relocate or a patch is not right",
            Problem::Signature => "its author's signature does not hold",
        })
    }
}

impl core::error::Error for Problem {}

/// A plugin file's header (see the [crate documentation](crate)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub chip: Chip,
    /// The compiler, target and SDK the plugin was built with.
    pub fingerprint: u64,
    /// The data address the image is linked at.
    pub base: u32,
    /// The image's bytes: read-only data, then code.
    pub image_len: u32,
    /// Where the code begins in the image.
    pub code_at: u32,
    /// Where the plugin's [`Entry`](crate::Entry) lies in the image.
    pub entry: u32,
    /// The word in the image that is to hold the address of the
    /// firmware's [`Api`](crate::Api).
    pub api: u32,
    /// How many words are to relocate.
    pub relocs: u32,
    /// What the image's address must be a multiple of.
    pub align: u32,
    /// The compiler it was built with ([`compiler::id`](crate::compiler::id)
    /// of its release).
    pub compiler: u32,
    /// Where the hot code begins in the image: code a device may run from
    /// faster memory than the rest (0 when there is none).
    pub hot_at: u32,
    /// The hot code's bytes (0: none).
    pub hot_len: u32,
    /// How many patches follow the words to relocate: the instructions
    /// that reach across the hot code's edge (see [`Relocator`](crate::Relocator)).
    pub patches: u32,
    /// SHA-256 of the header's first 60 bytes and of all after the header
    /// (see [`Header::digest`]).
    pub sha256: [u8; 32],
}

impl Header {
    /// The header at the start of `bytes` (at least [`HEADER_LEN`]),
    /// checked to hold together.
    pub fn read(bytes: &[u8]) -> Result<Header, Problem> {
        let Some(bytes) = bytes.get(..HEADER_LEN) else {
            return Err(if bytes.starts_with(&MAGIC) || bytes.len() < 4 {
                Problem::Length
            } else {
                Problem::NotAPlugin
            });
        };
        if bytes[..4] != MAGIC {
            return Err(Problem::NotAPlugin);
        }
        match le16(bytes, 4) {
            FORMAT => {}
            format if format < FORMAT => return Err(Problem::OlderFormat),
            _ => return Err(Problem::NewerFormat),
        }
        let chip = Chip::from_id(le16(bytes, 6)).ok_or(Problem::UnknownChip)?;
        let mut sha256 = [0; 32];
        sha256.copy_from_slice(&bytes[DIGESTED_LEN..HEADER_LEN]);
        let header = Header {
            chip,
            fingerprint: u64::from(le32(bytes, 8)) | (u64::from(le32(bytes, 12)) << 32),
            base: le32(bytes, 16),
            image_len: le32(bytes, 20),
            code_at: le32(bytes, 24),
            entry: le32(bytes, 28),
            api: le32(bytes, 32),
            relocs: le32(bytes, 36),
            align: le32(bytes, 40),
            compiler: le32(bytes, 44),
            hot_at: le32(bytes, 48),
            hot_len: le32(bytes, 52),
            patches: le32(bytes, 56),
            sha256,
        };
        if header.compiler == 0 || !header.holds_together() {
            return Err(Problem::Malformed);
        }
        Ok(header)
    }

    /// Whether the parts lie where they can: the entry and the table's
    /// word in the read-only data, the code after it, the hot code in the
    /// code, the image in the address space at its alignment.
    fn holds_together(&self) -> bool {
        let entry_len = ENTRY_LEN as u32;
        self.image_len > 0
            && self.image_len <= MAX_IMAGE_LEN
            && self.code_at < self.image_len
            && self.code_at.is_multiple_of(4)
            && self.entry.is_multiple_of(8)
            && self
                .entry
                .checked_add(entry_len)
                .is_some_and(|end| end <= self.code_at)
            && self.api.is_multiple_of(4)
            && self
                .api
                .checked_add(4)
                .is_some_and(|end| end <= self.code_at)
            && (self.api + 4 <= self.entry || self.entry + entry_len <= self.api)
            && self.relocs <= self.image_len / 4
            && self.hot_holds_together()
            && self.align.is_power_of_two()
            && (4..=4096).contains(&self.align)
            && self.base.is_multiple_of(self.align)
            && self
                .base
                .checked_add(self.image_len)
                .and_then(|end| end.checked_add(CODE_OFFSET))
                .is_some()
    }

    /// Whether the hot code lies in the code, in whole words, and the
    /// patches are not more than the code's instructions. Only RISC-V
    /// moves hot code: an Xtensa file has none.
    fn hot_holds_together(&self) -> bool {
        let placed = if self.hot_len == 0 {
            self.hot_at == 0
        } else {
            self.hot_at >= self.code_at
                && self.hot_at.is_multiple_of(4)
                && self.hot_len.is_multiple_of(4)
                && self
                    .hot_at
                    .checked_add(self.hot_len)
                    .is_some_and(|end| end <= self.image_len)
        };
        let riscv = self.chip == Chip::Esp32C3 || (self.hot_len == 0 && self.patches == 0);
        placed && riscv && self.patches <= self.image_len.saturating_sub(self.code_at) / 2
    }

    /// The header's bytes.
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0; HEADER_LEN];
        bytes[..DIGESTED_LEN].copy_from_slice(&self.digest_prefix());
        bytes[DIGESTED_LEN..].copy_from_slice(&self.sha256);
        bytes
    }

    /// The header's bytes its digest covers, before the image and words.
    /// Hardware loaders can feed these to their chip's SHA-256 engine.
    pub fn digest_prefix(&self) -> [u8; DIGESTED_LEN] {
        let mut bytes = [0; DIGESTED_LEN];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&FORMAT.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.chip.id().to_le_bytes());
        bytes[8..16].copy_from_slice(&self.fingerprint.to_le_bytes());
        for (i, value) in [
            self.base,
            self.image_len,
            self.code_at,
            self.entry,
            self.api,
            self.relocs,
            self.align,
            self.compiler,
            self.hot_at,
            self.hot_len,
            self.patches,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[16 + 4 * i..20 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// The digest begun with the header's part: fed with every byte after
    /// the header, in order, it ends as [`Header::sha256`] (see
    /// [`Header::holds`]).
    pub fn digest(&self) -> Sha256 {
        let mut sha = Sha256::new();
        sha.update(&self.digest_prefix());
        sha
    }

    /// Whether `sha`, begun by [`Header::digest`] and fed with all after
    /// the header, is the header's.
    pub fn holds(&self, sha: Sha256) -> bool {
        sha.finish() == self.sha256
    }

    /// Where the words to relocate begin in the file, the patches after
    /// them.
    pub fn relocs_at(&self) -> u64 {
        HEADER_LEN as u64 + u64::from(self.image_len)
    }

    /// How many words follow the image: those to relocate, then two for
    /// each patch.
    pub fn tail_words(&self) -> usize {
        self.relocs as usize + 2 * self.patches as usize
    }

    /// The file's length without its author's signature.
    pub fn file_len(&self) -> u64 {
        self.relocs_at() + 4 * self.tail_words() as u64
    }

    /// Whether the image offset `at` lies in the hot code.
    pub fn in_hot(&self, at: u32) -> bool {
        at.wrapping_sub(self.hot_at) < self.hot_len
    }

    /// Whether a file of `len` bytes with this header ends with an
    /// author's signature (at [`Header::file_len`]); not as long as either
    /// is [`Problem::Length`].
    pub fn signed(&self, len: u64) -> Result<bool, Problem> {
        match len.checked_sub(self.file_len()) {
            Some(0) => Ok(false),
            Some(n) if n == crate::signature::SIGNATURE_LEN as u64 => Ok(true),
            _ => Err(Problem::Length),
        }
    }

    /// The header of a whole plugin file and its author's signature, if
    /// it is signed, all checked (its length, its digest, its
    /// relocations, the signature).
    pub fn check_file(file: &[u8]) -> Result<(Header, Option<Signature>), Problem> {
        let header = Header::read(file)?;
        let end = header.file_len() as usize;
        let signed = header.signed(file.len() as u64)?;
        let mut sha = header.digest();
        sha.update(&file[HEADER_LEN..end]);
        if !header.holds(sha) {
            return Err(Problem::Damaged);
        }
        let words: alloc::vec::Vec<u32> = header.words(&file[..end]).collect();
        crate::check_relocations(&header, &words)?;
        if !signed {
            return Ok((header, None));
        }
        let signature = Signature::read(&file[end..]).ok_or(Problem::Signature)?;
        if signature.signed_len as usize != end || signature.holds(&file[..end]).is_err() {
            return Err(Problem::Signature);
        }
        Ok((header, Some(signature)))
    }

    /// The words after the image in a whole file whose length was checked.
    fn words<'a>(&self, file: &'a [u8]) -> impl Iterator<Item = u32> + 'a {
        let at = self.relocs_at() as usize;
        file[at..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|w| u32::from_le_bytes(*w))
    }
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;

    use alloc::vec::Vec;

    use super::*;

    /// A small, valid plugin file: 64 bytes of read-only data (the entry
    /// at 0, the table's word at 16, an address of the data at 20, one
    /// of the code at 24), then 32 bytes of code.
    pub(crate) fn file() -> Vec<u8> {
        let mut image = alloc::vec![0u8; 96];
        image[20..24].copy_from_slice(&(LINK_BASE + 40).to_le_bytes());
        image[24..28].copy_from_slice(&(LINK_BASE + CODE_OFFSET + 64).to_le_bytes());
        for (i, b) in image.iter_mut().enumerate().skip(64) {
            *b = i as u8;
        }
        seal(chip_header(Chip::Esp32C3, 96, 2), &image, &[20, 24])
    }

    pub(crate) fn chip_header(chip: Chip, image_len: u32, relocs: u32) -> Header {
        Header {
            chip,
            fingerprint: 0x0123_4567_89ab_cdef,
            base: LINK_BASE,
            image_len,
            code_at: 64,
            entry: 0,
            api: 16,
            relocs,
            align: 16,
            compiler: crate::compiler::id("1.99.0 (b940084d7 2026-09-28)"),
            hot_at: 0,
            hot_len: 0,
            patches: 0,
            sha256: [0; 32],
        }
    }

    /// The file of `header`, `image` and `relocs`, its digest made.
    pub(crate) fn seal(mut header: Header, image: &[u8], relocs: &[u32]) -> Vec<u8> {
        let mut rest = image.to_vec();
        for r in relocs {
            rest.extend_from_slice(&r.to_le_bytes());
        }
        let mut sha = header.digest();
        sha.update(&rest);
        header.sha256 = sha.finish();
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(&rest);
        file
    }

    #[test]
    fn a_file_reads_back_as_written() {
        let file = file();
        let (header, signature) = Header::check_file(&file).unwrap();
        assert_eq!(signature, None);
        assert_eq!(header.chip, Chip::Esp32C3);
        assert_eq!(header.fingerprint, 0x0123_4567_89ab_cdef);
        assert_eq!(header.file_len(), file.len() as u64);
        assert_eq!(header.to_bytes()[..], file[..HEADER_LEN]);
        assert_eq!(Chip::from_id(Chip::Esp32S3.id()), Some(Chip::Esp32S3));
    }

    #[test]
    fn what_is_not_a_plugin_file_is_said() {
        assert_eq!(Header::read(b"\x7fELF"), Err(Problem::NotAPlugin));
        assert_eq!(Header::read(&[0; 100]), Err(Problem::NotAPlugin));
        assert_eq!(Header::read(b"PPNP\x01\x00"), Err(Problem::Length));
        assert_eq!(Header::read(b""), Err(Problem::Length));
        let mut newer = file();
        newer[4] = 4;
        assert_eq!(Header::read(&newer), Err(Problem::NewerFormat));
        let mut older = file();
        older[4] = 2;
        assert_eq!(Header::read(&older), Err(Problem::OlderFormat));
        let mut chip = file();
        chip[6] = 7;
        assert_eq!(Header::read(&chip), Err(Problem::UnknownChip));
    }

    #[test]
    fn a_header_that_does_not_hold_together_is_refused() {
        let good = chip_header(Chip::Esp32S3, 96, 2);
        let bad = [
            Header {
                image_len: 0,
                ..good
            },
            Header {
                image_len: MAX_IMAGE_LEN + 4,
                ..good
            },
            Header {
                code_at: 96,
                ..good
            },
            Header {
                code_at: 62,
                ..good
            },
            Header { entry: 4, ..good },
            Header { entry: 56, ..good },
            Header { api: 18, ..good },
            Header { api: 64, ..good },
            Header { api: 8, ..good },
            Header { relocs: 25, ..good },
            Header { align: 2, ..good },
            Header { align: 24, ..good },
            Header {
                align: 8192,
                ..good
            },
            Header {
                base: LINK_BASE + 4,
                ..good
            },
            Header {
                base: u32::MAX - 64,
                ..good
            },
            // Hot code outside the code, not in whole words, or on Xtensa.
            Header {
                chip: Chip::Esp32C3,
                hot_at: 60,
                hot_len: 8,
                ..good
            },
            Header {
                chip: Chip::Esp32C3,
                hot_at: 66,
                hot_len: 8,
                ..good
            },
            Header {
                chip: Chip::Esp32C3,
                hot_at: 64,
                hot_len: 6,
                ..good
            },
            Header {
                chip: Chip::Esp32C3,
                hot_at: 88,
                hot_len: 12,
                ..good
            },
            Header { hot_at: 64, ..good },
            Header {
                hot_at: 64,
                hot_len: 8,
                ..good
            },
            Header { patches: 1, ..good },
            Header {
                chip: Chip::Esp32C3,
                patches: 17,
                ..good
            },
        ];
        for header in bad {
            assert_eq!(
                Header::read(&header.to_bytes()),
                Err(Problem::Malformed),
                "{header:?}"
            );
        }
        // No compiler named.
        let unnamed = Header {
            compiler: 0,
            ..good
        };
        assert_eq!(Header::read(&unnamed.to_bytes()), Err(Problem::Malformed));
        // Hot code at the code's end, on RISC-V.
        let hot = Header {
            chip: Chip::Esp32C3,
            hot_at: 88,
            hot_len: 8,
            patches: 16,
            ..good
        };
        assert_eq!(Header::read(&hot.to_bytes()), Ok(hot));
        assert!(hot.in_hot(88) && hot.in_hot(95) && !hot.in_hot(96) && !hot.in_hot(87));
    }

    #[test]
    fn length_digest_and_relocations_are_checked() {
        let file = file();
        assert_eq!(
            Header::check_file(&file[..file.len() - 1]),
            Err(Problem::Length)
        );
        let mut longer = file.clone();
        longer.push(0);
        assert_eq!(Header::check_file(&longer), Err(Problem::Length));
        for at in [HEADER_LEN, HEADER_LEN + 70, file.len() - 1, 9] {
            let mut changed = file.clone();
            changed[at] ^= 1;
            assert_eq!(Header::check_file(&changed), Err(Problem::Damaged), "{at}");
        }
        // Words listed out of order, though sealed.
        let header = chip_header(Chip::Esp32C3, 96, 2);
        let image = &file[HEADER_LEN..HEADER_LEN + 96];
        let unordered = seal(header, image, &[24, 20]);
        assert_eq!(Header::check_file(&unordered), Err(Problem::Relocation));
    }

    #[test]
    fn hardware_digest_prefix_covers_the_same_file_bytes() {
        let file = file();
        let header = Header::read(&file).unwrap();
        let prefix = header.digest_prefix();
        assert_eq!(prefix.as_slice(), &file[..HEADER_LEN - 32]);
        let mut sha = Sha256::new();
        sha.update(&prefix);
        sha.update(&file[HEADER_LEN..]);
        assert_eq!(sha.finish(), header.sha256);
        for at in [0, 8, 16, 44, 48, 56] {
            let mut changed = prefix;
            changed[at] ^= 1;
            let mut sha = Sha256::new();
            sha.update(&changed);
            sha.update(&file[HEADER_LEN..]);
            assert_ne!(sha.finish(), header.sha256, "header byte {at}");
        }
    }

    #[test]
    fn a_signed_file_says_who_signed_it() {
        let file = file();
        let mut signed = file.clone();
        let signature = crate::signature::sign(&file, &[5; 32]);
        signed.extend_from_slice(&signature.to_bytes());
        assert_eq!(
            Header::check_file(&signed).map(|(_, s)| s),
            Ok(Some(signature))
        );
        // Its trailer changed, or cut short.
        let mut changed = signed.clone();
        let last = changed.len() - 10;
        changed[last] ^= 1;
        assert_eq!(Header::check_file(&changed), Err(Problem::Signature));
        assert_eq!(
            Header::check_file(&signed[..signed.len() - 1]),
            Err(Problem::Length)
        );
        let mut not_a_trailer = file.clone();
        not_a_trailer.extend_from_slice(&[0; crate::signature::SIGNATURE_LEN]);
        assert_eq!(Header::check_file(&not_a_trailer), Err(Problem::Signature));
        // A signature of another file.
        let mut other = file.clone();
        other.extend_from_slice(&crate::signature::sign(b"other", &[5; 32]).to_bytes());
        assert_eq!(Header::check_file(&other), Err(Problem::Signature));
    }
}
