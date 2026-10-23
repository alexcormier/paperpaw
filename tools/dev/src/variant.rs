//! The variants a plugin is looked at in: device profile, language, light
//! or dark, and look (the built-in themes: corners and contrast).

use paperpaw_emulator::profiles::{self, Profile};

/// A built-in look: the settings that make it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub settings: &'static [(&'static str, &'static str)],
}

/// The looks PaperPaw comes with, the default first.
pub const THEMES: &[Theme] = &[
    Theme {
        name: "default",
        settings: &[],
    },
    Theme {
        name: "square",
        settings: &[("core.ui-corners", "core.corners.square")],
    },
    Theme {
        name: "round-strong",
        settings: &[
            ("core.ui-corners", "core.corners.round"),
            ("core.ui-contrast", "core.contrast.strong"),
        ],
    },
];

/// The profiles looked at unless `--profile` says otherwise: the devices
/// PaperPaw runs on.
pub const DEFAULT_PROFILES: &[&str] = &["xteink-x4", "xteink-x4-pro", "boox-go-7"];

pub fn theme(name: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.name == name)
}

/// One way the plugin is looked at.
#[derive(Debug, Clone)]
pub struct Variant {
    pub profile: &'static Profile,
    /// A language code (`en`, `de`) or `pseudo` (see `lint::pseudo`).
    pub language: String,
    pub night: bool,
    pub theme: &'static Theme,
}

impl Variant {
    /// The first variant of a profile: English, light, default look.
    pub fn of(profile: &'static Profile) -> Self {
        Variant {
            profile,
            language: String::from("en"),
            night: false,
            theme: &THEMES[0],
        }
    }

    /// The X4 in English, light, the default look: where budgets are
    /// measured.
    pub fn x4() -> Self {
        Self::of(profiles::find("xteink-x4").unwrap_or(&profiles::PROFILES[0]))
    }

    /// E.g. `xteink-x4 en light default`.
    pub fn name(&self) -> String {
        format!(
            "{} {} {} {}",
            self.profile.device.id,
            self.language,
            if self.night { "dark" } else { "light" },
            self.theme.name
        )
    }

    /// The name for files: `xteink-x4-en-light-default`.
    pub fn slug(&self) -> String {
        self.name().replace(' ', "-")
    }

    /// The settings this variant sets, as `(id, value)`.
    pub fn settings(&self) -> Vec<(&str, &str)> {
        let mut settings = vec![("core.language", self.language.as_str())];
        if self.night {
            settings.push(("core.night-mode", "true"));
        }
        settings.extend(self.theme.settings.iter().copied());
        settings
    }

    pub fn has_touch(&self) -> bool {
        self.profile.device.input.touch
    }
}

/// `settings` as a configuration file (`paperpaw.ini`): an id
/// `section.key` goes into `[section]`.
pub fn config<'a>(settings: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut config = String::new();
    for (id, value) in settings {
        let (section, key) = id.split_once('.').unwrap_or(("core", id));
        config = paperpaw_config::set(&config, section, key, value);
    }
    config
}

/// Every combination of what was chosen, profiles outermost.
pub fn matrix(
    profiles: &[&'static Profile],
    languages: &[String],
    nights: &[bool],
    themes: &[&'static Theme],
) -> Vec<Variant> {
    let mut variants = Vec::new();
    for &profile in profiles {
        for language in languages {
            for &night in nights {
                for &theme in themes {
                    variants.push(Variant {
                        profile,
                        language: language.clone(),
                        night,
                        theme,
                    });
                }
            }
        }
    }
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variant_names_its_settings() {
        let mut variant = Variant::x4();
        variant.night = true;
        variant.theme = theme("round-strong").unwrap();
        assert_eq!(variant.name(), "xteink-x4 en dark round-strong");
        let config = config(variant.settings());
        assert!(config.contains("[core]"), "{config}");
        assert!(config.contains("night-mode = true"), "{config}");
        assert!(
            config.contains("ui-contrast = core.contrast.strong"),
            "{config}"
        );
    }

    #[test]
    fn default_profiles_exist() {
        for id in DEFAULT_PROFILES {
            assert!(profiles::find(id).is_some(), "{id}");
        }
    }
}
