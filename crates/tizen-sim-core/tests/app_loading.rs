//! Secondary seam: App Source -> App, no Engine involved.

mod support;

use support::{fixture, fixture_entries, write_package};
use tizen_sim_core::app::{AccessPolicy, AccessRequest, App, AppError, AppSource, CspPolicy};
use tizen_sim_core::profile::VersionProfile;

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

/// A `config.xml` with a valid `<tizen:application>`, the given `<widget>`
/// children and the usual namespaces.
fn config_xml(children: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<widget xmlns="http://www.w3.org/ns/widgets" xmlns:tizen="http://tizen.org/ns/widgets">
    <tizen:application {VALID}/>
    {children}
</widget>"#
    )
}

fn load_xml(xml: &str) -> Result<App, AppError> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
    std::fs::write(dir.path().join("config.xml"), xml).unwrap();
    App::load(&AppSource::ProjectFolder(dir.path().to_path_buf()))
}

fn policy(children: &str) -> AccessPolicy {
    load_xml(&config_xml(children))
        .unwrap()
        .access_policy()
        .clone()
}

/// Elements count only in their own namespace, whatever prefix binds it
/// (TD reference/config-xml.md: W3C elements in `http://www.w3.org/ns/widgets`,
/// Tizen ones in `http://tizen.org/ns/widgets`).
#[test]
fn elements_are_matched_by_namespace_not_prefix() {
    let cases: &[(&str, &str, Result<&str, &str>)] = &[
        (
            "other prefixes bound to the right namespaces",
            r#"<w:widget xmlns:w="http://www.w3.org/ns/widgets" xmlns:t="http://tizen.org/ns/widgets">
                <t:application id="abcDEF1234.MyApp" package="abcDEF1234" required_version="2.4"/>
                <w:name>Prefixed</w:name>
            </w:widget>"#,
            Ok("Prefixed"),
        ),
        (
            "a root <widget> in no namespace",
            r#"<widget><tizen:application xmlns:tizen="http://tizen.org/ns/widgets" id="abcDEF1234.MyApp" package="abcDEF1234" required_version="2.4"/></widget>"#,
            Err("namespace"),
        ),
        (
            "tizen: bound to another namespace",
            r#"<widget xmlns="http://www.w3.org/ns/widgets" xmlns:tizen="http://example.com/not-tizen">
                <tizen:application id="abcDEF1234.MyApp" package="abcDEF1234" required_version="2.4"/>
            </widget>"#,
            Err("tizen:application"),
        ),
        (
            "<application> in the widgets namespace",
            r#"<widget xmlns="http://www.w3.org/ns/widgets">
                <application id="abcDEF1234.MyApp" package="abcDEF1234" required_version="2.4"/>
            </widget>"#,
            Err("tizen:application"),
        ),
    ];
    for (case, xml, expected) in cases {
        match (load_xml(xml), expected) {
            (Ok(app), Ok(name)) => assert_eq!(app.name(), *name, "{case}"),
            (Err(err), Err(word)) => assert!(err.to_string().contains(word), "{case}: {err}"),
            (result, _) => panic!("{case}: unexpected {result:?}"),
        }
    }

    // Access Policy elements in the wrong namespace don't count either.
    let wrong = r#"<tizen:access origin="*"/>
        <content-security-policy>script-src 'self'</content-security-policy>
        <allow-navigation>example.com</allow-navigation>"#;
    assert_eq!(policy(wrong), AccessPolicy::Warp(vec![]));
}

/// TD reference/config-xml.md: `<tizen:application>` occurs once, with
/// `id`, `package` and `required_version` all mandatory.
#[test]
fn missing_mandatory_elements_and_attributes_are_named() {
    let cases = [
        ("", "tizen:application"),
        (
            r#"<tizen:application package="abcDEF1234" required_version="2.4"/>"#,
            "`id`",
        ),
        (
            r#"<tizen:application id="abcDEF1234.MyApp" required_version="2.4"/>"#,
            "`package`",
        ),
        (
            r#"<tizen:application id="abcDEF1234.MyApp" package="abcDEF1234"/>"#,
            "`required_version`",
        ),
    ];
    for (children, named) in cases {
        let err = load_config("", children).unwrap_err();
        assert!(
            matches!(
                err,
                AppError::MissingElement { .. } | AppError::MissingAttribute { .. }
            ),
            "{children}: {err:?}"
        );
        assert_names(err, &["tizen:application", named]);
    }
}

