//! Pictures made small ([`Thumbnail`]): scaled into the box asked for and
//! never larger than the picture, drawn inside that box only, kept on
//! the storage and read back alike, and kept files that are broken (cut
//! short, another format, a header claiming a huge picture) read as no
//! thumbnail without a panic and without taking memory for what is not
//! there.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_image::{ColorWriter, GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::ui::{self, Thumbnail, fit_image};
use paperpaw_sdk::{MemoryStorage, PixelFormat, Rect, Storage};
use proptest::prelude::*;

use support::{drawn_outside, frame};

/// The box thumbnails are asked for here: a screen's size.
const BOX: (u16, u16) = (480, 800);

/// The system's allocator, noting the largest block each thread asked
/// for (to see what reading a broken file takes).
struct Noting;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for Noting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = LARGEST.try_with(|l| l.set(l.get().max(layout.size())));
        // SAFETY: as the system allocator, with the same layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: allocated by the system allocator with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Noting = Noting;

/// The largest block `f` asked for on this thread.
fn largest_block<R>(f: impl FnOnce() -> R) -> (R, usize) {
    LARGEST.with(|l| l.set(0));
    let result = f();
    (result, LARGEST.with(Cell::get))
}

/// A picture `width` × `height` on `storage`: a gradient, in gray or in
/// colour.
fn picture(storage: &MemoryStorage, width: u32, height: u32, color: bool) -> ImageRef {
    let path = if color { "p.rgb" } else { "p.gray" };
    if color {
        let mut writer = ColorWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width * 3).map(|x| ((x + y) % 256) as u8).collect();
            writer.row_rgb(&row).unwrap();
        }
        writer.finish().unwrap();
    } else {
        let mut writer = GrayWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).map(|x| ((x * 7 + y) % 256) as u8).collect();
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

fn format_strategy() -> impl Strategy<Value = PixelFormat> {
    proptest::sample::select(vec![
        PixelFormat::Mono1,
        PixelFormat::Gray4,
        PixelFormat::Gray8,
    ])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Made to fit a box, a thumbnail is no larger than the box nor the
    /// picture, keeps the picture's proportions (to a pixel), and is
    /// drawn inside it only.
    #[test]
    fn thumbnails_fit_their_box_and_draw_inside_it(
        source in (1u32..240, 1u32..240),
        color in any::<bool>(),
        box_size in (1u16..200, 1u16..200),
        format in format_strategy(),
        at in (0u16..200, 0u16..200),
    ) {
        let storage = MemoryStorage::new();
        let image = picture(&storage, source.0, source.1, color);
        let (w, h) = fit_image(&image, box_size.0, box_size.1);
        prop_assert!(w <= box_size.0 && h <= box_size.1);
        let Some(thumb) = Thumbnail::new(&storage, &image, w, h, format) else {
            // Nothing to make of a box less than a pixel high or wide.
            prop_assert!(w == 0 || h == 0);
            return Ok(());
        };
        prop_assert!(thumb.width() <= w.max(1) && thumb.height() <= h.max(1));
        prop_assert!(u32::from(thumb.width()) <= source.0);
        prop_assert!(u32::from(thumb.height()) <= source.1);
        let mut frame = frame(420, 420);
        thumb.draw(&mut frame.canvas(), at.0, at.1);
        let bound = Rect::new(at.0, at.1, thumb.width(), thumb.height());
        prop_assert_eq!(drawn_outside(&mut frame, bound), None);
    }

    /// Kept and read again with its stamp, a thumbnail is the same;
    /// another stamp reads none.
    #[test]
    fn kept_thumbnails_read_back_alike(
        source in (1u32..120, 1u32..120),
        format in format_strategy(),
        stamp in any::<u64>(),
    ) {
        let storage = MemoryStorage::new();
        let image = picture(&storage, source.0, source.1, false);
        let made = Thumbnail::new(&storage, &image, 40, 40, format).unwrap();
        prop_assert!(made.keep(&storage, "t.thumb", stamp));
        let read = Thumbnail::kept(&storage, "t.thumb", stamp, BOX).unwrap();
        prop_assert_eq!((read.width(), read.height(), read.bytes()), (made.width(), made.height(), made.bytes()));
        let mut a = frame(60, 60);
        made.draw(&mut a.canvas(), 5, 5);
        let mut b = frame(60, 60);
        read.draw(&mut b.canvas(), 5, 5);
        prop_assert!(a.view().pixel(10, 10) == b.view().pixel(10, 10));
        prop_assert!(Thumbnail::kept(&storage, "t.thumb", stamp.wrapping_add(1), BOX).is_none());
    }

    /// Any bytes at all where a thumbnail was kept: read as none or as a
    /// thumbnail of the size its header says, never a panic.
    #[test]
    fn any_bytes_kept_read_without_a_panic(
        header in proptest::collection::vec(any::<u8>(), 0..17),
        dims in (0u16..64, 0u16..64),
        bits in any::<u8>(),
        rest in proptest::collection::vec(any::<u8>(), 0..2000),
    ) {
        let storage = MemoryStorage::new();
        // Random bytes, and bytes in the format with random sizes.
        storage.write("random.thumb", &header).unwrap();
        prop_assert!(Thumbnail::kept(&storage, "random.thumb", 0, BOX).is_none() || header.len() >= 17);
        let mut data = b"QIT3".to_vec();
        data.extend_from_slice(&dims.0.to_le_bytes());
        data.extend_from_slice(&dims.1.to_le_bytes());
        data.push(bits);
        data.extend_from_slice(&7u64.to_le_bytes());
        data.extend_from_slice(&rest);
        storage.write("sized.thumb", &data).unwrap();
        if let Some(thumb) = Thumbnail::kept(&storage, "sized.thumb", 7, BOX) {
            prop_assert_eq!((thumb.width(), thumb.height()), dims);
            prop_assert!(thumb.bytes() <= rest.len());
            let mut frame = frame(80, 80);
            thumb.draw(&mut frame.canvas(), 3, 3);
            let bound = Rect::new(3, 3, dims.0, dims.1);
            prop_assert_eq!(drawn_outside(&mut frame, bound), None);
        }
    }
}

