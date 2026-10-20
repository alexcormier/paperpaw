//! Reading definitions from `.dict` or `.dict.dz` files.
//!
//! A `.dict.dz` is a dictzip file: gzip whose deflate stream is flushed
//! every `chunk_len` bytes, with the compressed size of each chunk in the
//! gzip header's `RA` field. One chunk can therefore be inflated on its
//! own; this is done as a stream through a 32 KiB window, keeping only the
//! bytes asked for.
//!
//! Offsets and sizes come from the `.idx` file and are not trusted: only
//! what lies within the definitions is read, and every buffer is
//! allocated fallibly.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{DecompressorOxide, decompress, inflate_flags};
use paperpaw_platform::Storage;

use crate::{Error, malformed, zeroed};

/// The deflate history: the largest window needed.
const WINDOW: usize = 32 * 1024;
/// Compressed bytes read at a time.
const INPUT: usize = 4096;

#[derive(Debug)]
pub(crate) enum DictFile {
    Plain { path: String, len: u64 },
    Zip(Dictzip),
}

#[derive(Debug)]
pub(crate) struct Dictzip {
    path: String,
    chunk_len: u64,
    /// The uncompressed size by the chunk table (the last chunk may be
    /// shorter).
    len: u64,
    /// Where the deflate data starts.
    data: u64,
    /// Start of each chunk relative to `data`, and the end of the last.
    chunks: Vec<u32>,
}

impl DictFile {
    pub fn open(storage: &dyn Storage, base: &str) -> Result<DictFile, Error> {
        let plain = format!("{base}.dict");
        if storage.exists(&plain) {
            let len = storage.size(&plain)?;
            return Ok(DictFile::Plain { path: plain, len });
        }
        let zip = format!("{base}.dict.dz");
        if !storage.exists(&zip) {
            return Err(malformed("no .dict or .dict.dz file"));
        }
        Ok(DictFile::Zip(Dictzip::open(storage, zip)?))
    }

    /// `size` bytes of the uncompressed definitions from `offset`, fewer
    /// where the definitions end.
    pub fn read(&self, storage: &dyn Storage, offset: u64, size: usize) -> Result<Vec<u8>, Error> {
        match self {
            DictFile::Plain { path, len } => {
                let size = within(offset, size, *len);
                let mut data = zeroed(size)?;
                let mut done = 0;
                while done < size {
                    let n = storage.read_at(path, offset + done as u64, &mut data[done..])?;
                    if n == 0 {
                        break;
                    }
                    done += n;
                }
                data.truncate(done);
                Ok(data)
            }
            DictFile::Zip(zip) => zip.read(storage, offset, size),
        }
    }
}

/// How many of `size` bytes from `offset` lie within the first `len`.
fn within(offset: u64, size: usize, len: u64) -> usize {
    usize::try_from(len.saturating_sub(offset)).map_or(size, |left| left.min(size))
}

fn u16_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from(u16::from_le_bytes([
        *data.get(at)?,
        *data.get(at + 1)?,
    ])))
}

impl Dictzip {
    fn open(storage: &dyn Storage, path: String) -> Result<Dictzip, Error> {
        let mut head = [0u8; 12];
        if storage.read_at(&path, 0, &mut head)? != 12 || head[..3] != [0x1f, 0x8b, 8] {
            return Err(malformed("not a gzip file"));
        }
        let flags = head[3];
        if flags & 4 == 0 {
            return Err(malformed("gzip without dictzip chunks"));
        }
        let extra_len = u16_at(&head, 10).unwrap_or(0) as usize;
        let mut extra = zeroed(extra_len)?;
        if storage.read_at(&path, 12, &mut extra)? != extra_len {
            return Err(malformed("gzip header cut short"));
        }
        // The `RA` subfield: version, chunk length, chunk count, sizes.
        let mut at = 0;
        let mut table = None;
        while at + 4 <= extra.len() {
            let len = u16_at(&extra, at + 2).unwrap_or(0) as usize;
            if &extra[at..at + 2] == b"RA" {
                table = extra.get(at + 4..at + 4 + len);
                break;
            }
            at += 4 + len;
        }
        let table = table.ok_or_else(|| malformed("gzip without dictzip chunks"))?;
        let chunk_len = u16_at(table, 2).unwrap_or(0);
        let count = u16_at(table, 4).unwrap_or(0) as usize;
        let sizes = table
            .get(6..6 + 2 * count)
            .filter(|_| chunk_len > 0 && count > 0)
            .ok_or_else(|| malformed("dictzip table"))?;
        let mut chunks = Vec::new();
        chunks
            .try_reserve_exact(count + 1)
            .map_err(|_| Error::OutOfMemory)?;
        // At most 65535 sizes below 65536 each: the sum fits in a u32.
        let mut start = 0;
        chunks.push(start);
        for &size in sizes.as_chunks::<2>().0 {
            start += u32::from(u16::from_le_bytes(size));
            chunks.push(start);
        }
        // The deflate data follows the optional name, comment and CRC.
        let mut data = 12 + extra_len as u64;
        for flag in [8u8, 16] {
            if flags & flag != 0 {
                data = skip_string(storage, &path, data)?;
            }
        }
        if flags & 2 != 0 {
            data += 2;
        }
        Ok(Dictzip {
            path,
            chunk_len,
            len: count as u64 * chunk_len,
            data,
            chunks,
        })
    }

