//! The Version Profile catalog, bundled as data.

use serde::Deserialize;
use std::sync::OnceLock;

/// One Tizen TV release and the Engine milestone its TVs ship.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionProfile {
    pub version: String,
    pub milestone: u32,
    /// Answers for `tizen.systeminfo.getCapability`.
    #[serde(default)]
    pub(crate) capabilities: toml::Table,
    /// Values the system info Shims report (model, display, locale, ...).
    #[serde(default)]
    pub(crate) device: toml::Table,
    /// Names of the Registrable Keys these TVs support.
    #[serde(default)]
    pub(crate) registrable_keys: Vec<String>,
}

impl VersionProfile {
    /// Whether these TVs have the Remote Key: every Mandatory Key, plus the
    /// Registrable Keys the profile lists.
    pub(crate) fn supports_key(&self, key: &crate::remote_keys::RemoteKey) -> bool {
        key.mandatory || self.registrable_keys.contains(&key.name)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown Version Profile `{0}` (known: {known})", known = known_versions())]
pub struct UnknownProfile(pub String);

#[derive(Deserialize)]
struct Catalog {
    profile: Vec<VersionProfile>,
}

fn catalog() -> &'static [VersionProfile] {
    static CATALOG: OnceLock<Vec<VersionProfile>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        toml::from_str::<Catalog>(include_str!("../data/profiles.toml"))
            .expect("bundled profiles.toml is valid")
            .profile
    })
}

fn known_versions() -> String {
    catalog()
        .iter()
        .map(|p| p.version.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl VersionProfile {
    pub fn all() -> &'static [VersionProfile] {
        catalog()
    }

    pub fn latest() -> &'static VersionProfile {
        catalog()
            .iter()
            .max_by_key(|p| p.milestone)
            .expect("at least one Version Profile is bundled")
    }

    pub fn find(version: &str) -> Result<&'static VersionProfile, UnknownProfile> {
        catalog()
            .iter()
            .find(|p| p.version == version)
            .ok_or_else(|| UnknownProfile(version.to_string()))
    }
}
