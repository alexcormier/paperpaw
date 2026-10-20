//! Hostile and unusual archives: directory records that lie about counts,
//! sizes and offsets, ZIP64 markers, odd entry names (path traversal,
//! absolute paths, backslashes, invalid UTF-8, duplicates), unsupported
//! entries, archive comments of every length, corrupt and expanding
//! deflate data. Each must give its data or a clean error, within the
//! reader's bounded buffers; random archives read back byte-identical.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_zip::{Entry, Method, ZipArchive, ZipError};
use proptest::prelude::*;

/// One archive file in memory.
struct OneFile(Vec<u8>);

impl Storage for OneFile {
    fn read_at(&self, _: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
    fn size(&self, _: &str) -> Result<u64, StorageError> {
        Ok(self.0.len() as u64)
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

/// The deflate history: no chunk the reader hands out is larger.
const WINDOW: usize = 32 * 1024;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// An entry as it is written, every field under the test's control.
#[derive(Clone)]
struct Raw {
    name: Vec<u8>,
    method: u16,
    flags: u16,
    /// The bytes after the local header (compressed, if deflated).
    payload: Vec<u8>,
    /// The uncompressed size the headers claim.
    size: u32,
    crc: u32,
    /// An extra field only in the local header (as Info-ZIP writes).
    local_extra: Vec<u8>,
}

impl Raw {
    fn stored(name: &str, data: &[u8]) -> Raw {
        Raw {
            name: name.as_bytes().to_vec(),
            method: 0,
            flags: 0,
            payload: data.to_vec(),
            size: data.len() as u32,
            crc: crc32(data),
            local_extra: Vec::new(),
        }
    }

    fn deflated(name: &str, data: &[u8]) -> Raw {
        Raw {
            method: 8,
            payload: miniz_oxide::deflate::compress_to_vec(data, 6),
            ..Raw::stored(name, data)
        }
    }
}

/// Where the parts of a written archive are.
struct Layout {
    local: Vec<usize>,
    central: Vec<usize>,
    eocd: usize,
}

fn put16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// A ZIP archive of `entries` with `comment`, as the specification lays
/// it out.
fn write(entries: &[Raw], comment: &[u8]) -> (Vec<u8>, Layout) {
    let mut out = Vec::new();
    let mut local = Vec::new();
    for e in entries {
        local.push(out.len());
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&e.flags.to_le_bytes());
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // time, date
        out.extend_from_slice(&e.crc.to_le_bytes());
        out.extend_from_slice(&(e.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&e.size.to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&(e.local_extra.len() as u16).to_le_bytes());
        out.extend_from_slice(&e.name);
        out.extend_from_slice(&e.local_extra);
        out.extend_from_slice(&e.payload);
    }
    let cd_offset = out.len();
    let mut central = Vec::new();
    for (e, &offset) in entries.iter().zip(&local) {
        central.push(out.len());
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&e.flags.to_le_bytes());
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&e.crc.to_le_bytes());
        out.extend_from_slice(&(e.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&e.size.to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0; 12]); // extra, comment, disk, attributes
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&e.name);
    }
    let eocd = out.len();
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&((eocd - cd_offset) as u32).to_le_bytes());
    out.extend_from_slice(&(cd_offset as u32).to_le_bytes());
    out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
    out.extend_from_slice(comment);
    (
        out,
        Layout {
            local,
            central,
            eocd,
        },
    )
}

fn open(data: Vec<u8>) -> (OneFile, Result<ZipArchive, ZipError>) {
    let storage = OneFile(data);
    let archive = ZipArchive::open(&storage, "x.zip");
    (storage, archive)
}

fn entry(storage: &OneFile, archive: &ZipArchive, name: &str) -> Entry {
    archive.entry(storage, name).unwrap().expect(name)
}

/// Streams `entry` whole, checking every chunk stays within the window.
fn streamed(storage: &OneFile, archive: &ZipArchive, entry: &Entry) -> Result<Vec<u8>, ZipError> {
    let mut out = Vec::new();
    archive.stream(storage, entry, &mut |chunk| {
        assert!(chunk.len() <= WINDOW, "chunk of {} bytes", chunk.len());
        out.extend_from_slice(chunk);
        true
    })?;
    Ok(out)
}

