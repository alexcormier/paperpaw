//! How much memory is free now ([`Env::memory`]), where the shell can
//! tell: for diagnostics, and for plugins that check their room before
//! large work. A device's memory is mostly its heap; on small devices the
//! stack's depth matters as well, as running over it is fatal.
//!
//! Each shell gives the kernel its probe (`KernelBuilder::memory`): the
//! allocator's numbers on the X4 and the emulator's budget on desktops.
//! Where a shell gives none, nothing is known.
//!
//! [`Env::memory`]: crate::Env::memory

use core::fmt;

/// Memory in use and free, in bytes, at the moment it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MemoryUse {
    /// Bytes of the heap free now.
    pub heap_free: u32,
    /// The heap's size.
    pub heap_size: u32,
    /// Bytes of the stack never used since start (its depth left at its
    /// deepest), where the shell measures it.
    pub stack_free: Option<u32>,
    /// The stack's size, where the shell knows it.
    pub stack_size: Option<u32>,
}

impl MemoryUse {
    /// The heap's free bytes and size; the stack not measured.
    pub const fn new(heap_free: u32, heap_size: u32) -> Self {
        Self {
            heap_free,
            heap_size,
            stack_free: None,
            stack_size: None,
        }
    }

    /// With the stack measured: `free` bytes never used of `size`.
    pub const fn with_stack(mut self, free: u32, size: u32) -> Self {
        self.stack_free = Some(free);
        self.stack_size = Some(size);
        self
    }
}

/// The shell's probe of memory; see the [module documentation](self).
#[derive(Clone, Copy, Default)]
pub struct Memory {
    read: Option<fn() -> Option<MemoryUse>>,
}

impl Memory {
    /// No probe: nothing is known.
    pub const NONE: Self = Self { read: None };

    /// The probe `read`, which says what is free now, or `None` where it
    /// cannot tell now (for shells).
    pub const fn new(read: fn() -> Option<MemoryUse>) -> Self {
        Self { read: Some(read) }
    }

    /// What is free now; `None` where the shell cannot tell. Cheap, but
    /// not free: a shell may walk the stack to measure its depth.
    pub fn read(&self) -> Option<MemoryUse> {
        self.read.and_then(|read| read())
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("available", &self.read.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_is_read_when_asked() {
        fn probe() -> Option<MemoryUse> {
            Some(MemoryUse {
                heap_free: 10,
                heap_size: 20,
                stack_free: None,
                stack_size: None,
            })
        }
        assert_eq!(Memory::NONE.read(), None);
        assert_eq!(Memory::new(probe).read().map(|m| m.heap_free), Some(10));
    }
}
