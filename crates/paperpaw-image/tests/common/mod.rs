//! Shared by the image tests: builders of small images (baseline and
//! progressive JPEG, PNG of every colour type and depth, GIF), written
//! here from the formats' specifications so that tests can make any
//! size, layout or damage they need, and a sink that checks what the
//! decoders hand over.

// Each test file uses a part of it.
#![allow(dead_code)]

use paperpaw_image::{Fit, Format, ImageError, ImageSink, Source, decode};

// ---------------------------------------------------------------------
// Sinks

/// A sink that checks the decoders' side of the contract: `start` once,
/// before any row, with at least a pixel each way; rows as wide as said;
/// never more rows than the height; nothing after it stopped them.
#[derive(Debug, Default)]
pub struct Checked {
    /// Asks for colour rows.
    pub color: bool,
    pub width: u32,
    pub height: u32,
    pub started: bool,
    pub rows: u32,
    /// Gray, or RGB for a colour sink (gray rows stored as gray RGB).
    pub pixels: Vec<u8>,
    /// Stops decoding at this row.
    pub stop_at: Option<u32>,
    /// Rows offered after the sink stopped decoding.
    pub after_stop: u32,
}

impl Checked {
    pub fn gray() -> Self {
        Self::default()
    }

    pub fn colour() -> Self {
        Self {
            color: true,
            ..Self::default()
        }
    }

    fn take_row(&mut self, len: usize, channels: usize) -> Result<(), ImageError> {
        assert!(self.started, "a row before start");
        assert_eq!(len, self.width as usize * channels, "row length");
        if self.stop_at.is_some_and(|at| self.rows >= at) {
            self.after_stop += 1;
            return Err(ImageError::Stopped);
        }
        assert!(
            self.rows < self.height,
            "more rows than the height {}",
            self.height
        );
        self.rows += 1;
        Ok(())
    }

    /// Whether every row came.
    pub fn complete(&self) -> bool {
        self.started && self.rows == self.height
    }
}

impl ImageSink for Checked {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        assert!(!self.started, "start twice");
        assert!(width > 0 && height > 0, "an empty size {width}x{height}");
        self.started = true;
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.take_row(gray.len(), 1)?;
        if self.color {
            for &g in gray {
                self.pixels.extend_from_slice(&[g, g, g]);
            }
        } else {
            self.pixels.extend_from_slice(gray);
        }
        Ok(())
    }

    fn wants_color(&self) -> bool {
        self.color
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        assert!(self.color, "colour rows for a gray sink");
        self.take_row(rgb.len(), 3)?;
        self.pixels.extend_from_slice(rgb);
        Ok(())
    }
}

/// Decodes `data` as `format` into a checked sink, gray or colour.
pub fn run(
    format: Format,
    data: &[u8],
    fit: Fit,
    color: bool,
) -> (Result<(), ImageError>, Checked) {
    let mut sink = if color {
        Checked::colour()
    } else {
        Checked::gray()
    };
    let mut source = data;
    let result = decode(format, &mut source, fit, &mut sink);
    if result.is_ok() {
        assert!(
            sink.complete(),
            "decoded, but {} of {} rows",
            sink.rows,
            sink.height
        );
    }
    (result, sink)
}

/// Decodes `data` (its format from its first bytes), expecting success.
pub fn decoded(data: &[u8], fit: Fit, color: bool) -> Checked {
    let format = Format::detect(data).expect("a known format");
    let (result, sink) = run(format, data, fit, color);
    result.expect("decodes");
    sink
}

/// A source that hands over at most `step` bytes a read.
pub struct Dribble<'a> {
    pub data: &'a [u8],
    pub step: usize,
}

impl Source for Dribble<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        let n = buf.len().min(self.step).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

/// A source that fails after `ok` bytes.
pub struct Failing<'a> {
    pub data: &'a [u8],
    pub ok: usize,
}

impl Source for Failing<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ImageError> {
        if self.ok == 0 {
            return Err(ImageError::Io);
        }
        let n = buf.len().min(self.ok).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        self.ok -= n;
        Ok(n)
    }
}

