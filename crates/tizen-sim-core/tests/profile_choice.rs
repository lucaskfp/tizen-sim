//! Choosing a Version Profile, and the Version Profiles and Engine Manifest
//! being data: no Engine needed.

use tizen_sim_core::engine::EngineManifest;
use tizen_sim_core::profile::{ProfileCatalog, RememberedProfiles};

const BUNDLED_PROFILES: &str = include_str!("../data/profiles.toml");
const BUNDLED_ENGINES: &str = include_str!("../data/engines.toml");

/// The bundled catalog plus an older and a newer Tizen release, each added
/// as data only.
fn catalog_with_more_releases() -> ProfileCatalog {
    ProfileCatalog::parse(&format!(
        "{BUNDLED_PROFILES}\n\
         [[profile]]\nversion = \"9.0\"\nmilestone = 120\n\n\
         [[profile]]\nversion = \"11.0\"\nmilestone = 140\n"
    ))
    .unwrap()
}

#[test]
fn adding_a_version_profile_is_a_data_only_change() {
    let catalog = catalog_with_more_releases();

    let versions: Vec<&str> = catalog.all().iter().map(|p| p.version.as_str()).collect();
    assert!(
        versions.contains(&"11.0") && versions.contains(&"10.0"),
        "{versions:?}"
    );
    assert_eq!(catalog.latest().version, "11.0");
    assert_eq!(catalog.find("9.0").unwrap().milestone, 120);
}

#[test]
fn adding_an_engine_manifest_entry_is_a_data_only_change() {
    let manifest = EngineManifest::parse(&format!(
        "{BUNDLED_ENGINES}\n\
         [[engine]]\nprofile = \"11.0\"\nmilestone = 140\nplatform = \"linux-x64\"\n\
         url = \"https://example.test/chrome-linux64.zip\"\nsha256 = \"{}\"\n\
         executable = \"chrome-linux64/chrome\"\n",
        "ab".repeat(32)
    ))
    .unwrap();

    let spec = manifest.for_profile("11.0", "linux-x64").unwrap();
    assert_eq!(spec.url, "https://example.test/chrome-linux64.zip");
    assert!(manifest.for_profile("10.0", "windows-x64").is_ok());
    assert!(manifest.for_profile("11.0", "windows-x64").is_err());
}

#[test]
fn an_explicit_profile_wins() {
    let catalog = catalog_with_more_releases();
    let state = tempfile::tempdir().unwrap();
    let remembered = RememberedProfiles::new(state.path());
    remembered.remember("Fixture001.Hello", "10.0").unwrap();

    let chosen = catalog
        .choose(Some("9.0"), "Fixture001.Hello", &remembered)
        .unwrap();
    assert_eq!(chosen.version, "9.0");
}

#[test]
fn a_new_app_gets_the_latest_and_a_second_run_reuses_the_last_profile() {
    let catalog = catalog_with_more_releases();
    let state = tempfile::tempdir().unwrap();

    let first = catalog
        .choose(
            None,
            "Fixture001.Hello",
            &RememberedProfiles::new(state.path()),
        )
        .unwrap();
    assert_eq!(first.version, "11.0");

    RememberedProfiles::new(state.path())
        .remember("Fixture001.Hello", "9.0")
        .unwrap();
    // A later run, e.g. another process, reads it back from the state directory.
    let remembered = RememberedProfiles::new(state.path());
    let second = catalog
        .choose(None, "Fixture001.Hello", &remembered)
        .unwrap();
    assert_eq!(second.version, "9.0");
    // Other Apps are unaffected.
    let other = catalog.choose(None, "Other0001.App", &remembered).unwrap();
    assert_eq!(other.version, "11.0");
}

#[test]
fn a_remembered_profile_that_no_longer_exists_falls_back_to_the_latest() {
    let catalog = catalog_with_more_releases();
    let state = tempfile::tempdir().unwrap();
    let remembered = RememberedProfiles::new(state.path());
    remembered.remember("Fixture001.Hello", "1.0").unwrap();

    let chosen = catalog
        .choose(None, "Fixture001.Hello", &remembered)
        .unwrap();
    assert_eq!(chosen.version, "11.0");
}

#[test]
fn an_unknown_explicit_profile_is_an_error_naming_the_known_ones() {
    let catalog = catalog_with_more_releases();
    let state = tempfile::tempdir().unwrap();
    let err = catalog
        .choose(
            Some("2.4"),
            "Fixture001.Hello",
            &RememberedProfiles::new(state.path()),
        )
        .unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("2.4") && message.contains("11.0"),
        "{message}"
    );
}