#[test]
fn boxes_of_nothing_make_a_pixel_at_most() {
    let storage = MemoryStorage::new();
    let image = picture(&storage, 50, 30, false);
    for (w, h) in [(0, 0), (0, 10), (10, 0)] {
        if let Some(thumb) = Thumbnail::new(&storage, &image, w, h, PixelFormat::Gray8) {
            assert!(thumb.width() <= w.max(1), "{w}x{h}: {}", thumb.width());
            assert!(thumb.height() <= h.max(1), "{w}x{h}: {}", thumb.height());
        }
    }
    let empty = ImageRef {
        path: "p.gray".into(),
        width: 0,
        height: 0,
    };
    assert_eq!(fit_image(&empty, 100, 100), (0, 0));
}

#[test]
fn proportions_are_kept_when_fitting_a_box() {
    let wide = ImageRef {
        path: String::new(),
        width: 400,
        height: 100,
    };
    assert_eq!(fit_image(&wide, 200, 200), (200, 50));
    let tall = ImageRef {
        path: String::new(),
        width: 100,
        height: 400,
    };
    assert_eq!(fit_image(&tall, 200, 200), (50, 200));
    // Huge pictures in large boxes: no overflow.
    let huge = ImageRef {
        path: String::new(),
        width: u32::MAX,
        height: 1,
    };
    assert_eq!(fit_image(&huge, u16::MAX, u16::MAX), (u16::MAX, 0));
}

/// A kept thumbnail whose header claims a picture far larger than the
/// file (a file cut short, or another program's): read as none, without
/// first taking the memory the header claims (a 4096 × 4096 gray one,
/// 8 MB, for a file of 17 bytes; on a phone a header can claim
/// gigabytes).
#[test]
fn a_header_claiming_a_huge_picture_takes_no_memory_for_it() {
    let storage = MemoryStorage::new();
    let mut data = b"QIT3".to_vec();
    data.extend_from_slice(&4096u16.to_le_bytes());
    data.extend_from_slice(&4096u16.to_le_bytes());
    data.push(4);
    data.extend_from_slice(&1u64.to_le_bytes());
    storage.write("big.thumb", &data).unwrap();
    let (thumb, largest) = largest_block(|| Thumbnail::kept(&storage, "big.thumb", 1, BOX));
    assert!(thumb.is_none());
    assert!(
        largest < 64 * 1024,
        "{largest} bytes taken for a file of 17"
    );
}

#[test]
fn a_kept_file_cut_short_or_of_another_kind_reads_as_none() {
    let storage = MemoryStorage::new();
    let image = picture(&storage, 80, 60, false);
    let made = Thumbnail::new(&storage, &image, 40, 30, PixelFormat::Gray4).unwrap();
    assert!(made.keep(&storage, "a.thumb", 9));
    let whole = paperpaw_sdk::read_file(&storage, "a.thumb", 1 << 20).unwrap();
    // Cut short by a byte: none.
    storage.write("a.thumb", &whole[..whole.len() - 1]).unwrap();
    assert!(Thumbnail::kept(&storage, "a.thumb", 9, BOX).is_none());
    // Bits a pixel no thumbnail has.
    let mut other = whole.clone();
    other[8] = 8;
    storage.write("a.thumb", &other).unwrap();
    assert!(Thumbnail::kept(&storage, "a.thumb", 9, BOX).is_none());
    // An older format.
    let mut old = whole.clone();
    old[3] = b'1';
    storage.write("a.thumb", &old).unwrap();
    assert!(Thumbnail::kept(&storage, "a.thumb", 9, BOX).is_none());
    // More than it needs: read as it was kept.
    let mut longer = whole.clone();
    longer.extend_from_slice(b"more");
    storage.write("a.thumb", &longer).unwrap();
    let read = Thumbnail::kept(&storage, "a.thumb", 9, BOX).unwrap();
    assert_eq!(read.bytes(), made.bytes());
    // A picture that is gone: no thumbnail.
    let gone = ImageRef {
        path: "gone.gray".into(),
        width: 10,
        height: 10,
    };
    assert!(Thumbnail::new(&storage, &gone, 5, 5, PixelFormat::Gray8).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// A picture drawn from its file is scaled into its area, filling it
    /// and drawing nowhere else; in a canvas clipped to part of it, that
    /// part only.
    #[test]
    fn pictures_are_drawn_into_their_area_only(
        source in (1u32..200, 1u32..200),
        color in any::<bool>(),
        area in (0u16..100, 0u16..100, 1u16..250, 1u16..250),
        clip in (0u16..300, 0u16..300, 0u16..300, 0u16..300),
    ) {
        let storage = MemoryStorage::new();
        let image = picture(&storage, source.0, source.1, color);
        let area = Rect::new(area.0, area.1, area.2, area.3);
        let mut frame = frame(360, 360);
        ui::draw_image(&mut frame.canvas(), &storage, &image, area);
        prop_assert!(support::drawn_in(&frame, area));
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
        let clip = Rect::new(clip.0, clip.1, clip.2, clip.3);
        let mut frame = support::frame(360, 360);
        ui::draw_image(&mut frame.canvas().clipped(clip), &storage, &image, area);
        let both = area.intersection(&clip).unwrap_or_default();
        prop_assert_eq!(drawn_outside(&mut frame, both), None);
    }
}
