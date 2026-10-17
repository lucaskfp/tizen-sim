//! App loading: turns an App Source into an App by reading its `config.xml`.
//! A Package is unpacked first, then read like a Project Folder. A Dev Server
//! URL App is read from the `config.xml` it is paired with.
//!
//! What a TV checks when it installs the App is checked here too, so an App
//! that wouldn't install fails to load (TD reference/config-xml.md,
//! guides/concepts/web-runtime.md, reference/security-privileges.md).

use crate::profile::VersionProfile;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use url::{Host, Url};

const WIDGET_NS: &str = "http://www.w3.org/ns/widgets";
const TIZEN_NS: &str = "http://tizen.org/ns/widgets";
/// The TV metadata naming the gamepad models an App needs
/// (TV guides/user-interaction/gamepad.html).
const GAMEPAD_METADATA: &str = "http://samsung.com/tv/metadata/gamepad";

/// Where an App's files come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppSource {
    /// A directory containing the App's unpacked files, including `config.xml`.
    ProjectFolder(PathBuf),
    /// A packaged App: a `.wgt` file, which is a zip archive (W3C Widget
    /// Packaging, which TD guides/concepts/web-runtime.md follows).
    Package(PathBuf),
    /// The developer's own bundler dev server, paired with the App's
    /// `config.xml`, which can live anywhere.
    DevServerUrl { url: Url, config: PathBuf },
}

impl AppSource {
    /// A `.wgt` file is a Package; any other path is taken for a Project Folder.
    pub fn from_path(path: PathBuf) -> AppSource {
        let is_package = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wgt"));
        if is_package {
            AppSource::Package(path)
        } else {
            AppSource::ProjectFolder(path)
        }
    }

    /// The same source with its paths absolute, so another process (the GUI
    /// adopting a Session) finds it from any working directory. A path that
    /// can't be made absolute is kept as it is.
    pub fn absolute(&self) -> AppSource {
        let absolute = |path: &PathBuf| std::path::absolute(path).unwrap_or_else(|_| path.clone());
        match self {
            AppSource::ProjectFolder(path) => AppSource::ProjectFolder(absolute(path)),
            AppSource::Package(path) => AppSource::Package(absolute(path)),
            AppSource::DevServerUrl { url, config } => AppSource::DevServerUrl {
                url: url.clone(),
                config: absolute(config),
            },
        }
    }
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
    access_policy: AccessPolicy,
    required_gamepads: Vec<String>,
    files: AppFiles,
    /// Where a Package was unpacked; removed once the last clone is dropped.
    _unpacked: Option<Arc<tempfile::TempDir>>,
}

/// Where the App's files are served from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppFiles {
    /// A Project Folder, or the directory a Package was unpacked into.
    Folder(PathBuf),
    /// The developer's bundler dev server, at the Dev Server URL.
    Bundler(Url),
}

/// The network and navigation rules an App declares in its `config.xml`
/// (TD guides/concepts/web-runtime.md, "Content security policy").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessPolicy {
    /// No CSP element: network access is governed by the `<access>`
    /// elements, per the W3C Widget Access Request Policy (WARP).
    Warp(Vec<AccessRequest>),
    /// At least one `tizen:content-security-policy`, `-report-only` or
    /// `tizen:allow-navigation` element.
    Csp(CspPolicy),
}

/// The CSP-mode elements, each its first occurrence ("if more than 1, the
/// first occurrence is applied", TD reference/config-xml.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CspPolicy {
    /// `<tizen:content-security-policy>`, trimmed.
    pub policy: Option<String>,
    /// `<tizen:content-security-policy-report-only>`, trimmed.
    pub report_only: Option<String>,
    /// The domains in `<tizen:allow-navigation>`, e.g. `tizen.org *.tizen.org`.
    pub allow_navigation: Option<Vec<String>>,
}