    fn read(&self, storage: &dyn Storage, offset: u64, size: usize) -> Result<Vec<u8>, Error> {
        let size = within(offset, size, self.len);
        let end = offset + size as u64;
        let mut out = Vec::new();
        out.try_reserve_exact(size)
            .map_err(|_| Error::OutOfMemory)?;
        if size == 0 {
            return Ok(out);
        }
        let mut inflater = Inflater::new()?;
        // Both are below the chunk count, as `end` is within `len`.
        let first = (offset / self.chunk_len) as usize;
        let last = ((end - 1) / self.chunk_len) as usize;
        for chunk in first..=last {
            let base = chunk as u64 * self.chunk_len;
            // What a chunk inflates to beyond `chunk_len` bytes is not
            // used: that part of the definition comes from the next one.
            let stop = (base + self.chunk_len).min(end);
            self.inflate(storage, chunk, &mut inflater, &mut |at, bytes| {
                // `bytes` are the chunk's uncompressed bytes from `at`.
                let from = base + at;
                let to = from + bytes.len() as u64;
                let (lo, hi) = (from.max(offset), to.min(stop));
                if lo < hi {
                    out.extend_from_slice(&bytes[(lo - from) as usize..(hi - from) as usize]);
                }
                to < stop
            })?;
        }
        Ok(out)
    }

    /// Inflates chunk `chunk`, passing its bytes to `sink` with their
    /// position in the chunk; the sink returns `false` to stop.
    fn inflate(
        &self,
        storage: &dyn Storage,
        chunk: usize,
        inflater: &mut Inflater,
        sink: &mut dyn FnMut(u64, &[u8]) -> bool,
    ) -> Result<(), Error> {
        let (Some(&from), Some(&to)) = (self.chunks.get(chunk), self.chunks.get(chunk + 1)) else {
            return Err(malformed("dictzip table"));
        };
        let mut offset = self.data + u64::from(from);
        let mut remaining = (to - from) as usize;
        let Inflater {
            decoder,
            window,
            input,
        } = inflater;
        let decoder = &mut decoder[0];
        decoder.init();
        let (mut start, mut stop, mut position, mut produced) = (0usize, 0usize, 0usize, 0u64);
        loop {
            if start == stop && remaining > 0 {
                let n = remaining.min(INPUT);
                let read = storage.read_at(&self.path, offset, &mut input[..n])?;
                if read == 0 {
                    return Err(malformed("dictzip data cut short"));
                }
                offset += read as u64;
                remaining -= read;
                start = 0;
                stop = read;
            }
            // Chunks end with a flush, not with the end of the stream, so
            // the decoder is always told that more may follow.
            let flags = inflate_flags::TINFL_FLAG_HAS_MORE_INPUT;
            let (status, consumed, written) =
                decompress(decoder, &input[start..stop], window, position, flags);
            start += consumed;
            if written > 0 {
                if !sink(produced, &window[position..position + written]) {
                    return Ok(());
                }
                produced += written as u64;
            }
            position = (position + written) & (WINDOW - 1);
            match status {
                TINFLStatus::Done => return Ok(()),
                // The end of the chunk.
                TINFLStatus::NeedsMoreInput if remaining == 0 && start == stop => return Ok(()),
                // Every round consumes or writes something: a decoder doing
                // neither would never finish.
                TINFLStatus::HasMoreOutput | TINFLStatus::NeedsMoreInput
                    if consumed > 0 || written > 0 => {}
                _ => return Err(malformed("corrupt dictzip data")),
            }
        }
    }
}

/// What inflating a chunk needs, about 47 KB, made once per read.
struct Inflater {
    /// One decoder, made in place: built as a value, its 10 KB would go
    /// over the stack first.
    decoder: Vec<DecompressorOxide>,
    window: Vec<u8>,
    input: Vec<u8>,
}

impl Inflater {
    fn new() -> Result<Inflater, Error> {
        let mut decoder = Vec::new();
        decoder
            .try_reserve_exact(1)
            .map_err(|_| Error::OutOfMemory)?;
        decoder.resize_with(1, DecompressorOxide::default);
        Ok(Inflater {
            decoder,
            window: zeroed(WINDOW)?,
            input: zeroed(INPUT)?,
        })
    }
}

/// The offset after the NUL-terminated string at `at`.
fn skip_string(storage: &dyn Storage, path: &str, mut at: u64) -> Result<u64, Error> {
    let mut buf = [0u8; 256];
    loop {
        let n = storage.read_at(path, at, &mut buf)?;
        if n == 0 {
            return Err(malformed("gzip header cut short"));
        }
        if let Some(zero) = buf[..n].iter().position(|&b| b == 0) {
            return Ok(at + zero as u64 + 1);
        }
        at += n as u64;
    }
}
