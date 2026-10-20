//! Where an app image ends, as the ESP-IDF bootloader reads it: what lies
//! behind it in its partition is free (the X4 keeps native plugins there,
//! the web flasher a bundle of plugins to install).

/// The first byte of an app image.
const IMAGE_MAGIC: u8 = 0xE9;
/// The image's header.
const HEAD_LEN: u32 = 24;
/// A segment's header: its load address and length.
const SEGMENT_HEAD_LEN: u32 = 8;
/// More segments than an image has.
const MAX_SEGMENTS: u8 = 16;
/// The largest image taken (the largest flash the chips map).
const MAX_LEN: u32 = 16 << 20;

/// How long an app image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageExtent {
    /// Its bytes, its checksum and appended SHA-256 included.
    pub len: u32,
    /// It ends with the SHA-256 of all before (32 bytes): the image's
    /// identity.
    pub sha256: bool,
}

impl ImageExtent {
    /// The extent of the image whose bytes `read(offset, buf)` fills
    /// (offsets from its start; `false` when it cannot); `None` if it is
    /// no app image.
    pub fn read(read: &mut dyn FnMut(u32, &mut [u8]) -> bool) -> Option<ImageExtent> {
        let mut head = [0u8; HEAD_LEN as usize];
        if !read(0, &mut head) || head[0] != IMAGE_MAGIC || head[1] > MAX_SEGMENTS {
            return None;
        }
        let mut at = HEAD_LEN;
        for _ in 0..head[1] {
            let mut segment = [0u8; SEGMENT_HEAD_LEN as usize];
            if !read(at, &mut segment) {
                return None;
            }
            let len = u32::from_le_bytes([segment[4], segment[5], segment[6], segment[7]]);
            at = at
                .checked_add(SEGMENT_HEAD_LEN)?
                .checked_add(len)
                .filter(|&end| end <= MAX_LEN)?;
        }
        // The checksum byte ends a block of 16; the SHA-256 follows.
        let sha256 = head[23] == 1;
        let len = ((at + 16) & !15) + if sha256 { 32 } else { 0 };
        Some(ImageExtent { len, sha256 })
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    /// An image with segments of `lens` bytes, its SHA-256 appended or
    /// not.
    fn image(lens: &[u32], sha256: bool) -> Vec<u8> {
        let mut image = alloc::vec![0u8; 24];
        image[0] = IMAGE_MAGIC;
        image[1] = lens.len() as u8;
        image[23] = u8::from(sha256);
        for &len in lens {
            image.extend_from_slice(&0x3C00_0020u32.to_le_bytes());
            image.extend_from_slice(&len.to_le_bytes());
            image.extend(core::iter::repeat_n(0xAB, len as usize));
        }
        image
    }

    fn extent(image: &[u8]) -> Option<ImageExtent> {
        ImageExtent::read(&mut |at, buf| {
            let at = at as usize;
            match image.get(at..at + buf.len()) {
                Some(bytes) => {
                    buf.copy_from_slice(bytes);
                    true
                }
                None => false,
            }
        })
    }

    #[test]
    fn the_end_is_after_the_checksum_and_the_digest() {
        // 24 + 8 + 100 + 8 + 4 = 144: the checksum at 159, the block ends
        // at 160, the digest at 192.
        let image = image(&[100, 4], true);
        assert_eq!(
            extent(&image),
            Some(ImageExtent {
                len: 192,
                sha256: true
            })
        );
        // Segments that end a block: the checksum needs one of its own.
        let image = image_of(&[112]);
        assert_eq!(extent(&image).map(|e| e.len), Some(144 + 16));
        assert_eq!(extent(&image_of(&[])).map(|e| e.len), Some(32));
    }

    fn image_of(lens: &[u32]) -> Vec<u8> {
        image(lens, false)
    }

    #[test]
    fn what_is_no_image_is_none() {
        assert_eq!(extent(&[0xFF; 64]), None, "erased flash");
        let mut many = image(&[4], true);
        many[1] = 17;
        assert_eq!(extent(&many), None);
        let mut cut = image(&[100, 4], true);
        cut.truncate(136);
        assert_eq!(extent(&cut), None, "a segment's header missing");
        let mut huge = image(&[4], false);
        huge[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(extent(&huge), None);
    }
}
