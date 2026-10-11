//! API availability: which members of the Shimmed platform APIs a Version
//! Profile has, and which of them are deprecated there. Bundled as data
//! (`data/apis.toml`).

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
struct ApiEntry {
    name: String,
    since: Option<String>,
    deprecated: Option<String>,
    removed: Option<String>,
}

/// What a Version Profile has of the APIs listed in the catalog, for the
/// Shims (see `shim::session_data`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ApiAvailability {
    /// Members the profile lacks: not yet added, or already removed.
    pub absent: Vec<String>,
    /// Members that still work but log a deprecation warning.
    pub deprecated: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiCatalog {
    #[serde(rename = "api", default)]
    entries: Vec<ApiEntry>,
}

impl ApiCatalog {
    pub fn parse(toml: &str) -> Result<ApiCatalog, toml::de::Error> {
        let catalog: ApiCatalog = toml::from_str(toml)?;
        for entry in &catalog.entries {
            for version in [&entry.since, &entry.deprecated, &entry.removed]
                .into_iter()
                .flatten()
            {
                if parse_version(version).is_none() {
                    return Err(serde::de::Error::custom(format!(
                        "`{}`: `{version}` is not a Tizen version",
                        entry.name
                    )));
                }
            }
        }
        Ok(catalog)
    }

    /// The catalog bundled with the simulator.
    pub fn bundled() -> &'static ApiCatalog {
        static CATALOG: OnceLock<ApiCatalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            ApiCatalog::parse(include_str!("../data/apis.toml"))
                .expect("bundled apis.toml is valid")
        })
    }

    /// Every member the catalog lists.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.name.as_str())
    }

    /// What Tizen `version` (e.g. `3.0`) has of the listed APIs.
    pub fn for_version(&self, version: &str) -> ApiAvailability {
        let Some(version) = parse_version(version) else {
            return ApiAvailability::default();
        };
        let reached = |at: &Option<String>| {
            at.as_deref()
                .and_then(parse_version)
                .is_some_and(|at| version >= at)
        };
        let mut availability = ApiAvailability::default();
        for entry in &self.entries {
            let added = entry.since.is_none() || reached(&entry.since);
            if !added || reached(&entry.removed) {
                availability.absent.push(entry.name.clone());
            } else if reached(&entry.deprecated) {
                availability.deprecated.push(entry.name.clone());
            }
        }
        availability
    }
}

/// A Tizen version as numbers, so `10.0` sorts after `9.0`.
fn parse_version(version: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = version.split('.').map(|part| part.parse().ok()).collect();
    let mut parts = parts.filter(|parts| !parts.is_empty())?;
    // `8.0` and `8.0.0` are the same release.
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> ApiCatalog {
        ApiCatalog::parse(
            r#"
            [[api]]
            name = "tizen.a.uptime"
            since = "8.0"

            [[api]]
            name = "Enum.NEW"
            since = "10.0"

            [[api]]
            name = "tizen.a.old"
            deprecated = "4.0"
            removed = "6.0"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn members_newer_than_the_profile_are_absent() {
        let on_3 = catalog().for_version("3.0");
        assert!(on_3.absent.contains(&"tizen.a.uptime".to_string()));
        assert!(on_3.absent.contains(&"Enum.NEW".to_string()));

        let on_8 = catalog().for_version("8.0");
        assert!(!on_8.absent.contains(&"tizen.a.uptime".to_string()));
        assert!(on_8.absent.contains(&"Enum.NEW".to_string()));
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(
            !catalog()
                .for_version("10.0")
                .absent
                .contains(&"Enum.NEW".to_string())
        );
        assert!(
            !catalog()
                .for_version("9.0")
                .absent
                .contains(&"tizen.a.uptime".to_string())
        );
    }

    #[test]
    fn deprecated_members_warn_until_removed() {
        let names = |v: &str| catalog().for_version(v);
        assert_eq!(names("3.0").deprecated, Vec::<String>::new());
        assert!(!names("3.0").absent.contains(&"tizen.a.old".to_string()));
        assert_eq!(names("4.0").deprecated, vec!["tizen.a.old".to_string()]);
        assert_eq!(names("5.5").deprecated, vec!["tizen.a.old".to_string()]);
        assert!(names("6.0").deprecated.is_empty());
        assert!(names("6.0").absent.contains(&"tizen.a.old".to_string()));
    }

    #[test]
    fn a_malformed_version_is_rejected() {
        let error = ApiCatalog::parse("[[api]]\nname = \"x\"\nsince = \"soon\"\n").unwrap_err();
        assert!(error.to_string().contains("soon"), "{error}");
    }

    #[test]
    fn the_bundled_catalog_parses() {
        let on_3 = ApiCatalog::bundled().for_version("3.0");
        assert!(
            on_3.absent
                .contains(&"tizen.systeminfo.getDeviceUptime".to_string())
        );
        // The latest lacks only what was removed before it.
        assert_eq!(
            ApiCatalog::bundled().for_version("10.0").absent,
            ["tizen.filesystem.resolve"]
        );
    }
}
