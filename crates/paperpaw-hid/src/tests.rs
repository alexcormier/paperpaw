use alloc::vec::Vec;

use crate::{ALL, Decoder, HidError, Key, KeyChange, ReportMap};

fn press(key: Key) -> KeyChange {
    KeyChange { key, pressed: true }
}

fn release(key: Key) -> KeyChange {
    KeyChange {
        key,
        pressed: false,
    }
}

fn boot(keys: [u8; 6]) -> [u8; 8] {
    let mut report = [0u8; 8];
    report[2..].copy_from_slice(&keys);
    report
}

/// What a cheap page-turner sends: a keyboard (report id 1, with LEDs)
/// and a consumer control (report id 2, four bits as a bitmap).
#[rustfmt::skip]
const COMBINED: &[u8] = &[
    0x05, 0x01,       // Usage Page (Generic Desktop)
    0x09, 0x06,       // Usage (Keyboard)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x01,       //   Report ID (1)
    0x05, 0x07,       //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0,       //   Usage Minimum (Left Control)
    0x29, 0xE7,       //   Usage Maximum (Right GUI)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x08,       //   Report Count (8)
    0x81, 0x02,       //   Input (Data, Variable, Absolute): modifiers
    0x95, 0x01,       //   Report Count (1)
    0x75, 0x08,       //   Report Size (8)
    0x81, 0x01,       //   Input (Constant): reserved byte
    0x95, 0x05,       //   Report Count (5)
    0x75, 0x01,       //   Report Size (1)
    0x05, 0x08,       //   Usage Page (LEDs)
    0x19, 0x01,       //   Usage Minimum (Num Lock)
    0x29, 0x05,       //   Usage Maximum (Kana)
    0x91, 0x02,       //   Output (Data, Variable, Absolute): LEDs
    0x95, 0x01,       //   Report Count (1)
    0x75, 0x03,       //   Report Size (3)
    0x91, 0x01,       //   Output (Constant): LED padding
    0x95, 0x06,       //   Report Count (6)
    0x75, 0x08,       //   Report Size (8)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x65,       //   Logical Maximum (101)
    0x05, 0x07,       //   Usage Page (Keyboard/Keypad)
    0x19, 0x00,       //   Usage Minimum (0)
    0x29, 0x65,       //   Usage Maximum (101)
    0x81, 0x00,       //   Input (Data, Array): six keycodes
    0xC0,             // End Collection
    0x05, 0x0C,       // Usage Page (Consumer)
    0x09, 0x01,       // Usage (Consumer Control)
    0xA1, 0x01,       // Collection (Application)
    0x85, 0x02,       //   Report ID (2)
    0x15, 0x00,       //   Logical Minimum (0)
    0x25, 0x01,       //   Logical Maximum (1)
    0x75, 0x01,       //   Report Size (1)
    0x95, 0x04,       //   Report Count (4)
    0x09, 0xE9,       //   Usage (Volume Increment)
    0x09, 0xEA,       //   Usage (Volume Decrement)
    0x09, 0xE2,       //   Usage (Mute)
    0x09, 0xCD,       //   Usage (Play/Pause)
    0x81, 0x02,       //   Input (Data, Variable, Absolute): four keys
    0x95, 0x04,       //   Report Count (4)
    0x81, 0x01,       //   Input (Constant): padding to a byte
    0xC0,             // End Collection
];

/// A consumer control with one 16-bit array, usages 0 to 0x3FF.
#[rustfmt::skip]
const CONSUMER_ARRAY: &[u8] = &[
    0x05, 0x0C,       // Usage Page (Consumer)
    0x09, 0x01,       // Usage (Consumer Control)
    0xA1, 0x01,       // Collection (Application)
    0x15, 0x00,       //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (1023)
    0x19, 0x00,       //   Usage Minimum (0)
    0x2A, 0xFF, 0x03, //   Usage Maximum (1023)
    0x75, 0x10,       //   Report Size (16)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x00,       //   Input (Data, Array)
    0xC0,             // End Collection
];

