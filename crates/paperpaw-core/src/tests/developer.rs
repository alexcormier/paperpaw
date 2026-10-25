//! The developer link over USB (ADR 0031): reserved in this release, so
//! a plugin that opens it hears at once that it is not there.

use super::*;
use paperpaw_sdk::developer::{self, DevError, DevEvent};

const LINK: Id = Id::new_static("tools.link");
const EVENTS: Id = Id::new_static("tools.link-events");

struct Tools(Log);

impl Plugin for Tools {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("tools"), "Tools", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::DEVELOPER)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.on(
            paperpaw_sdk::event::Event::<DevEvent>::new("tools.link-events"),
            move |_, event, _| {
                log.borrow_mut().push(alloc::format!("{event:?}"));
            },
        )
    }
}

#[test]
fn the_developer_link_is_not_there_yet() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Tools(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.output.commands.push(developer::open(LINK, EVENTS));
    kernel.run_commands();
    let failed = DevEvent::Failed {
        link: LINK,
        error: DevError::NotAvailable,
    };
    assert_eq!(*log.borrow(), [alloc::format!("{failed:?}")]);
}