/// Property tests of `cases` cases (or as many as `PROPTEST_CASES` says); failures are turned into named
/// tests, not kept in files.
pub fn config(cases: u32) -> proptest::test_runner::Config {
    proptest::test_runner::Config {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(cases),
        failure_persistence: None,
        ..proptest::test_runner::Config::default()
    }
}

/// A small generator of numbers, for image contents.
pub struct Lcg(pub u64);

impl Lcg {
    pub fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    /// A number in `low..=high`.
    pub fn range(&mut self, low: i32, high: i32) -> i32 {
        low + (self.next() % (high - low + 1) as u32) as i32
    }
}

// ---------------------------------------------------------------------
// JPEG

/// Bits of entropy-coded data, most significant first, with 0xFF
/// stuffed.
#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    count: u32,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        for i in (0..bits).rev() {
            self.acc = self.acc << 1 | (value >> i & 1);
            self.count += 1;
            if self.count == 8 {
                let byte = self.acc as u8;
                self.out.push(byte);
                if byte == 0xff {
                    self.out.push(0);
                }
                self.acc = 0;
                self.count = 0;
            }
        }
    }

    /// Pads the last byte with ones.
    fn align(&mut self) {
        while self.count != 0 {
            self.put(1, 1);
        }
    }
}

/// A Huffman table as a `DHT` segment gives it.
#[derive(Clone)]
pub struct HuffmanTable {
    pub counts: [u8; 16],
    pub values: Vec<u8>,
}

impl HuffmanTable {
    /// Every value a code of 4 bits.
    pub fn four_bits(values: &[u8]) -> Self {
        let mut counts = [0u8; 16];
        counts[3] = values.len() as u8;
        Self {
            counts,
            values: values.to_vec(),
        }
    }

    /// The canonical code (and its length) of `value`.
    fn code(&self, value: u8) -> (u32, u32) {
        let (mut code, mut k) = (0u32, 0usize);
        for len in 1..=16u32 {
            for _ in 0..self.counts[len as usize - 1] {
                if self.values[k] == value {
                    return (code, len);
                }
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        panic!("no code for {value:#x}");
    }

    /// The segment's payload for table `class_id` (class << 4 | id).
    pub fn payload(&self, class_id: u8) -> Vec<u8> {
        let mut out = vec![class_id];
        out.extend_from_slice(&self.counts);
        out.extend_from_slice(&self.values);
        out
    }
}

/// DC categories 0 to 11.
pub fn dc_table() -> HuffmanTable {
    HuffmanTable::four_bits(&(0..12).collect::<Vec<u8>>())
}

/// End of block, and runs of up to four zeros before values of one or
/// two bits.
pub fn ac_table() -> HuffmanTable {
    HuffmanTable::four_bits(&[
        0x00, 0x01, 0x02, 0x11, 0x12, 0x21, 0x22, 0x31, 0x32, 0x41, 0x42,
    ])
}

/// The size category of `v` and its bits.
fn magnitude(v: i32) -> (u32, u32) {
    if v == 0 {
        return (0, 0);
    }
    let size = 32 - v.unsigned_abs().leading_zeros();
    let bits = if v < 0 { v + (1 << size) - 1 } else { v };
    (size, bits as u32)
}

/// Coefficients of a block kept by the builder: the DC and the first
/// five AC coefficients in zig-zag order (quantized).
pub type Block = [i32; 6];

/// A JPEG to build, baseline or progressive, of one (gray) or three
/// components.
#[derive(Clone, Debug)]
pub struct Jpeg {
    pub width: u16,
    pub height: u16,
    /// Each component's sampling (h, v).
    pub sampling: Vec<(u8, u8)>,
    /// MCUs between restart markers (0: none).
    pub restart: u16,
    /// Each component's blocks, row by row over whole MCUs.
    pub blocks: Vec<Vec<Block>>,
}

/// Segment `marker` with `payload`.
fn segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    out.extend_from_slice(&[0xff, marker]);
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
}

impl Jpeg {
    /// Blocks of random content: DCs within `dc`, AC values within
    /// -2..=2 (only even ones if `even`, as progressive refinement of a
    /// bit needs here).
    pub fn new(width: u16, height: u16, sampling: &[(u8, u8)], seed: u64, even: bool) -> Self {
        let mut jpeg = Self {
            width,
            height,
            sampling: sampling.to_vec(),
            restart: 0,
            blocks: Vec::new(),
        };
        if jpeg.sampling.len() == 1 {
            jpeg.sampling[0] = (1, 1);
        }
        let mut rng = Lcg(seed);
        for c in 0..jpeg.sampling.len() {
            let (stride, rows) = jpeg.grid(c);
            let blocks = (0..stride * rows)
                .map(|_| {
                    let mut block = [0i32; 6];
                    block[0] = rng.range(-120, 120);
                    for ac in &mut block[1..] {
                        *ac = match rng.range(0, 5) {
                            0 => -2,
                            1 => 2,
                            2 if !even => -1,
                            3 if !even => 1,
                            _ => 0,
                        };
                    }
                    block
                })
                .collect();
            jpeg.blocks.push(blocks);
        }
        jpeg
    }