/// A consumer control whose array lists its usages, counted from 1.
#[rustfmt::skip]
const CONSUMER_LIST: &[u8] = &[
    0x05, 0x0C,       // Usage Page (Consumer)
    0x09, 0x01,       // Usage (Consumer Control)
    0xA1, 0x01,       // Collection (Application)
    0x15, 0x01,       //   Logical Minimum (1)
    0x25, 0x03,       //   Logical Maximum (3)
    0x09, 0xB5,       //   Usage (Scan Next Track)
    0x09, 0xB6,       //   Usage (Scan Previous Track)
    0x09, 0xE9,       //   Usage (Volume Increment)
    0x75, 0x08,       //   Report Size (8)
    0x95, 0x01,       //   Report Count (1)
    0x81, 0x00,       //   Input (Data, Array)
    0xC0,             // End Collection
];

fn combined() -> Decoder {
    Decoder::new(ReportMap::parse(COMBINED).unwrap())
}

#[test]
fn boot_press_and_release() {
    let mut d = Decoder::boot_keyboard();
    assert_eq!(
        d.report(None, &boot([0x4F, 0, 0, 0, 0, 0])),
        [press(Key::Right)]
    );
    assert_eq!(d.report(None, &boot([0x4F, 0, 0, 0, 0, 0])), []);
    assert_eq!(d.report(None, &boot([0; 6])), [release(Key::Right)]);
    assert_eq!(d.report(None, &boot([0; 6])), []);
}

#[test]
fn boot_two_keys() {
    let mut d = Decoder::boot_keyboard();
    assert_eq!(
        d.report(None, &boot([0x04, 0, 0, 0, 0, 0])),
        [press(Key::A)]
    );
    assert_eq!(
        d.report(None, &boot([0x04, 0x2C, 0, 0, 0, 0])),
        [press(Key::Space)]
    );
    // The first key goes up, a third comes: release before press.
    assert_eq!(
        d.report(None, &boot([0x2C, 0x27, 0, 0, 0, 0])),
        [release(Key::A), press(Key::Digit0)]
    );
    assert_eq!(d.reset(), [release(Key::Space), release(Key::Digit0)]);
    assert_eq!(d.reset(), []);
}

#[test]
fn boot_roll_over_and_unknown_keys_change_nothing() {
    let mut d = Decoder::boot_keyboard();
    d.report(None, &boot([0x4E, 0, 0, 0, 0, 0]));
    assert_eq!(d.report(None, &boot([1; 6])), []);
    assert_eq!(d.report(None, &boot([0x4E, 1, 0, 0, 0, 0])), []);
    // Still down: no release, no second press.
    assert_eq!(d.report(None, &boot([0x4E, 0, 0, 0, 0, 0])), []);
    // A modifier alone, and keys that are not known (F1, 0x3A).
    assert_eq!(
        d.report(None, &[0x02, 0, 0x3A, 0, 0, 0, 0, 0]),
        [release(Key::PageDown)]
    );
    assert_eq!(d.report(None, &boot([0x3A, 0, 0, 0, 0, 0])), []);
}

#[test]
fn boot_short_report_and_report_id_are_ignored() {
    let mut d = Decoder::boot_keyboard();
    assert_eq!(d.report(None, &[0, 0, 0x4F]), []);
    assert_eq!(d.report(None, &[]), []);
    assert_eq!(d.report(Some(1), &boot([0x4F, 0, 0, 0, 0, 0])), []);
    assert_eq!(
        d.report(Some(0), &boot([0x4F, 0, 0, 0, 0, 0])),
        [press(Key::Right)]
    );
}

#[test]
fn combined_keyboard_arrows() {
    let mut d = combined();
    let right = [0, 0, 0x4F, 0, 0, 0, 0, 0];
    assert_eq!(d.report(Some(1), &right), [press(Key::Right)]);
    assert_eq!(d.report(Some(1), &[0; 8]), [release(Key::Right)]);
    let left = [0x02, 0, 0x50, 0, 0, 0, 0, 0];
    assert_eq!(d.report(Some(1), &left), [press(Key::Left)]);
    assert_eq!(d.report(Some(1), &[0; 8]), [release(Key::Left)]);
    // Modifiers are not keys.
    assert_eq!(d.report(Some(1), &[0x01, 0, 0, 0, 0, 0, 0, 0]), []);
}

