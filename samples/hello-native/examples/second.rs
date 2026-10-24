//! A second native plugin, to try several at once: an action that says
//! so, and a service the sample's could use. `cargo x4 --example second`
//! (or `cargo +esp x4pro --example second`), then pack it as for the
//! sample, into `plugins/second/code.ppn`.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

use paperpaw_sdk::{ActionSpec, Command, Id, Manifest, Plugin, RegisterError, Registrar, Version};

struct Second;

impl Plugin for Second {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("second"), "Second", Version::new(0, 1, 0))
            .description("A second native plugin, to try several at once.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(
            ActionSpec::new(Id::new_static("second.hello"), "second.hello").handler(|_, _, out| {
                out.push(Command::Notify(String::from(
                    "Hello from the second plugin",
                )));
            }),
        )?;
        r.provide(
            Id::new_static("second.version"),
            SecondVersion(Version::new(0, 1, 0)),
            Version::new(1, 0, 0),
        )
    }
}

paperpaw_sdk::plugin!(Second);

/// Its version, offered to other plugins.
#[repr(C)]
pub struct SecondVersion(pub Version);
paperpaw_sdk::service_type!(SecondVersion, "second.version/1");
