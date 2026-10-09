//! App loading: turns an App Source into an App by reading its `config.xml`.

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
    required_version: Option<String>,
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
    #[error("entry point `{entry_point}` declared in config.xml does not exist in {root}")]
    EntryPointMissing { root: PathBuf, entry_point: String },
}

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
    pub fn required_version(&self) -> Option<&str> {
        self.required_version.as_deref()
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
    let id = application
        .attribute("id")
        .ok_or_else(|| AppError::MissingAttribute {
            path: path.clone(),
            element: "tizen:application",
            attribute: "id",
        })?
        .to_string();
    let required_version = application.attribute("required_version").map(String::from);
    let package = application
        .attribute("package")
        .unwrap_or_default()
        .to_string();
    let name = widget
        .children()
        .find(|n| n.has_tag_name((WIDGET_NS, "name")))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| id.clone());
    let version = widget.attribute("version").unwrap_or_default().to_string();
    let privileges = widget
        .children()
        .filter(|n| n.has_tag_name((TIZEN_NS, "privilege")))
        .filter_map(|n| n.attribute("name"))
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
        entry_point,
        root: root.to_path_buf(),
    })
}