#[test]
fn combined_consumer_bitmap_volume() {
    let mut d = combined();
    assert_eq!(d.report(Some(2), &[0b0001]), [press(Key::VolumeUp)]);
    assert_eq!(d.report(Some(2), &[0b0000]), [release(Key::VolumeUp)]);
    assert_eq!(d.report(Some(2), &[0b0010]), [press(Key::VolumeDown)]);
    assert_eq!(
        d.report(Some(2), &[0b1100]),
        [
            release(Key::VolumeDown),
            press(Key::Mute),
            press(Key::PlayPause)
        ]
    );
    assert_eq!(
        d.report(Some(2), &[0]),
        [release(Key::Mute), release(Key::PlayPause)]
    );
}

#[test]
fn combined_reports_do_not_release_each_other() {
    let mut d = combined();
    d.report(Some(1), &[0, 0, 0x4F, 0, 0, 0, 0, 0]);
    // A consumer report says nothing about the keyboard's keys.
    assert_eq!(d.report(Some(2), &[0b0001]), [press(Key::VolumeUp)]);
    assert_eq!(d.report(Some(1), &[0; 8]), [release(Key::Right)]);
    assert_eq!(d.reset(), [release(Key::VolumeUp)]);
}

#[test]
fn combined_unknown_or_short_reports_change_nothing() {
    let mut d = combined();
    assert_eq!(d.report(Some(3), &[0xFF; 8]), []);
    assert_eq!(d.report(None, &[0xFF; 8]), []);
    assert_eq!(d.report(Some(1), &[0, 0, 0x4F]), []);
    assert_eq!(d.report(Some(2), &[]), []);
    assert_eq!(d.report(Some(1), &[0, 0, 1, 1, 1, 1, 1, 1]), []);
}

#[test]
fn consumer_array_of_16_bits() {
    let mut d = Decoder::new(ReportMap::parse(CONSUMER_ARRAY).unwrap());
    assert_eq!(d.report(None, &[0xE9, 0x00]), [press(Key::VolumeUp)]);
    assert_eq!(
        d.report(None, &[0xCD, 0x00]),
        [release(Key::VolumeUp), press(Key::PlayPause)]
    );
    // An usage that is not known (AC Home, 0x223), then none.
    assert_eq!(d.report(None, &[0x23, 0x02]), [release(Key::PlayPause)]);
    assert_eq!(d.report(None, &[0x00, 0x00]), []);
    assert_eq!(d.report(None, &[0xB5]), []);
    assert_eq!(d.report(None, &[0xB5, 0x00]), [press(Key::NextTrack)]);
}

#[test]
fn consumer_array_with_a_list_of_usages() {
    let mut d = Decoder::new(ReportMap::parse(CONSUMER_LIST).unwrap());
    assert_eq!(d.report(None, &[2]), [press(Key::PreviousTrack)]);
    assert_eq!(
        d.report(None, &[3]),
        [release(Key::PreviousTrack), press(Key::VolumeUp)]
    );
    // Outside the logical range: nothing.
    assert_eq!(d.report(None, &[0]), [release(Key::VolumeUp)]);
    assert_eq!(d.report(None, &[4]), []);
    assert_eq!(d.report(None, &[1]), [press(Key::NextTrack)]);
}

