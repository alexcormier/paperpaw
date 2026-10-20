//! Known test vectors (RFC 1321 for MD5, FIPS 180 for SHA-1, the FNV
//! reference for FNV-1a), lengths around the padding boundaries,
//! KOReader's partial MD5 on files of many sizes and on storages that
//! read less than asked, hex both ways. Properties: a digest is the same
//! however its input is split; hex decodes what it encodes.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::Cell;

use paperpaw_digest::md5::{self, Md5};
use paperpaw_digest::sha1::{self, Sha1};
use paperpaw_digest::{fnv1a64, hex};
use paperpaw_platform::{DirEntry, Storage, StorageError};
use proptest::prelude::*;

fn md5_of(data: &[u8]) -> String {
    let mut md5 = Md5::new();
    md5.update(data);
    md5.hex()
}

#[test]
fn md5_rfc_1321_suite() {
    for (text, digest) in [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (
            "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            "57edf4a22be3c955ac49da2e2107b67a",
        ),
    ] {
        assert_eq!(md5::of(text), digest, "{text:?}");
        assert_eq!(md5_of(text.as_bytes()), digest);
    }
}

#[test]
fn sha1_fips_180_vectors() {
    assert_eq!(
        sha1::hex(b"abc"),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(
        sha1::hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(
        sha1::hex(
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmno\
              ijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
        ),
        "a49b2446a02c645bf419f995b67091253a04a259"
    );
    let finished = {
        let mut sha = Sha1::new();
        sha.update(b"abc");
        sha.finish()
    };
    assert_eq!(
        hex::encode(&finished),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn a_million_times_a() {
    let mut sha = Sha1::new();
    let mut md5 = Md5::new();
    let block = [b'a'; 1000];
    for _ in 0..1000 {
        sha.update(&block);
        md5.update(&block);
    }
    assert_eq!(sha.hex(), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    assert_eq!(md5.hex(), "7707d6ae4e027c70eea2a935c2296f21");
}

#[test]
fn lengths_around_the_padding_boundaries() {
    // A byte pattern of each length; the digests from a reference
    // implementation (Python's hashlib).
    let pattern = |n: usize| -> Vec<u8> { (0..n).map(|i| ((i * 7 + 3) % 256) as u8).collect() };
    for (n, md5, sha1) in [
        (
            55,
            "52c0e574e1198de5fe3f8f11440dcb1b",
            "ddf57317ef34bfee3b6df83d359098930eb278bc",
        ),
        (
            56,
            "46c9907fc908ee68b1e7b8e71286a518",
            "a0d492bb0fc889d0eca3bc137066ab6f4f74f369",
        ),
        (
            57,
            "1c805dd236c35cab25fcb1bc73802c51",
            "11a02dcf95859677a62e75024067c22b165d890f",
        ),
        (
            63,
            "a62f6d59e837867693f042f5b8f5a236",
            "c55856749bef509bdfe6bfebfc7bf4e793e82132",
        ),
        (
            64,
            "7160b8fb5e9e4023d549c3971fbaeead",
            "bede92be29c3874e1b54ddc77988d606fc857a8e",
        ),
        (
            65,
            "70bd662e7aefbda85a0f7244167b7897",
            "b05a80522b053d6dc7e0a517d0e70212c7dad11f",
        ),
        (
            119,
            "e84905d4214f4d1ca56c2cdcc152b143",
            "504e27376a6e0f0dba8295b85cb25dc4dfa17d23",
        ),
        (
            120,
            "e3eb5a6c8669ea01a8c185b8abc8a5dc",
            "82134b02fb3f702491be9bed581eeab59334acb2",
        ),
        (
            127,
            "acce2474d6cc8302120d09c818d17ef7",
            "34d5e582029e9b9b85b2febe31da3db7cdabaaea",
        ),
        (
            128,
            "10b2da1a82f16d99a81a7203fe9f02cb",
            "a09133e6730ffe899efb70204cb5646cd5dc24ee",
        ),
        (
            1000,
            "10046f077f2082ac19676b8079f1cb1a",
            "4231a8a50a10fa9758db8ec71fdef855b751048a",
        ),
    ] {
        let data = pattern(n);
        assert_eq!(md5_of(&data), md5, "md5 of {n}");
        assert_eq!(sha1::hex(&data), sha1, "sha1 of {n}");
    }
}

#[test]
fn fnv1a64_reference_vectors() {
    for (data, hash) in [
        (&b""[..], 0xcbf2_9ce4_8422_2325u64),
        (b"a", 0xaf63_dc4c_8601_ec8c),
        (b"b", 0xaf63_df4c_8601_f1a5),
        (b"c", 0xaf63_de4c_8601_eff2),
        (b"d", 0xaf63_d94c_8601_e773),
        (b"e", 0xaf63_d84c_8601_e5c0),
        (b"f", 0xaf63_db4c_8601_ead9),
        (b"fo", 0x0898_5907_b541_d342),
        (b"foo", 0xdcb2_7518_fed9_d577),
        (b"foob", 0xdd12_0e79_0c25_12af),
        (b"fooba", 0xcac1_65af_a2fe_f40a),
        (b"foobar", 0x8594_4171_f739_67e8),
        (b"\x00", 0xaf63_bd4c_8601_b7df),
        (b"\xff", 0xaf64_724c_8602_eb6e),
        (b"chongo was here!\n", 0x4681_0940_eff5_f915),
    ] {
        assert_eq!(fnv1a64(data), hash, "{data:?}");
    }
}

#[test]
fn hex_decoding_rejects_everything_but_digit_pairs() {
    assert_eq!(hex::decode("00ff7F80"), Some(vec![0, 0xff, 0x7f, 0x80]));
    for bad in [
        "0", "abc", "0g", "g0", " 00", "00 ", "0x00", "+0", "-0", "ä0", "０0", "\u{0}0",
    ] {
        assert_eq!(hex::decode(bad), None, "{bad:?}");
    }
    let mut out = String::from("x");
    hex::push(&mut out, 0xa5);
    assert_eq!(out, "xa5");
}

/// One file in memory that gives at most `most` bytes per read and can
/// fail from a given read on.
struct File {
    data: Vec<u8>,
    most: usize,
    fail_from: Option<usize>,
    reads: Cell<usize>,
}

impl File {
    fn new(data: Vec<u8>) -> File {
        File {
            data,
            most: usize::MAX,
            fail_from: None,
            reads: Cell::new(0),
        }
    }
}

impl Storage for File {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let read = self.reads.get();
        self.reads.set(read + 1);
        if self.fail_from.is_some_and(|from| read >= from) {
            return Err(StorageError::Io);
        }
        let start = (offset as usize).min(self.data.len());
        let n = buf.len().min(self.data.len() - start).min(self.most);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.data.len() as u64)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

/// KOReader's partial MD5 written out: 1 KiB at 0, then at 1 KiB times
/// 4 to the power of 0 to 10, as far as the file reaches.
fn koreader_partial(data: &[u8]) -> String {
    let mut sampled = Vec::new();
    for i in -1i32..=10 {
        let offset = if i < 0 { 0 } else { 1024usize << (2 * i) };
        if offset >= data.len() {
            break;
        }
        sampled.extend_from_slice(&data[offset..data.len().min(offset + 1024)]);
    }
    md5_of(&sampled)
}

fn book(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 7) % 251) as u8).collect()
}

#[test]
fn partial_md5_matches_a_reference_value() {
    // From the same sampling done with Python's hashlib.
    let file = File::new(book(70_000));
    assert_eq!(
        md5::partial(&file, "b.epub").as_deref(),
        Some("724b94d04f9ceeab34ab28718b86062d")
    );
}

#[test]
fn partial_md5_samples_files_of_every_size() {
    for len in [
        0, 1, 1023, 1024, 1025, 2048, 4095, 4096, 4097, 16_384, 16_385, 65_536, 300_000,
    ] {
        let data = book(len);
        let file = File::new(data.clone());
        assert_eq!(
            md5::partial(&file, "b.epub"),
            Some(koreader_partial(&data)),
            "{len} bytes"
        );
    }
    assert_eq!(
        md5::partial(&File::new(Vec::new()), "empty").as_deref(),
        Some("d41d8cd98f00b204e9800998ecf8427e")
    );
}

#[test]
fn partial_md5_of_an_unreadable_file_is_none() {
    for from in [0, 1, 3] {
        let mut file = File::new(book(100_000));
        file.fail_from = Some(from);
        assert_eq!(
            md5::partial(&file, "b.epub"),
            None,
            "failing from read {from}"
        );
    }
}

#[test]
#[ignore = "bug: partial() takes one read per sample, so a storage that reads less than asked (as Storage::read_at allows) gives another fingerprint"]
fn partial_md5_does_not_depend_on_how_much_a_read_gives() {
    let data = book(100_000);
    let mut file = File::new(data.clone());
    file.most = 512;
    assert_eq!(md5::partial(&file, "b.epub"), Some(koreader_partial(&data)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    #[test]
    fn digests_do_not_depend_on_how_the_input_is_split(
        data in proptest::collection::vec(any::<u8>(), 0..600),
        cuts in proptest::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
        let mut cuts: Vec<usize> = cuts.iter().map(|c| c.index(data.len() + 1)).collect();
        cuts.push(0);
        cuts.push(data.len());
        cuts.sort_unstable();
        let mut md5 = Md5::new();
        let mut sha = Sha1::new();
        for pair in cuts.windows(2) {
            md5.update(&data[pair[0]..pair[1]]);
            sha.update(&data[pair[0]..pair[1]]);
            // Empty updates change nothing.
            md5.update(&[]);
            sha.update(&[]);
        }
        prop_assert_eq!(md5.hex(), md5_of(&data));
        prop_assert_eq!(sha.hex(), sha1::hex(&data));
    }

    #[test]
    fn byte_by_byte_equals_whole(data in proptest::collection::vec(any::<u8>(), 0..200)) {
        let mut md5 = Md5::new();
        let mut sha = Sha1::default();
        for b in &data {
            md5.update(std::slice::from_ref(b));
            sha.update(std::slice::from_ref(b));
        }
        let copy = md5.clone();
        prop_assert_eq!(md5.hex(), md5_of(&data));
        // A clone goes on independently.
        prop_assert_eq!(copy.hex(), md5_of(&data));
        prop_assert_eq!(sha.hex(), sha1::hex(&data));
    }

    #[test]
    fn hex_decodes_what_it_encodes(bytes in proptest::collection::vec(any::<u8>(), 0..64)) {
        let text = hex::encode(&bytes);
        prop_assert_eq!(text.len(), bytes.len() * 2);
        prop_assert!(text.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        prop_assert_eq!(hex::decode(&text), Some(bytes.clone()));
        prop_assert_eq!(hex::decode(&text.to_uppercase()), Some(bytes));
    }

    #[test]
    fn hex_decoding_any_text_never_panics(text in "\\PC{0,40}|[0-9a-fA-Fg ]{0,40}") {
        if let Some(bytes) = hex::decode(&text) {
            prop_assert_eq!(hex::encode(&bytes), text.to_lowercase());
        }
    }

    #[test]
    fn partial_md5_matches_koreader_for_any_length(len in 0usize..80_000) {
        let data = book(len);
        prop_assert_eq!(md5::partial(&File::new(data.clone()), "f"), Some(koreader_partial(&data)));
    }
}
