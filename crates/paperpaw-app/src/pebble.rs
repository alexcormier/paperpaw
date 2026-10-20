//! Pebble: a theme for touch screens, shipped switched off. Lists sit on
//! rounded cards, as groups in a phone's settings do, and the chosen row
//! is a light bar (a frame on panels of black and white alone). It only
//! sets defaults, as a theme's `settings.ini` would; what the user
//! chooses still wins.

use alloc::borrow::Cow;

use paperpaw_core::sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, Version};

/// The theme.
#[derive(Debug)]
pub struct Pebble;

/// Its settings: the rest of PaperPaw's look stays as it is.
const DEFAULTS: [(&str, &str); 3] = [
    ("core.ui-groups", "core.groups.cards"),
    ("core.ui-focus", "core.focus.soft"),
    ("home.continue.frame", "core.frame.card"),
];

impl Plugin for Pebble {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: Id::new_static("pebble"),
            name: Cow::Borrowed("Pebble"),
            description: Cow::Borrowed(
                "A theme for touch screens: lists on rounded cards, a light highlight.",
            ),
            version: Version::new(1, 0, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (setting, value) in DEFAULTS {
            r.set_setting_default(Id::new_static(setting), Cow::Borrowed(value))?;
        }
        Ok(())
    }
}