#[test]
fn consumer_bitmap_without_report_id() {
    #[rustfmt::skip]
    let descriptor = [
        0x05, 0x0C,       // Usage Page (Consumer)
        0x09, 0x01,       // Usage (Consumer Control)
        0xA1, 0x01,       // Collection (Application)
        0x15, 0x00,       //   Logical Minimum (0)
        0x25, 0x01,       //   Logical Maximum (1)
        0x75, 0x01,       //   Report Size (1)
        0x95, 0x03,       //   Report Count (3)
        0x09, 0xB6,       //   Usage (Scan Previous Track)
        0x09, 0xB5,       //   Usage (Scan Next Track)
        0x09, 0xCD,       //   Usage (Play/Pause)
        0x81, 0x02,       //   Input (Data, Variable)
        0x95, 0x01,       //   Report Count (1)
        0x75, 0x05,       //   Report Size (5)
        0x81, 0x03,       //   Input (Constant, Variable): padding
        0xC0,             // End Collection
    ];
    let mut d = Decoder::new(ReportMap::parse(&descriptor).unwrap());
    assert_eq!(d.report(None, &[0b001]), [press(Key::PreviousTrack)]);
    assert_eq!(
        d.report(None, &[0b110]),
        [
            release(Key::PreviousTrack),
            press(Key::NextTrack),
            press(Key::PlayPause)
        ]
    );
    assert_eq!(
        d.report(None, &[0]),
        [release(Key::NextTrack), release(Key::PlayPause)]
    );
}

#[test]
fn bits_of_other_pages_and_vendor_reports_count_for_offsets() {
    #[rustfmt::skip]
    let descriptor = [
        0x06, 0x00, 0xFF, // Usage Page (Vendor)
        0x09, 0x01,       // Usage (1)
        0x15, 0x00,       // Logical Minimum (0)
        0x26, 0xFF, 0x00, // Logical Maximum (255)
        0x75, 0x08,       // Report Size (8)
        0x95, 0x03,       // Report Count (3)
        0x81, 0x02,       // Input (Data, Variable): three vendor bytes
        0x05, 0x0C,       // Usage Page (Consumer)
        0x09, 0xE9,       // Usage (Volume Increment)
        0x95, 0x01,       // Report Count (1)
        0x75, 0x01,       // Report Size (1)
        0x81, 0x02,       // Input (Data, Variable)
    ];
    let mut d = Decoder::new(ReportMap::parse(&descriptor).unwrap());
    assert_eq!(d.report(None, &[0xFF, 0xFF, 0xFF, 0]), []);
    assert_eq!(d.report(None, &[0, 0, 0, 1]), [press(Key::VolumeUp)]);
    assert_eq!(d.report(None, &[0, 0, 0]), []);
}

#[test]
fn push_pop_and_long_items() {
    #[rustfmt::skip]
    let descriptor = [
        0xFE, 0x02, 0x10, 0xAA, 0xBB, // a long item, skipped
        0x05, 0x0C,       // Usage Page (Consumer)
        0x15, 0x00,       // Logical Minimum (0)
        0x25, 0x01,       // Logical Maximum (1)
        0xA4,             // Push
        0x05, 0x01,       // Usage Page (Generic Desktop)
        0x75, 0x08,       // Report Size (8)
        0xB4,             // Pop: the page and the size are back
        0x75, 0x01,       // Report Size (1)
        0x95, 0x01,       // Report Count (1)
        0x09, 0xEA,       // Usage (Volume Decrement)
        0x81, 0x02,       // Input (Data, Variable)
    ];
    let mut d = Decoder::new(ReportMap::parse(&descriptor).unwrap());
    assert_eq!(d.report(None, &[1]), [press(Key::VolumeDown)]);
}

#[test]
fn descriptor_errors() {
    assert_eq!(ReportMap::parse(&[0x05]), Err(HidError::Truncated));
    assert_eq!(ReportMap::parse(&[0x06, 0x00]), Err(HidError::Truncated));
    assert_eq!(
        ReportMap::parse(&[0xFE, 0x05, 0x01]),
        Err(HidError::Truncated)
    );
    assert_eq!(ReportMap::parse(&[0xB4]), Err(HidError::Unsupported));
    assert_eq!(
        ReportMap::parse(&[0x86, 0x00, 0x01]),
        Err(HidError::Unsupported)
    );
    assert_eq!(ReportMap::parse(&[0xA4; 5]), Err(HidError::TooLarge));
    // 65535 bytes in one report.
    assert_eq!(
        ReportMap::parse(&[0x75, 0x08, 0x96, 0xFF, 0xFF, 0x81, 0x02]),
        Err(HidError::TooLarge)
    );
    // A key array of 64 bits.
    assert_eq!(
        ReportMap::parse(&[
            0x05, 0x07, 0x19, 0, 0x29, 9, 0x75, 0x40, 0x95, 1, 0x81, 0x00
        ]),
        Err(HidError::Unsupported)
    );
}

