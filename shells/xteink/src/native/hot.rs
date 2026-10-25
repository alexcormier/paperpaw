//! The X4's arena for hot code (ADR 0030): internal RAM, where a loop
//! that misses the flash's cache runs several times faster. It is a
//! static, so its place is the same at every start of a firmware: the
//! index of the plugins in flash records each one's part of it, and the
//! images there are relocated for it. The ESP32-C3 serves its internal
//! RAM to the instruction bus too, at the data address plus
//! [`IRAM_OFFSET`]. The heap is smaller by the arena (`HEAP_SIZE`) until
//! the plugins are loaded; then what none took goes to the heap.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use esp_alloc::{HeapRegion, MemoryCapability};
use log::{info, warn};
use paperpaw_native::index::{Arena, HOT_MAX};

/// The arena's bytes: as much as one plugin may take.
pub const LEN: usize = HOT_MAX as usize;
/// How far the instruction bus's view of internal RAM lies from the data
/// bus's.
const IRAM_OFFSET: u32 = 0x0070_0000;
/// The internal RAM both buses see, by its data addresses.
const DRAM: core::ops::Range<u32> = 0x3FC8_0000..0x3FCE_0000;
/// The least worth giving to the heap: a region of its own costs some.
const MIN_REGION: u32 = 1024;

#[repr(C, align(16))]
struct Bytes(UnsafeCell<[u8; LEN]>);

// SAFETY: written only while plugins are loaded, on one thread, before
// any code in it runs; after that only run, or given to the heap.
unsafe impl Sync for Bytes {}

static HOT_ARENA: Bytes = Bytes(UnsafeCell::new([0; LEN]));

/// The arena went to the heap: no hot code is placed in it any more.
static RELEASED: AtomicBool = AtomicBool::new(false);

/// The arena's first data address.
fn data() -> u32 {
    HOT_ARENA.0.get() as u32
}

/// The arena, as the index places hot code in it: none where it does not
/// lie in internal RAM the instruction bus sees.
pub fn arena() -> Arena {
    let start = data();
    if RELEASED.load(Ordering::Relaxed) || !DRAM.contains(&start) || start + LEN as u32 > DRAM.end {
        return Arena::new(0, 0);
    }
    Arena::new(start + IRAM_OFFSET, LEN as u32)
}

/// Copies `len` bytes of hot code, relocated, from data address `from`
/// (its image, mapped) to its part of the arena, which runs at
/// instruction address `hot`; `false` if that part is not the arena's.
///
/// # Safety
///
/// `from` holds `len` bytes to read.
pub unsafe fn copy(from: u32, hot: u32, len: u32) -> bool {
    let arena = arena();
    let fits = hot >= arena.start
        && hot
            .checked_add(len)
            .is_some_and(|end| end <= arena.start + arena.len);
    if !fits {
        return false;
    }
    let at = (hot - arena.start) as usize;
    // SAFETY: within the arena, checked above; nothing runs there yet.
    unsafe {
        core::ptr::copy_nonoverlapping(
            from as *const u8,
            HOT_ARENA.0.get().cast::<u8>().add(at),
            len as usize,
        );
        // Written to memory before any of it runs.
        core::arch::asm!("fence");
    }
    true
}

/// Gives the arena from `used` bytes on to the heap, once, when all
/// plugins are loaded (or none will be).
pub fn release(used: u32) {
    // Only the main loop's thread gets here (the chip has no swap).
    if RELEASED.load(Ordering::Relaxed) {
        return;
    }
    RELEASED.store(true, Ordering::Relaxed);
    let end = data() + LEN as u32;
    let start = (data() + used).next_multiple_of(16).min(end);
    let len = end - start;
    let room = esp_alloc::HEAP
        .stats()
        .region_stats
        .iter()
        .any(Option::is_none);
    if len < MIN_REGION || !room {
        if !room {
            warn!("native: the heap takes no more regions; {len} B of the arena stay unused");
        }
        return;
    }
    // SAFETY: the arena's tail, which no hot code was placed in and none
    // will be (`RELEASED`), given once.
    unsafe {
        esp_alloc::HEAP.add_region(HeapRegion::new(
            start as *mut u8,
            len as usize,
            MemoryCapability::Internal.into(),
        ));
    }
    info!("native: hot code takes {used} B of internal RAM; {len} B went to the heap");
}