fn is_invalid(result: Result<ZipArchive, ZipError>) -> bool {
    matches!(result, Err(ZipError::Invalid(_)))
}

#[test]
fn an_archive_without_entries_opens_empty() {
    let (data, _) = write(&[], b"");
    assert_eq!(data.len(), 22);
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    assert!(archive.is_empty());
    assert_eq!(archive.entries(&storage).count(), 0);
    assert_eq!(archive.entry(&storage, "anything").unwrap(), None);
}

#[test]
fn files_shorter_than_an_end_record_are_invalid() {
    for len in 0..22 {
        let (_, archive) = open(vec![0x50; len]);
        assert!(is_invalid(archive), "{len} bytes");
    }
    // The signature alone, without the rest of the record.
    let (_, archive) = open(vec![0x50, 0x4b, 0x05, 0x06]);
    assert!(is_invalid(archive));
}

#[test]
fn an_entry_count_larger_than_the_directory_is_invalid() {
    let (mut data, layout) = write(&[Raw::stored("a", b"1"), Raw::stored("b", b"2")], b"");
    for count in [3u16, 100, 0xfffe] {
        put16(&mut data, layout.eocd + 10, count);
        let (_, archive) = open(data.clone());
        assert!(is_invalid(archive), "count {count}");
    }
}

#[test]
fn a_smaller_entry_count_lists_only_that_many() {
    let (mut data, layout) = write(&[Raw::stored("a", b"1"), Raw::stored("b", b"2")], b"");
    put16(&mut data, layout.eocd + 10, 1);
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    assert_eq!(archive.len(), 1);
    let a = entry(&storage, &archive, "a");
    assert_eq!(archive.read(&storage, &a, 10).unwrap(), b"1");
}

#[test]
fn a_directory_beyond_the_end_of_the_file_is_invalid() {
    let (data, layout) = write(&[Raw::stored("a", b"1")], b"");
    // Its size, its offset, and both so their sum overflows 32 bits.
    let mut size = data.clone();
    put32(&mut size, layout.eocd + 12, 0x1000);
    let mut offset = data.clone();
    put32(&mut offset, layout.eocd + 16, data.len() as u32);
    let mut both = data;
    put32(&mut both, layout.eocd + 12, 0xffff_fff0);
    put32(&mut both, layout.eocd + 16, 0xffff_fff0);
    for data in [size, offset, both] {
        let (_, archive) = open(data);
        assert!(is_invalid(archive));
    }
}

#[test]
fn zip64_markers_are_unsupported() {
    let (data, layout) = write(&[Raw::stored("a", b"1")], b"");
    let mut count = data.clone();
    put16(&mut count, layout.eocd + 10, 0xffff);
    let mut offset = data;
    put32(&mut offset, layout.eocd + 16, 0xffff_ffff);
    for data in [count, offset] {
        let (_, archive) = open(data);
        assert_eq!(archive.unwrap_err(), ZipError::Unsupported("ZIP64"));
    }
}

#[test]
fn zip64_entry_sizes_are_refused_by_read_and_end_in_an_error_when_streamed() {
    // A ZIP64 entry keeps 0xffffffff in the 32-bit size fields.
    let mut stored = Raw::stored("big", b"0123456789");
    stored.size = 0xffff_ffff;
    let mut deflated = Raw::deflated("packed", b"0123456789");
    deflated.size = 0xffff_ffff;
    let (mut data, layout) = write(&[stored, deflated], b"");
    put32(&mut data, layout.central[0] + 20, 0xffff_ffff);
    put32(&mut data, layout.central[1] + 20, 0xffff_ffff);
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    for name in ["big", "packed"] {
        let e = entry(&storage, &archive, name);
        assert_eq!(e.size, 0xffff_ffff);
        assert_eq!(archive.read(&storage, &e, 1 << 20), Err(ZipError::TooLarge));
    }
    // Streamed, stored data runs into the end of the file, never past it;
    // deflated data ends where its stream says.
    let big = entry(&storage, &archive, "big");
    assert!(matches!(
        streamed(&storage, &archive, &big),
        Err(ZipError::Invalid(_))
    ));
    let packed = entry(&storage, &archive, "packed");
    assert_eq!(
        streamed(&storage, &archive, &packed).unwrap(),
        b"0123456789"
    );
}

