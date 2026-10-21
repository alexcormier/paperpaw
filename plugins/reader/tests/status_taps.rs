//! Status items that do something when tapped: a tap on one in a status
//! line does it; a tap beside it shows the next line, as before.

// Tests panic on purpose when something is wrong.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use paperpaw_sdk::Point;

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The recorder's item alone at the bottom left.
const CONFIG: &str = "[reader]\nstatus.bottom-left = recorder.status\nstatus.bottom-center =\nstatus.bottom-right =\n";

#[test]
fn a_tapped_status_item_does_what_it_says() {
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/alice.epub", ALICE, CONFIG);
    reading.open();
    let height = i32::from(TOUCH.display.height);
    let width = i32::from(TOUCH.display.width);
    // Beside the item: the next line, nothing opened.
    reading.tap(Point::new(width - 40, height - 4));
    assert!(reading.heard.borrow().slots.is_empty());
    // Back to the line of items, then on the item.
    reading.tap(Point::new(width - 40, height - 4));
    reading.tap(Point::new(width - 40, height - 4));
    reading.tap(Point::new(40, height - 4));
    let slots = reading.heard.borrow().slots.clone();
    assert_eq!(slots.len(), 1, "{slots:?}");
    assert_eq!(slots[0].0, "book-info");
    assert_eq!(slots[0].1.get("from"), Some("status"));
}
