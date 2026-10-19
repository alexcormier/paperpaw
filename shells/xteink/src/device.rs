//! The X4's own plugin: what only this device has to set (for
//! developers, in their group of settings).

use alloc::borrow::Cow;

use quireink_sdk::builtin::groups;
use quireink_sdk::setting::Setting;
use quireink_sdk::{
    Id, Manifest, Plugin, RegisterError, Registrar, SDK_VERSION, SettingSpec, Version,
};

pub const PLUGIN: Id = Id::new_static("x4");
/// The processor sleeps lightly while idle on battery (see `power`).
pub const LIGHT_SLEEP: Setting<bool> = Setting::new("x4.light-sleep");

#[derive(Debug, Default)]
pub struct X4;

/// Whether the X4 sleeps lightly between button reads.
static SETTINGS: [SettingSpec; 1] = [SettingSpec::fixed_switch(
    LIGHT_SLEEP.static_str(),
    "x4.light-sleep",
    true,
    groups::DEVELOPER,
)
.described("x4.light-sleep.help")];

impl Plugin for X4 {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PLUGIN,
            name: Cow::Borrowed("Xteink X4"),
            description: Cow::Borrowed("What only the Xteink X4 has to set."),
            version: Version::new(0, 1, 0),
            sdk: SDK_VERSION,
        }
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
