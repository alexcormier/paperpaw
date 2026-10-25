//! How book text is inked: smooth or black and white, and how dark. Part
//! of the standard type of book pages; each book may have its own.

use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::builtin::{groups, settings};

pub(super) static SETTINGS: [SettingSpec; 2] = [
    // On at first; off where it costs a refresh in grays (see
    // `smooth_text_default`).
    SettingSpec::switch(
        settings::SMOOTH_TEXT.static_str(),
        "core.setting.smooth-text",
        true,
        groups::READING_DISPLAY,
    )
    .scoped()
    .description("core.setting.smooth-text.help"),
    SettingSpec::choice(
        settings::TEXT_CONTRAST.static_str(),
        "core.setting.text-contrast",
        settings::TEXT_CONTRAST_LEVELS,
        settings::TEXT_CONTRAST_LEVELS[1],
        groups::READING_DISPLAY,
    )
    .scoped()
    .description("core.setting.text-contrast.help"),
];

/// The default of [`settings::SMOOTH_TEXT`] on a panel of `gray_levels`
/// grays (`None`: many, or colour), where it is not the declared one:
/// off where the panel shows black and white alone, which shows smooth
/// letters only with a slower refresh in grays of its own.
pub(crate) fn smooth_text_default(gray_levels: Option<u16>) -> Option<&'static str> {
    (gray_levels == Some(2)).then_some("false")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooth_text_is_off_at_first_only_in_black_and_white() {
        assert_eq!(smooth_text_default(Some(2)), Some("false"));
        assert_eq!(smooth_text_default(Some(4)), None);
        assert_eq!(smooth_text_default(Some(16)), None);
        assert_eq!(smooth_text_default(None), None);
    }
}
