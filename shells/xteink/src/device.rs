//! The device's own plugin: what only the X4, the X4 Pro, the X4
//! Classic or the X3 has to set.
//! All sleep lightly between readings of their buttons (for developers,
//! in their group of settings); the X4 Pro's front light has its
//! brightness and warmth, in the quick panel and the reader's panel too.

use alloc::borrow::Cow;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, SettingSpec, Version};

pub const PLUGIN: Id = Id::new_static("x4");
/// The processor sleeps lightly while idle on battery (see `power`).
pub const LIGHT_SLEEP: Setting<bool> = Setting::new("x4.light-sleep");
/// How quickly presses are answered on battery: the processor sleeps less
/// between readings of the buttons, and the X4 Pro's panel stays powered
/// (see `firmware`).
pub const BUTTON_RESPONSE: Setting<paperpaw_sdk::setting::Text> =
    Setting::new("x4.button-response");
/// The choices of [`BUTTON_RESPONSE`].
pub const RESPONSE_NORMAL: &str = "x4.button-response.normal";
pub const RESPONSE_FASTER: &str = "x4.button-response.faster";
/// How the X4 shows fast changes (menus, a focus moved): the panel's own
/// waveform, or a quicker one with more or less ghosting (see `firmware`).
#[cfg(feature = "x4")]
pub const FAST_REFRESH: Setting<paperpaw_sdk::setting::Text> = Setting::new("x4.fast-refresh");
/// Whether the panel's controller is an SSD1677: only its waveforms can be
/// made here, so only then the settings of them are offered. Set at start,
/// once the controller is known (`main`).
#[cfg(feature = "x4")]
static SSD1677: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Says whether the panel's controller is an SSD1677.
#[cfg(feature = "x4")]
pub fn set_ssd1677(yes: bool) {
    SSD1677.store(yes, core::sync::atomic::Ordering::Relaxed);
}

/// The frames and the kick of a fast refresh, used where [`FAST_REFRESH`] is
/// [`FAST_CUSTOM`]; and of a black-and-white refresh, the one screens get that
/// ask for the fastest (games), always. A data plugin sets them in its
/// `settings.ini` or by an action (see `waveform::ordered` for how they are
/// kept in order).
#[cfg(feature = "x4")]
pub const FAST_FRAMES: Setting<i64> = Setting::new("x4.ssd1677.fast-frames");
#[cfg(feature = "x4")]
pub const FAST_KICK: Setting<i64> = Setting::new("x4.ssd1677.fast-kick");
#[cfg(feature = "x4")]
pub const MONO_FRAMES: Setting<i64> = Setting::new("x4.ssd1677.mono-frames");
#[cfg(feature = "x4")]
pub const MONO_KICK: Setting<i64> = Setting::new("x4.ssd1677.mono-kick");
/// The choices of [`FAST_REFRESH`], the slowest and cleanest first.
#[cfg(feature = "x4")]
pub const FAST_PANEL: &str = "x4.fast-refresh.panel";
#[cfg(feature = "x4")]
pub const FAST_CLEAN: &str = "x4.fast-refresh.clean";
#[cfg(feature = "x4")]
pub const FAST_BALANCED: &str = "x4.fast-refresh.balanced";
#[cfg(feature = "x4")]
pub const FAST_QUICK: &str = "x4.fast-refresh.quick";
/// The frames and the kick of [`FAST_FRAMES`] and [`FAST_KICK`].
#[cfg(feature = "x4")]
pub const FAST_CUSTOM: &str = "x4.fast-refresh.custom";
/// The X4 Pro's front light: how bright, in percent (0 off).
#[cfg_attr(
    not(feature = "x4-pro"),
    expect(dead_code, reason = "only the X4 Pro has a light")
)]
pub const FRONTLIGHT: Setting<i64> = Setting::new("x4.frontlight");
/// How much of the light is warm, in percent.
#[cfg_attr(
    not(feature = "x4-pro"),
    expect(dead_code, reason = "only the X4 Pro has a light")
)]
pub const WARMTH: Setting<i64> = Setting::new("x4.warmth");

#[derive(Debug, Default)]
pub struct X4;

/// How quickly presses are answered; whether it sleeps lightly between
/// button reads.
static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::choice(
        BUTTON_RESPONSE.static_str(),
        "x4.button-response",
        &[RESPONSE_NORMAL, RESPONSE_FASTER],
        RESPONSE_NORMAL,
        groups::CONTROLS,
    )
    .description("x4.button-response.help"),
    SettingSpec::switch(
        LIGHT_SLEEP.static_str(),
        "x4.light-sleep",
        true,
        groups::DEVELOPER,
    )
    .description("x4.light-sleep.help"),
];

