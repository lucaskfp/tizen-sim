//! App loading: turns an App Source into an App by reading its `config.xml`.
//!
//! What a TV checks when it installs the App is checked here too, so an App
//! that wouldn't install fails to load (TD reference/config-xml.md,
//! guides/concepts/web-runtime.md, reference/security-privileges.md).

use crate::profile::VersionProfile;
use std::path::{Path, PathBuf};

const WIDGET_NS: &str = "http://www.w3.org/ns/widgets";
const TIZEN_NS: &str = "http://tizen.org/ns/widgets";

/// Where an App's files come from.
#[derive(Debug, Clone)]
pub enum AppSource {
    /// A directory containing the App's unpacked files, including `config.xml`.
    ProjectFolder(PathBuf),
}

/// A Tizen TV web application, as declared by its `config.xml`.
#[derive(Debug, Clone)]
pub struct App {
    id: String,
    package: String,
    name: String,
    version: String,
    privileges: Vec<String>,
    required_version: String,
    background_support: bool,
    background_categories: Vec<String>,
    entry_point: String,
    root: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("no config.xml found at {path}")]
    ConfigMissing { path: PathBuf },
    #[error("could not read {path}: {source}")]
    ConfigUnreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not well-formed XML (line {line}, column {column}): {message}")]
    ConfigMalformed {
        path: PathBuf,
        line: u32,
        column: u32,
        message: String,
    },
    #[error("{path}: the root element must be <widget> in the {WIDGET_NS} namespace")]
    NotAWidget { path: PathBuf },
    #[error("{path}: missing required element <{element}>")]
    MissingElement {
        path: PathBuf,
        element: &'static str,
    },
    #[error("{path}: <{element}> is missing its required `{attribute}` attribute")]
    MissingAttribute {
        path: PathBuf,
        element: &'static str,
        attribute: &'static str,
    },
    #[error("{path}: <{element} {attribute}=\"{value}\"> is invalid: {expected}")]
    InvalidAttribute {
        path: PathBuf,
        element: &'static str,
        attribute: &'static str,
        value: String,
        expected: String,
    },
    #[error(
        "{path}: declares the {level}-level privilege {privilege}; a TV refuses to install an App \
         that declares partner- or platform-level privileges"
    )]
    PrivilegeLevelTooHigh {
        path: PathBuf,
        privilege: String,
        level: &'static str,
    },
    #[error(
        "the App's required_version is {required}, but Tizen {profile} TVs are older; \
         a TV refuses to install it"
    )]
    RequiresNewerTizen { required: String, profile: String },
    #[error("entry point `{entry_point}` declared in config.xml does not exist in {root}")]
    EntryPointMissing { root: PathBuf, entry_point: String },
}

/// Privileges above the public level, which a public App may not declare:
/// every Web API privilege TD reference/security-privileges.md lists as
/// partner or platform level.
const RESTRICTED_PRIVILEGES: &[(&str, &str)] = &[
    (
        "http://tizen.org/privilege/appmanager.certificate",
        "partner",
    ),
    ("http://tizen.org/privilege/appmanager.kill", "partner"),
    ("http://tizen.org/privilege/bluetoothmanager", "platform"),
    ("http://tizen.org/privilege/bookmark.read", "platform"),
    ("http://tizen.org/privilege/bookmark.write", "platform"),
    (
        "http://tizen.org/privilege/networkbearerselection",
        "partner",
    ),
    (
        "http://tizen.org/privilege/packagemanager.install",
        "platform",
    ),
    ("http://tizen.org/privilege/systemmanager", "partner"),
    ("http://tizen.org/privilege/tee.client", "partner"),
];

impl App {
    pub fn load(source: &AppSource) -> Result<App, AppError> {
        match source {
            AppSource::ProjectFolder(root) => load_folder(root),
        }
    }

    /// The application id from `<tizen:application id>`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The package id from `<tizen:application package>`.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The App's display name from `<name>`, falling back to its id.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The App's version from `<widget version>`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The privileges declared with `<tizen:privilege name>`.
    pub fn privileges(&self) -> &[String] {
        &self.privileges
    }

    /// The minimum Tizen version the App declares support for.
    pub fn required_version(&self) -> &str {
        &self.required_version
    }

    /// Fails as installation does on a TV whose Tizen version is lower than
    /// the App's `required_version` (TD guides/concepts/web-runtime.md).
    pub fn check_installable_on(&self, profile: &VersionProfile) -> Result<(), AppError> {
        if parse_tizen_version(&self.required_version) > parse_tizen_version(&profile.version) {
            return Err(AppError::RequiresNewerTizen {
                required: self.required_version.clone(),
                profile: profile.version.clone(),
            });
        }
        Ok(())
    }

    /// Whether the App keeps running while hidden: background support plus at
    /// least one `<tizen:background-category>`. Without a category, "a web
    /// application process can be suspended in the background" (TD
    /// reference/config-xml.md, `background-support`), so it is suspended here.
    pub fn runs_in_background(&self) -> bool {
        self.background_support && !self.background_categories.is_empty()
    }

    /// The entry page, relative to the App's root.
    pub fn entry_point(&self) -> &str {
        &self.entry_point
    }