/// TD reference/config-xml.md: `required_version` is a version such as
/// `2.4`; TD guides/concepts/web-runtime.md: installation aborts when the
/// platform's version is lower.
#[test]
fn required_version_is_parsed_and_compared_with_the_profile() {
    let profile = VersionProfile::latest();
    assert_eq!(profile.version, "10.0");
    let installable = |required: &str| {
        let app = load_application(&format!(
            r#"id="abcDEF1234.MyApp" package="abcDEF1234" required_version="{required}""#
        ))
        .unwrap();
        assert_eq!(app.required_version(), required);
        app.check_installable_on(profile).is_ok()
    };
    for (required, expected) in [
        ("2.3", true),
        ("9.0", true),
        ("10.0", true),
        ("10.0.0", true),
        ("6.5.1", true),
        ("10.1", false),
        ("10.0.1", false),
        ("11.0", false),
        ("99.0", false),
    ] {
        assert_eq!(
            installable(required),
            expected,
            "required_version {required}"
        );
    }

    for invalid in [
        "", "latest", "2", "2.", ".4", "2.4.1.1", "2.x", "-2.4", " 2.4",
    ] {
        let err = load_application(&format!(
            r#"id="abcDEF1234.MyApp" package="abcDEF1234" required_version="{invalid}""#
        ))
        .unwrap_err();
        assert!(
            matches!(err, AppError::InvalidAttribute { .. }),
            "{invalid:?}: {err:?}"
        );
        assert_names(err, &["required_version"]);
    }
}

/// TD guides/concepts/web-runtime.md: any `tizen:content-security-policy`,
/// `-report-only` or `tizen:allow-navigation` puts the App in CSP mode;
/// otherwise it is in WARP mode.
#[test]
fn csp_elements_select_csp_mode_and_warp_is_the_default() {
    let csp = |policy, report_only, allow_navigation| {
        AccessPolicy::Csp(CspPolicy {
            policy,
            report_only,
            allow_navigation,
        })
    };
    let s = |text: &str| Some(text.to_string());
    let cases = [
        ("", AccessPolicy::Warp(vec![])),
        (
            r#"<access origin="*"/>"#,
            AccessPolicy::Warp(vec![AccessRequest::Any]),
        ),
        (
            "<tizen:content-security-policy> script-src 'self' </tizen:content-security-policy>",
            csp(s("script-src 'self'"), None, None),
        ),
        (
            "<tizen:content-security-policy-report-only>img-src *</tizen:content-security-policy-report-only>",
            csp(None, s("img-src *"), None),
        ),
        (
            "<tizen:allow-navigation>tizen.org *.tizen.org</tizen:allow-navigation>",
            csp(
                None,
                None,
                Some(vec!["tizen.org".into(), "*.tizen.org".into()]),
            ),
        ),
        (
            r#"<access origin="*"/><tizen:allow-navigation>example.com</tizen:allow-navigation>"#,
            csp(None, None, Some(vec!["example.com".into()])),
        ),
    ];
    for (children, expected) in cases {
        assert_eq!(policy(children), expected, "{children}");
    }
}

/// TD guides/concepts/web-runtime.md: the default CSP applies unless
/// `config.xml` defines one, which replaces it.
#[test]
fn the_default_csp_applies_only_without_a_configured_one() {
    let enforced = |children: &str| match policy(children) {
        AccessPolicy::Csp(csp) => csp.enforced().map(String::from),
        warp => panic!("{children}: expected CSP mode, got {warp:?}"),
    };
    assert_eq!(
        enforced("<tizen:allow-navigation>example.com</tizen:allow-navigation>").as_deref(),
        Some(CspPolicy::DEFAULT)
    );
    assert_eq!(
        enforced(
            "<tizen:content-security-policy>default-src 'self'</tizen:content-security-policy>"
        )
        .as_deref(),
        Some("default-src 'self'")
    );
    assert_eq!(
        enforced(
            "<tizen:content-security-policy-report-only>default-src 'self'</tizen:content-security-policy-report-only>"
        ),
        None
    );
}

