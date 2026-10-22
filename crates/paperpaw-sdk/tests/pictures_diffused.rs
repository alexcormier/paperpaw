//! Pictures diffused into a frame's few grays ([`ui::draw_image`]): the
//! same pixels whether the frame is drawn whole, in bands, each band more
//! than once, or only a part of it again (a notice closing over the
//! picture); colour pictures as their grays.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_graphics::Framebuffer;
use paperpaw_image::{ColorWriter, GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{Color, MemoryStorage, PixelFormat, Rect, ui};

const W: u16 = 120;
const H: u16 = 90;

/// A picture of `width` × `height` with gradients both ways, gray or in
/// colour.
fn picture(storage: &MemoryStorage, width: u32, height: u32, color: bool) -> ImageRef {
    let gray = |x: u32, y: u32| ((x * 255 / (width - 1) + y * 3) % 256) as u8;
    let path = if color { "p.rgb" } else { "p.gray" };
    if color {
        let mut writer = ColorWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).flat_map(|x| [gray(x, y); 3]).collect();
            writer.row_rgb(&row).unwrap();
        }
        writer.finish().unwrap();
    } else {
        let mut writer = GrayWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).map(|x| gray(x, y)).collect();
            writer.row(&row).unwrap();
        }
        writer.finish().unwrap();
    }
    ImageRef {
        path: path.into(),
        width,
        height,
    }
}

/// Where the picture goes: partly off the frame's top, scaled up.
const AREA: Rect = Rect::new(7, 0, 100, 80);

fn draw(frame: &mut Framebuffer, storage: &MemoryStorage, image: &ImageRef) {
    ui::draw_image(&mut frame.canvas(), storage, image, AREA);
}

#[test]
fn bands_and_parts_drawn_again_show_the_same_pixels() {
    for format in [PixelFormat::Mono1, PixelFormat::Gray2] {
        for color in [false, true] {
            let storage = MemoryStorage::new();
            let image = picture(&storage, 61, 47, color);
            let mut whole = Framebuffer::new(W, H, format).unwrap();
            draw(&mut whole, &storage, &image);

            // In bands of 7 rows, each drawn twice.
            let mut joined = Framebuffer::new(W, H, format).unwrap();
            let mut band = Framebuffer::banded(W, H, 7, format).unwrap();
            for top in (0..H).step_by(7) {
                band.set_top(top);
                for _ in 0..2 {
                    band.canvas().clear(Color::WHITE);
                    draw(&mut band, &storage, &image);
                }
                joined.write_rows(top, band.view());
            }
            assert_eq!(
                whole.view().data,
                joined.view().data,
                "{format:?} {color}: bands"
            );

            // A part drawn again, as where a notice over it closed.
            let part = Rect::new(30, 33, 41, 19);
            let mut again = Framebuffer::new(W, H, format).unwrap();
            draw(&mut again, &storage, &image);
            again.canvas().clipped(part).fill(part, Color::BLACK);
            {
                let mut canvas = again.canvas();
                let mut clipped = canvas.clipped(part);
                clipped.clear(Color::WHITE);
                ui::draw_image(&mut clipped, &storage, &image, AREA);
            }
            assert_eq!(
                whole.view().data,
                again.view().data,
                "{format:?} {color}: a part again"
            );
        }
    }
}

#[test]
fn a_flat_gray_is_diffused_not_a_grid() {
    // A dark gray in black and white: about a third of the pixels white
    // (a little fewer: the diffusion darkens dark grays), and not the
    // same 4 × 4 tile all over, as an ordered dither has it.
    let storage = MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "g.gray");
    writer.start(64, 64).unwrap();
    for _ in 0..64 {
        writer.row(&[90; 64]).unwrap();
    }
    writer.finish().unwrap();
    let image = ImageRef {
        path: "g.gray".into(),
        width: 64,
        height: 64,
    };
    let mut frame = Framebuffer::new(64, 64, PixelFormat::Mono1).unwrap();
    ui::draw_image(
        &mut frame.canvas(),
        &storage,
        &image,
        Rect::new(0, 0, 64, 64),
    );
    let white = |x: u16, y: u16| frame.view().pixel(x, y) == Color::WHITE;
    let count = (0..64u16)
        .flat_map(|y| (0..64u16).map(move |x| (x, y)))
        .filter(|&(x, y)| white(x, y))
        .count();
    assert!((1000..=1600).contains(&count), "{count} of 4096 white");
    let tile = |tx: u16, ty: u16| -> Vec<bool> {
        (0..4u16)
            .flat_map(|y| (0..4u16).map(move |x| (x, y)))
            .map(|(x, y)| white(tx * 4 + x, ty * 4 + y))
            .collect()
    };
    let first = tile(0, 0);
    assert!(
        (0..16)
            .flat_map(|ty| (0..16).map(move |tx| (tx, ty)))
            .any(|(tx, ty)| tile(tx, ty) != first)
    );
}