    /// All blocks of one level `dc` and no AC.
    pub fn flat(width: u16, height: u16, sampling: &[(u8, u8)], dc: [i32; 3]) -> Self {
        let mut jpeg = Self::new(width, height, sampling, 0, true);
        for (c, blocks) in jpeg.blocks.iter_mut().enumerate() {
            for block in blocks {
                *block = [dc[c], 0, 0, 0, 0, 0];
            }
        }
        jpeg
    }

    fn max(&self) -> (usize, usize) {
        (
            self.sampling
                .iter()
                .map(|s| usize::from(s.0))
                .max()
                .unwrap(),
            self.sampling
                .iter()
                .map(|s| usize::from(s.1))
                .max()
                .unwrap(),
        )
    }

    fn mcus(&self) -> (usize, usize) {
        let (h, v) = self.max();
        (
            usize::from(self.width).div_ceil(8 * h),
            usize::from(self.height).div_ceil(8 * v),
        )
    }

    /// Blocks a row and rows of blocks of component `c`, whole MCUs.
    fn grid(&self, c: usize) -> (usize, usize) {
        let (mx, my) = self.mcus();
        let (h, v) = self.sampling[c];
        (mx * usize::from(h), my * usize::from(v))
    }

    fn headers(&self, sof: u8) -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        // A comment, to be skipped.
        segment(&mut out, 0xfe, b"made by the tests");
        let mut dqt = vec![0x00];
        dqt.extend((0..64).map(|k| if k == 0 { 8u8 } else { 1 + k % 3 }));
        segment(&mut out, 0xdb, &dqt);
        let mut frame = vec![8];
        frame.extend_from_slice(&self.height.to_be_bytes());
        frame.extend_from_slice(&self.width.to_be_bytes());
        frame.push(self.sampling.len() as u8);
        for (i, &(h, v)) in self.sampling.iter().enumerate() {
            frame.extend_from_slice(&[i as u8 + 1, h << 4 | v, 0]);
        }
        segment(&mut out, sof, &frame);
        let mut dht = dc_table().payload(0x00);
        dht.extend(ac_table().payload(0x10));
        segment(&mut out, 0xc4, &dht);
        if self.restart > 0 {
            segment(&mut out, 0xdd, &self.restart.to_be_bytes());
        }
        out
    }

    /// The scan header for `members` with spectral range and
    /// approximation.
    fn scan_header(out: &mut Vec<u8>, members: &[usize], ss: u8, se: u8, ah_al: u8) {
        let mut sos = vec![members.len() as u8];
        for &c in members {
            sos.extend_from_slice(&[c as u8 + 1, 0x00]);
        }
        sos.extend_from_slice(&[ss, se, ah_al]);
        segment(out, 0xda, &sos);
    }

