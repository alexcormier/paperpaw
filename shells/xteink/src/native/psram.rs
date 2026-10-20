//! The X4 Pro: an image is read into PSRAM, relocated there and run from
//! there. esp-hal maps PSRAM through the MMU, whose entries serve the
//! instruction bus too: code seen at data address A runs at A +
//! `CODE_OFFSET`. No flash is written; plugins take PSRAM only, as much
//! as their images.

use core::alloc::Layout;

use esp_alloc::MemoryCapability;
use paperpaw_native::{CODE_OFFSET, HEADER_LEN, Header, Relocator};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;
use paperpaw_sdk::native::CodeRoom;

use super::api::API;

/// The data cache's line: an image's memory is written back from it in
/// whole lines.
const CACHE_LINE: usize = 32;

unsafe extern "C" {
    fn rom_Cache_WriteBack_Addr(addr: u32, size: u32);
    fn Cache_Suspend_DCache_Autoload() -> u32;
    fn Cache_Resume_DCache_Autoload(value: u32);
    fn Cache_Invalidate_Addr(addr: u32, size: u32);
}

/// Places images in PSRAM.
#[derive(Debug, Default)]
pub struct Psram {
    /// Bytes the images placed at this start take, and how many.
    used: u64,
    placed: u32,
}

impl Psram {
    /// Reads the image of the plugin file at `path` into PSRAM, checked
    /// by its digest and relocated, and makes it runnable; its data
    /// address.
    pub fn put(
        &mut self,
        storage: &dyn Storage,
        path: &str,
        header: &Header,
        words: &[u32],
    ) -> Result<u32, PluginProblem> {
        #[cfg(feature = "remote")]
        super::set_room(super::Room::Psram);
        let len = header.image_len as usize;
        let align = (header.align as usize).max(CACHE_LINE);
        let layout = Layout::from_size_align(len, align).map_err(|_| PluginProblem::Damaged)?;
        // SAFETY: not zero-sized (a header's image never is). Kept for
        // good: plugins are not unloaded while PaperPaw runs.
        let ptr = unsafe { esp_alloc::HEAP.alloc_caps(MemoryCapability::External.into(), layout) };
        if ptr.is_null() {
            return Err(PluginProblem::NoRoom);
        }
        self.used += layout.size().next_multiple_of(align) as u64;
        self.placed += 1;
        // SAFETY: `len` bytes just allocated, used only here.
        let image = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
        let base = ptr as u32;
        let placed = fill(storage, path, header, words, image, base);
        if let Err(problem) = placed {
            // SAFETY: allocated above with this layout, nothing else
            // holds it.
            unsafe { alloc::alloc::dealloc(ptr, layout) };
            return Err(problem);
        }
        // Out of the data cache into PSRAM, and nothing old in the
        // instruction cache for the code's addresses.
        // SAFETY: ROM functions on memory this loader owns.
        unsafe {
            let autoload = Cache_Suspend_DCache_Autoload();
            rom_Cache_WriteBack_Addr(base, len as u32);
            Cache_Resume_DCache_Autoload(autoload);
            Cache_Invalidate_Addr(base + CODE_OFFSET, len as u32);
        }
        Ok(base)
    }

    /// Nothing to do once all are placed.
    pub fn finish(&mut self) {}

    /// The room for plugins at this start: what they took, and the PSRAM
    /// free beside it (shared with everything else kept there).
    pub fn room(&self) -> Option<CodeRoom> {
        let free = esp_alloc::HEAP.free_caps(MemoryCapability::External.into()) as u64;
        Some(CodeRoom::in_psram(self.used, free, self.placed))
    }
}

/// Reads the image into `image`, checks the file's digest and relocates
/// it for `base`.
fn fill(
    storage: &dyn Storage,
    path: &str,
    header: &Header,
    words: &[u32],
    image: &mut [u8],
    base: u32,
) -> Result<(), PluginProblem> {
    super::read_exact(storage, path, HEADER_LEN as u64, image)
        .map_err(|_| PluginProblem::Damaged)?;
    let mut sha = header.digest();
    sha.update(image);
    super::digest_words(&mut sha, words);
    if !header.holds(sha) {
        return Err(PluginProblem::Damaged);
    }
    let api = &API as *const _ as u32;
    Relocator::new(header, words, base, api)
        .and_then(|relocator| relocator.apply(0, image))
        .map_err(super::problem)
}
