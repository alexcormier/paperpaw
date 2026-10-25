//! Another firmware's app image, installed from the card in place of
//! PaperPaw (the way back to CrossPoint, say): not signed, so nothing
//! says who made it. What can be checked is whether it is a whole app
//! image for this device, undamaged, as the bootloader would start it:
//! its header and application descriptor ([`ForeignImage::read`]), then,
//! as it is written, every segment, its checksum and the SHA-256 it ends
//! with ([`ForeignCheck`]). The same checks as CrossPoint's installer
//! makes, and two more: the descriptor (a bootloader or a whole flash
//! image is refused) and the hash (an image without one is refused).
//!
//! An ESP-IDF app image: a header of 24 bytes (the magic 0xE9, the
//! number of segments, the chip at 12, whether a hash follows at 23),
//! then each segment (its address and length, 8 bytes, and its data), the
//! first beginning with the application descriptor; zeros up to the last
//! byte of a 16-byte block, which is the checksum (0xEF and every
//! segment's bytes, XORed); then the SHA-256 of all before it. Some
//! images say which board they are for: CrossPoint's tag
//! `CROSSPOINT-BOARD-V1:<board>;`, anywhere in the image.

use alloc::string::String;

use paperpaw_digest::sha256::Sha256;
use paperpaw_platform::Storage;

use super::{APP_HEAD_LEN, AppInfo, Firmware, InstallFailure, read_exact};

/// The header's length.
const HEADER_LEN: u64 = 24;
/// A segment's header.
const SEGMENT_HEAD: usize = 8;
/// The first bytes kept: the header, the first segment's head and the
/// descriptor's magic word.
const FIRST_LEN: usize = 36;
/// Segments an image has at most (ESP-IDF's).
const MAX_SEGMENTS: u8 = 16;
/// The SHA-256 at the end.
const HASH_LEN: u64 = 32;
/// The descriptor's magic word, the first of the first segment.
const DESCRIPTOR_MAGIC: u32 = 0xABCD_5432;
/// CrossPoint's board tag, and the longest board name read after it.
const TAG: &[u8] = b"CROSSPOINT-BOARD-V1:";
const TAG_NAME_MAX: usize = 16;

/// What an image file on the card says about itself, before it is
/// installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignImage {
    pub path: String,
    /// Its header and descriptor: the chip, and the version and project
    /// its maker gave it.
    pub info: AppInfo,
    /// The file's length.
    pub len: u64,
}

impl ForeignImage {
    /// Reads the image file at `path`: an app image, or
    /// [`InstallFailure::NotAnUpdate`] (a bootloader, a whole flash image,
    /// anything else).
    pub fn read(storage: &dyn Storage, path: &str) -> Result<Self, InstallFailure> {
        let len = storage.size(path).map_err(|_| InstallFailure::Unreadable)?;
        let mut head = [0; APP_HEAD_LEN];
        if len < APP_HEAD_LEN as u64 {
            return Err(InstallFailure::NotAnUpdate);
        }
        read_exact(storage, path, 0, &mut head)?;
        let info = AppInfo::read(&head).map_err(|_| InstallFailure::NotAnUpdate)?;
        Ok(Self {
            path: String::from(path),
            info,
            len,
        })
    }

    /// Whether it is for this device and has room: its chip, and not
    /// PaperPaw for another device (the X4 Pro's on an X4 Classic).
    pub fn fits(&self, firmware: &Firmware) -> Result<(), InstallFailure> {
        let other_paperpaw =
            self.info.project.starts_with("paperpaw-") && self.info.project != firmware.project;
        if self.info.chip != firmware.chip || other_paperpaw {
            return Err(InstallFailure::OtherDevice);
        }
        if self.len > firmware.room {
            return Err(InstallFailure::TooLarge);
        }
        Ok(())
    }
}

/// Where the walk through the segments is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Walk {
    Header,
    /// A segment's head, so many of its bytes read.
    Head(usize),
    /// A segment's data, so many bytes left.
    Data(u32),
    /// After the segments: the checksum at `body_end - 1`.
    Tail,
    /// The checksum read.
    Done,
}