    /// Restart marker before unit `index` (an MCU or, in progressive scans
    /// of one component, a block), where due.
    fn restart_before(&self, bits: &mut BitWriter, index: usize, predictions: &mut [i32]) {
        if self.restart > 0 && index > 0 && index.is_multiple_of(usize::from(self.restart)) {
            bits.align();
            let n = (index / usize::from(self.restart) - 1) % 8;
            bits.out.extend_from_slice(&[0xff, 0xd0 + n as u8]);
            predictions.fill(0);
        }
    }

    /// Each MCU's blocks in order: (component, block index).
    fn interleaved(&self) -> Vec<Vec<(usize, usize)>> {
        let (mcus_x, mcus_y) = self.mcus();
        let mut order = Vec::new();
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                let mut mcu = Vec::new();
                for (c, &(h, v)) in self.sampling.iter().enumerate() {
                    let (h, v) = (usize::from(h), usize::from(v));
                    let stride = self.grid(c).0;
                    for by in 0..v {
                        for bx in 0..h {
                            mcu.push((c, (my * v + by) * stride + mx * h + bx));
                        }
                    }
                }
                order.push(mcu);
            }
        }
        order
    }

    fn dc(bits: &mut BitWriter, diff: i32) {
        let dc = dc_table();
        let (size, value) = magnitude(diff);
        let (code, len) = dc.code(size as u8);
        bits.put(code, len);
        bits.put(value, size);
    }

    /// AC values in zig-zag order from 1, then the end of the block; in a
    /// band that ends before the block (`band`), only if the band's last
    /// value is zero.
    fn ac(bits: &mut BitWriter, values: &[i32], band: bool) {
        let ac = ac_table();
        let mut run = 0u8;
        for &v in values {
            if v == 0 {
                run += 1;
                continue;
            }
            let (size, value) = magnitude(v);
            let (code, len) = ac.code(run << 4 | size as u8);
            bits.put(code, len);
            bits.put(value, size);
            run = 0;
        }
        if !band || run > 0 {
            let (code, len) = ac.code(0x00);
            bits.put(code, len);
        }
    }

    /// As a baseline JPEG.
    pub fn baseline(&self) -> Vec<u8> {
        let mut out = self.headers(0xc0);
        let members: Vec<usize> = (0..self.sampling.len()).collect();
        Self::scan_header(&mut out, &members, 0, 63, 0);
        let mut bits = BitWriter::default();
        let mut predictions = vec![0i32; self.sampling.len()];
        for (index, mcu) in self.interleaved().iter().enumerate() {
            self.restart_before(&mut bits, index, &mut predictions);
            for &(c, b) in mcu {
                let block = &self.blocks[c][b];
                Self::dc(&mut bits, block[0] - predictions[c]);
                predictions[c] = block[0];
                Self::ac(&mut bits, &block[1..], false);
            }
        }
        bits.align();
        out.extend_from_slice(&bits.out);
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }

    /// The blocks of component `c` a scan of it alone covers, in order.
    fn own_blocks(&self, c: usize) -> Vec<usize> {
        let (h_max, v_max) = self.max();
        let (h, v) = self.sampling[c];
        let width = (usize::from(self.width) * usize::from(h)).div_ceil(h_max);
        let height = (usize::from(self.height) * usize::from(v)).div_ceil(v_max);
        let stride = self.grid(c).0;
        let mut order = Vec::new();
        for by in 0..height.div_ceil(8) {
            for bx in 0..width.div_ceil(8) {
                order.push(by * stride + bx);
            }
        }
        order
    }

    /// As a progressive JPEG: the DC's high bits (all components
    /// together), the AC's high bits (each component alone), then the
    /// last bit of each. Needs even AC values ([`Jpeg::new`]).
    pub fn progressive(&self) -> Vec<u8> {
        self.progressive_scans(true)
    }

    /// [`Jpeg::progressive`], with the refining scans or without them.
    pub fn progressive_scans(&self, refine: bool) -> Vec<u8> {
        let mut out = self.headers(0xc2);
        let components = self.sampling.len();
        let members: Vec<usize> = (0..components).collect();
        // DC, all bits but the last.
        Self::scan_header(&mut out, &members, 0, 0, 0x01);
        let mut bits = BitWriter::default();
        let mut predictions = vec![0i32; components];
        for (index, mcu) in self.interleaved().iter().enumerate() {
            self.restart_before(&mut bits, index, &mut predictions);
            for &(c, b) in mcu {
                let dc = self.blocks[c][b][0] >> 1;
                Self::dc(&mut bits, dc - predictions[c]);
                predictions[c] = dc;
            }
        }
        bits.align();
        out.extend_from_slice(&bits.out);
        // AC 1 to 5 of each component, all bits but the last.
        for c in 0..components {
            Self::scan_header(&mut out, &[c], 1, 5, 0x01);
            let mut bits = BitWriter::default();
            for (index, &b) in self.own_blocks(c).iter().enumerate() {
                self.restart_before(&mut bits, index, &mut [0]);
                let values: Vec<i32> = self.blocks[c][b][1..].iter().map(|v| v >> 1).collect();
                Self::ac(&mut bits, &values, true);
            }
            bits.align();
            out.extend_from_slice(&bits.out);
        }
        if refine {
            // The DC's last bit.
            Self::scan_header(&mut out, &members, 0, 0, 0x10);
            let mut bits = BitWriter::default();
            for (index, mcu) in self.interleaved().iter().enumerate() {
                self.restart_before(&mut bits, index, &mut [0]);
                for &(c, b) in mcu {
                    bits.put((self.blocks[c][b][0] & 1) as u32, 1);
                }
            }
            bits.align();
            out.extend_from_slice(&bits.out);
            // The AC's last bit: none new, a correction bit (0) for each
            // coefficient already set.
            for c in 0..components {
                Self::scan_header(&mut out, &[c], 1, 5, 0x10);
                let mut bits = BitWriter::default();
                let (code, len) = ac_table().code(0x00);
                for (index, &b) in self.own_blocks(c).iter().enumerate() {
                    self.restart_before(&mut bits, index, &mut [0]);
                    bits.put(code, len);
                    for &v in &self.blocks[c][b][1..] {
                        if v != 0 {
                            bits.put(0, 1);
                        }
                    }
                }
                bits.align();
                out.extend_from_slice(&bits.out);
            }
        }
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }
}