/// One `<access>` element, as the WARP "rule for processing an access
/// element" adds it to the access-request list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessRequest {
    /// `origin="*"`: all network access.
    Any,
    Origin {
        scheme: String,
        /// Lowercase and, for http(s), IDNA-encoded, as the WARP rule requires.
        host: String,
        /// The explicit port, or the scheme's default.
        port: u16,
        subdomains: bool,
    },
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
    #[error("could not read the Package {path}: {source}")]
    PackageUnreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a valid Package (a .wgt zip archive): {message}")]
    PackageCorrupt { path: PathBuf, message: String },
    #[error("unsupported Dev Server URL {url}: tizen-sim proxies plain http:// dev servers only")]
    DevServerUrlUnsupported { url: Url },
    #[error("could not unpack the Package {path}: {source}")]
    UnpackFailed {
        path: PathBuf,
        source: std::io::Error,
    },
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
    /// Reads the App from its source, unpacking a Package in the system's
    /// temporary directory (see `load_unpacking_in`).
    pub fn load(source: &AppSource) -> Result<App, AppError> {
        Self::load_unpacking_in(source, &std::env::temp_dir())
    }

    /// Reads the App from its source. A Package is unpacked into a fresh
    /// directory inside `parent`, which becomes the App's root and lasts as
    /// long as the App and its clones.
    pub fn load_unpacking_in(source: &AppSource, parent: &Path) -> Result<App, AppError> {
        match source {
            AppSource::ProjectFolder(root) => load_folder(root, root),
            AppSource::Package(package) => {
                let unpacked = unpack(package, parent)?;
                let mut app = load_folder(unpacked.path(), package)?;
                app._unpacked = Some(Arc::new(unpacked));
                Ok(app)
            }
            AppSource::DevServerUrl { url, config } => {
                // Proxying https would mean terminating TLS for the bundler.
                if url.scheme() != "http" {
                    return Err(AppError::DevServerUrlUnsupported { url: url.clone() });
                }
                read_config(config, config, AppFiles::Bundler(url.clone()))
            }
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

    /// The App's network and navigation rules.
    pub fn access_policy(&self) -> &AccessPolicy {
        &self.access_policy
    }

    /// The gamepad models the `gamepad` metadata names, one of which must be
    /// connected when the App launches (TV guides/user-interaction/gamepad.html,
    /// "Checking for Specific Gamepads"). Empty without the metadata.
    pub fn required_gamepads(&self) -> &[String] {
        &self.required_gamepads
    }

    /// Where the App's files are served from.
    pub fn files(&self) -> &AppFiles {
        &self.files
    }

    /// The directory the App's files are in; `None` for a Dev Server URL App.
    pub fn folder(&self) -> Option<&Path> {
        match &self.files {
            AppFiles::Folder(root) => Some(root),
            AppFiles::Bundler(_) => None,
        }
    }

    /// The path (and query) of the entry page on the Session's dev server:
    /// the entry point below the App's root. For a Dev Server URL App the
    /// root is the URL's path, taken as a folder, and the URL's query is kept,
    /// so `http://host/tv?mode=dev` starts at `/tv/index.html?mode=dev`.
    pub fn entry_path(&self) -> String {
        match &self.files {
            AppFiles::Folder(_) => format!("/{}", self.entry_point.trim_start_matches('/')),
            AppFiles::Bundler(url) => {
                let mut root = url.clone();
                if !root.path().ends_with('/') {
                    root.set_path(&format!("{}/", root.path()));
                }
                let mut entry = root.join(&self.entry_point).unwrap_or(root);
                entry.set_query(url.query());
                entry[url::Position::BeforePath..url::Position::AfterQuery].to_string()
            }
        }
    }
}

/// Reads the App in `root`. Errors name files under `shown_as` instead, so
/// a Package's problems point at the `.wgt`, not where it was unpacked.
fn load_folder(root: &Path, shown_as: &Path) -> Result<App, AppError> {
    let app = read_config(
        &root.join("config.xml"),
        &shown_as.join("config.xml"),
        AppFiles::Folder(root.to_path_buf()),
    )?;
    if !root.join(&app.entry_point).is_file() {
        return Err(AppError::EntryPointMissing {
            root: shown_as.to_path_buf(),
            entry_point: app.entry_point,
        });
    }
    Ok(app)
}

/// Reads the App declared by the `config.xml` at `file`, whose files are
/// served from `files`. Errors name the file as `shown_as`.
fn read_config(file: &Path, shown_as: &Path, files: AppFiles) -> Result<App, AppError> {
    let path = shown_as.to_path_buf();
    let text = match std::fs::read_to_string(file) {
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
    let access_policy = access_policy(widget);
    // "To add multiple gamepad models, separate the model names using '::'"
    // (TV guides/user-interaction/gamepad.html). Which occurrence counts when
    // the key repeats is unverified; the first is taken.
    let required_gamepads = widget
        .children()
        .filter(|n| n.has_tag_name((TIZEN_NS, "metadata")))
        .find(|n| n.attribute("key") == Some(GAMEPAD_METADATA))
        .and_then(|n| n.attribute("value"))
        .map(|value| {
            value
                .split("::")
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

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
        access_policy,
        required_gamepads,
        files,
        _unpacked: None,
    })
}

/// Unpacks a Package into a new directory inside `parent`.
fn unpack(package: &Path, parent: &Path) -> Result<tempfile::TempDir, AppError> {
    let corrupt = |message: String| AppError::PackageCorrupt {
        path: package.to_path_buf(),
        message,
    };
    let zip_error = |e: zip::result::ZipError| corrupt(e.to_string());
    let unpack_failed = |source| AppError::UnpackFailed {
        path: package.to_path_buf(),
        source,
    };
    let file = std::fs::File::open(package).map_err(|source| AppError::PackageUnreadable {
        path: package.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(zip_error)?;
    std::fs::create_dir_all(parent).map_err(unpack_failed)?;
    let dir = tempfile::Builder::new()
        .prefix("tizen-sim-package-")
        .tempdir_in(parent)
        .map_err(unpack_failed)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(zip_error)?;
        let name = entry.name().map_err(zip_error)?.to_string();
        // Simulator guards that keep a Package's files inside its directory.
        // How a TV treats such entries is unverified.
        let absolute = name.starts_with(['/', '\\']) || name.get(1..2) == Some(":");
        let Some(relative) = entry.enclosed_name().filter(|_| !absolute) else {
            return Err(corrupt(format!(
                "entry `{name}` points outside the Package"
            )));
        };
        if entry.is_symlink() {
            return Err(corrupt(format!(
                "entry `{name}` is a symbolic link, which tizen-sim does not unpack"
            )));
        }
        let dest = dir.path().join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(unpack_failed)?;
            continue;
        }
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .map_err(|e| corrupt(format!("entry `{name}`: {e}")))?;
        if let Some(folder) = dest.parent() {
            std::fs::create_dir_all(folder).map_err(unpack_failed)?;
        }
        std::fs::write(&dest, contents).map_err(unpack_failed)?;
    }
    Ok(dir)
}

fn access_policy(widget: roxmltree::Node) -> AccessPolicy {
    let first_text = |name: &str| {
        widget
            .children()
            .find(|n| n.has_tag_name((TIZEN_NS, name)))
            .map(|n| n.text().unwrap_or_default().trim().to_string())
    };
    let policy = first_text("content-security-policy");
    let report_only = first_text("content-security-policy-report-only");
    let allow_navigation = first_text("allow-navigation")
        .map(|domains| domains.split_ascii_whitespace().map(String::from).collect());
    if policy.is_some() || report_only.is_some() || allow_navigation.is_some() {
        return AccessPolicy::Csp(CspPolicy {
            policy,
            report_only,
            allow_navigation,
        });
    }
    AccessPolicy::Warp(
        widget
            .children()
            .filter(|n| n.has_tag_name((WIDGET_NS, "access")))
            .filter_map(AccessRequest::from_element)
            .collect(),
    )
}

impl AccessPolicy {
    /// In WARP mode, whether the `<access>` elements grant access to `url`;
    /// `None` in CSP mode, where they aren't used.
    pub fn warp_grants(&self, url: &str) -> Option<bool> {
        match self {
            AccessPolicy::Warp(requests) => Some(requests.iter().any(|r| r.grants(url))),
            AccessPolicy::Csp(_) => None,
        }
    }
}

/// Schemes an `<access>` origin may use. WARP leaves the set to the user
/// agent; which ones the Tizen runtime supports is unverified.
const ACCESS_SCHEMES: &[&str] = &["http", "https", "ws", "wss"];

impl AccessRequest {
    /// W3C WARP, "Rule for processing an access element": `None` for an
    /// element in error, which is ignored.
    fn from_element(element: roxmltree::Node) -> Option<AccessRequest> {
        let origin = single_attribute_value(element.attribute("origin")?);
        if origin == "*" {
            return Some(AccessRequest::Any);
        }
        // Only scheme and authority, with a host and no user info. The url
        // crate lowercases hosts and IDNA-encodes http(s) ones. It also
        // can't tell `https://a.com` from `https://a.com/`, so a lone
        // trailing `/` is accepted; whether WARP counts it as a path is
        // unverified.
        let url = Url::parse(&origin).ok()?;
        let only_scheme_and_authority = url.username().is_empty()
            && url.password().is_none()
            && matches!(url.path(), "" | "/")
            && url.query().is_none()
            && url.fragment().is_none();
        if !only_scheme_and_authority || !ACCESS_SCHEMES.contains(&url.scheme()) {
            return None;
        }
        Some(AccessRequest::Origin {
            scheme: url.scheme().to_string(),
            host: url.host()?.to_string(),
            port: url.port_or_known_default()?,
            // A valid boolean case-sensitively matches `true` or `false`;
            // anything else is false (W3C Widget Packaging, "Boolean attribute").
            subdomains: element
                .attribute("subdomains")
                .is_some_and(|value| single_attribute_value(value) == "true"),
        })
    }

    /// W3C WARP, "Rules for Granting Access to a Network Resource".
    pub fn grants(&self, url: &str) -> bool {
        let AccessRequest::Origin {
            scheme,
            host,
            port,
            subdomains,
        } = self
        else {
            return true;
        };
        let Ok(url) = Url::parse(url) else {
            return false;
        };
        let Some(candidate) = url.host() else {
            return false;
        };
        // Subdomains only exist for domain names, not IP addresses.
        let candidate_name = candidate.to_string();
        let host_matches = candidate_name == *host
            || (*subdomains
                && matches!(candidate, Host::Domain(_))
                && candidate_name.ends_with(&format!(".{host}")));
        url.scheme() == scheme && host_matches && url.port_or_known_default() == Some(*port)
    }
}

/// W3C Widget Packaging, "Rule for Getting a Single Attribute Value": runs
/// of space characters collapse into one, and the ends are trimmed.
fn single_attribute_value(value: &str) -> String {
    value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
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
