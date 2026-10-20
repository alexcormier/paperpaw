//! The emulator as a library: the emulated device, its profiles and what
//! measures it (heap budget, storage trace), for the `paperpaw-emu`
//! window and for tools that drive the device themselves, such as
//! `paperpaw-dev` (`tools/dev`).
//!
//! Linking it puts its allocator in charge ([`budget::BudgetAllocator`]),
//! so that the kernel's heap is counted against the device's budget.

pub mod budget;
pub mod device;
pub mod headless;
pub mod http;
pub mod light;
pub mod panel;
pub mod profiles;
pub mod trace;
pub mod wifi;

#[global_allocator]
static ALLOCATOR: budget::BudgetAllocator = budget::BudgetAllocator;
