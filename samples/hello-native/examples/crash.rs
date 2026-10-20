//! A native plugin that panics as it registers, at every start: to try
//! safe mode. PaperPaw starts twice without getting to its first frame,
//! then without code plugins, switches this one off and names it.
//! `cargo x4 --example crash` (or `cargo +esp x4pro --example crash`),
//! then pack it into `plugins/crash/code.ppn`.

#![no_std]
#![no_main]

use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

struct Crash;

impl Plugin for Crash {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("crash"), "Crash", Version::new(0, 1, 0))
            .description("Panics at every start, to try safe mode.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        panic!("the crash plugin crashes, as it should")
    }
}

paperpaw_sdk::plugin!(Crash);
