//! The plugins an X4 keeps in flash: written once into the free end of
//! the app partition it runs from, packed one after the other, each
//! relocated for where it is mapped; an index in the region's first
//! sector says what lies where, so later starts only map them.
//!
//! The index holds the firmware it was made by (an image of another, or
//! another build, has other free MMU entries and another table): found
//! behind another firmware, it is taken for none. Plugins are placed in
//! the order they are loaded; at a start, those that are as the index
//! says stay, and from the first that is not, everything is written anew
//! ([`Layout`]). The index is erased before the first image is written
//! and written last, so a start cut short leaves no index that lies.
//!
//! A plugin's hot code runs from an arena of internal RAM, whose place is
//! the same at every start of a firmware: each plugin takes its part
//! after those before it, at most [`HOT_MAX`], as long as the arena holds
//! it; what does not fit runs in place, from flash. The index records
//! each one's part, as its image is relocated for it.
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..4 | `PPNX` |
//! | 4..6 | the format, 2 |
//! | 6..8 | how many plugins are placed |
//! | 8..40 | the firmware (the SHA-256 its image ends with) |
//! | 40..72 | SHA-256 of bytes 0..40 and of the entries |
//! | 72.. | 52 bytes a plugin: its file's SHA-256 (its header's), where its image lies in flash, its length, the data address it was relocated for, the address of the table written into it, the instruction address its hot code runs at (0: in place) |

use alloc::vec::Vec;

use paperpaw_digest::sha256::Sha256;

use crate::header::Header;

/// The index's first bytes.
pub const MAGIC: [u8; 4] = *b"PPNX";
/// The index's format this crate reads and writes: 2 places hot code.
pub const FORMAT: u16 = 2;
/// The flash's erase sector: the index takes one, each image begins at
/// one.
pub const SECTOR: u32 = 4096;
const HEAD_LEN: usize = 72;
const ENTRY_LEN: usize = 52;
/// The most plugins one index holds.
pub const MAX_PLACED: usize = (SECTOR as usize - HEAD_LEN) / ENTRY_LEN;
/// The most hot code one plugin runs from the arena: more runs in place.
pub const HOT_MAX: u32 = 16 * 1024;

/// A plugin placed in flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Its file's SHA-256 ([`Header::sha256`]).
    pub sha256: [u8; 32],
    /// Where its image lies in flash.
    pub at: u32,
    /// The image's length.
    pub len: u32,
    /// The data address it was relocated for.
    pub base: u32,
    /// The address of the firmware's table written into it.
    pub api: u32,
    /// The instruction address its hot code was relocated for in the
    /// arena (0: in place).
    pub hot: u32,
}

/// What lies in the plugins' region (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    /// The firmware that placed them: the SHA-256 its image ends with.
    pub firmware: [u8; 32],
    pub placed: Vec<Placed>,
}

impl Index {
    /// The index at the start of `sector`, if one is there whole.
    pub fn read(sector: &[u8]) -> Option<Index> {
        let head = sector.get(..HEAD_LEN)?;
        if head[..4] != MAGIC || u16::from_le_bytes([head[4], head[5]]) != FORMAT {
            return None;
        }
        let count = usize::from(u16::from_le_bytes([head[6], head[7]]));
        if count > MAX_PLACED {
            return None;
        }
        let entries = sector.get(HEAD_LEN..HEAD_LEN + count * ENTRY_LEN)?;
        if digest(&head[..40], entries) != head[40..72] {
            return None;
        }
        let mut firmware = [0; 32];
        firmware.copy_from_slice(&head[8..40]);
        let placed = entries
            .as_chunks::<ENTRY_LEN>()
            .0
            .iter()
            .map(|e| {
                let word = |at: usize| u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]);
                let mut sha256 = [0; 32];
                sha256.copy_from_slice(&e[..32]);
                Placed {
                    sha256,
                    at: word(32),
                    len: word(36),
                    base: word(40),
                    api: word(44),
                    hot: word(48),
                }
            })
            .collect();
        Some(Index { firmware, placed })
    }

    /// The index's bytes (at most a [`SECTOR`]).
    pub fn to_bytes(&self) -> Vec<u8> {
        let count = self.placed.len().min(MAX_PLACED);
        let mut bytes = Vec::with_capacity(HEAD_LEN + count * ENTRY_LEN);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&FORMAT.to_le_bytes());
        bytes.extend_from_slice(&(count as u16).to_le_bytes());
        bytes.extend_from_slice(&self.firmware);
        bytes.extend_from_slice(&[0; 32]);
        for placed in &self.placed[..count] {
            bytes.extend_from_slice(&placed.sha256);
            for value in [placed.at, placed.len, placed.base, placed.api, placed.hot] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let sha = digest(&bytes[..40], &bytes[HEAD_LEN..]);
        bytes[40..72].copy_from_slice(&sha);
        bytes
    }
}