#[test]
fn a_record_signature_out_of_place_is_invalid() {
    let (mut data, layout) = write(&[Raw::stored("a", b"1")], b"");
    put32(&mut data, layout.central[0], 0x0403_4b50);
    let (_, archive) = open(data);
    assert!(is_invalid(archive));
}

#[test]
fn a_name_running_past_the_end_of_the_file_is_an_error() {
    let (mut data, layout) = write(&[Raw::stored("a", b"1")], b"");
    put16(&mut data, layout.central[0] + 28, 0xffff);
    let (_, archive) = open(data);
    assert!(is_invalid(archive));
}

#[test]
fn local_headers_out_of_place_are_errors_when_read() {
    let (data, layout) = write(&[Raw::stored("a", b"hello")], b"");
    // Beyond the end of the file.
    let mut outside = data.clone();
    put32(&mut outside, layout.central[0] + 42, 0x7fff_0000);
    // Onto the central directory, which is no local header.
    let mut misplaced = data.clone();
    put32(
        &mut misplaced,
        layout.central[0] + 42,
        layout.central[0] as u32,
    );
    // Its signature damaged.
    let mut damaged = data;
    put32(&mut damaged, layout.local[0], 0xdead_beef);
    for data in [outside, misplaced, damaged] {
        let (storage, archive) = open(data);
        let archive = archive.unwrap();
        let e = entry(&storage, &archive, "a");
        assert!(matches!(
            archive.read(&storage, &e, 100),
            Err(ZipError::Invalid(_))
        ));
        assert!(archive.reader(&storage, &e).is_err());
        assert!(archive.stored_range(&storage, &e).is_err());
    }
}

