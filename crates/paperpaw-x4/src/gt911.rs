//! The X4 Pro's touch screen: a GT911 controller on I²C (address 0x5D,
//! chosen by the interrupt line held low while it comes out of reset).
//!
//! Its report: a status byte (0x814E; the top bit set when there is a new
//! report, the low four bits the number of touches), then 8 bytes for
//! each touch from 0x814F (an id, x and y as 16-bit little endian
//! numbers, a size). A status with 0x10 set says the key below the
//! screen is touched. The status is cleared once read, for the next
//! report. Its coordinates are those of its own frame (480 × 800 on the
//! X4 Pro, read from 0x8146), which [`Transform`] turns into the screen's.

use embedded_hal::i2c::I2c;

/// The address it answers at, as reset on the X4 Pro.
pub const ADDRESS: u8 = 0x5D;

const PRODUCT_ID: u16 = 0x8140;
const RESOLUTION: u16 = 0x8146;
const STATUS: u16 = 0x814E;
const POINTS: u16 = 0x814F;

/// Touches reported at once, at most.
pub const MAX_POINTS: usize = 5;

/// A finger on the screen, in the controller's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    I2c,
    /// Something else answers there.
    NotGt911,
}

/// Checks the controller is there (its product id is "911") and reads its
/// frame: width and height. The bus is shared (with the X4 Pro's fuel
/// gauge and clock), so it is lent to each call.
pub fn probe<I: I2c>(i2c: &mut I) -> Result<(u16, u16), Error> {
    let mut id = [0u8; 4];
    read(i2c, PRODUCT_ID, &mut id)?;
    if &id[..3] != b"911" {
        return Err(Error::NotGt911);
    }
    let mut res = [0u8; 4];
    read(i2c, RESOLUTION, &mut res)?;
    Ok((
        u16::from_le_bytes([res[0], res[1]]),
        u16::from_le_bytes([res[2], res[3]]),
    ))
}

/// A new report: how many touches went into the points, and whether the
/// key below the screen (the X4 Pro's home key) is touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub count: usize,
    pub key: bool,
}

/// The touches of a new report into `points`; `None` if there is no new
/// one (nothing moved).
pub fn touches<I: I2c>(
    i2c: &mut I,
    points: &mut [Point; MAX_POINTS],
) -> Result<Option<Report>, Error> {
    let mut status = [0u8; 1];
    read(i2c, STATUS, &mut status)?;
    if status[0] & 0x80 == 0 {
        return Ok(None);
    }
    let count = usize::from(status[0] & 0x0F).min(MAX_POINTS);
    let mut raw = [0u8; 8 * MAX_POINTS];
    if count > 0 {
        read(i2c, POINTS, &mut raw[..8 * count])?;
    }
    // Read: the next report may come.
    let [high, low] = STATUS.to_be_bytes();
    i2c.write(ADDRESS, &[high, low, 0])
        .map_err(|_| Error::I2c)?;
    Ok(Some(Report {
        count: parse(&raw[..8 * count], points),
        // A key touched, beside the fingers.
        key: status[0] & 0x10 != 0,
    }))
}

/// The touches in `raw` (8 bytes each) into `points`; how many.
fn parse(raw: &[u8], points: &mut [Point; MAX_POINTS]) -> usize {
    let mut n = 0;
    for (chunk, point) in raw.as_chunks::<8>().0.iter().zip(points.iter_mut()) {
        *point = Point {
            id: chunk[0],
            x: u16::from_le_bytes([chunk[1], chunk[2]]),
            y: u16::from_le_bytes([chunk[3], chunk[4]]),
        };
        n += 1;
    }
    n
}

fn read<I: I2c>(i2c: &mut I, register: u16, out: &mut [u8]) -> Result<(), Error> {
    i2c.write_read(ADDRESS, &register.to_be_bytes(), out)
        .map_err(|_| Error::I2c)
}

/// How the controller's frame lies on the screen: its coordinates turned
/// into the screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    /// The controller's frame.
    pub from: (u16, u16),
    /// The screen's size.
    pub to: (u16, u16),
    /// Its x is the screen's y.
    pub swap: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl Transform {
    /// The screen's pixel under the controller's `(x, y)`.
    pub fn apply(&self, x: u16, y: u16) -> (u16, u16) {
        let (x, y, fw, fh) = if self.swap {
            (y, x, self.from.1, self.from.0)
        } else {
            (x, y, self.from.0, self.from.1)
        };
        let scale = |v: u16, from: u16, to: u16| -> u16 {
            if from == 0 {
                return 0;
            }
            (u32::from(v.min(from - 1)) * u32::from(to) / u32::from(from)) as u16
        };
        let mut sx = scale(x, fw, self.to.0);
        let mut sy = scale(y, fh, self.to.1);
        if self.mirror_x {
            sx = self.to.0.saturating_sub(1 + sx);
        }
        if self.mirror_y {
            sy = self.to.1.saturating_sub(1 + sy);
        }
        (sx, sy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_read() {
        let raw = [1, 0x10, 0x00, 0x20, 0x03, 9, 0, 0];
        let mut points = [Point { id: 0, x: 0, y: 0 }; MAX_POINTS];
        assert_eq!(parse(&raw, &mut points), 1);
        assert_eq!(
            points[0],
            Point {
                id: 1,
                x: 16,
                y: 800
            }
        );
    }

    #[test]
    fn coordinates_turn() {
        let t = Transform {
            from: (480, 800),
            to: (480, 800),
            swap: false,
            mirror_x: true,
            mirror_y: false,
        };
        assert_eq!(t.apply(0, 0), (479, 0));
        let swapped = Transform {
            from: (480, 800),
            to: (800, 480),
            swap: true,
            mirror_x: false,
            mirror_y: true,
        };
        assert_eq!(swapped.apply(0, 0), (0, 479));
        assert_eq!(swapped.apply(479, 799), (799, 0));
    }
}