fn digest(head: &[u8], entries: &[u8]) -> [u8; 32] {
    let mut sha = Sha256::new();
    sha.update(head);
    sha.update(entries);
    sha.finish()
}

/// The flash plugins are placed in, and where it is mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// The region's first byte in flash, the index's sector.
    pub start: u32,
    /// The byte after its last.
    pub end: u32,
    /// The data address `start` is mapped at.
    pub base: u32,
}

impl Region {
    /// A region from `start` to `end` mapped at `base`; `None` unless
    /// `start` and `base` begin a sector and the region holds more than
    /// its index.
    pub fn new(start: u32, end: u32, base: u32) -> Option<Region> {
        (start.is_multiple_of(SECTOR) && base.is_multiple_of(SECTOR) && end > start + SECTOR)
            .then_some(Region { start, end, base })
    }
}

/// The internal RAM hot code runs from: its first instruction address
/// (a multiple of 4) and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Arena {
    pub start: u32,
    pub len: u32,
}

impl Arena {
    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }
}

/// There is no room left for a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRoom;

/// Where a plugin goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Where its image goes in flash.
    pub at: u32,
    /// The data address it is mapped at, to relocate it for.
    pub base: u32,
    /// The instruction address in the arena its hot code runs at, to
    /// relocate it for and copy it to (0: in place).
    pub hot: u32,
    /// The flash already holds it so: it only needs mapping.
    pub present: bool,
    /// It is the first written at this start: the index is to be erased
    /// before.
    pub erase_index: bool,
}

/// The plugins placed at a start, one after the other, and how that
/// compares with the index found (see the [module documentation](self)).
#[derive(Debug)]
pub struct Layout {
    region: Region,
    firmware: [u8; 32],
    api: u32,
    arena: Arena,
    /// The arena's bytes given to hot code so far.
    hot_used: u32,
    /// What the index found says, if it is this firmware's.
    old: Vec<Placed>,
    placed: Vec<Placed>,
    /// Every plugin placed so far is as the old index says.
    same: bool,
    /// An image was written.
    written: bool,
}

impl Layout {
    /// Begins placing plugins in `region` for the `firmware` whose table
    /// lies at `api` and whose hot code runs from `arena`, `found` the
    /// index read from the region.
    pub fn new(
        region: Region,
        firmware: [u8; 32],
        api: u32,
        arena: Arena,
        found: Option<Index>,
    ) -> Self {
        let old = found
            .filter(|index| index.firmware == firmware)
            .map(|index| index.placed)
            .unwrap_or_default();
        Self {
            region,
            firmware,
            api,
            arena,
            hot_used: 0,
            old,
            placed: Vec::new(),
            same: true,
            written: false,
        }
    }

    /// Where the plugin of `header` goes, after those placed before.
    pub fn place(&mut self, header: &Header) -> Result<Placement, NoRoom> {
        if self.placed.len() >= MAX_PLACED {
            return Err(NoRoom);
        }
        let at = match self.placed.last() {
            Some(last) => (last.at + last.len).next_multiple_of(SECTOR),
            None => self.region.start + SECTOR,
        };
        if at
            .checked_add(header.image_len)
            .is_none_or(|end| end > self.region.end)
        {
            return Err(NoRoom);
        }
        let base = self.region.base + (at - self.region.start);
        if !base.is_multiple_of(header.align) {
            return Err(NoRoom);
        }
        let hot = self.hot(header.hot_len);
        let placed = Placed {
            sha256: header.sha256,
            at,
            len: header.image_len,
            base,
            api: self.api,
            hot,
        };
        let present = self.same && self.old.get(self.placed.len()) == Some(&placed);
        self.same &= present;
        let erase_index = !present && !self.written;
        self.written |= !present;
        self.placed.push(placed);
        Ok(Placement {
            at,
            base,
            hot,
            present,
            erase_index,
        })
    }

