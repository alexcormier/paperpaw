use core::fmt;

/// A semantic version (`major.minor.patch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

/// The SDK version implemented by this crate. Plugins declare the version
/// they were built against in their [`Manifest`](crate::Manifest).
pub const SDK_VERSION: Version = Version::new(0, 4, 0);

/// The version of the formats of data plugins this PaperPaw reads (their
/// `plugin.ini`, translations, key maps, `settings.ini`, styles; ADR
/// 0020). A data plugin says which it is made for with `formats` in
/// `[plugin]` (1 if it does not); one made for a later version is not
/// loaded, rather than read wrongly. 2 adds `marks.ini` (see
/// [`decorations::MarkRules`](crate::decorations::MarkRules)); 3 adds
/// `menus.ini`, `actions.ini`, `icons.ini`, `points/`, `setup.ini` and
/// `[fonts]` in `plugin.ini`.
pub const DATA_FORMATS: u32 = 3;

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// The digits of `text` as a number, at compile time (for
    /// [`package_version!`](crate::package_version)); 0 where it has none.
    #[doc(hidden)]
    pub const fn number(text: &str) -> u16 {
        let bytes = text.as_bytes();
        let mut value: u16 = 0;
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            value = value
                .saturating_mul(10)
                .saturating_add((bytes[i] - b'0') as u16);
            i += 1;
        }
        value
    }

    /// Parses `1`, `1.2` or `1.2.3`.
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version::new(major, minor, patch))
    }

    /// Whether code requiring `required` works with this version, following
    /// SemVer (for `0.x`, the minor version must match).
    pub fn satisfies(&self, required: &Version) -> bool {
        let same_line = if required.major == 0 {
            self.major == 0 && self.minor == required.minor
        } else {
            self.major == required.major
        };
        same_line && self >= required
    }
}

/// The version of the crate it is written in (its `Cargo.toml`'s, without
/// a pre-release such as `-dev`), for a plugin's
/// [`Manifest::version`](crate::Manifest): a plugin released with its crate
/// says the same version everywhere.
///
/// ```
/// const VERSION: paperpaw_sdk::Version = paperpaw_sdk::package_version!();
/// assert_eq!(VERSION.to_string(), env!("CARGO_PKG_VERSION").split('-').next().unwrap());
/// ```
#[macro_export]
macro_rules! package_version {
    () => {
        $crate::Version::new(
            $crate::Version::number(env!("CARGO_PKG_VERSION_MAJOR")),
            $crate::Version::number(env!("CARGO_PKG_VERSION_MINOR")),
            $crate::Version::number(env!("CARGO_PKG_VERSION_PATCH")),
        )
    };
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_version_matches_crate_version() {
        // Between releases the crate is the next one with `-dev`.
        let number = |text: &str| text.parse().ok();
        let crate_version = Version::new(
            number(env!("CARGO_PKG_VERSION_MAJOR")).unwrap(),
            number(env!("CARGO_PKG_VERSION_MINOR")).unwrap(),
            number(env!("CARGO_PKG_VERSION_PATCH")).unwrap(),
        );
        assert_eq!(crate_version, SDK_VERSION);
    }

    #[test]
    fn the_package_version_is_the_crates() {
        assert_eq!(crate::package_version!(), SDK_VERSION);
        assert_eq!(Version::number("12"), 12);
        assert_eq!(Version::number(""), 0);
    }

    #[test]
    fn parses_short_forms() {
        assert_eq!(Version::parse("2"), Some(Version::new(2, 0, 0)));
        assert_eq!(Version::parse("0.3"), Some(Version::new(0, 3, 0)));
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("x"), None);
    }

    #[test]
    fn semver_compatibility() {
        let v = Version::new(1, 4, 2);
        assert!(v.satisfies(&Version::new(1, 0, 0)));
        assert!(v.satisfies(&Version::new(1, 4, 2)));
        assert!(!v.satisfies(&Version::new(1, 5, 0)));
        assert!(!v.satisfies(&Version::new(2, 0, 0)));

        let pre = Version::new(0, 2, 5);
        assert!(pre.satisfies(&Version::new(0, 2, 0)));
        assert!(!pre.satisfies(&Version::new(0, 1, 0)));
    }
}