/// An image checked as it is read: [`ForeignCheck::absorb`] every byte of
/// it in order, then [`ForeignCheck::finish`].
#[derive(Debug)]
pub struct ForeignCheck {
    chip: u16,
    /// This device's name in CrossPoint's tag (`x4`, `x4pro`, `x4c`).
    board: &'static str,
    len: u64,
    at: u64,
    first: [u8; FIRST_LEN],
    walk: Walk,
    segments: u8,
    head: [u8; SEGMENT_HEAD],
    xor: u8,
    body_end: u64,
    checksum: Option<bool>,
    sha: Sha256,
    hash: [u8; HASH_LEN as usize],
    /// The tag's bytes matched so far, and the board name after it.
    tag_at: usize,
    name: Option<String>,
    /// The board the image names, the first tag found.
    tagged: Option<String>,
    problem: Option<InstallFailure>,
}

impl ForeignCheck {
    /// Checks an image of `len` bytes for a device with `chip` that
    /// CrossPoint's tag names `board`.
    pub fn new(chip: u16, board: &'static str, len: u64) -> Self {
        Self {
            chip,
            board,
            len,
            at: 0,
            first: [0; FIRST_LEN],
            walk: Walk::Header,
            segments: 0,
            head: [0; SEGMENT_HEAD],
            xor: 0,
            body_end: 0,
            checksum: None,
            sha: Sha256::new(),
            hash: [0; HASH_LEN as usize],
            tag_at: 0,
            name: None,
            tagged: None,
            problem: None,
        }
    }

    /// The image's next bytes.
    pub fn absorb(&mut self, chunk: &[u8]) {
        let before = self.at;
        let end = before + chunk.len() as u64;
        self.at = end;
        if end > self.len {
            self.problem.get_or_insert(InstallFailure::Damaged);
            return;
        }
        let hash_at = self.len.saturating_sub(HASH_LEN);
        let hashed = hash_at.saturating_sub(before).min(chunk.len() as u64) as usize;
        self.sha.update(&chunk[..hashed]);
        if end > hash_at {
            let from = hash_at.saturating_sub(before) as usize;
            let into = (before + from as u64 - hash_at) as usize;
            let bytes = &chunk[from..];
            self.hash[into..into + bytes.len()].copy_from_slice(bytes);
        }
        if before < FIRST_LEN as u64 {
            let n = (FIRST_LEN - before as usize).min(chunk.len());
            self.first[before as usize..][..n].copy_from_slice(&chunk[..n]);
            if before as usize + n == FIRST_LEN {
                self.check_first();
            }
        }
        for &byte in chunk {
            self.tag(byte);
        }
        if self.problem.is_none() {
            self.walk(before, chunk);
        }
    }

    /// Whether the image is whole, undamaged and for this device.
    pub fn finish(self) -> Result<(), InstallFailure> {
        if let Some(problem) = self.problem {
            return Err(problem);
        }
        if self.at < FIRST_LEN as u64 {
            return Err(InstallFailure::NotAnUpdate);
        }
        if self.first[23] != 1 {
            // No hash: nothing to check it by.
            return Err(InstallFailure::NotAnUpdate);
        }
        if self
            .tagged
            .as_deref()
            .is_some_and(|board| board != self.board)
        {
            return Err(InstallFailure::OtherDevice);
        }
        let whole = self.at == self.len
            && self.walk == Walk::Done
            && self.checksum == Some(true)
            && self.body_end + HASH_LEN == self.len;
        if !whole || self.sha.finish() != self.hash {
            return Err(InstallFailure::Damaged);
        }
        Ok(())
    }

    /// The header and the descriptor's magic: an app image for this chip.
    fn check_first(&mut self) {
        let first = &self.first;
        let magic = u32::from_le_bytes([first[32], first[33], first[34], first[35]]);
        if first[0] != 0xE9 || magic != DESCRIPTOR_MAGIC || !(1..=MAX_SEGMENTS).contains(&first[1])
        {
            self.problem = Some(InstallFailure::NotAnUpdate);
        } else if u16::from_le_bytes([first[12], first[13]]) != self.chip {
            self.problem = Some(InstallFailure::OtherDevice);
        }
    }

