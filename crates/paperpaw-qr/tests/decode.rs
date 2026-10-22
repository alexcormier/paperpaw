//! QR codes are read back by a small decoder written from ISO/IEC 18004
//! (its tables, not the encoder's): finder, separator, timing and
//! alignment patterns, the dark module, format information (one of the
//! standard's eight strings for level M, both copies the same), version
//! information (the standard's strings for versions 7 to 10), the number
//! of data modules, Reed-Solomon syndromes of every block, and finally
//! the byte-mode data with its terminator and padding. Capacity limits
//! give `None`, never a panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_qr::QrCode;
use proptest::prelude::*;

/// The standard's format information strings for level M, by mask.
const FORMAT_M: [u32; 8] = [
    0x5412, 0x5125, 0x5E7C, 0x5B4B, 0x45F9, 0x40CE, 0x4F97, 0x4AA0,
];

/// The standard's version information strings, versions 7 to 10.
const VERSION_INFO: [u32; 4] = [0x07C94, 0x085BC, 0x09A99, 0x0A4D3];

/// Alignment pattern centres by version (Annex E).
const ALIGNMENT: [&[usize]; 11] = [
    &[],
    &[],
    &[6, 18],
    &[6, 22],
    &[6, 26],
    &[6, 30],
    &[6, 34],
    &[6, 22, 38],
    &[6, 24, 42],
    &[6, 26, 46],
    &[6, 28, 50],
];

/// Total codewords and the blocks for level M as `(count, total, data)`
/// by version (Table 9).
/// `(count, total codewords, data codewords)` of a group of blocks.
type Group = (usize, usize, usize);

const BLOCKS_M: [(usize, &[Group]); 11] = [
    (0, &[]),
    (26, &[(1, 26, 16)]),
    (44, &[(1, 44, 28)]),
    (70, &[(1, 70, 44)]),
    (100, &[(2, 50, 32)]),
    (134, &[(2, 67, 43)]),
    (172, &[(4, 43, 27)]),
    (196, &[(4, 49, 31)]),
    (242, &[(2, 60, 38), (2, 61, 39)]),
    (292, &[(3, 58, 36), (2, 59, 37)]),
    (346, &[(4, 69, 43), (1, 70, 44)]),
];

/// Bytes each version holds in byte mode at level M (Table 7).
const CAPACITY_M: [usize; 11] = [0, 14, 26, 42, 62, 84, 106, 122, 152, 180, 213];

/// Remainder bits after the codewords (Table 1).
fn remainder_bits(version: usize) -> usize {
    match version {
        2..=6 => 7,
        _ => 0,
    }
}

/// GF(256) with the QR polynomial: `(exp, log)` tables.
fn gf_tables() -> ([u8; 512], [u8; 256]) {
    let mut exp = [0u8; 512];
    let mut log = [0u8; 256];
    let mut x: u16 = 1;
    for (i, slot) in exp.iter_mut().take(255).enumerate() {
        *slot = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
    }
    exp.copy_within(..255, 255);
    (exp, log)
}

/// Whether the codeword polynomial of `block` is zero at the first
/// `ecc` powers of the generator, as every Reed-Solomon codeword is.
fn syndromes_vanish(block: &[u8], ecc: usize) -> bool {
    let (exp, log) = gf_tables();
    let mul = |a: u8, b: u8| -> u8 {
        if a == 0 || b == 0 {
            0
        } else {
            exp[log[a as usize] as usize + log[b as usize] as usize]
        }
    };
    (0..ecc).all(|i| {
        let root = exp[i];
        block.iter().fold(0u8, |acc, &c| mul(acc, root) ^ c) == 0
    })
}

fn mask_bit(mask: usize, x: usize, y: usize) -> bool {
    // `i` is the row, `j` the column, as the standard writes them.
    let (i, j) = (y, x);
    match mask {
        0 => (i + j) % 2 == 0,
        1 => i % 2 == 0,
        2 => j % 3 == 0,
        3 => (i + j) % 3 == 0,
        4 => (i / 2 + j / 3) % 2 == 0,
        5 => (i * j) % 2 + (i * j) % 3 == 0,
        6 => ((i * j) % 2 + (i * j) % 3) % 2 == 0,
        _ => ((i * j) % 3 + (i + j) % 2) % 2 == 0,
    }
}

