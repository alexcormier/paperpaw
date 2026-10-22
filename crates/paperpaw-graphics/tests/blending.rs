//! Pictures with transparency blended over what is drawn: clear pixels
//! leave it, opaque ones are drawn as gray rows are, those between are
//! the mix of both, dithered so that its lightness holds on panels of two
//! or four grays; how see-through the whole row is counts too.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::pixels;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Color, PixelFormat};

const W: u16 = 32;
const H: u16 = 32;

/// A screen of `format`, `under` everywhere, then each row of it covered
/// by `gray` as opaque as `alpha`, the row as opaque as `opacity`.
fn blended(format: PixelFormat, under: Color, gray: u8, alpha: u8, opacity: u8) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H, format).unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.clear(under);
        let row: Vec<u8> = (0..W).flat_map(|_| [gray, alpha]).collect();
        for y in 0..i32::from(H) {
            canvas.draw_gray_alpha_row(0, y, &row, opacity);
        }
    }
    fb
}

/// The mean lightness of the screen.
fn mean(fb: &Framebuffer) -> u32 {
    let all = pixels(fb);
    all.iter().map(|c| u32::from(c.luma())).sum::<u32>() / all.len() as u32
}

#[test]
fn clear_pixels_leave_what_is_drawn() {
    for format in common::formats() {
        let fb = blended(format, Color::BLACK, 255, 0, 255);
        assert!(pixels(&fb).iter().all(|c| *c == Color::BLACK), "{format:?}");
        let fb = blended(format, Color::BLACK, 255, 255, 0);
        assert!(pixels(&fb).iter().all(|c| *c == Color::BLACK), "{format:?}");
    }
}

#[test]
fn opaque_pixels_are_drawn_as_gray_rows() {
    for format in common::formats() {
        let mut fb = Framebuffer::new(W, H, format).unwrap();
        {
            let mut canvas = fb.canvas();
            canvas.clear(Color::BLACK);
            canvas.draw_gray_row(0, 0, &[100; W as usize]);
        }
        let over = blended(format, Color::BLACK, 100, 255, 255);
        let row = |fb: &Framebuffer| pixels(fb)[..usize::from(W)].to_vec();
        assert_eq!(row(&over), row(&fb), "{format:?}");
    }
}

#[test]
fn half_opaque_is_the_mix_and_dithering_keeps_its_lightness() {
    // Black half opaque over white, white half opaque over black, and a
    // quarter of a gray over white; on every panel the mean is the mix,
    // near enough.
    let cases = [
        (Color::WHITE, 0, 128, 255, 127),
        (Color::BLACK, 255, 128, 255, 128),
        (Color::WHITE, 0, 255, 64, 191),
        (Color::WHITE, 80, 255, 255, 80),
    ];
    for format in common::formats() {
        // In black and white, pictures are diffused after Atkinson, which
        // passes on seven eighths of the error: light grays come out a
        // little lighter, dark ones a little darker.
        let near = if format == PixelFormat::Mono1 { 25 } else { 10 };
        for (under, gray, alpha, opacity, expected) in cases {
            let got = mean(&blended(format, under, gray, alpha, opacity));
            assert!(
                got.abs_diff(expected) <= near,
                "{format:?}: {gray} at {alpha}/{opacity}: {got}, not {expected}"
            );
        }
    }
}

#[test]
fn on_two_grays_the_mix_is_a_pattern_of_both() {
    let fb = blended(PixelFormat::Mono1, Color::WHITE, 0, 128, 255);
    let dark = pixels(&fb).iter().filter(|c| **c == Color::BLACK).count();
    let all = usize::from(W) * usize::from(H);
    assert!(dark.abs_diff(all / 2) <= all / 16, "{dark} of {all}");
}

#[test]
fn in_night_mode_the_page_is_blended_as_it_shows() {
    // Kept pictures over an inverted page: a clear one shows the page
    // inverted; an opaque one as it is.
    for format in [PixelFormat::Gray4, PixelFormat::Gray8] {
        let mut fb = Framebuffer::new(W, H, format).unwrap();
        fb.set_inverted(true);
        fb.set_pictures_kept(true);
        {
            let mut canvas = fb.canvas();
            canvas.clear(Color::WHITE);
            canvas.draw_gray_alpha_row(0, 0, &[255, 0, 255, 255], 255);
        }
        let all = pixels(&fb);
        assert_eq!(all[0], Color::BLACK, "{format:?}: the page");
        assert_eq!(all[1], Color::WHITE, "{format:?}: the picture");
    }
}

#[test]
fn rows_are_clipped_at_the_edges() {
    let mut fb = Framebuffer::new(W, H, PixelFormat::Gray8).unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.draw_gray_alpha_row(-3, 0, &[0, 255].repeat(40), 255);
        canvas.draw_gray_alpha_row(30, 1, &[0, 255].repeat(5), 255);
        canvas.draw_gray_alpha_row(0, i32::from(H), &[0, 255], 255);
        // An odd byte at the end is no pixel.
        canvas.draw_gray_alpha_row(0, 2, &[0, 255, 0], 255);
    }
    let all = pixels(&fb);
    let w = usize::from(W);
    assert!(all[..w].iter().all(|c| *c == Color::BLACK));
    assert!(all[w..w + 30].iter().all(|c| *c == Color::WHITE));
    assert!(all[w + 30..2 * w].iter().all(|c| *c == Color::BLACK));
    assert_eq!(all[2 * w], Color::BLACK);
    assert_eq!(all[2 * w + 1], Color::WHITE);
}
