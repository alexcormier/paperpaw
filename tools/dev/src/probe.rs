//! The tool's own plugin in the app: a widget that draws nothing and
//! notes the area it is given, so that checks know a widget's cell
//! exactly (the grid gives every widget of a size the same area).
//!
//! Sessions add it besides the plugins being checked (see [`add`]); the
//! checks leave its namespace, [`PLUGIN`], out.
//!
//! What it keeps is the thread's: a session builds and runs its devices
//! on one thread, and sessions on other threads (tests run side by side)
//! must not change which plugins a device gets or the area it notes.

use std::cell::Cell;

use paperpaw_core::KernelBuilder;
use paperpaw_platform::Rect;
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{Canvas, Env, Id, Manifest, Plugin, RegisterError, Registrar, Version};

use crate::AddPlugins;

/// The tool's plugin id, left out of every check.
pub const PLUGIN: &str = "paperpaw-dev";

/// The widget that notes its area.
pub const WIDGET: Id = Id::new_static("paperpaw-dev.probe");

thread_local! {
    /// The area the probe was last drawn in.
    static AREA: Cell<Option<Rect>> = const { Cell::new(None) };
    /// The plugins of the target, added after the probe (see [`add`]).
    static TARGET: Cell<Option<AddPlugins>> = const { Cell::new(None) };
}

/// Sets what [`add`] adds besides the probe, on this thread.
pub fn set_target(add: Option<AddPlugins>) {
    TARGET.set(add);
}

/// Adds the probe and the target's plugins (see [`set_target`]).
pub fn add(builder: KernelBuilder) -> KernelBuilder {
    let builder = builder.plugin(Probe);
    match TARGET.get() {
        Some(add) => add(builder),
        None => builder,
    }
}

/// Forgets the area noted, before the probe is drawn anew.
pub fn forget() {
    AREA.set(None);
}

/// The area the probe was drawn in since [`forget`].
pub fn area() -> Option<Rect> {
    AREA.get()
}

#[derive(Debug)]
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static(PLUGIN),
            "paperpaw-dev",
            Version::new(0, 1, 0),
        )
        .description("Measures where widgets are drawn, for paperpaw-dev.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_widget(WidgetSpec::new(WIDGET, "paperpaw-dev.probe", |_| {
            Box::new(Noting)
        }))
    }
}

struct Noting;

impl Widget for Noting {
    fn live(&self) -> bool {
        false
    }

    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        1
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, area: Rect) {
        AREA.set(Some(area));
    }
}
