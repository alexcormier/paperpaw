//! What happens when the firmware panics: it says why in its log, keeps
//! the message through the reset (for `boot` over USB: the log may not
//! reach the computer, e.g. when the radio took the USB link along) and
//! starts the X4 again, where halting would leave it on with nothing to
//! be done until a watchdog came.

use core::fmt::Write;

/// The last panic's message (zero-terminated), kept through the reset.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut LAST: [u8; 400] = [0; 400];

/// Writes into a fixed buffer, cutting what does not fit.
struct Cut<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl Write for Cut<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if self.len + 1 < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    esp_println::println!("PANIC: {info}");
    let mut buf = [0u8; 400];
    let mut cut = Cut {
        buf: &mut buf,
        len: 0,
    };
    // The message first: the location's path is long.
    let _ = write!(cut, "{}", info.message());
    if let Some(at) = info.location() {
        let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
        let _ = write!(cut, " ({file}:{})", at.line());
    }
    // SAFETY: one core, the firmware stopped: nothing else touches it.
    unsafe { core::ptr::addr_of_mut!(LAST).write_volatile(buf) };
    esp_hal::system::software_reset()
}

/// The last panic's message, if the last run ended in one; cleared.
pub fn take_last() -> Option<alloc::string::String> {
    // SAFETY: as in `panic`; read once at start or over USB.
    let buf = unsafe { core::ptr::addr_of!(LAST).read_volatile() };
    unsafe { core::ptr::addr_of_mut!(LAST).write_volatile([0; 400]) };
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    // Unset memory after a power-up is noise: printable text only.
    let text = core::str::from_utf8(&buf[..len]).ok()?;
    (!text.is_empty() && text.chars().all(|c| !c.is_control() || c == '\n'))
        .then(|| alloc::string::String::from(text))
}
