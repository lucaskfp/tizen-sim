//! The Version Profile catalog, bundled as data.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One Tizen TV release and the Engine milestone its TVs ship.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionProfile {
    pub version: String,
    pub milestone: u32,
    /// The user agent these TVs' browser engine sends; `None` keeps the
    /// Engine's own.
    #[serde(default)]
    pub user_agent: Option<String>,
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
#[error("unknown Version Profile `{version}` (known: {known})")]
pub struct UnknownProfile {
    version: String,
    known: String,
}

/// The Version Profiles, one per Tizen TV release. Bundled as data
/// (`data/profiles.toml`), so supporting a new release needs no code change.
#[derive(Debug, Deserialize)]
pub struct ProfileCatalog {
    #[serde(rename = "profile")]
    profiles: Vec<VersionProfile>,
}

impl ProfileCatalog {
    pub fn parse(toml: &str) -> Result<ProfileCatalog, toml::de::Error> {
        let catalog: ProfileCatalog = toml::from_str(toml)?;
        if catalog.profiles.is_empty() {
            return Err(serde::de::Error::custom("no [[profile]] entries"));
        }
        Ok(catalog)
    }

    /// The catalog bundled with the simulator.
    pub fn bundled() -> &'static ProfileCatalog {
        static CATALOG: OnceLock<ProfileCatalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            ProfileCatalog::parse(include_str!("../data/profiles.toml"))
                .expect("bundled profiles.toml is valid")
        })
    }

    pub fn all(&self) -> &[VersionProfile] {
        &self.profiles
    }

    /// The newest release: the one with the newest Engine.
    pub fn latest(&self) -> &VersionProfile {
        self.profiles
            .iter()
            .max_by_key(|p| p.milestone)
            .expect("a catalog has at least one Version Profile")
    }

    pub fn find(&self, version: &str) -> Result<&VersionProfile, UnknownProfile> {
        self.profiles
            .iter()
            .find(|p| p.version == version)
            .ok_or_else(|| UnknownProfile {
                version: version.to_string(),
                known: self
                    .profiles
                    .iter()
                    .map(|p| p.version.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }

    /// The Version Profile for a Session of the App `app_id`: the `explicit`
    /// choice, otherwise the last one used for the App, otherwise the latest.
    pub fn choose(
        &self,
        explicit: Option<&str>,
        app_id: &str,
        remembered: &RememberedProfiles,
    ) -> Result<&VersionProfile, UnknownProfile> {
        if let Some(version) = explicit {
            return self.find(version);
        }
        Ok(remembered
            .get(app_id)
            // A release dropped from the catalog since: as for a new App.
            .and_then(|version| self.find(&version).ok())
            .unwrap_or_else(|| self.latest()))
    }
}

impl VersionProfile {
    pub fn all() -> &'static [VersionProfile] {
        ProfileCatalog::bundled().all()
    }

    pub fn latest() -> &'static VersionProfile {
        ProfileCatalog::bundled().latest()
    }

    pub fn find(version: &str) -> Result<&'static VersionProfile, UnknownProfile> {
        ProfileCatalog::bundled().find(version)
    }
}

/// The last Version Profile used for each App, kept in the per-user state.
pub struct RememberedProfiles {
    dir: PathBuf,
}

impl RememberedProfiles {
    /// The profiles remembered under the per-user state directory.
    pub fn new(state_dir: &Path) -> RememberedProfiles {
        RememberedProfiles {
            dir: state_dir.join("last-profile"),
        }
    }

    pub fn get(&self, app_id: &str) -> Option<String> {
        let version = std::fs::read_to_string(self.file(app_id)).ok()?;
        Some(version.trim().to_string()).filter(|v| !v.is_empty())
    }

    pub fn remember(&self, app_id: &str, version: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.file(app_id), version)
    }

    fn file(&self, app_id: &str) -> PathBuf {
        self.dir.join(crate::home::app_file_name(app_id))
    }
}
