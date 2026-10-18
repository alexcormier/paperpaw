//! The stack's depth. It lies above everything else in RAM and grows down
//! towards it: run over, it writes into the memory below (the radio's
//! firmware state lies last) with nothing to stop it, and something fails
//! later, somewhere else. So the stack is filled with a pattern at start,
//! which shows how deep it went ([`left`]), and the lowest part is checked
//! now and then ([`check`]): overwritten, the X4 starts again saying why.

/// What the unused stack holds.
const PAINT: u32 = 0xA5A5_5A5A;
/// Bytes at the stack's end checked by [`check`].
const BAND: usize = 256;
/// Bytes left unpainted below the stack pointer at start: in use then.
const MARGIN: usize = 1024;

unsafe extern "C" {
    /// The stack's lowest address (esp-hal's linker script).
    static _stack_end: u32;
    /// The stack's highest address, where it starts.
    static _stack_start: u32;
    /// esp-hal's guard word near the stack's end, watched for writes.
    static __stack_chk_guard: u32;
}

/// The lowest address painted: the word after esp-hal's guard.
fn bottom() -> usize {
    core::ptr::addr_of!(__stack_chk_guard) as usize + 4
}

fn pointer() -> usize {
    let sp: usize;
    // SAFETY: reads a register.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp) };
    sp
}

/// Fills the stack below what is in use with the pattern: once, first
/// thing at start.
pub fn paint() {
    let top = pointer() - MARGIN;
    let mut at = bottom();
    while at < top {
        // SAFETY: unused stack, below the stack pointer, above the guard.
        unsafe { (at as *mut u32).write_volatile(PAINT) };
        at += 4;
    }
}

/// Bytes of the stack never used since start (the depth left at its
/// deepest), and the stack's size.
#[cfg(feature = "remote")]
pub fn left() -> (usize, usize) {
    let mut at = bottom();
    let sp = pointer();
    // SAFETY: within the stack, below the stack pointer: not in use.
    while at < sp && unsafe { (at as *const u32).read_volatile() } == PAINT {
        at += 4;
    }
    let end = core::ptr::addr_of!(_stack_end) as usize;
    let start = core::ptr::addr_of!(_stack_start) as usize;
    (at - end, start - end)
}

/// Starts the X4 again (by a panic, which keeps the message) if the stack
/// reached its end since start.
pub fn check() {
    let reached = (bottom()..bottom() + BAND)
        .step_by(4)
        // SAFETY: as in `left`; the stack is never this deep here.
        .any(|at| unsafe { (at as *const u32).read_volatile() } != PAINT);
    if reached {
        panic!("the stack ran over");
    }
}