/// Where the first marker `marker` is in `data`, from `from`.
pub fn find_marker(data: &[u8], marker: u8, from: usize) -> Option<usize> {
    (from..data.len().saturating_sub(1)).find(|&i| data[i] == 0xff && data[i + 1] == marker)
}

// ---------------------------------------------------------------------
// PNG

/// The CRC-32 of PNG chunks.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Appends a chunk with its CRC.
pub fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    out.extend_from_slice(&checked);
    out.extend_from_slice(&crc32(&checked).to_be_bytes());
}

pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Samples a pixel in PNG colour type `color`.
pub fn png_channels(color: u8) -> usize {
    match color {
        0 | 3 => 1,
        4 => 2,
        2 => 3,
        _ => 4,
    }
}

/// A PNG to build.
#[derive(Clone, Debug)]
pub struct Png {
    pub width: u32,
    pub height: u32,
    pub color: u8,
    pub depth: u8,
    pub interlace: u8,
    pub palette: Vec<[u8; 3]>,
    /// Alpha of palette entries (a `tRNS` chunk), for colour type 3.
    pub trns: Option<Vec<u8>>,
    /// Each row's samples, `channels` a pixel, each below 2^depth.
    pub samples: Vec<Vec<u16>>,
    /// The filter of each row (0 to 4), round and round.
    pub filters: Vec<u8>,
    /// Bytes of image data an IDAT chunk at most.
    pub idat: usize,
}

impl Png {
    /// Random samples.
    pub fn new(width: u32, height: u32, color: u8, depth: u8, seed: u64) -> Self {
        let mut rng = Lcg(seed);
        let channels = png_channels(color);
        let max = (1u32 << depth) - 1;
        let palette: Vec<[u8; 3]> = (0..=max.min(255))
            .map(|_| [rng.next() as u8, rng.next() as u8, rng.next() as u8])
            .collect();
        let samples = (0..height)
            .map(|_| {
                (0..width as usize * channels)
                    .map(|_| (rng.next() % (max + 1)) as u16)
                    .collect()
            })
            .collect();
        Self {
            width,
            height,
            color,
            depth,
            interlace: 0,
            palette: if color == 3 { palette } else { Vec::new() },
            trns: None,
            samples,
            filters: vec![0],
            idat: 1 << 16,
        }
    }

