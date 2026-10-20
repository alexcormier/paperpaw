//! The tool's own plugin in the app: a widget that draws nothing and
//! notes the area it is given, so that checks know a widget's cell
//! exactly (the grid gives every widget of a size the same area).
//!
//! Sessions add it besides the plugins being checked (see [`add`]); the
//! checks leave its namespace, [`PLUGIN`], out.

use std::borrow::Cow;
use std::sync::Mutex;

use paperpaw_core::KernelBuilder;
use paperpaw_platform::Rect;
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{
    Canvas, Env, Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, Version,
};

use crate::AddPlugins;

/// The tool's plugin id, left out of every check.
pub const PLUGIN: &str = "paperpaw-dev";

/// The widget that notes its area.
pub const WIDGET: Id = Id::new_static("paperpaw-dev.probe");

/// The area the probe was last drawn in.
static AREA: Mutex<Option<Rect>> = Mutex::new(None);

/// The plugins of the target, added after the probe (see [`add`]).
static TARGET: Mutex<Option<AddPlugins>> = Mutex::new(None);

/// Sets what [`add`] adds besides the probe.
pub fn set_target(add: Option<AddPlugins>) {
    if let Ok(mut target) = TARGET.lock() {
        *target = add;
    }
}

/// Adds the probe and the target's plugins (see [`set_target`]).
pub fn add(builder: KernelBuilder) -> KernelBuilder {
    let builder = builder.plugin(Probe);
    match TARGET.lock().ok().and_then(|t| *t) {
        Some(add) => add(builder),
        None => builder,
    }
}

/// Forgets the area noted, before the probe is drawn anew.
pub fn forget() {
    if let Ok(mut area) = AREA.lock() {
        *area = None;
    }
}

/// The area the probe was drawn in since [`forget`].
pub fn area() -> Option<Rect> {
    AREA.lock().ok().and_then(|a| *a)
}

#[derive(Debug)]
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static(PLUGIN),
            name: Cow::Borrowed("paperpaw-dev"),
            description: Cow::Borrowed("Measures where widgets are drawn, for paperpaw-dev."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
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
        if let Ok(mut noted) = AREA.lock() {
            *noted = Some(area);
        }
    }
}
