//! Waveforms made here, to try on the X4's SSD1677 (the waveform lab,
//! `shells/xteink/src/lab.rs`): what is driven at each pixel, for how many
//! frames, when the panel changes from the frame shown to the new one.
//!
//! The controller's table is [`LUT_LEN`] bytes (see `display::GRAY_LUT`,
//! the stock gray waveform, whose layout this follows):
//!
//! - five waveforms of ten bytes (`LUT0` to `LUT4`; `LUT4` is VCOM's, left
//!   alone here): a byte for each of ten phase groups, with the drive of
//!   its four phases A to D in two bits each, A in the top ones: `00`
//!   nothing, `01` black, `10` white;
//! - ten groups of five bytes: how many frames phases A to D last, and how
//!   often the group repeats;
//! - five bytes of frame rate.
//!
//! For each pixel the controller takes the waveform numbered by its bit in
//! the memory of the frame shown (high) and in that of the new frame (low),
//! a set bit being white: `LUT0` black stays black, `LUT1` black to white,
//! `LUT2` white to black, `LUT3` white stays white. That is how the gray
//! waveform reads them too.
//!
//! These waveforms change only what the table says: the voltages are the
//! stock gray waveform's (the driver sends them, not this module), the
//! frame rate is its, and no waveform drives longer than [`MAX_FRAMES`] a
//! phase. How the panel looks with them, and how long a refresh takes, is
//! what the lab is for; none of it is known without a device.

use crate::display::GRAY_LUT;

/// Bytes of the waveform table (without the voltages that follow it).
pub const LUT_LEN: usize = 105;

/// The most frames a direct drive lasts: the stock gray waveform drives
/// a level white for 21 frames at most.
pub const MAX_FRAMES: u8 = 30;

/// The most frames of the opposite colour a kick drives first.
pub const MAX_KICK: u8 = 3;

/// The most frames, the kick's too, a fast refresh may drive when it is a
/// direct drive: about as long as the panel's own fast refresh (about 500
/// ms), so that fast stays faster than the partial refresh of a page. An
/// estimate, from about 80 ms and 20 ms a frame in the owner's two
/// readings (1 frame 100 ms, 4 frames 160 ms), not a measurement of this
/// many frames.
pub const FAST_MOST_FRAMES: u8 = 20;

/// The direct drives of the fast and of the black-and-white refreshes
/// (`fast` is `None` where the panel's own waveform shows fast changes),
/// brought into the order the modes have, slowest and cleanest to fastest
/// and dirtiest: fast no longer than [`FAST_MOST_FRAMES`], black and white
/// no longer than fast.
pub const fn ordered(fast: Option<Direct>, mono: Direct) -> (Option<Direct>, Direct) {
    let fast = match fast {
        Some(drive) => Some(drive.within(FAST_MOST_FRAMES)),
        None => None,
    };
    let mono = match fast {
        Some(fast) => mono.within(fast.total_frames()),
        None => mono.within(FAST_MOST_FRAMES),
    };
    (fast, mono)
}

/// What a phase does to a pixel.
const BLACK: u8 = 0b01;
const WHITE: u8 = 0b10;

/// Where the parts of the table begin.
const VS: usize = 0;
const TIMING: usize = 50;
/// The waveforms of the pixels that change: black to white, white to black.
const BLACK_TO_WHITE: usize = 1;
const WHITE_TO_BLACK: usize = 2;

/// A direct drive: a pixel that changes is driven to its new colour for
/// `frames` frames, after `kick` frames of the other colour (which starts
/// the particles moving); a pixel that does not change is left alone.
/// Black and white drive the same time, so the drive is balanced between
/// the two directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Direct {
    frames: u8,
    kick: u8,
}

