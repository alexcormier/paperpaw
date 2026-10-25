//! Keys from Bluetooth keyboards and remotes: decodes the input reports of
//! HID over GATT into presses and releases of [`Key`]s, whose
//! [names](Key::name) are the button inputs the key maps see (ADR 0027).
//!
//! A [`ReportMap`] reads the device's report descriptor, a [`Decoder`]
//! turns each raw report into [`KeyChange`]s. No I/O: the caller reads the
//! reports and the report map from the device.

#![no_std]

extern crate alloc;

mod decoder;
mod key;
mod map;

#[cfg(test)]
mod tests;

pub use decoder::{Decoder, KeyChange};
pub use key::{ALL, Key};
pub use map::ReportMap;

/// Why a report descriptor was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidError {
    /// The descriptor ends inside an item.
    Truncated,
    /// Something that cannot be read: an unbalanced `Pop`, a report id
    /// above 255, a key field wider than 32 bits.
    Unsupported,
    /// More than the decoder keeps: a report over 1 KB, `Push` nested
    /// deeper than 4, an array of over 65535 slots.
    TooLarge,
}

impl core::fmt::Display for HidError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            HidError::Truncated => "report descriptor ends inside an item",
            HidError::Unsupported => "report descriptor not supported",
            HidError::TooLarge => "report descriptor too large",
        })
    }
}

impl core::error::Error for HidError {}