/// How fast changes are shown.
#[cfg(feature = "x4")]
static FAST_SETTINGS: [SettingSpec; 5] = [
    SettingSpec::choice(
        FAST_REFRESH.static_str(),
        "x4.fast-refresh",
        &[
            FAST_PANEL,
            FAST_CLEAN,
            FAST_BALANCED,
            FAST_QUICK,
            FAST_CUSTOM,
        ],
        FAST_BALANCED,
        groups::DISPLAY,
    )
    .description("x4.fast-refresh.help"),
    SettingSpec::number(
        FAST_FRAMES.static_str(),
        "x4.ssd1677.fast-frames",
        1,
        30,
        6,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.fast-frames.help"),
    SettingSpec::number(
        FAST_KICK.static_str(),
        "x4.ssd1677.fast-kick",
        0,
        3,
        0,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.fast-kick.help"),
    SettingSpec::number(
        MONO_FRAMES.static_str(),
        "x4.ssd1677.mono-frames",
        1,
        30,
        1,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.mono-frames.help"),
    SettingSpec::number(
        MONO_KICK.static_str(),
        "x4.ssd1677.mono-kick",
        0,
        3,
        0,
        groups::DEVELOPER,
    )
    .description("x4.ssd1677.mono-kick.help"),
];

/// The X4 Pro's front light, set with sliders: how bright in single
/// percents, so that `−` and `+` set the dim end finely (see `light`), its
/// colour in fives.
#[cfg(feature = "x4-pro")]
static LIGHT_SETTINGS: [SettingSpec; 2] = {
    use paperpaw_sdk::SettingExtra;
    static BRIGHTNESS: SettingExtra = SettingExtra::new().unit("core.unit.percent").slider();
    static PERCENT: SettingExtra = SettingExtra::new()
        .unit("core.unit.percent")
        .step(5)
        .slider();
    [
        SettingSpec::number(
            FRONTLIGHT.static_str(),
            "x4.frontlight",
            0,
            100,
            0,
            groups::DISPLAY,
        )
        .with_extra(&BRIGHTNESS)
        .description("x4.frontlight.help"),
        SettingSpec::number(
            WARMTH.static_str(),
            "x4.warmth",
            0,
            100,
            30,
            groups::DISPLAY,
        )
        .with_extra(&PERCENT)
        .description("x4.warmth.help"),
    ]
};

impl Plugin for X4 {
    fn manifest(&self) -> Manifest {
        #[cfg(feature = "x4")]
        let (name, description) = ("Xteink X4", "What only the Xteink X4 has to set.");
        #[cfg(feature = "x4-pro")]
        let (name, description) = (
            "Xteink X4 Pro",
            "What only the Xteink X4 Pro has to set: its front light.",
        );
        #[cfg(feature = "x4-classic")]
        let (name, description) = (
            "Xteink X4 Classic",
            "What only the Xteink X4 Classic has to set.",
        );
        #[cfg(feature = "x3")]
        let (name, description) = ("Xteink X3", "What only the Xteink X3 has to set.");
        Manifest::new(PLUGIN, name, Version::new(0, 1, 0))
            .description(description)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        #[cfg(feature = "x4")]
        if SSD1677.load(core::sync::atomic::Ordering::Relaxed) {
            r.add_settings(&FAST_SETTINGS)?;
        }
        #[cfg(feature = "x4-pro")]
        register_light(r)?;
        #[cfg(feature = "waveform-lab")]
        {
            r.add_screens(&crate::lab::SCREENS)?;
            r.add_actions(&crate::lab::ACTIONS)?;
            r.add_menu_entries(&crate::lab::ENTRIES)?;
        }
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The front light's settings, also first in the reader's panel and in
/// the quick panel.
#[cfg(feature = "x4-pro")]
fn register_light(r: &mut Registrar) -> Result<(), RegisterError> {
    use paperpaw_sdk::MenuEntry;
    use paperpaw_sdk::builtin::menus;
    r.add_settings(&LIGHT_SETTINGS)?;
    for (n, item) in [FRONTLIGHT, WARMTH].into_iter().enumerate() {
        for (menu, order) in [(menus::READER_PANEL, -10), (menus::QUICK, 20)] {
            r.add_menu_entry(MenuEntry::new(menu, item.clone().into()).order(order + n as i16))?;
        }
    }
    r.add_icon(&FRONTLIGHT, Cow::Borrowed("bulb"))?;
    r.add_icon(&WARMTH, Cow::Borrowed("temperature"))?;
    Ok(())
}