impl Direct {
    /// `frames` frames (at least 1, at most [`MAX_FRAMES`]) after `kick`
    /// frames of the other colour (at most [`MAX_KICK`]); what is more or
    /// less is taken as the nearest.
    pub const fn new(frames: u8, kick: u8) -> Self {
        let frames = if frames < 1 {
            1
        } else if frames > MAX_FRAMES {
            MAX_FRAMES
        } else {
            frames
        };
        let kick = if kick > MAX_KICK { MAX_KICK } else { kick };
        Self { frames, kick }
    }

    pub const fn frames(&self) -> u8 {
        self.frames
    }

    pub const fn kick(&self) -> u8 {
        self.kick
    }

    /// The drive cut to `most` frames in all (at least 1), the kick's
    /// too: the kick first, if it takes more than all but one.
    pub const fn within(&self, most: u8) -> Self {
        let most = if most < 1 { 1 } else { most };
        let kick = if self.kick > most - 1 {
            most - 1
        } else {
            self.kick
        };
        let room = most - kick;
        let frames = if self.frames > room {
            room
        } else {
            self.frames
        };
        Self { frames, kick }
    }

    /// Frames the drive lasts in all, the kick's too.
    pub const fn total_frames(&self) -> u8 {
        self.frames + self.kick
    }