/// TD reference/config-xml.md: for each of these elements, "if more than 1,
/// the first occurrence is applied".
#[test]
fn the_first_occurrence_of_each_csp_element_wins() {
    let children = "
        <tizen:content-security-policy>script-src 'self'</tizen:content-security-policy>
        <tizen:content-security-policy>script-src *</tizen:content-security-policy>
        <tizen:content-security-policy-report-only>img-src 'self'</tizen:content-security-policy-report-only>
        <tizen:content-security-policy-report-only>img-src *</tizen:content-security-policy-report-only>
        <tizen:allow-navigation>first.example.com</tizen:allow-navigation>
        <tizen:allow-navigation>second.example.com</tizen:allow-navigation>";
    assert_eq!(
        policy(children),
        AccessPolicy::Csp(CspPolicy {
            policy: Some("script-src 'self'".into()),
            report_only: Some("img-src 'self'".into()),
            allow_navigation: Some(vec!["first.example.com".into()]),
        })
    );
}

/// W3C Widget Access Request Policy (TD guides/concepts/web-runtime.md names
/// it for WARP mode): "Rule for processing an access element" and "Rules for
/// Granting Access to a Network Resource".
#[test]
fn access_elements_grant_origins_with_the_warp_subdomain_rules() {
    let exact = r#"<access origin="https://example.net"/>"#;
    let port = r#"<access origin="http://dahut.example.com:4242"/>"#;
    let subdomains = r#"<access origin="http://example.org" subdomains="true"/>"#;
    let cases: &[(&str, &str, bool)] = &[
        // No <access>: everything external is denied.
        ("", "https://example.net/", false),
        // Scheme, host and port must all match.
        (exact, "https://example.net/a?b", true),
        (exact, "https://example.net:443/", true),
        (exact, "http://example.net/", false),
        (exact, "https://example.net:8443/", false),
        (exact, "https://www.example.net/", false),
        (port, "http://dahut.example.com:4242/", true),
        (port, "http://dahut.example.com/", false),
        // Hosts compare case-insensitively.
        (
            r#"<access origin="https://EXAMPLE.net"/>"#,
            "https://example.NET/",
            true,
        ),
        // subdomains="true" adds subdomains at any depth, but not look-alikes.
        (subdomains, "http://example.org/", true),
        (subdomains, "http://a.b.example.org/", true),
        (subdomains, "http://badexample.org/", false),
        (subdomains, "http://example.org.evil.com/", false),
        // subdomains defaults to false, and anything but "true" or "false"
        // (case-sensitively, after trimming) is false.
        (
            r#"<access origin="http://example.org"/>"#,
            "http://a.example.org/",
            false,
        ),
        (
            r#"<access origin="http://example.org" subdomains="yes"/>"#,
            "http://a.example.org/",
            false,
        ),
        (
            r#"<access origin="http://example.org" subdomains="TRUE"/>"#,
            "http://a.example.org/",
            false,
        ),
        (
            r#"<access origin="http://example.org" subdomains=" true "/>"#,
            "http://a.example.org/",
            true,
        ),
        // An IP address has no subdomains.
        (
            r#"<access origin="http://10.0.0.1" subdomains="true"/>"#,
            "http://10.0.0.1/",
            true,
        ),
        // "*" grants everything, whatever else is declared.
        (
            r#"<access origin="https://example.net"/><access origin="*"/>"#,
            "http://other.org:81/",
            true,
        ),
        (r#"<access origin=" * "/>"#, "http://other.org/", true),
        // Every <access> adds to the union.
        (
            r#"<access origin="https://a.com"/><access origin="https://b.com"/>"#,
            "https://b.com/",
            true,
        ),
        // Elements in error are ignored: no origin, a path, a query, a
        // fragment, user info, no host.
        (
            r#"<access subdomains="true"/>"#,
            "https://example.net/",
            false,
        ),
        (
            r#"<access origin="https://example.net/path"/>"#,
            "https://example.net/path",
            false,
        ),
        (
            r#"<access origin="https://example.net?q"/>"#,
            "https://example.net/",
            false,
        ),
        (
            r#"<access origin="https://example.net#f"/>"#,
            "https://example.net/",
            false,
        ),
        (
            r#"<access origin="https://user@example.net"/>"#,
            "https://example.net/",
            false,
        ),
        (
            r#"<access origin="example.net"/>"#,
            "https://example.net/",
            false,
        ),
        // A trailing "/" is an empty path, not an extra component.
        (
            r#"<access origin="https://example.net/"/>"#,
            "https://example.net/",
            true,
        ),
    ];
    for (children, url, granted) in cases {
        assert_eq!(
            policy(children).warp_grants(url),
            Some(*granted),
            "{children} -> {url}"
        );
    }
    assert_eq!(
        policy(r#"<access origin="*"/><tizen:allow-navigation>a.com</tizen:allow-navigation>"#)
            .warp_grants("https://a.com/"),
        None,
        "<access> is not used in CSP mode"
    );
}

/// Writes `entries` as `app.wgt` in a throwaway directory and loads it.
fn load_package(entries: &[(String, Vec<u8>)]) -> Result<App, AppError> {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    write_package(&wgt, entries);
    App::load(&AppSource::Package(wgt))
}

fn entry(name: &str, contents: &str) -> (String, Vec<u8>) {
    (name.to_string(), contents.as_bytes().to_vec())
}

#[test]
fn a_package_is_unpacked_and_loaded_like_a_project_folder() {
    let mut entries = fixture_entries("hello");
    entries.push(entry("css/style.css", "body {}"));
    let app = load_package(&entries).unwrap();
    assert_eq!(app.id(), "Fixture001.Hello");
    assert_eq!(app.entry_point(), "index.html");
    assert!(app.root().join("index.html").is_file());
    assert!(app.root().join("css/style.css").is_file());
}

#[test]
fn an_unpacked_package_is_removed_with_the_last_app_using_it() {
    let app = load_package(&fixture_entries("hello")).unwrap();
    let root = app.root().to_path_buf();
    let copy = app.clone();
    drop(app);
    assert!(root.is_dir(), "a clone still uses it");
    drop(copy);
    assert!(!root.exists(), "{} was left behind", root.display());
}

/// TD guides/concepts/web-runtime.md: Packages may carry W3C widget
/// signatures (`author-signature.xml`, `signature1.xml`). tizen-sim doesn't
/// verify them (signing is out of scope), so signed and unsigned Packages
/// load alike.
#[test]
fn packages_load_with_and_without_signature_files() {
    let unsigned = fixture_entries("hello");
    let mut signed = unsigned.clone();
    signed.push(entry("author-signature.xml", "<Signature/>"));
    signed.push(entry("signature1.xml", "<Signature/>"));
    for entries in [unsigned, signed] {
        let app = load_package(&entries).unwrap();
        assert_eq!(app.id(), "Fixture001.Hello");
    }
}

#[test]
fn malformed_packages_fail_with_the_problem_named() {
    let dir = tempfile::tempdir().unwrap();
    let valid_path = dir.path().join("valid.wgt");
    write_package(&valid_path, &fixture_entries("hello"));
    let valid = std::fs::read(&valid_path).unwrap();

    for (name, bytes) in [
        ("not-a-zip.wgt", b"this is not a zip archive".as_slice()),
        ("empty.wgt", b"".as_slice()),
        ("truncated.wgt", &valid[..valid.len() / 2]),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let err = App::load(&AppSource::Package(path)).unwrap_err();
        assert!(
            matches!(err, AppError::PackageCorrupt { .. }),
            "{name}: {err:?}"
        );
        assert_names(err, &[name, "not a valid Package"]);
    }

    let err = App::load(&AppSource::Package(dir.path().join("absent.wgt"))).unwrap_err();
    assert!(matches!(err, AppError::PackageUnreadable { .. }), "{err:?}");
    assert_names(err, &["absent.wgt"]);

    let err = load_package(&[entry("index.html", "<html></html>")]).unwrap_err();
    assert!(matches!(err, AppError::ConfigMissing { .. }), "{err:?}");
    assert_names(err, &["app.wgt", "config.xml"]);

    let mut escaping = fixture_entries("hello");
    escaping.push(entry("../escape.txt", "outside"));
    let err = load_package(&escaping).unwrap_err();
    assert!(matches!(err, AppError::PackageCorrupt { .. }), "{err:?}");
    assert_names(err, &["../escape.txt"]);

    // config.xml problems name the file inside the Package.
    let mut broken = fixture_entries("hello");
    broken.retain(|(name, _)| name != "config.xml");
    broken.push(entry("config.xml", "<widget"));
    let err = load_package(&broken).unwrap_err();
    assert!(matches!(err, AppError::ConfigMalformed { .. }), "{err:?}");
    assert_names(err, &["app.wgt", "config.xml", "line"]);
}
