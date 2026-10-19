//! What crosses between the firmware and a plugin before the SDK's types
//! do: the plugin's [`Entry`], and the firmware's [`Api`], the few things
//! a plugin cannot bring itself.

use core::ffi::c_void;

/// The symbol of a plugin's [`Entry`], a static in [`ENTRY_SECTION`].
pub const ENTRY_SYMBOL: &str = "PAPERPAW_ENTRY";
/// The section a plugin's [`Entry`] is put in: the first of its image.
pub const ENTRY_SECTION: &str = ".rodata.paperpaw_entry";
/// The symbol of the word that is to hold the address of the firmware's
/// [`Api`]; `paperpaw-plugin.x` makes it, a plugin reads it as an extern
/// static.
pub const API_SYMBOL: &str = "PAPERPAW_API";
/// The bytes of an [`Entry`] on the chips plugin files are for.
pub(crate) const ENTRY_LEN: usize = 16;

/// What a plugin exports, at [`Header::entry`](crate::Header::entry).
#[repr(C)]
#[derive(Debug)]
pub struct Entry {
    /// The plugin's fingerprint, as its file's header says (the packer
    /// copies it from here).
    pub fingerprint: u64,
    /// Makes the plugin. What it returns is the SDK's to read (its
    /// `native::take`): the firmware owns it.
    pub make: unsafe extern "C" fn() -> *mut c_void,
}

/// What the firmware gives every plugin: its heap (every allocation is
/// the kernel's, so what one side boxes the other may drop), its panics
/// and its clock. It grows only at its end, and says its size.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Api {
    /// This table's bytes ([`Api::SIZE`] of the firmware's build).
    pub size: u32,
    /// Allocates as `GlobalAlloc::alloc` does; null when there is no
    /// room.
    pub alloc: unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
    /// Frees what `alloc` or `realloc` gave, with the same size and
    /// alignment.
    pub dealloc: unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
    /// Resizes as `GlobalAlloc::realloc` does.
    pub realloc:
        unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize, new_size: usize) -> *mut u8,
    /// The plugin panicked, saying why (UTF-8): the firmware's own panic,
    /// which keeps the message and starts the device again.
    pub panic: unsafe extern "C" fn(message: *const u8, len: usize) -> !,
    /// Microseconds since the device started.
    pub micros: extern "C" fn() -> u64,
}

impl Api {
    /// The size of this crate's table.
    pub const SIZE: u32 = core::mem::size_of::<Api>() as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_as_long_as_the_format_says() {
        // The chips are 32-bit; on a computer pointers are wider but the
        // entry is as long: its fingerprint aligns it to 8.
        assert_eq!(core::mem::size_of::<Entry>(), ENTRY_LEN);
        assert_eq!(core::mem::align_of::<Entry>(), 8);
    }
}