    /// The arena's part for `len` bytes of hot code, after those given
    /// before: 0 if there is none or it does not fit.
    fn hot(&mut self, len: u32) -> u32 {
        let fits = len > 0
            && len <= HOT_MAX
            && self
                .hot_used
                .checked_add(len)
                .is_some_and(|end| end <= self.arena.len);
        if !fits {
            return 0;
        }
        let hot = self.arena.start + self.hot_used;
        self.hot_used = (self.hot_used + len).next_multiple_of(4);
        hot
    }

    /// The arena's bytes given to hot code.
    pub fn hot_used(&self) -> u32 {
        self.hot_used
    }

    /// The region plugins are placed in.
    pub fn region(&self) -> Region {
        self.region
    }

    /// The end in flash of the last image placed (the region's first byte
    /// after its index if none was).
    pub fn end(&self) -> u32 {
        self.placed
            .last()
            .map_or(self.region.start + SECTOR, |last| last.at + last.len)
    }

    /// The index to write once every plugin is placed, if the one in
    /// flash no longer says what lies there.
    pub fn index(&self) -> Option<Index> {
        let changed = !self.same || self.old.len() != self.placed.len();
        changed.then(|| Index {
            firmware: self.firmware,
            placed: self.placed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::tests::chip_header;
    use crate::header::{Chip, LINK_BASE};

    const FIRMWARE: [u8; 32] = [7; 32];
    const API: u32 = 0x3FC8_0000;
    const ARENA: Arena = Arena {
        start: 0x4039_0000,
        len: 20 * 1024,
    };

    fn region() -> Region {
        Region::new(
            0x6F_3000,
            0x7F_0000 + 0x10000,
            LINK_BASE + 0x60_0000 + 0x3000,
        )
        .unwrap()
    }

    fn header(len: u32, sha: u8) -> Header {
        Header {
            sha256: [sha; 32],
            ..chip_header(Chip::Esp32C3, len, 0)
        }
    }

    /// Places `headers` after an index `found`, as a start would.
    fn start(found: Option<Index>, headers: &[Header]) -> (Vec<Placement>, Option<Index>) {
        let mut layout = Layout::new(region(), FIRMWARE, API, ARENA, found);
        let placements = headers.iter().map(|h| layout.place(h).unwrap()).collect();
        (placements, layout.index())
    }

    #[test]
    fn plugins_are_packed_from_a_sector_each() {
        let (placements, index) = start(None, &[header(5000, 1), header(100, 2)]);
        let region = region();
        assert_eq!(placements[0].at, region.start + SECTOR);
        assert_eq!(placements[0].base, region.base + SECTOR);
        assert_eq!(
            placements[1].at,
            region.start + 3 * SECTOR,
            "after 5000 bytes"
        );
        assert!(placements.iter().all(|p| !p.present));
        assert!(placements[0].erase_index && !placements[1].erase_index);
        let index = index.unwrap();
        assert_eq!(Index::read(&index.to_bytes()), Some(index.clone()));
        assert_eq!(index.placed[1].len, 100);
    }

    #[test]
    fn a_later_start_only_maps() {
        let headers = [header(5000, 1), header(100, 2)];
        let (_, index) = start(None, &headers);
        let (placements, again) = start(index, &headers);
        assert!(placements.iter().all(|p| p.present && !p.erase_index));
        assert_eq!(again, None, "the index stays");
    }

    #[test]
    fn from_the_first_change_everything_is_written_anew() {
        let (_, index) = start(None, &[header(5000, 1), header(100, 2), header(100, 3)]);
        // The second plugin changed (same size): it and the third are
        // written, the first stays.
        let (placements, again) = start(
            index.clone(),
            &[header(5000, 1), header(100, 9), header(100, 3)],
        );
        let present: Vec<bool> = placements.iter().map(|p| p.present).collect();
        assert_eq!(present, [true, false, false]);
        assert!(placements[1].erase_index);
        assert_eq!(again.unwrap().placed[1].sha256, [9; 32]);
        // The last one went: the index is written shorter.
        let (placements, again) = start(index, &[header(5000, 1), header(100, 2)]);
        assert!(placements.iter().all(|p| p.present));
        assert_eq!(again.unwrap().placed.len(), 2);
    }

    #[test]
    fn an_index_of_another_firmware_is_none() {
        let headers = [header(100, 1)];
        let (_, index) = start(None, &headers);
        let mut other = index.unwrap();
        other.firmware = [8; 32];
        let (placements, again) = start(Some(other), &headers);
        assert!(!placements[0].present);
        assert!(again.is_some());
        // Another table address (another build) is a change too.
        let mut layout = Layout::new(region(), FIRMWARE, API + 4, ARENA, start(None, &headers).1);
        assert!(!layout.place(&headers[0]).unwrap().present);
    }

    #[test]
    fn the_region_has_its_end() {
        let region = region();
        let room = region.end - region.start - SECTOR;
        let mut layout = Layout::new(region, FIRMWARE, API, ARENA, None);
        assert_eq!(layout.place(&header(room + 4, 1)), Err(NoRoom));
        assert!(layout.place(&header(room - 2 * SECTOR, 1)).is_ok());
        assert_eq!(layout.place(&header(3 * SECTOR, 2)), Err(NoRoom));
        assert!(layout.place(&header(SECTOR, 3)).is_ok());
        assert_eq!(layout.end(), region.end - SECTOR);
        assert_eq!(Region::new(0x1001, 0x9000, LINK_BASE), None);
        assert_eq!(
            Region::new(0x1000, 0x2000, LINK_BASE),
            None,
            "only an index"
        );
    }

    fn hot(len: u32, hot_len: u32, sha: u8) -> Header {
        Header {
            hot_at: len - hot_len,
            hot_len,
            ..header(len, sha)
        }
    }

    #[test]
    fn hot_code_takes_the_arena_in_turn_while_it_fits() {
        let headers = [
            hot(5000, 400, 1),
            header(100, 2),
            hot(20_000, HOT_MAX + 4, 3),
            hot(20_000, HOT_MAX, 4),
            hot(5000, 3700, 5),
            hot(5000, 8, 6),
        ];
        let mut layout = Layout::new(region(), FIRMWARE, API, ARENA, None);
        let hot: Vec<u32> = headers
            .iter()
            .map(|h| layout.place(h).unwrap().hot)
            .collect();
        let arena = ARENA.start;
        // None, too much for one plugin, more than the arena has left.
        assert_eq!(hot, [arena, 0, 0, arena + 400, 0, arena + 400 + HOT_MAX]);
        assert_eq!(layout.hot_used(), 400 + HOT_MAX + 8);
        let index = layout.index().unwrap();
        assert_eq!(Index::read(&index.to_bytes()), Some(index.clone()));
        assert_eq!(index.placed[3].hot, arena + 400);
    }

    #[test]
    fn a_plugin_is_the_same_only_with_its_hot_code_where_it_was() {
        let (_, index) = start(None, &[hot(5000, 400, 1), hot(5000, 400, 2)]);
        // The first's hot code grew (another file): the second's moves.
        let (placements, again) = start(index.clone(), &[hot(5000, 404, 9), hot(5000, 400, 2)]);
        assert!(placements.iter().all(|p| !p.present));
        assert_eq!(placements[1].hot, ARENA.start + 404);
        assert!(again.is_some());
        // Another arena (another firmware's, the same index): all anew.
        let mut layout = Layout::new(
            region(),
            FIRMWARE,
            API,
            Arena {
                start: ARENA.start + 16,
                ..ARENA
            },
            index,
        );
        assert!(!layout.place(&hot(5000, 400, 1)).unwrap().present);
    }

    #[test]
    fn a_damaged_index_is_none() {
        let (_, index) = start(None, &[header(100, 1)]);
        let bytes = index.unwrap().to_bytes();
        for at in [0, 5, 7, 20, 50, 80, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            assert_eq!(Index::read(&changed), None, "{at}");
        }
        assert_eq!(Index::read(&bytes[..bytes.len() - 1]), None);
        assert_eq!(Index::read(&[0xFF; SECTOR as usize]), None, "erased flash");
    }
}