    /// The bytes of row `y`, packed.
    fn packed(&self, y: usize) -> Vec<u8> {
        let row = &self.samples[y];
        match self.depth {
            8 => row.iter().map(|&s| s as u8).collect(),
            16 => row.iter().flat_map(|&s| s.to_be_bytes()).collect(),
            bits => {
                let mut out = vec![0u8; (row.len() * usize::from(bits)).div_ceil(8)];
                for (i, &s) in row.iter().enumerate() {
                    let bit = i * usize::from(bits);
                    out[bit / 8] |= (s as u8) << (8 - usize::from(bits) - bit % 8);
                }
                out
            }
        }
    }

    /// The image data before compression: each row filtered.
    pub fn raw(&self) -> Vec<u8> {
        let bpp = (png_channels(self.color) * usize::from(self.depth))
            .div_ceil(8)
            .max(1);
        let mut out = Vec::new();
        let mut previous = vec![0u8; self.packed(0).len()];
        for y in 0..self.height as usize {
            let row = self.packed(y);
            let filter = self.filters[y % self.filters.len()];
            out.push(filter);
            for i in 0..row.len() {
                let a = if i >= bpp { row[i - bpp] } else { 0 };
                let b = previous[i];
                let c = if i >= bpp { previous[i - bpp] } else { 0 };
                let predicted = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => {
                        let p = i16::from(a) + i16::from(b) - i16::from(c);
                        let (pa, pb, pc) = (
                            (p - i16::from(a)).abs(),
                            (p - i16::from(b)).abs(),
                            (p - i16::from(c)).abs(),
                        );
                        if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        }
                    }
                };
                out.push(row[i].wrapping_sub(predicted));
            }
            previous = row;
        }
        out
    }

    pub fn ihdr(&self) -> Vec<u8> {
        let mut ihdr = self.width.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[self.depth, self.color, 0, 0, self.interlace]);
        ihdr
    }

    /// The file, with `zlib` as its compressed image data.
    pub fn with_zlib(&self, zlib: &[u8]) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        png_chunk(&mut out, b"IHDR", &self.ihdr());
        png_chunk(&mut out, b"tEXt", b"Comment\0made by the tests");
        if !self.palette.is_empty() {
            let plte: Vec<u8> = self.palette.iter().flatten().copied().collect();
            png_chunk(&mut out, b"PLTE", &plte);
        }
        if let Some(trns) = &self.trns {
            png_chunk(&mut out, b"tRNS", trns);
        }
        for part in zlib.chunks(self.idat.max(1)) {
            png_chunk(&mut out, b"IDAT", part);
        }
        png_chunk(&mut out, b"IEND", &[]);
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        self.with_zlib(&miniz_oxide::deflate::compress_to_vec_zlib(&self.raw(), 6))
    }

    /// The gray each pixel should come out as: transparency over white;
    /// the decoder may round a level differently.
    pub fn expected_gray(&self) -> Vec<u8> {
        let channels = png_channels(self.color);
        let max = (1u32 << self.depth) - 1;
        let to8 = |s: u16| -> u32 {
            match self.depth {
                16 => u32::from(s >> 8),
                _ => u32::from(s) * 255 / max,
            }
        };
        let over = |v: u32, a: u32| (v * a + 255 * (255 - a)) / 255;
        let mut out = Vec::new();
        for row in &self.samples {
            for p in row.chunks(channels) {
                let gray = match self.color {
                    0 => to8(p[0]),
                    2 => u32::from(paperpaw_image::luma(to8(p[0]), to8(p[1]), to8(p[2]))),
                    3 => {
                        let [r, g, b] = self
                            .palette
                            .get(usize::from(p[0]))
                            .copied()
                            .unwrap_or([0; 3]);
                        let a = self
                            .trns
                            .as_ref()
                            .and_then(|t| t.get(usize::from(p[0])))
                            .map_or(255, |&a| u32::from(a));
                        let y = paperpaw_image::luma(r.into(), g.into(), b.into());
                        over(u32::from(y), a)
                    }
                    4 => over(to8(p[0]), to8(p[1])),
                    _ => over(
                        u32::from(paperpaw_image::luma(to8(p[0]), to8(p[1]), to8(p[2]))),
                        to8(p[3]),
                    ),
                };
                out.push(gray as u8);
            }
        }
        out
    }
}