    /// Looks for CrossPoint's tag, a byte at a time.
    fn tag(&mut self, byte: u8) {
        if let Some(name) = &mut self.name {
            if byte == b';' {
                if self.tagged.is_none() && !name.is_empty() {
                    self.tagged = self.name.take();
                } else {
                    self.name = None;
                }
            } else if (byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
                && name.len() < TAG_NAME_MAX
            {
                name.push(char::from(byte));
            } else {
                self.name = None;
            }
            return;
        }
        if byte == TAG[self.tag_at] {
            self.tag_at += 1;
            if self.tag_at == TAG.len() {
                self.tag_at = 0;
                self.name = Some(String::new());
            }
        } else {
            // The tag's first byte comes in it only once.
            self.tag_at = usize::from(byte == TAG[0]);
        }
    }

    /// Walks through the segments of `chunk`, which starts at `before`.
    fn walk(&mut self, before: u64, chunk: &[u8]) {
        let mut i = 0;
        while i < chunk.len() {
            let at = before + i as u64;
            let rest = &chunk[i..];
            match self.walk {
                Walk::Header => {
                    let n = (HEADER_LEN.saturating_sub(at) as usize).min(rest.len());
                    i += n;
                    if at + n as u64 == HEADER_LEN {
                        self.segments = self.first[1];
                        if self.segments == 0 {
                            self.problem = Some(InstallFailure::NotAnUpdate);
                            return;
                        }
                        self.walk = Walk::Head(0);
                    }
                }
                Walk::Head(have) => {
                    let n = (SEGMENT_HEAD - have).min(rest.len());
                    self.head[have..have + n].copy_from_slice(&rest[..n]);
                    i += n;
                    self.walk = if have + n == SEGMENT_HEAD {
                        let h = &self.head;
                        let len = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
                        if u64::from(len) > self.len {
                            self.problem = Some(InstallFailure::Damaged);
                            return;
                        }
                        self.data(len, at + n as u64)
                    } else {
                        Walk::Head(have + n)
                    };
                }
                Walk::Data(left) => {
                    let n = (left as usize).min(rest.len());
                    self.xor = rest[..n].iter().fold(self.xor, |x, b| x ^ b);
                    i += n;
                    self.walk = self.data(left - n as u32, at + n as u64);
                }
                Walk::Tail => {
                    let last = self.body_end - 1;
                    if last >= at + rest.len() as u64 {
                        return;
                    }
                    let byte = rest[(last - at) as usize];
                    self.checksum = Some(byte == 0xEF ^ self.xor);
                    self.walk = Walk::Done;
                    return;
                }
                Walk::Done => return,
            }
        }
    }

    /// At `at`, `left` bytes of a segment's data to come; with none left,
    /// the next segment's head, or after the last the tail.
    fn data(&mut self, left: u32, at: u64) -> Walk {
        if left > 0 {
            return Walk::Data(left);
        }
        self.segments = self.segments.saturating_sub(1);
        if self.segments > 0 {
            return Walk::Head(0);
        }
        // Zeros up to the checksum, the last byte of a 16-byte block.
        self.body_end = (at + 1).next_multiple_of(16);
        Walk::Tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// An app image as ESP-IDF builds one: `segments` of these lengths,
    /// the first with the descriptor, for `chip`, with a hash.
    fn image(chip: u16, segments: &[usize], extra: &[u8]) -> Vec<u8> {
        let mut image = vec![0u8; 24];
        image[0] = 0xE9;
        image[1] = segments.len() as u8;
        image[12..14].copy_from_slice(&chip.to_le_bytes());
        image[23] = 1;
        let mut xor = 0xEFu8;
        for (n, &len) in segments.iter().enumerate() {
            let mut data: Vec<u8> = (0..len).map(|i| (i * 7 + n) as u8).collect();
            if n == 0 {
                data[..4].copy_from_slice(&DESCRIPTOR_MAGIC.to_le_bytes());
                data[16..21].copy_from_slice(b"1.2.3");
                data[48..58].copy_from_slice(b"crosspoint");
                data[100..100 + extra.len()].copy_from_slice(extra);
            }
            image.extend_from_slice(&0x4200_0000u32.to_le_bytes());
            image.extend_from_slice(&(len as u32).to_le_bytes());
            xor = data.iter().fold(xor, |x, b| x ^ b);
            image.extend_from_slice(&data);
        }
        while image.len() % 16 != 15 {
            image.push(0);
        }
        image.push(xor);
        let hash = paperpaw_digest::sha256::digest(&image);
        image.extend_from_slice(&hash);
        image
    }

    fn check(image: &[u8], piece: usize) -> Result<(), InstallFailure> {
        check_for(image, piece, "x4")
    }

    fn check_for(image: &[u8], piece: usize, board: &'static str) -> Result<(), InstallFailure> {
        let mut check = ForeignCheck::new(5, board, image.len() as u64);
        for chunk in image.chunks(piece) {
            check.absorb(chunk);
        }
        check.finish()
    }

    #[test]
    fn a_whole_image_passes_in_any_pieces() {
        let image = image(5, &[300, 1000, 17], b"");
        for piece in [1, 3, 16, 24, 36, 4096] {
            assert_eq!(check(&image, piece), Ok(()), "pieces of {piece}");
        }
    }

    #[test]
    fn damage_is_found() {
        let good = image(5, &[300, 1000], b"");
        // A byte of the data, the checksum, the hash.
        for at in [200, good.len() - 33, good.len() - 1] {
            let mut bad = good.clone();
            bad[at] ^= 1;
            assert_eq!(check(&bad, 512), Err(InstallFailure::Damaged), "byte {at}");
        }
        // Cut short, or longer than it says.
        assert_eq!(
            check(&good[..good.len() - 16], 512),
            Err(InstallFailure::Damaged)
        );
        let mut long = good.clone();
        long.extend_from_slice(&[0; 32]);
        assert_eq!(check(&long, 512), Err(InstallFailure::Damaged));
    }

    #[test]
    fn what_is_no_app_image_is_refused() {
        // Another chip.
        assert_eq!(
            check(&image(9, &[300], b""), 64),
            Err(InstallFailure::OtherDevice)
        );
        // A bootloader (another descriptor), or a whole flash image.
        let mut boot = image(5, &[300], b"");
        boot[32..36].copy_from_slice(&0x50u32.to_le_bytes());
        assert_eq!(check(&boot, 64), Err(InstallFailure::NotAnUpdate));
        let mut flash = vec![0xFFu8; 0x1000];
        flash.extend(image(5, &[300], b""));
        assert_eq!(check(&flash, 64), Err(InstallFailure::NotAnUpdate));
        // Without a hash.
        let mut unhashed = image(5, &[300], b"");
        unhashed[23] = 0;
        assert_eq!(check(&unhashed, 64), Err(InstallFailure::NotAnUpdate));
        // Too short to say anything.
        assert_eq!(check(&[0xE9; 10], 64), Err(InstallFailure::NotAnUpdate));
    }

    #[test]
    fn it_fits_this_device_and_its_room() {
        let firmware = Firmware::new("0.5.0", "paperpaw-xteink-classic", 9).room(0x64_0000);
        let mut image = ForeignImage {
            path: String::from("crosspoint.bin"),
            info: AppInfo {
                chip: 9,
                version: String::from("1.2.3"),
                project: String::from("crosspoint"),
            },
            len: 0x60_0000,
        };
        assert_eq!(image.fits(&firmware), Ok(()));
        image.len = 0x64_0001;
        assert_eq!(image.fits(&firmware), Err(InstallFailure::TooLarge));
        image.len = 0x60_0000;
        // PaperPaw for the X4 Pro, on the same chip: another device.
        image.info.project = String::from("paperpaw-xteink-pro");
        assert_eq!(image.fits(&firmware), Err(InstallFailure::OtherDevice));
        image.info.project = String::from("paperpaw-xteink-classic");
        assert_eq!(image.fits(&firmware), Ok(()));
        image.info.chip = 5;
        assert_eq!(image.fits(&firmware), Err(InstallFailure::OtherDevice));
    }

    #[test]
    fn crosspoints_board_tag_must_name_this_one() {
        let tagged = image(5, &[400], b"xx CROSSPOINT-BOARD-V1:x4pro; yy");
        assert_eq!(check_for(&tagged, 7, "x4pro"), Ok(()));
        assert_eq!(
            check_for(&tagged, 7, "x4c"),
            Err(InstallFailure::OtherDevice)
        );
        // Untagged images pass, as CrossPoint lets them; a broken tag is
        // no tag.
        assert_eq!(check_for(&image(5, &[400], b""), 7, "x4c"), Ok(()));
        let broken = image(5, &[400], b"CROSSPOINT-BOARD-V1:x4 c;");
        assert_eq!(check_for(&broken, 7, "x4c"), Ok(()));
        let twice = image(5, &[400], b"CCROSSPOINT-BOARD-V1:x4;");
        assert_eq!(check_for(&twice, 5, "x4"), Ok(()));
        assert_eq!(
            check_for(&twice, 5, "x4c"),
            Err(InstallFailure::OtherDevice)
        );
    }
}
