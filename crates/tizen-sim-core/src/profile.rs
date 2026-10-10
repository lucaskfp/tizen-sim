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

    /// The device values the Shims report, with a Session's `overrides`
    /// replacing the profile's values of the same name.
    pub fn device_values(
        &self,
        overrides: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<serde_json::Value, UnknownDeviceValue> {
        let mut values = serde_json::to_value(&self.device).expect("TOML converts to JSON");
        let table = values.as_object_mut().expect("device is a table");
        for (name, value) in overrides {
            let slot = table.get_mut(name).ok_or_else(|| {
                let mut known: Vec<&str> = self.device.keys().map(String::as_str).collect();
                known.sort_unstable();
                UnknownDeviceValue {
                    name: name.clone(),
                    known: known.join(", "),
                }
            })?;
            *slot = value.clone();
        }
        Ok(values)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown device value `{name}` (known: {known})")]
pub struct UnknownDeviceValue {
    name: String,
    known: String,
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