#[test]
fn empty_and_useless_descriptors_decode_nothing() {
    let mut d = Decoder::new(ReportMap::parse(&[]).unwrap());
    assert_eq!(d.report(None, &[1, 2, 3]), []);
    // A mouse: buttons and movement, no keys.
    #[rustfmt::skip]
    let mouse = [
        0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00,
        0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01,
        0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05,
        0x81, 0x01, 0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x81,
        0x25, 0x7F, 0x75, 0x08, 0x95, 0x02, 0x81, 0x06, 0xC0, 0xC0,
    ];
    let mut d = Decoder::new(ReportMap::parse(&mouse).unwrap());
    assert_eq!(d.report(None, &[7, 1, 1]), []);
}

#[test]
fn truncated_descriptors_and_reports_never_panic() {
    for descriptor in [COMBINED, CONSUMER_ARRAY, CONSUMER_LIST] {
        for len in 0..=descriptor.len() {
            if let Ok(map) = ReportMap::parse(&descriptor[..len]) {
                let mut d = Decoder::new(map);
                for id in [None, Some(0), Some(1), Some(2), Some(255)] {
                    for size in 0..12 {
                        let data: Vec<u8> = (0..size).map(|i| 0xFF - i as u8).collect();
                        let _ = d.report(id, &data);
                    }
                }
                let _ = d.reset();
            }
        }
    }
}

#[test]
fn key_names() {
    assert_eq!(Key::Right.name(), "hid.right");
    assert_eq!(Key::PageDown.name(), "hid.page-down");
    assert_eq!(Key::VolumeUp.name(), "hid.volume-up");
    assert_eq!(Key::PlayPause.name(), "hid.play-pause");
    assert_eq!(Key::NextTrack.name(), "hid.next-track");
    assert_eq!(Key::PreviousTrack.name(), "hid.previous-track");
    assert_eq!(Key::A.name(), "hid.a");
    assert_eq!(Key::Z.name(), "hid.z");
    assert_eq!(Key::Digit0.name(), "hid.0");
    assert_eq!(Key::Digit9.name(), "hid.9");
    assert_eq!(ALL.len(), 19 + 26 + 10);
    for (i, key) in ALL.iter().enumerate() {
        let name = key.name();
        assert!(name.starts_with("hid."));
        assert!(
            name[4..]
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        );
        assert!(ALL[..i].iter().all(|other| other.name() != name));
    }
}

#[test]
fn keyboard_usages() {
    let usages: &[(u16, Key)] = &[
        (0x04, Key::A),
        (0x1D, Key::Z),
        (0x1E, Key::Digit1),
        (0x26, Key::Digit9),
        (0x27, Key::Digit0),
        (0x28, Key::Enter),
        (0x29, Key::Escape),
        (0x2A, Key::Backspace),
        (0x2B, Key::Tab),
        (0x2C, Key::Space),
        (0x4A, Key::Home),
        (0x4B, Key::PageUp),
        (0x4D, Key::End),
        (0x4E, Key::PageDown),
        (0x4F, Key::Right),
        (0x50, Key::Left),
        (0x51, Key::Down),
        (0x52, Key::Up),
        (0x7F, Key::Mute),
        (0x80, Key::VolumeUp),
        (0x81, Key::VolumeDown),
    ];
    let mut d = Decoder::boot_keyboard();
    for (usage, key) in usages {
        let usage = *usage as u8;
        assert_eq!(d.report(None, &boot([usage, 0, 0, 0, 0, 0])), [press(*key)]);
        assert_eq!(d.report(None, &boot([0; 6])), [release(*key)]);
    }
}
