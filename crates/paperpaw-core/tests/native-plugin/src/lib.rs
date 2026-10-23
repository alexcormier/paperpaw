//! A native plugin built apart from the kernel that loads it
//! (`../native_plugin.rs`): the SDK's sample plugin, and a service whose
//! type is the SDK's, which the kernel's build finds by its name (and,
//! published the old way, not by its `TypeId`).

#![no_std]

extern crate alloc;

use paperpaw_sample_hello::Hello;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

/// The sample, with one more thing: its version as a service.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Hello.manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        Hello.register(r)?;
        r.publish(Id::new_static("hello.version"), Version::new(1, 2, 3))
    }
}

paperpaw_sdk::plugin!(Probe);
