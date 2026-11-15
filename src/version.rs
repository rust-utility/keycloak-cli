//! The crate version encodes the Keycloak version it wraps.
//!
//! Same scheme as the [`keycloak`](https://crates.io/crates/keycloak) crate: a
//! crate version `x.y.(z * 100 + v)` wraps Keycloak `x.y.z` with the fix release
//! `v`, so `26.8.100` wraps Keycloak `26.8.1` and `26.7.301` would be the second
//! fix release for Keycloak `26.7.3`.
//!
//! The single source of truth is `CARGO_PKG_VERSION`; [`keycloak_version`]
//! converts it, so bumping the crate version is enough to follow a new Keycloak
//! release.

use std::fmt;
use std::sync::LazyLock;

/// Crate version, e.g. `26.8.100`.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A `major.minor.patch` version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses `x.y.z`; anything else (pre-releases, four components) is rejected.
    pub fn parse(input: &str) -> Option<Self> {
        let mut parts = input.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self::new(major, minor, patch))
    }

    /// The Keycloak version wrapped by this crate version.
    ///
    /// `x.y.(z * 100 + v)` becomes `x.y.z`; see [`Version::fix`] for `v`.
    pub const fn keycloak(self) -> Self {
        Self::new(self.major, self.minor, self.patch / 100)
    }

    /// The fix release against the same Keycloak version: `v` in `z * 100 + v`.
    pub const fn fix(self) -> u64 {
        self.patch % 100
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Version of the crate itself.
pub fn crate_version() -> Version {
    Version::parse(CRATE_VERSION)
        .unwrap_or_else(|| panic!("CARGO_PKG_VERSION {CRATE_VERSION} is not of the form x.y.z"))
}

/// Version of the Keycloak Admin REST API this build wraps.
pub fn keycloak_version() -> Version {
    crate_version().keycloak()
}

/// The `--version` string, e.g. `26.8.100 (Keycloak Admin REST API 26.8.1)`.
pub fn version_string() -> &'static str {
    static STRING: LazyLock<String> = LazyLock::new(|| {
        format!(
            "{CRATE_VERSION} (Keycloak Admin REST API {})",
            keycloak_version()
        )
    });
    &STRING
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x_y_z() {
        assert_eq!(Version::parse("26.8.100"), Some(Version::new(26, 8, 100)));
        assert_eq!(Version::parse("1.2.3"), Some(Version::new(1, 2, 3)));
    }

    #[test]
    fn rejects_other_shapes() {
        for input in ["", "26", "26.8", "26.8.100.1", "26.8.x", "v26.8.100"] {
            assert_eq!(Version::parse(input), None, "{input:?} parsed");
        }
    }

    #[test]
    fn encodes_the_keycloak_version() {
        assert_eq!(Version::new(26, 8, 100).keycloak(), Version::new(26, 8, 1));
        assert_eq!(Version::new(26, 7, 300).keycloak(), Version::new(26, 7, 3));
        // A fix release keeps the wrapped Keycloak version.
        assert_eq!(Version::new(26, 7, 301).keycloak(), Version::new(26, 7, 3));
        assert_eq!(Version::new(26, 7, 301).fix(), 1);
        // Versions from before the scheme (26.6.2 == Keycloak 26.6.0 with fix 2).
        assert_eq!(Version::new(26, 6, 2).keycloak(), Version::new(26, 6, 0));
        assert_eq!(Version::new(26, 6, 2).fix(), 2);
    }

    #[test]
    fn crate_version_follows_the_scheme() {
        let version = crate_version();
        assert_eq!(version, Version::parse(CRATE_VERSION).unwrap());
        assert!(
            version.fix() < 100,
            "{version} has more than 99 fix releases"
        );
        assert_eq!(
            version_string(),
            format!(
                "{CRATE_VERSION} (Keycloak Admin REST API {})",
                version.keycloak()
            )
        );
    }
}
