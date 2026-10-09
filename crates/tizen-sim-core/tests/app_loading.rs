//! Secondary seam: App Source -> App, no Engine involved.

mod support;

use support::fixture;
use tizen_sim_core::app::{App, AppError, AppSource};

fn load(name: &str) -> Result<App, AppError> {
    App::load(&AppSource::ProjectFolder(fixture(name)))
}

#[test]
fn reads_id_entry_point_and_required_version() {
    let app = load("hello").unwrap();
    assert_eq!(app.id(), "Fixture001.Hello");
    assert_eq!(app.entry_point(), "index.html");
    assert_eq!(app.required_version(), Some("2.3"));
}

#[test]
fn missing_config_names_the_file() {
    let err = load("missing-config").unwrap_err();
    assert!(matches!(err, AppError::ConfigMissing { .. }), "{err:?}");
    assert!(err.to_string().contains("config.xml"), "{err}");
}

#[test]
fn malformed_config_names_the_position() {
    let err = load("malformed-config").unwrap_err();
    assert!(matches!(err, AppError::ConfigMalformed { .. }), "{err:?}");
    let msg = err.to_string();
    assert!(msg.contains("config.xml") && msg.contains("line"), "{msg}");
}

#[test]
fn missing_application_element_is_named() {
    let err = load("no-application").unwrap_err();
    assert!(err.to_string().contains("tizen:application"), "{err}");
}

#[test]
fn missing_entry_point_file_is_named() {
    let err = load("missing-entry-point").unwrap_err();
    assert!(matches!(err, AppError::EntryPointMissing { .. }), "{err:?}");
    assert!(err.to_string().contains("main.html"), "{err}");
}
