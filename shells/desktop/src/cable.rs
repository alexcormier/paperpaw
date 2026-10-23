//! The cable transfer (USB) on standard input and output (`--cable`):
//! the emulator speaks as a device does over its cable
//! (`docs/usb-transfer.md`), so the workshop and
//! `tools/usb-transfer.py --emulator` try it without a device. Lines that
//! are not the cable's are passed over; the emulator's own words go to
//! standard error.

use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Receiver};

/// The lines that come on standard input, read on a thread of their own.
pub fn lines() -> Receiver<String> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if send.send(line).is_err() {
                break;
            }
        }
    });
    receive
}

/// Sends `lines` on standard output.
pub fn send(lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let mut out = std::io::stdout().lock();
    for line in lines {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.flush();
}