/// The colour types and depths PNG allows.
pub const PNG_KINDS: [(u8, u8); 15] = [
    (0, 1),
    (0, 2),
    (0, 4),
    (0, 8),
    (0, 16),
    (2, 8),
    (2, 16),
    (3, 1),
    (3, 2),
    (3, 4),
    (3, 8),
    (4, 8),
    (4, 16),
    (6, 8),
    (6, 16),
];

// ---------------------------------------------------------------------
// GIF

/// A GIF to build: a screen and its first picture.
#[derive(Clone, Debug)]
pub struct Gif {
    pub width: u16,
    pub height: u16,
    pub global: Option<Vec<[u8; 3]>>,
    pub transparent: Option<u8>,
    pub left: u16,
    pub top: u16,
    pub frame_width: u16,
    pub frame_height: u16,
    pub interlaced: bool,
    pub local: Option<Vec<[u8; 3]>>,
    pub min_code_size: u8,
    /// The picture's indices, row by row (in display order).
    pub indices: Vec<u8>,
    /// A clear code after so many indices (`None`: the table fills up
    /// and stays full).
    pub clear_every: Option<usize>,
    pub trailer: bool,
}

/// Whether `data` is a GIF whose first picture is interlaced and has no
/// width, which panics the decoder (the bug of
/// `gif::interlaced_picture_of_no_width_is_an_error_not_a_panic`):
/// property tests leave such files out until it is mended.
pub fn gif_interlaced_without_width(data: &[u8]) -> bool {
    let byte = |at: usize| data.get(at).copied();
    if !data.starts_with(b"GIF8") {
        return false;
    }
    let Some(packed) = byte(10) else {
        return false;
    };
    let mut at = 13;
    if packed & 0x80 != 0 {
        at += 3 * (2 << (packed & 7));
    }
    loop {
        match byte(at) {
            Some(0x21) => {
                at += 2;
                // Sub-blocks to their terminator (the control
                // extension's first block among them).
                loop {
                    let Some(len) = byte(at) else {
                        return false;
                    };
                    at += 1 + usize::from(len);
                    if len == 0 {
                        break;
                    }
                }
            }
            Some(0x2c) => {
                let (Some(w0), Some(w1), Some(flags)) = (byte(at + 5), byte(at + 6), byte(at + 9))
                else {
                    return false;
                };
                return w0 == 0 && w1 == 0 && flags & 0x40 != 0;
            }
            _ => return false,
        }
    }
}

impl Gif {
    /// A picture filling the screen, random indices of a random global
    /// palette of `colors` entries (a power of two from 2).
    pub fn new(width: u16, height: u16, colors: usize, seed: u64) -> Self {
        let mut rng = Lcg(seed);
        let palette = (0..colors)
            .map(|_| [rng.next() as u8, rng.next() as u8, rng.next() as u8])
            .collect();
        let indices = (0..usize::from(width) * usize::from(height))
            .map(|_| (rng.next() as usize % colors) as u8)
            .collect();
        Self {
            width,
            height,
            global: Some(palette),
            transparent: None,
            left: 0,
            top: 0,
            frame_width: width,
            frame_height: height,
            interlaced: false,
            local: None,
            min_code_size: (colors.trailing_zeros() as u8).max(2),
            indices,
            clear_every: Some(200),
            trailer: true,
        }
    }

    fn table_bits(entries: usize) -> u8 {
        (entries.next_power_of_two().trailing_zeros() as u8).max(1) - 1
    }