struct Grid {
    size: usize,
    version: usize,
    dark: Vec<bool>,
}

impl Grid {
    fn of(code: &QrCode) -> Grid {
        let size = code.size();
        assert!(
            (21..=57).contains(&size) && (size - 17).is_multiple_of(4),
            "size {size}"
        );
        let mut dark = Vec::new();
        for y in 0..size {
            for x in 0..size {
                dark.push(code.dark(x, y));
            }
        }
        // Outside the square, nothing is dark.
        assert!(!code.dark(size, 0) && !code.dark(0, size) && !code.dark(usize::MAX, 3));
        Grid {
            size,
            version: (size - 17) / 4,
            dark,
        }
    }

    fn at(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.size + x]
    }

    fn alignment_centres(&self) -> Vec<(usize, usize)> {
        let positions = ALIGNMENT[self.version];
        let last = positions.len().saturating_sub(1);
        let mut centres = Vec::new();
        for (i, &x) in positions.iter().enumerate() {
            for (j, &y) in positions.iter().enumerate() {
                let on_finder =
                    (i == 0 && j == 0) || (i == 0 && j == last) || (i == last && j == 0);
                if !on_finder {
                    centres.push((x, y));
                }
            }
        }
        centres
    }

    /// Modules of patterns and information, not of data.
    fn function(&self, x: usize, y: usize) -> bool {
        let s = self.size;
        let corner = (x < 9 && y < 9) || (x >= s - 8 && y < 9) || (x < 9 && y >= s - 8);
        let timing = x == 6 || y == 6;
        let alignment = self
            .alignment_centres()
            .iter()
            .any(|&(cx, cy)| x.abs_diff(cx) <= 2 && y.abs_diff(cy) <= 2);
        let version = self.version >= 7
            && ((x >= s - 11 && x < s - 8 && y < 6) || (y >= s - 11 && y < s - 8 && x < 6));
        corner || timing || alignment || version
    }

    fn check_patterns(&self) {
        let s = self.size;
        for (fx, fy) in [(0, 0), (s - 7, 0), (0, s - 7)] {
            for dy in 0..7usize {
                for dx in 0..7usize {
                    let ring = dx.abs_diff(3).max(dy.abs_diff(3));
                    assert_eq!(
                        self.at(fx + dx, fy + dy),
                        ring != 2,
                        "finder at ({fx}, {fy})"
                    );
                }
            }
        }
        // Separators: light around the finders.
        for i in 0..8 {
            for (x, y) in [
                (7, i),
                (i, 7),
                (s - 8, i),
                (s - 1 - i, 7),
                (7, s - 1 - i),
                (i, s - 8),
            ] {
                assert!(!self.at(x, y), "separator ({x}, {y})");
            }
        }
        for i in 8..s - 8 {
            assert_eq!(self.at(i, 6), i % 2 == 0, "timing row at {i}");
            assert_eq!(self.at(6, i), i % 2 == 0, "timing column at {i}");
        }
        for (cx, cy) in self.alignment_centres() {
            for y in cy - 2..=cy + 2 {
                for x in cx - 2..=cx + 2 {
                    let ring = x.abs_diff(cx).max(y.abs_diff(cy));
                    assert_eq!(self.at(x, y), ring != 1, "alignment at ({cx}, {cy})");
                }
            }
        }
        assert!(self.at(8, s - 8), "dark module");
    }

    /// The mask, from both copies of the format information.
    fn format_mask(&self) -> usize {
        let s = self.size;
        let mut first = 0u32;
        let mut second = 0u32;
        // Bit 0 (least significant) first.
        let copy1: Vec<(usize, usize)> = (0..6)
            .map(|i| (8, i))
            .chain([(8, 7), (8, 8), (7, 8)])
            .chain((9..15).map(|i| (14 - i, 8)))
            .collect();
        let copy2: Vec<(usize, usize)> = (0..8)
            .map(|i| (s - 1 - i, 8))
            .chain((8..15).map(|i| (8, s - 15 + i)))
            .collect();
        for (bit, (&(x1, y1), &(x2, y2))) in copy1.iter().zip(&copy2).enumerate() {
            first |= u32::from(self.at(x1, y1)) << bit;
            second |= u32::from(self.at(x2, y2)) << bit;
        }
        assert_eq!(first, second, "the two copies of the format information");
        FORMAT_M
            .iter()
            .position(|&f| f == first)
            .unwrap_or_else(|| panic!("format {first:015b} is no level M string"))
    }

    fn check_version_info(&self) {
        if self.version < 7 {
            return;
        }
        let s = self.size;
        let mut top_right = 0u32;
        let mut bottom_left = 0u32;
        for i in 0..18 {
            let (a, b) = (s - 11 + i % 3, i / 3);
            top_right |= u32::from(self.at(a, b)) << i;
            bottom_left |= u32::from(self.at(b, a)) << i;
        }
        assert_eq!(
            top_right,
            VERSION_INFO[self.version - 7],
            "version information"
        );
        assert_eq!(bottom_left, top_right);
    }

    /// The data modules' bits, unmasked, in the standard's zigzag order.
    fn data_bits(&self, mask: usize) -> Vec<bool> {
        let s = self.size;
        let mut bits = Vec::new();
        let mut right = s - 1;
        let mut upward = true;
        loop {
            if right == 6 {
                right = 5;
            }
            for k in 0..s {
                let y = if upward { s - 1 - k } else { k };
                for x in [right, right - 1] {
                    if !self.function(x, y) {
                        bits.push(self.at(x, y) ^ mask_bit(mask, x, y));
                    }
                }
            }
            upward = !upward;
            if right < 2 {
                break;
            }
            right -= 2;
        }
        bits
    }
}

