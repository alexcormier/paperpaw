//! Any bytes as a JPEG, PNG or GIF, decoded at a size and in gray or
//! colour as the input chooses; the sink checks that every row has the
//! announced width and that no more rows come than announced.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_image::{Fit, Format, ImageError, ImageSink, decode, dimensions};

struct Check {
    color: bool,
    width: usize,
    rows_left: u32,
}

impl Check {
    fn take_row(&mut self) {
        assert!(self.rows_left > 0, "more rows than announced");
        self.rows_left -= 1;
    }
}

impl ImageSink for Check {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        assert!(width > 0 && height > 0);
        self.width = width as usize;
        self.rows_left = height;
        Ok(())
    }

    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        assert_eq!(gray.len(), self.width);
        self.take_row();
        Ok(())
    }

    fn wants_color(&self) -> bool {
        self.color
    }

    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        assert_eq!(rgb.len(), self.width * 3);
        self.take_row();
        Ok(())
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&[short, long, flags], data)) = data.split_first_chunk::<3>() else {
        return;
    };
    let _ = dimensions(data);
    let fit = Fit::screen(u32::from(short).max(1) * 4, u32::from(long).max(1) * 4);
    for format in [Format::Jpeg, Format::Png, Format::Gif] {
        let mut sink = Check {
            color: flags & 1 != 0,
            width: 0,
            rows_left: 0,
        };
        let mut source = data;
        let _ = decode(format, &mut source, fit, &mut sink);
    }
});
