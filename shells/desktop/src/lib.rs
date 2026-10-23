//! The emulator as a library: the emulated device, its profiles and what
//! measures it (heap budget, storage trace), for the `paperpaw-emu`
//! window and for tools that drive the device themselves, such as
//! `paperpaw-dev` (`tools/dev`) and the browser's emulator
//! (`shells/web`). Without the feature `window` it has no window and
//! builds for `wasm32` too.
//!
//! Linking it puts its allocator in charge ([`budget::BudgetAllocator`]),
//! so that the kernel's heap is counted against the device's budget.

pub mod budget;
pub mod device;
pub mod headless;
pub mod host;
pub mod http;
pub mod light;
pub mod panel;
pub mod profiles;
pub mod registry;
pub mod stdio;
pub mod trace;
pub mod wifi;

#[global_allocator]
static ALLOCATOR: budget::BudgetAllocator = budget::BudgetAllocator;