/// Reads `code` back, checking every structure on the way.
fn decode(code: &QrCode) -> Vec<u8> {
    let grid = Grid::of(code);
    let version = grid.version;
    grid.check_patterns();
    grid.check_version_info();
    let mask = grid.format_mask();

    let bits = grid.data_bits(mask);
    let (total, blocks) = BLOCKS_M[version];
    assert_eq!(
        bits.len(),
        total * 8 + remainder_bits(version),
        "data modules"
    );
    let codewords: Vec<u8> = bits[..total * 8]
        .chunks(8)
        .map(|byte| byte.iter().fold(0u8, |acc, &b| (acc << 1) | u8::from(b)))
        .collect();

    // De-interleave: data codewords column by column, then the error
    // correction the same way.
    let shapes: Vec<(usize, usize)> = blocks
        .iter()
        .flat_map(|&(count, total, data)| std::iter::repeat_n((total, data), count))
        .collect();
    let ecc = shapes[0].0 - shapes[0].1;
    assert!(shapes.iter().all(|(t, d)| t - d == ecc));
    let mut data_blocks: Vec<Vec<u8>> = vec![Vec::new(); shapes.len()];
    let mut at = 0;
    let longest = shapes.iter().map(|s| s.1).max().unwrap();
    for i in 0..longest {
        for (b, &(_, data)) in shapes.iter().enumerate() {
            if i < data {
                data_blocks[b].push(codewords[at]);
                at += 1;
            }
        }
    }
    let mut ecc_blocks: Vec<Vec<u8>> = vec![Vec::new(); shapes.len()];
    for _ in 0..ecc {
        for block in &mut ecc_blocks {
            block.push(codewords[at]);
            at += 1;
        }
    }
    assert_eq!(at, total);
    for (data, ecc_part) in data_blocks.iter().zip(&ecc_blocks) {
        let block: Vec<u8> = data.iter().chain(ecc_part).copied().collect();
        assert!(syndromes_vanish(&block, ecc), "Reed-Solomon block");
    }

    // The bit stream: byte mode, the count, the bytes, the terminator,
    // zeros to a byte, then alternating pad bytes.
    let stream: Vec<bool> = data_blocks
        .concat()
        .iter()
        .flat_map(|&byte| (0..8).rev().map(move |i| (byte >> i) & 1 == 1))
        .collect();
    let read = |from: usize, n: usize| -> usize {
        stream[from..from + n]
            .iter()
            .fold(0, |acc, &b| (acc << 1) | usize::from(b))
    };
    assert_eq!(read(0, 4), 0b0100, "byte mode");
    let count_bits = if version < 10 { 8 } else { 16 };
    let count = read(4, count_bits);
    let mut pos = 4 + count_bits;
    let data: Vec<u8> = (0..count).map(|i| read(pos + 8 * i, 8) as u8).collect();
    pos += 8 * count;
    let terminator = (stream.len() - pos).min(4);
    assert_eq!(read(pos, terminator), 0, "terminator");
    pos += terminator;
    while !pos.is_multiple_of(8) {
        assert!(!stream[pos], "zeros to a byte");
        pos += 1;
    }
    for (i, at) in (pos..stream.len()).step_by(8).enumerate() {
        let pad = if i % 2 == 0 { 0xEC } else { 0x11 };
        assert_eq!(read(at, 8), pad, "pad byte {i}");
    }
    data
}

