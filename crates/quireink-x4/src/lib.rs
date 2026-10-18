//! The Xteink X4's hardware, for QuireInk's firmware: its display, its
//! microSD card, its buttons and its battery. Written against
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
pub mod buttons;
pub mod card;
pub mod display;
pub mod remote;
pub mod sent;
pub mod sntp;