    /// The waveform table: the stock gray waveform's, with everything but
    /// the changing pixels' first phase group cleared.
    pub fn lut(&self) -> [u8; LUT_LEN] {
        let mut lut = [0u8; LUT_LEN];
        lut.copy_from_slice(&GRAY_LUT[..LUT_LEN]);
        // No drive in any phase of any waveform, and no frames in the
        // groups the gray waveform uses; its other groups, its frame rate
        // stay as they are.
        lut[VS..VS + 50].fill(0);
        lut[TIMING..TIMING + 15].fill(0);
        // Phase A the kick (where there is one), phase B the drive.
        let phases = |kick: u8, drive: u8| (kick << 6) | (drive << 4);
        let (kick_black, kick_white) = match self.kick {
            0 => (0, 0),
            _ => (BLACK, WHITE),
        };
        lut[BLACK_TO_WHITE * 10] = phases(kick_black, WHITE);
        lut[WHITE_TO_BLACK * 10] = phases(kick_white, BLACK);
        lut[TIMING] = self.kick;
        lut[TIMING + 1] = self.frames;
        lut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The drive of phase `phase` (0 is A) of waveform `lut` in group 0.
    fn drive(table: &[u8; LUT_LEN], lut: usize, phase: usize) -> u8 {
        (table[VS + lut * 10] >> (6 - 2 * phase)) & 0b11
    }

    #[test]
    fn a_drive_is_cut_to_its_most_frames_the_kick_first() {
        let cut = Direct::new(10, 3).within(6);
        assert_eq!((cut.frames(), cut.kick()), (3, 3));
        let cut = Direct::new(10, 3).within(2);
        assert_eq!((cut.frames(), cut.kick()), (1, 1));
        let cut = Direct::new(10, 0).within(0);
        assert_eq!((cut.frames(), cut.kick()), (1, 0));
        let same = Direct::new(4, 1).within(8);
        assert_eq!((same.frames(), same.kick()), (4, 1));
    }

    #[test]
    fn modes_keep_their_order() {
        // Fast is cut; black and white never takes longer than fast.
        let (fast, mono) = ordered(Some(Direct::new(30, 3)), Direct::new(30, 0));
        assert_eq!(fast.unwrap().total_frames(), FAST_MOST_FRAMES);
        assert_eq!(mono.total_frames(), FAST_MOST_FRAMES);
        let (fast, mono) = ordered(Some(Direct::new(2, 0)), Direct::new(5, 1));
        assert_eq!(fast.unwrap().total_frames(), 2);
        assert_eq!(mono.total_frames(), 2);
        // The panel's own for fast: black and white still stays short.
        let (fast, mono) = ordered(None, Direct::new(30, 3));
        assert!(fast.is_none());
        assert_eq!(mono.total_frames(), FAST_MOST_FRAMES);
        let (_, mono) = ordered(None, Direct::new(1, 0));
        assert_eq!((mono.frames(), mono.kick()), (1, 0));
    }

    #[test]
    fn pixels_that_do_not_change_are_not_driven() {
        let table = Direct::new(8, 2).lut();
        for lut in [0, 3, 4] {
            assert!(
                table[VS + lut * 10..VS + lut * 10 + 10]
                    .iter()
                    .all(|&b| b == 0),
                "waveform {lut}"
            );
        }
        // Only group 0 of the two that change has a drive.
        for lut in [BLACK_TO_WHITE, WHITE_TO_BLACK] {
            assert!(
                table[VS + lut * 10 + 1..VS + lut * 10 + 10]
                    .iter()
                    .all(|&b| b == 0)
            );
        }
    }

    #[test]
    fn a_pixel_is_driven_to_its_new_colour() {
        let table = Direct::new(8, 0).lut();
        assert_eq!(drive(&table, BLACK_TO_WHITE, 0), 0, "no kick");
        assert_eq!(drive(&table, BLACK_TO_WHITE, 1), WHITE);
        assert_eq!(drive(&table, WHITE_TO_BLACK, 1), BLACK);
        assert_eq!((table[TIMING], table[TIMING + 1]), (0, 8));
    }

    #[test]
    fn a_kick_drives_the_other_colour_first() {
        let table = Direct::new(6, 2).lut();
        assert_eq!(drive(&table, BLACK_TO_WHITE, 0), BLACK);
        assert_eq!(drive(&table, BLACK_TO_WHITE, 1), WHITE);
        assert_eq!(drive(&table, WHITE_TO_BLACK, 0), WHITE);
        assert_eq!(drive(&table, WHITE_TO_BLACK, 1), BLACK);
        assert_eq!((table[TIMING], table[TIMING + 1]), (2, 6));
        assert_eq!(Direct::new(6, 2).total_frames(), 8);
    }

    #[test]
    fn both_directions_drive_alike() {
        // The same frames of black and of white, so the drive is balanced.
        for (frames, kick) in [(1, 0), (8, 0), (8, 3), (30, 3)] {
            let table = Direct::new(frames, kick).lut();
            let sum = |lut: usize, color: u8| -> u32 {
                (0..2)
                    .filter(|&p| drive(&table, lut, p) == color)
                    .map(|p| u32::from(table[TIMING + p]))
                    .sum()
            };
            assert_eq!(
                sum(BLACK_TO_WHITE, WHITE) + sum(BLACK_TO_WHITE, BLACK),
                sum(WHITE_TO_BLACK, WHITE) + sum(WHITE_TO_BLACK, BLACK)
            );
            assert_eq!(
                sum(BLACK_TO_WHITE, WHITE),
                sum(WHITE_TO_BLACK, BLACK),
                "{frames}+{kick}"
            );
            assert_eq!(
                sum(BLACK_TO_WHITE, BLACK),
                sum(WHITE_TO_BLACK, WHITE),
                "{frames}+{kick}"
            );
        }
    }

    #[test]
    fn what_is_asked_is_kept_within_bounds() {
        assert_eq!(Direct::new(0, 0).frames(), 1);
        assert_eq!(Direct::new(200, 9).frames(), MAX_FRAMES);
        assert_eq!(Direct::new(200, 9).kick(), MAX_KICK);
        // No phase is longer than the bound, in any table.
        for frames in 0..=255u8 {
            let table = Direct::new(frames, 255).lut();
            assert!(
                table[TIMING..TIMING + 15]
                    .iter()
                    .enumerate()
                    .all(|(i, &f)| i % 5 == 4 || f <= MAX_FRAMES)
            );
        }
    }

    #[test]
    fn the_rest_of_the_table_is_the_stock_gray_waveform() {
        let table = Direct::new(8, 1).lut();
        assert_eq!(table[TIMING + 15..], GRAY_LUT[TIMING + 15..LUT_LEN]);
        assert_eq!(table.len(), LUT_LEN);
    }
}