    fn table(out: &mut Vec<u8>, colors: &[[u8; 3]]) {
        let entries = 2usize << Self::table_bits(colors.len());
        for i in 0..entries {
            out.extend_from_slice(&colors.get(i).copied().unwrap_or([0; 3]));
        }
    }

    /// The indices as LZW codes, each index a code of its own (the
    /// decoder's table grows all the same, and so its code size).
    pub fn lzw(&self) -> Vec<u8> {
        let min = u32::from(self.min_code_size);
        let clear = 1u32 << min;
        let (mut acc, mut count) = (0u64, 0u32);
        let mut out = Vec::new();
        let mut emit = |code: u32, size: u32, out: &mut Vec<u8>| {
            acc |= u64::from(code) << count;
            count += size;
            while count >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                count -= 8;
            }
        };
        let (mut size, mut next, mut first) = (min + 1, clear + 2, true);
        emit(clear, size, &mut out);
        // In the order stored: interlaced rows by passes.
        let rows: Vec<usize> = if self.interlaced {
            let h = usize::from(self.frame_height);
            [(0, 8), (4, 8), (2, 4), (1, 2)]
                .into_iter()
                .flat_map(|(start, step)| (start..h).step_by(step))
                .collect()
        } else {
            (0..usize::from(self.frame_height)).collect()
        };
        let w = usize::from(self.frame_width);
        let mut written = 0usize;
        for r in rows {
            for &index in self.indices.iter().skip(r * w).take(w) {
                if self
                    .clear_every
                    .is_some_and(|n| written > 0 && written.is_multiple_of(n))
                {
                    emit(clear, size, &mut out);
                    (size, next, first) = (min + 1, clear + 2, true);
                }
                emit(u32::from(index), size, &mut out);
                written += 1;
                if first {
                    first = false;
                } else if next < 4096 {
                    next += 1;
                    if next == 1 << size && size < 12 {
                        size += 1;
                    }
                }
            }
        }
        emit(clear + 1, size, &mut out);
        if count > 0 {
            out.push(acc as u8);
        }
        out
    }

    /// The file, with `data` as the picture's LZW data.
    pub fn with_data(&self, data: &[u8]) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        match &self.global {
            Some(colors) => {
                out.extend_from_slice(&[0x80 | Self::table_bits(colors.len()), 0, 0]);
                Self::table(&mut out, colors);
            }
            None => out.extend_from_slice(&[0, 0, 0]),
        }
        // An application extension, to be skipped.
        out.extend_from_slice(&[0x21, 0xff, 11]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[3, 1, 0, 0, 0]);
        if let Some(index) = self.transparent {
            out.extend_from_slice(&[0x21, 0xf9, 4, 1, 0, 0, index, 0]);
        }
        out.push(0x2c);
        for v in [self.left, self.top, self.frame_width, self.frame_height] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let mut packed = if self.interlaced { 0x40 } else { 0 };
        if let Some(colors) = &self.local {
            packed |= 0x80 | Self::table_bits(colors.len());
        }
        out.push(packed);
        if let Some(colors) = &self.local {
            Self::table(&mut out, colors);
        }
        out.push(self.min_code_size);
        for block in data.chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
        if self.trailer {
            out.push(0x3b);
        }
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        self.with_data(&self.lzw())
    }

    /// The gray screen the picture should show: white around the
    /// picture and where it is transparent.
    pub fn expected_gray(&self) -> Vec<u8> {
        let colors = self.local.as_ref().or(self.global.as_ref()).unwrap();
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let mut out = vec![255u8; w * h];
        for (i, &index) in self.indices.iter().enumerate() {
            let x = usize::from(self.left) + i % usize::from(self.frame_width);
            let y = usize::from(self.top) + i / usize::from(self.frame_width);
            if x >= w || y >= h || self.transparent == Some(index) {
                continue;
            }
            let [r, g, b] = colors.get(usize::from(index)).copied().unwrap_or([0; 3]);
            out[y * w + x] = paperpaw_image::luma(r.into(), g.into(), b.into());
        }
        out
    }
}
