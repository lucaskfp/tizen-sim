//! The Version Profile catalog, bundled as data.

use serde::Deserialize;
use std::sync::OnceLock;

/// One Tizen TV release and the Engine milestone its TVs ship.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionProfile {
    pub version: String,
    pub milestone: u32,
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