#[test]
fn the_smallest_version_that_holds_the_data_is_chosen() {
    for (version, &capacity) in CAPACITY_M.iter().enumerate().skip(1) {
        let fits = vec![b'x'; capacity];
        let code = QrCode::new(&fits).unwrap();
        assert_eq!(code.size(), 17 + 4 * version, "{} bytes", fits.len());
        assert_eq!(decode(&code), fits);
        if version < 10 {
            let more = vec![b'y'; capacity + 1];
            assert_eq!(QrCode::new(&more).unwrap().size(), 21 + 4 * version);
        }
    }
}

#[test]
fn data_beyond_version_ten_is_refused() {
    for len in [214, 215, 1000, 100_000] {
        assert!(QrCode::new(&vec![0u8; len]).is_none(), "{len} bytes");
    }
}

#[test]
fn empty_and_single_bytes_decode() {
    assert_eq!(decode(&QrCode::new(b"").unwrap()), b"");
    for byte in [0x00, 0x01, 0x7f, 0x80, 0xEC, 0x11, 0xff] {
        let code = QrCode::new(&[byte]).unwrap();
        assert_eq!(code.size(), 21);
        assert_eq!(decode(&code), [byte]);
    }
}

#[test]
fn an_address_decodes() {
    let url = b"http://192.168.178.42:8080/?t=7F3A9C";
    assert_eq!(decode(&QrCode::new(url).unwrap()), url);
    let wifi = "WIFI:T:WPA;S:Café Ünïcode;P:p@ss;w0rd\\;;;".as_bytes();
    assert_eq!(decode(&QrCode::new(wifi).unwrap()), wifi);
}

#[test]
fn the_same_data_gives_the_same_code() {
    let a = QrCode::new(b"stable").unwrap();
    let b = QrCode::new(b"stable").unwrap();
    assert_eq!(a, b);
    assert_ne!(a, QrCode::new(b"stablE").unwrap());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    #[test]
    fn any_data_that_fits_decodes(data in proptest::collection::vec(any::<u8>(), 0..=213)) {
        let code = QrCode::new(&data).unwrap();
        let version = (code.size() - 17) / 4;
        // The smallest version it fits.
        prop_assert!(data.len() <= CAPACITY_M[version]);
        prop_assert!(version == 1 || data.len() > CAPACITY_M[version - 1]);
        prop_assert_eq!(decode(&code), data);
    }

    #[test]
    fn every_mask_is_one_the_standard_knows(text in "[ -~]{0,60}") {
        let code = QrCode::new(text.as_bytes()).unwrap();
        let mask = Grid::of(&code).format_mask();
        prop_assert!(mask < 8);
    }
}
