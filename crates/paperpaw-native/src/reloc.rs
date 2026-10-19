//! Moving an image to where it runs: the words that hold an address get
//! the distance moved added, the firmware's table's word its address.

use crate::header::{CODE_OFFSET, Header, Problem};

/// Checks the words a file lists to relocate: increasing, each a whole
/// word of the image, none the table's word, as many as the header says.
pub fn check_relocations(
    header: &Header,
    words: impl IntoIterator<Item = u32>,
) -> Result<(), Problem> {
    let mut count = 0u32;
    let mut next = 0u32;
    for at in words {
        let fits = at >= next
            && at.is_multiple_of(4)
            && at.checked_add(4).is_some_and(|end| end <= header.image_len)
            && at != header.api;
        if !fits {
            return Err(Problem::Relocation);
        }
        next = at + 4;
        count += 1;
    }
    if count != header.relocs {
        return Err(Problem::Relocation);
    }
    Ok(())
}

/// Relocates an image, a piece at a time, for the data address it is
/// placed at.
#[derive(Debug)]
pub struct Relocator<'a> {
    header: &'a Header,
    words: &'a [u32],
    /// What every address in the image moves by.
    delta: u32,
    /// The address of the firmware's table.
    api: u32,
}

impl<'a> Relocator<'a> {
    /// For the image of `header` placed at data address `base`, its
    /// `words` to relocate (checked here) and the table at `api`.
    pub fn new(header: &'a Header, words: &'a [u32], base: u32, api: u32) -> Result<Self, Problem> {
        check_relocations(header, words.iter().copied())?;
        if !base.is_multiple_of(header.align)
            || base
                .checked_add(header.image_len)
                .and_then(|end| end.checked_add(CODE_OFFSET))
                .is_none()
        {
            return Err(Problem::Malformed);
        }
        Ok(Self {
            header,
            words,
            delta: base.wrapping_sub(header.base),
            api,
        })
    }

    /// Relocates `piece`, the image's bytes from offset `at` on (a
    /// multiple of 4; every piece but the image's last a whole number of
    /// words). Each word moved must hold an address in the image, in
    /// either bus's view of it.
    pub fn apply(&self, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        if !at.is_multiple_of(4) {
            return Err(Problem::Relocation);
        }
        let end = at.saturating_add(piece.len() as u32);
        let first = self.words.partition_point(|&w| w < at);
        for &word in self.words[first..].iter().take_while(|&&w| w < end) {
            let i = (word - at) as usize;
            let Some(bytes) = piece.get_mut(i..i + 4) else {
                return Err(Problem::Relocation);
            };
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !self.points_inside(value) {
                return Err(Problem::Relocation);
            }
            bytes.copy_from_slice(&value.wrapping_add(self.delta).to_le_bytes());
        }
        let api = self.header.api;
        if (at..end).contains(&api) {
            let i = (api - at) as usize;
            let Some(bytes) = piece.get_mut(i..i + 4) else {
                return Err(Problem::Relocation);
            };
            bytes.copy_from_slice(&self.api.to_le_bytes());
        }
        Ok(())
    }

    /// Whether `value` is an address in the image as linked, by data or
    /// by code (its end included: a slice may point there).
    fn points_inside(&self, value: u32) -> bool {
        let start = self.header.base;
        let len = self.header.image_len;
        value.wrapping_sub(start) <= len || value.wrapping_sub(start + CODE_OFFSET) <= len
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::header::tests::{chip_header, file};
    use crate::header::{Chip, HEADER_LEN, LINK_BASE};

    fn le(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn words_move_with_the_image_in_any_pieces() {
        let file = file();
        let header = Header::check_file(&file).unwrap();
        let words = [20, 24];
        let base = 0x3C80_0040;
        for piece in [4, 8, 20, 96] {
            let relocator = Relocator::new(&header, &words, base, 0x3FC8_1234).unwrap();
            let mut image: Vec<u8> = file[HEADER_LEN..HEADER_LEN + 96].to_vec();
            for (i, chunk) in image.chunks_mut(piece).enumerate() {
                relocator.apply((i * piece) as u32, chunk).unwrap();
            }
            assert_eq!(le(&image, 16), 0x3FC8_1234, "the table's word");
            assert_eq!(le(&image, 20), base + 40, "data");
            assert_eq!(le(&image, 24), base + CODE_OFFSET + 64, "code");
            assert_eq!(
                image[64..],
                file[HEADER_LEN + 64..HEADER_LEN + 96],
                "code unchanged"
            );
        }
    }

    #[test]
    fn bad_words_are_refused() {
        let header = chip_header(Chip::Esp32S3, 96, 2);
        for words in [[24, 20], [20, 20], [21, 24], [20, 96], [16, 20], [20, 94]] {
            assert_eq!(
                Relocator::new(&header, &words, LINK_BASE, 0).unwrap_err(),
                Problem::Relocation,
                "{words:?}"
            );
        }
        assert_eq!(
            check_relocations(&header, [20]),
            Err(Problem::Relocation),
            "fewer than the header says"
        );
        assert_eq!(
            Relocator::new(&header, &[20, 24], LINK_BASE + 8, 0).unwrap_err(),
            Problem::Malformed,
            "a place not aligned as the image wants"
        );
        // A word that holds no address in the image.
        let relocator = Relocator::new(&header, &[20, 24], LINK_BASE, 0).unwrap();
        let mut image = [0u8; 96];
        image[20..24].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        assert_eq!(relocator.apply(0, &mut image), Err(Problem::Relocation));
        assert_eq!(
            relocator.apply(2, &mut image[2..]),
            Err(Problem::Relocation)
        );
    }
}
