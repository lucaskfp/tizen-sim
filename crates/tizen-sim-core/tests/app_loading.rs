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
    assert_eq!(app.required_version(), "2.3");
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

#[test]
fn reads_name_version_package_and_privileges() {
    let app = load("tizen-apis-privileged").unwrap();
    assert_eq!(app.name(), "Tizen APIs fixture");
    assert_eq!(app.version(), "1.2.3");
    assert_eq!(app.package(), "Fixture005");
    assert_eq!(
        app.privileges(),
        ["http://tizen.org/privilege/application.info"]
    );
}

#[test]
fn an_app_without_privileges_declares_none() {
    let app = load("tizen-apis").unwrap();
    assert!(app.privileges().is_empty());
}

/// Loads an App whose `config.xml` has the given `<widget>` attributes and
/// children, from a throwaway Project Folder.
fn load_config(widget_attributes: &str, children: &str) -> Result<App, AppError> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
    std::fs::write(
        dir.path().join("config.xml"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<widget xmlns="http://www.w3.org/ns/widgets" xmlns:tizen="http://tizen.org/ns/widgets" {widget_attributes}>
    {children}
</widget>"#
        ),
    )
    .unwrap();
    App::load(&AppSource::ProjectFolder(dir.path().to_path_buf()))
}

fn load_application(attributes: &str) -> Result<App, AppError> {
    load_config("", &format!("<tizen:application {attributes}/>"))
}

const VALID: &str = r#"id="abcDEF1234.MyApp" package="abcDEF1234" required_version="2.4""#;

fn assert_names(err: AppError, words: &[&str]) {
    let msg = err.to_string();
    for word in words {
        assert!(msg.contains(word), "`{word}` missing from: {msg}");
    }
}

/// TD reference/config-xml.md, `<tizen:application>`: `package` is 10
/// alphanumeric characters; `id` is `<package>.<name>` with a 1-52 byte
/// alphanumeric name; `required_version` is mandatory.
#[test]
fn tizen_application_attributes_are_validated() {
    assert!(load_application(VALID).is_ok());

    let err = load_application(r#"id="abcDEF1234.MyApp" required_version="2.4""#).unwrap_err();
    assert_names(err, &["tizen:application", "package"]);
    for package in ["abc", "abcDEF12345", "abcDEF-123"] {
        let err = load_application(&format!(
            r#"id="{package}.MyApp" package="{package}" required_version="2.4""#
        ))
        .unwrap_err();
        assert_names(err, &["package", package, "10 alphanumeric"]);
    }

    let err =
        load_application(r#"id="zzzzzz1234.MyApp" package="abcDEF1234" required_version="2.4""#)
            .unwrap_err();
    assert_names(err, &["id", "zzzzzz1234.MyApp", "abcDEF1234."]);
    let long_name = "a".repeat(53);
    for name in ["", "My-App", long_name.as_str()] {
        let id = format!("abcDEF1234.{name}");
        let err = load_application(&format!(
            r#"id="{id}" package="abcDEF1234" required_version="2.4""#
        ))
        .unwrap_err();
        assert_names(err, &["id", &id, "1-52 alphanumeric"]);
    }
    assert!(
        load_application(&format!(
            r#"id="abcDEF1234.{}" package="abcDEF1234" required_version="2.4""#,
            "a".repeat(52)
        ))
        .is_ok()
    );

    let err = load_application(r#"id="abcDEF1234.MyApp" package="abcDEF1234""#).unwrap_err();
    assert_names(err, &["tizen:application", "required_version"]);
    let err =
        load_application(r#"id="abcDEF1234.MyApp" package="abcDEF1234" required_version="latest""#)
            .unwrap_err();
    assert_names(err, &["required_version", "latest"]);
}

/// TD reference/config-xml.md, `<widget version>`: `[0-255].[0-255].[0-65535]`.
#[test]
fn widget_version_has_the_documented_form_when_present() {
    let application = format!("<tizen:application {VALID}/>");
    for version in ["0.0.0", "1.2.3", "255.255.65535"] {
        let app = load_config(&format!(r#"version="{version}""#), &application).unwrap();
        assert_eq!(app.version(), version);
    }
    for version in [
        "1.0",
        "1.2.3.4",
        "256.0.0",
        "1.256.0",
        "1.0.65536",
        "1.0.x",
        "",
        "01.0.0",
    ] {
        let err = load_config(&format!(r#"version="{version}""#), &application).unwrap_err();
        assert_names(err, &["widget", "version", "[0-255].[0-255].[0-65535]"]);
    }
}

/// TD guides/concepts/web-runtime.md: installation aborts when a public App
/// declares a partner- or platform-level privilege (levels from TD
/// reference/security-privileges.md).
#[test]
fn partner_and_platform_privileges_are_refused() {
    for (privilege, level) in [
        ("http://tizen.org/privilege/appmanager.kill", "partner"),
        (
            "http://tizen.org/privilege/packagemanager.install",
            "platform",
        ),
    ] {
        let err = load_config(
            "",
            &format!(r#"<tizen:application {VALID}/><tizen:privilege name="{privilege}"/>"#),
        )
        .unwrap_err();
        assert!(
            matches!(err, AppError::PrivilegeLevelTooHigh { .. }),
            "{err:?}"
        );
        assert_names(err, &[privilege, level]);
    }
    assert!(load_config(
        "",
        &format!(
            r#"<tizen:application {VALID}/><tizen:privilege name="http://tizen.org/privilege/tv.inputdevice"/>"#
        ),
    )
    .is_ok());
}
