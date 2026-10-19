//! The Xteink X4's and X4 Pro's hardware, for QuireInk's firmware: their
//! displays (the X4's SSD1677 in [`display`], the X4 Pro's UC8179 in
//! [`uc8179`]), the X4's microSD card, buttons and battery, and the X4
//! Pro's touch screen ([`gt911`]), fuel gauge ([`cw2017`]) and clock
//! ([`bm8563`]). Written against
//! `embedded-hal`, so all of it is tested on a computer, against
//! simulated hardware; the firmware only connects the ESP32-C3's pins to
//! it (which pin does what: the firmware's `board` module,
//! `shells/xteink/src/board.rs`). Also what any X4 firmware does alike
//! and can be tested here: remote control over USB ([`remote`]), the
//! frame kept for the panel ([`sent`]) and the clock set over the network
//! ([`sntp`]).
//!
//! QuireInk draws the screen upright, 480 × 800, in bands of rows; the
//! display writes them turned into the controller's memory, which keeps
//! the frame, so the firmware holds none (see [`display`]).

#![no_std]

extern crate alloc;

pub mod battery;
pub mod bm8563;
pub mod buttons;
pub mod card;
pub mod cw2017;
pub mod display;
pub mod gt911;
pub mod remote;
pub mod sent;
pub mod sntp;
pub mod uc8179;
