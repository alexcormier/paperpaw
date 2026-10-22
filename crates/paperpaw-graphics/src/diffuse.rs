//! Error diffusion as Atkinson's ([`Atkinson`]): how pictures become the
//! few grays of a panel, row by row from the top. Each pixel's error is
//! passed on, six eighths of it, to the two pixels after it, the three
//! below and the one below those: line drawings stay clean (no specks in
//! white paper, no grid as an ordered dither leaves), photos keep their
//! tones but the lightest and darkest, which come out a little lighter
//! and darker. Integers only (chips without floating point), two rows of
//! error at a time.

use alloc::vec::Vec;

/// The error a picture's rows leave to the next two, `width` pixels each.
#[derive(Debug, Clone, Default)]
pub struct Atkinson {
    /// The error left to the row to come (first) and the one after.
    rows: [Vec<i16>; 2],
}

impl Atkinson {
    /// For rows `width` pixels wide, with no error yet; none where memory
    /// is short.
    pub fn new(width: usize) -> Option<Self> {
        let row = || {
            let mut row = Vec::new();
            row.try_reserve_exact(width).ok()?;
            row.resize(width, 0i16);
            Some(row)
        };
        Some(Self {
            rows: [row()?, row()?],
        })
    }

    /// The width of its rows.
    pub fn width(&self) -> usize {
        self.rows[0].len()
    }

    /// No error any more, as at a picture's first row.
    pub fn reset(&mut self) {
        for row in &mut self.rows {
            row.fill(0);
        }
    }

    /// The error of `other` (as wide) in place of its own.
    pub fn copy_from(&mut self, other: &Atkinson) {
        for (row, theirs) in self.rows.iter_mut().zip(&other.rows) {
            row.copy_from_slice(theirs);
        }
    }

    /// Row `grays` (as wide as its rows) as grays of `levels` levels (2
    /// to 16: black and white, four grays, sixteen), each passed to `put`
    /// with its column as the gray of its level (0 to 255); then ready
    /// for the next row.
    pub fn row(&mut self, grays: &[u8], levels: u8, mut put: impl FnMut(usize, u8)) {
        let top = i32::from(levels.clamp(2, 16)) - 1;
        // A level's step: whole for 2, 4 and 16 grays (255 = 3 × 5 × 17),
        // so no division for each pixel.
        let step = 255 / top;
        let [here, below] = &mut self.rows;
        let last = grays.len().min(here.len()).saturating_sub(1);
        // The error left to the two pixels after this one.
        let mut ahead = (0i32, 0i32);
        let add = |cell: &mut i16, part: i32| *cell = saturate(i32::from(*cell) + part);
        for (column, &gray) in grays.iter().enumerate().take(here.len()) {
            let value = i32::from(gray) + i32::from(here[column]) + ahead.0;
            let level = (value.clamp(0, 255) * top + 127) / 255;
            let shown = level * step;
            let part = (value - shown) / 8;
            ahead = (ahead.1 + part, part);
            if column > 0 {
                add(&mut below[column - 1], part);
            }
            add(&mut below[column], part);
            if column < last {
                add(&mut below[column + 1], part);
            }
            // The row after the next, where this pixel's error is spent.
            here[column] = saturate(part);
            put(column, shown as u8);
        }
        self.rows.swap(0, 1);
    }
}

fn saturate(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_stay_and_a_middle_gray_is_half_white() {
        let mut atkinson = Atkinson::new(64).unwrap();
        let mut white = 0;
        for _ in 0..64 {
            atkinson.row(&[128; 64], 2, |_, gray| white += u32::from(gray == 255));
        }
        let share = white * 100 / (64 * 64);
        assert!((40..=60).contains(&share), "{share} % white");
        let mut shown = Vec::new();
        atkinson.reset();
        atkinson.row(&[0, 255, 0, 255], 2, |_, gray| shown.push(gray));
        assert_eq!(&shown[..4], [0, 255, 0, 255]);
    }

    #[test]
    fn four_grays_are_their_levels() {
        let mut atkinson = Atkinson::new(4).unwrap();
        let mut shown = Vec::new();
        atkinson.row(&[0, 85, 170, 255], 4, |_, gray| shown.push(gray));
        assert_eq!(shown, [0, 85, 170, 255]);
    }
}