    /// The directory the App's files are served from.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn load_folder(root: &Path) -> Result<App, AppError> {
    let path = root.join("config.xml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::ConfigMissing { path });
        }
        Err(source) => return Err(AppError::ConfigUnreadable { path, source }),
    };
    let doc = roxmltree::Document::parse(&text).map_err(|e| {
        let pos = e.pos();
        AppError::ConfigMalformed {
            path: path.clone(),
            line: pos.row,
            column: pos.col,
            message: e.to_string(),
        }
    })?;

    let widget = doc.root_element();
    if !widget.has_tag_name((WIDGET_NS, "widget")) {
        return Err(AppError::NotAWidget { path });
    }

    let application = widget
        .children()
        .find(|n| n.has_tag_name((TIZEN_NS, "application")))
        .ok_or_else(|| AppError::MissingElement {
            path: path.clone(),
            element: "tizen:application",
        })?;
    let attribute = |name: &'static str| {
        application
            .attribute(name)
            .map(String::from)
            .ok_or_else(|| AppError::MissingAttribute {
                path: path.clone(),
                element: "tizen:application",
                attribute: name,
            })
    };
    let invalid = |element, attribute, value: &str, expected: String| AppError::InvalidAttribute {
        path: path.clone(),
        element,
        attribute,
        value: value.to_string(),
        expected,
    };
    // All three are mandatory (TD reference/config-xml.md, <tizen:application>).
    let id = attribute("id")?;
    let package = attribute("package")?;
    let required_version = attribute("required_version")?;
    if package.len() != 10 || !is_alphanumeric(&package) {
        return Err(invalid(
            "tizen:application",
            "package",
            &package,
            "must be 10 alphanumeric characters".into(),
        ));
    }
    let valid_id = id
        .strip_prefix(&format!("{package}."))
        .is_some_and(|name| (1..=52).contains(&name.len()) && is_alphanumeric(name));
    if !valid_id {
        return Err(invalid(
            "tizen:application",
            "id",
            &id,
            format!("must be {package}.<name>, where the name is 1-52 alphanumeric characters"),
        ));
    }
    if !is_tizen_version(&required_version) {
        return Err(invalid(
            "tizen:application",
            "required_version",
            &required_version,
            "must be a Tizen version such as 2.4".into(),
        ));
    }
    let name = widget
        .children()
        .find(|n| n.has_tag_name((WIDGET_NS, "name")))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| id.clone());
    let version = widget.attribute("version").unwrap_or_default().to_string();
    if widget.attribute("version").is_some() && !is_widget_version(&version) {
        return Err(invalid(
            "widget",
            "version",
            &version,
            "expected [0-255].[0-255].[0-65535]".into(),
        ));
    }
    let privileges: Vec<String> = widget
        .children()
        .filter(|n| n.has_tag_name((TIZEN_NS, "privilege")))
        .filter_map(|n| n.attribute("name"))
        .map(String::from)
        .collect();
    // Installation aborts when a public App declares a partner- or
    // platform-level privilege (TD guides/concepts/web-runtime.md).
    for privilege in &privileges {
        if let Some(&(_, level)) = RESTRICTED_PRIVILEGES.iter().find(|(p, _)| p == privilege) {
            return Err(AppError::PrivilegeLevelTooHigh {
                path,
                privilege: privilege.clone(),
                level,
            });
        }
    }
    let background_support = widget
        .children()
        .filter(|n| n.has_tag_name((TIZEN_NS, "setting")))
        .find_map(|n| n.attribute("background-support"))
        == Some("enable");
    let background_categories = widget
        .children()
        .filter(|n| n.has_tag_name((TIZEN_NS, "background-category")))
        .filter_map(|n| n.attribute("value"))
        .map(String::from)
        .collect();

    // Per the W3C widget spec, without <content> the start file is index.html.
    let entry_point = widget
        .children()
        .find(|n| n.has_tag_name((WIDGET_NS, "content")))
        .and_then(|n| n.attribute("src"))
        .unwrap_or("index.html")
        .to_string();
    if !root.join(&entry_point).is_file() {
        return Err(AppError::EntryPointMissing {
            root: root.to_path_buf(),
            entry_point,
        });
    }

    Ok(App {
        id,
        package,
        name,
        version,
        privileges,
        required_version,
        background_support,
        background_categories,
        entry_point,
        root: root.to_path_buf(),
    })
}

fn is_alphanumeric(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `[0-255].[0-255].[0-65535]`. Rejecting leading zeros is an assumption the
/// doc doesn't state: unverified.
fn is_widget_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().zip([255, 255, 65535]).all(|(part, max)| {
            is_number(part)
                && (part.len() == 1 || !part.starts_with('0'))
                && part.parse::<u32>().is_ok_and(|n| n <= max)
        })
}

/// A Tizen version such as `2.4`. The doc gives only the two-part form; also
/// accepting `6.5.0` is unverified.
fn is_tizen_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    (2..=3).contains(&parts.len()) && parts.iter().all(|part| is_number(part) && part.len() <= 5)
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Major, minor and patch, missing parts as 0. Callers pass validated versions.
fn parse_tizen_version(text: &str) -> [u32; 3] {
    let mut version = [0; 3];
    for (slot, part) in version.iter_mut().zip(text.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    version
}