#[test]
fn an_extra_field_only_in_the_local_header_is_skipped() {
    let mut stored = Raw::stored("s.txt", b"stored text");
    stored.local_extra = vec![0x55, 0x54, 5, 0, 1, 2, 3, 4, 5];
    let mut deflated = Raw::deflated("d.txt", &[b'z'; 5000]);
    deflated.local_extra = vec![0xaa; 300];
    let (data, layout) = write(&[stored, deflated], b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    let s = entry(&storage, &archive, "s.txt");
    assert_eq!(archive.read(&storage, &s, 100).unwrap(), b"stored text");
    let range = archive.stored_range(&storage, &s).unwrap().unwrap();
    assert_eq!(range, ((layout.local[0] + 30 + 5 + 9) as u64, 11));
    let d = entry(&storage, &archive, "d.txt");
    assert_eq!(
        archive.read(&storage, &d, 10_000).unwrap(),
        vec![b'z'; 5000]
    );
    assert_eq!(archive.stored_range(&storage, &d).unwrap(), None);
}

#[test]
fn names_are_kept_as_written_without_resolving_paths() {
    // The reader extracts nothing; whoever writes entries to storage must
    // not trust these names. They are found exactly as written.
    let names = [
        "../../etc/passwd",
        "/absolute/path.txt",
        "dir\\windows\\style.txt",
        "C:/drive.txt",
        "a/./b/../c.txt",
        "",
        "trailing/",
    ];
    let entries: Vec<Raw> = names
        .iter()
        .enumerate()
        .map(|(i, name)| Raw::stored(name, format!("entry {i}").as_bytes()))
        .collect();
    let (data, _) = write(&entries, b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    assert_eq!(archive.len(), names.len());
    for (i, name) in names.iter().enumerate() {
        let e = entry(&storage, &archive, name);
        assert_eq!(e.name, *name);
        assert_eq!(
            archive.read(&storage, &e, 100).unwrap(),
            format!("entry {i}").as_bytes()
        );
    }
    // No normalisation: the resolved paths are other names.
    assert_eq!(archive.entry(&storage, "etc/passwd").unwrap(), None);
    assert_eq!(archive.entry(&storage, "a/c.txt").unwrap(), None);
    assert_eq!(
        archive.entry(&storage, "dir/windows/style.txt").unwrap(),
        None
    );
}

#[test]
fn names_in_invalid_utf8_are_listed_lossily_and_still_read() {
    // Old archivers write names in a code page (here CP437 "Größe").
    let mut raw = Raw::stored("x", b"code page");
    raw.name = b"Gr\x94\xe1e.txt".to_vec();
    let (data, _) = write(&[raw, Raw::stored("plain.txt", b"utf-8")], b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    let mut entries: Vec<Entry> = archive.entries(&storage).map(Result::unwrap).collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(entries[0].name, "Gr\u{fffd}\u{fffd}e.txt");
    assert_eq!(
        archive.read(&storage, &entries[0], 100).unwrap(),
        b"code page"
    );
    // Through its record, too.
    let again = archive
        .entry_by_record(&storage, entries[0].record())
        .unwrap();
    assert_eq!(again, entries[0]);
    assert_eq!(entries[1].name, "plain.txt");
}

#[test]
fn duplicate_names_are_both_listed_and_one_is_found() {
    let (data, _) = write(
        &[
            Raw::stored("dup.txt", b"first"),
            Raw::stored("dup.txt", b"second"),
        ],
        b"",
    );
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    let listed: Vec<Entry> = archive.entries(&storage).map(Result::unwrap).collect();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|e| e.name == "dup.txt"));
    let found = entry(&storage, &archive, "dup.txt");
    let content = archive.read(&storage, &found, 100).unwrap();
    assert!(content == b"first" || content == b"second");
}

#[test]
fn many_names_with_colliding_lookups_are_all_found() {
    // Enough entries that names share index neighbourhoods; every one is
    // found by its own name and gives its own content.
    let entries: Vec<Raw> = (0..2000)
        .map(|i| Raw::stored(&format!("OEBPS/{i:04}.xhtml"), format!("{i}").as_bytes()))
        .collect();
    let (data, _) = write(&entries, b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    assert_eq!(archive.len(), 2000);
    for i in (0..2000).step_by(37) {
        let e = entry(&storage, &archive, &format!("OEBPS/{i:04}.xhtml"));
        assert_eq!(
            archive.read(&storage, &e, 10).unwrap(),
            format!("{i}").as_bytes()
        );
    }
    assert_eq!(archive.entry(&storage, "OEBPS/2000.xhtml").unwrap(), None);
}

#[test]
fn encrypted_entries_and_other_methods_are_unsupported_others_still_read() {
    let mut encrypted = Raw::stored("secret.txt", b"cipher");
    encrypted.flags = 1;
    let mut bzip2 = Raw::stored("b.bz2", b"BZh");
    bzip2.method = 12;
    let mut lzma = Raw::stored("l.lzma", b"lzma");
    lzma.method = 14;
    let (data, _) = write(
        &[encrypted, bzip2, Raw::stored("ok.txt", b"fine"), lzma],
        b"",
    );
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    assert_eq!(archive.len(), 4);
    assert_eq!(
        archive.entry(&storage, "secret.txt"),
        Err(ZipError::Unsupported("encrypted entries"))
    );
    for name in ["b.bz2", "l.lzma"] {
        assert_eq!(
            archive.entry(&storage, name),
            Err(ZipError::Unsupported("compression method"))
        );
    }
    let results: Vec<_> = archive.entries(&storage).collect();
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 3);
    let ok = entry(&storage, &archive, "ok.txt");
    assert_eq!(archive.read(&storage, &ok, 10).unwrap(), b"fine");
}

#[test]
fn archive_comments_of_every_length_are_found_past() {
    // Around the steps the end record is searched in, and the longest.
    let lengths = (0..8)
        .chain(990..1030)
        .chain(2030..2060)
        .chain([4096, 30_000, 65_534, 65_535]);
    for len in lengths {
        let comment = vec![b'#'; len];
        let (data, _) = write(&[Raw::deflated("a.txt", b"content")], &comment);
        let (storage, archive) = open(data);
        let archive = archive.unwrap_or_else(|e| panic!("comment of {len}: {e}"));
        let e = entry(&storage, &archive, "a.txt");
        assert_eq!(archive.read(&storage, &e, 100).unwrap(), b"content");
    }
}

#[test]
fn more_trailing_data_than_a_comment_can_hold_is_invalid() {
    let (mut data, _) = write(&[Raw::stored("a", b"1")], b"");
    data.extend(std::iter::repeat_n(0u8, 70_000));
    let (_, archive) = open(data);
    assert!(is_invalid(archive));
}

#[test]
fn crc_is_not_checked_damaged_stored_bytes_come_back_as_they_are() {
    // Documented: the builder leaves the CRC unchecked, and so does the
    // reader. A changed byte of stored data is not detected.
    let (mut data, layout) = write(&[Raw::stored("a.txt", b"original")], b"");
    let at = layout.local[0] + 30 + 5;
    data[at] = b'O';
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    let e = entry(&storage, &archive, "a.txt");
    assert_eq!(archive.read(&storage, &e, 100).unwrap(), b"Original");
}

#[test]
fn compressed_data_cut_short_is_an_error() {
    let text: Vec<u8> = (0..20_000u32).map(|i| (i * 31 % 253) as u8).collect();
    let mut raw = Raw::deflated("cut.bin", &text);
    let full = raw.payload.len();
    raw.payload.truncate(full / 2);
    let (data, _) = write(&[raw], b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();
    let e = entry(&storage, &archive, "cut.bin");
    assert!(matches!(
        archive.read(&storage, &e, 1 << 20),
        Err(ZipError::Invalid(_))
    ));
}

#[test]
fn corrupt_deflate_streams_are_errors() {
    // Block type 3 is reserved; a dynamic block with impossible code
    // lengths; random bytes.
    let payloads: [&[u8]; 3] = [
        &[0x07, 0x00],
        &[0x05, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        &[0xde, 0xad, 0xbe, 0xef, 0x13, 0x37, 0x42, 0x99, 0x01, 0x02],
    ];
    for payload in payloads {
        let raw = Raw {
            method: 8,
            payload: payload.to_vec(),
            size: 100,
            ..Raw::stored("bad", b"")
        };
        let (data, _) = write(&[raw], b"");
        let (storage, archive) = open(data);
        let archive = archive.unwrap();
        let e = entry(&storage, &archive, "bad");
        assert!(
            archive.read(&storage, &e, 1 << 20).is_err(),
            "{payload:02x?}"
        );
    }
}

#[test]
fn a_zip_bomb_is_cut_off_at_the_limit_with_bounded_chunks() {
    // 16 MiB of zeros compress to some 16 KiB; the headers claim 100 bytes.
    let zeros = vec![0u8; 16 << 20];
    let mut bomb = Raw::deflated("bomb", &zeros);
    assert!(bomb.payload.len() < 64 * 1024);
    bomb.size = 100;
    let mut honest = Raw::deflated("honest", &zeros);
    honest.size = zeros.len() as u32;
    let (data, _) = write(&[bomb, honest], b"");
    let (storage, archive) = open(data);
    let archive = archive.unwrap();

    let bomb = entry(&storage, &archive, "bomb");
    assert_eq!(
        archive.read(&storage, &bomb, 1 << 20),
        Err(ZipError::TooLarge)
    );
    let honest = entry(&storage, &archive, "honest");
    assert_eq!(
        archive.read(&storage, &honest, 1 << 20),
        Err(ZipError::TooLarge)
    );

    // A sink that stops early gets no more than it asked for plus a chunk.
    let mut seen = 0usize;
    archive
        .stream(&storage, &honest, &mut |chunk| {
            assert!(chunk.len() <= WINDOW);
            seen += chunk.len();
            seen < 100_000
        })
        .unwrap();
    assert!((100_000..100_000 + WINDOW).contains(&seen), "{seen}");
}

#[test]
fn a_deflated_entry_claiming_a_small_size_gives_its_data_or_an_error() {
    // The window is sized by the claimed size; data that refers further
    // back than that must not come out wrong.
    let data: Vec<u8> = (0..40_000u32)
        .map(|i| {
            if i % 5000 < 2500 {
                (i % 7) as u8
            } else {
                (i * 13 % 251) as u8
            }
        })
        .collect();
    let mut raw = Raw::deflated("liar", &data);
    for claimed in [0u32, 1, 100, 1000, 30_000] {
        raw.size = claimed;
        let (bytes, _) = write(&[raw.clone()], b"");
        let (storage, archive) = open(bytes);
        let archive = archive.unwrap();
        let e = entry(&storage, &archive, "liar");
        if let Ok(out) = streamed(&storage, &archive, &e) {
            assert_eq!(out, data, "claimed {claimed}");
        }
    }
}

/// Builds, damages and reads archives of random entries.
mod properties {
    use super::*;

    fn entries() -> impl Strategy<Value = Vec<(String, Vec<u8>, bool)>> {
        proptest::collection::vec(
            (
                "[a-zA-Z0-9_./ äöü-]{1,40}",
                prop_oneof![
                    proptest::collection::vec(any::<u8>(), 0..3000),
                    // Repetitive data, which deflate shortens.
                    (any::<u8>(), 0usize..70_000).prop_map(|(b, n)| {
                        (0..n).map(|i| b.wrapping_add((i / 100) as u8)).collect()
                    }),
                ],
                any::<bool>(),
            ),
            0..6,
        )
    }

    fn build(entries: &[(String, Vec<u8>, bool)]) -> Vec<u8> {
        // Unique names, as the lookup by name expects.
        let raws: Vec<Raw> = entries
            .iter()
            .enumerate()
            .map(|(i, (name, data, deflate))| {
                let name = format!("{i}/{name}");
                if *deflate {
                    Raw::deflated(&name, data)
                } else {
                    Raw::stored(&name, data)
                }
            })
            .collect();
        write(&raws, b"").0
    }

    /// Opens and reads everything, ignoring errors: it must not panic.
    fn exercise(data: Vec<u8>) {
        let (storage, archive) = open(data);
        let Ok(archive) = archive else {
            return;
        };
        for e in archive.entries(&storage).flatten() {
            let _ = archive.read(&storage, &e, 1 << 20);
            let _ = archive.stored_range(&storage, &e);
            let mut total = 0usize;
            let _ = archive.stream(&storage, &e, &mut |chunk| {
                assert!(chunk.len() <= WINDOW);
                total += chunk.len();
                total < 1 << 20
            });
            let _ = archive.entry(&storage, &e.name);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

        #[test]
        fn archives_read_back_byte_identical(
            files in entries(),
            pieces in proptest::collection::vec(1usize..5000, 1..8),
        ) {
            let (storage, archive) = open(build(&files));
            let archive = archive.unwrap();
            prop_assert_eq!(archive.len(), files.len());
            for (i, (name, data, deflate)) in files.iter().enumerate() {
                let e = entry(&storage, &archive, &format!("{i}/{name}"));
                let method = if *deflate { Method::Deflate } else { Method::Stored };
                prop_assert_eq!(e.method, method);
                prop_assert_eq!(e.size as usize, data.len());
                prop_assert_eq!(&archive.read(&storage, &e, 1 << 20).unwrap(), data);
                prop_assert_eq!(&streamed(&storage, &archive, &e).unwrap(), data);
                // A piece at a time, in sizes that do not fit the chunks.
                let mut reader = archive.reader(&storage, &e).unwrap();
                let mut out = Vec::new();
                for &size in pieces.iter().cycle() {
                    let mut piece = vec![0u8; size];
                    let n = reader.read(&storage, &mut piece).unwrap();
                    if n == 0 {
                        break;
                    }
                    out.extend_from_slice(&piece[..n]);
                }
                prop_assert_eq!(&out, data);
                if !deflate {
                    let (offset, len) = archive.stored_range(&storage, &e).unwrap().unwrap();
                    let start = offset as usize;
                    prop_assert_eq!(&storage.0[start..start + len as usize], &data[..]);
                }
            }
        }

        #[test]
        fn arbitrary_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..600)) {
            exercise(data);
        }

        #[test]
        fn arbitrary_bytes_behind_a_valid_end_record_never_panic(
            body in proptest::collection::vec(any::<u8>(), 0..600),
            count in any::<u16>(),
            cd_size in any::<u32>(),
            cd_offset in any::<u32>(),
        ) {
            // The end record points into random bytes.
            let mut data = body;
            data.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
            data.extend_from_slice(&[0; 4]);
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&(cd_size % 700).to_le_bytes());
            data.extend_from_slice(&(cd_offset % 700).to_le_bytes());
            data.extend_from_slice(&0u16.to_le_bytes());
            exercise(data);
        }

        #[test]
        fn damaged_archives_never_panic(
            files in entries(),
            damage in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..12),
        ) {
            let mut data = build(&files);
            for (at, value) in damage {
                let at = at.index(data.len());
                data[at] = value;
            }
            exercise(data);
        }
    }
}
