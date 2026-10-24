//! The heap's peak since start, for test builds (`remote`): esp-alloc's
//! own (`max_usage`) counts what [`crate::largest_block`] takes to find
//! the largest free block, nearly all that is free, so every look at it
//! raised the peak to what was used then and that block. This one leaves
//! those allocations out (esp-alloc's `alloc-hooks`).

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};

use critical_section::Mutex;
use esp_alloc::export::enumset::EnumSet;
use esp_alloc::{EspHeap, MemoryCapability};

static PEAK: Mutex<Cell<usize>> = Mutex::new(Cell::new(0));
static PROBING: AtomicBool = AtomicBool::new(false);

/// The most the heap held since start, the probes left out.
pub fn peak() -> usize {
    critical_section::with(|cs| PEAK.borrow(cs).get())
}

/// `probe`, its allocations not counted in the peak.
pub fn probing<R>(probe: impl FnOnce() -> R) -> R {
    PROBING.store(true, Ordering::Relaxed);
    let result = probe();
    PROBING.store(false, Ordering::Relaxed);
    result
}

/// esp-alloc calls it after each allocation, outside its lock.
#[unsafe(no_mangle)]
fn _esp_alloc_alloc(heap: &EspHeap, _: EnumSet<MemoryCapability>, ptr: usize, _: usize) {
    if ptr == 0 || PROBING.load(Ordering::Relaxed) {
        return;
    }
    let used = heap.used();
    critical_section::with(|cs| {
        let peak = PEAK.borrow(cs);
        if used > peak.get() {
            peak.set(used);
        }
    });
}

/// And before each freeing: the peak needs nothing then.
#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &EspHeap, _: usize, _: usize) {}
