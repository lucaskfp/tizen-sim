//! The Session Registry: the per-user list of running Sessions, and the
//! client for a Session's control endpoint (ADR-0005).
//!
//! Each Session writes its entry into its own Session directory. The
//! directory's lock says whether the owner is still alive, so an entry left
//! behind by a killed owner is never listed, and is removed on sight.

use crate::home::Home;
use crate::session::{KeyOutcome, SessionEvent, SessionStatus};
use crate::session_dir;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ENTRY_FILE: &str = "session.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Long enough for a reload, which waits up to 30s for the page to load.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

/// How a running Session can be found and controlled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// The Session id, unique among running Sessions.
    pub id: String,
    pub app_id: String,
    /// The Version Profile, e.g. `10.0`.
    pub profile: String,
    /// The process that owns the Session.
    pub pid: u32,
    /// The control endpoint's base URL, e.g. `http://127.0.0.1:1234`.
    pub endpoint: String,
    /// The secret every control request must carry.
    pub token: String,
    /// The Engine's native window (an `HWND` on Windows), for the GUI to
    /// embed (ADR-0006). `None` for a headless Engine, before the App has
    /// loaded, or where windows aren't embedded.
    pub window: Option<u64>,
}

impl RegistryEntry {
    pub fn connect(&self) -> SessionClient {
        SessionClient::new(&self.endpoint, &self.token)
    }
}

/// The running Sessions of one user.
pub struct SessionRegistry {
    dir: PathBuf,
}

impl SessionRegistry {
    pub fn new(home: &Home) -> SessionRegistry {
        SessionRegistry {
            dir: session_dir::sessions_dir(home),
        }
    }

    /// The running Sessions, oldest first. Entries whose owner has died are
    /// removed instead of listed.
    pub fn list(&self) -> Vec<RegistryEntry> {
        let mut entries: Vec<(std::time::SystemTime, RegistryEntry)> =
            session_dir::remove_abandoned(&self.dir)
                .into_iter()
                .filter_map(|dir| {
                    let path = dir.join(ENTRY_FILE);
                    let entry = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
                    // The lock file is never written to, so this is when the Session started.
                    let started = std::fs::metadata(dir.join(session_dir::LOCK_FILE))
                        .and_then(|m| m.modified());
                    Some((started.unwrap_or(std::time::UNIX_EPOCH), entry))
                })
                .collect();
        entries.sort_by_key(|(started, _)| *started);
        entries.into_iter().map(|(_, entry)| entry).collect()
    }

    /// The Session a command should act on: the one `id` names (its full id
    /// or a unique prefix of it), or else the only one running.
    pub fn choose(&self, id: Option<&str>) -> Result<RegistryEntry, ChooseError> {
        choose(self.list(), id)
    }
}

/// Writes a Session's entry into its Session directory, replacing any
/// earlier one in a single step so a reader never sees half of it.
pub(crate) fn write_entry(session_dir: &Path, entry: &RegistryEntry) -> io::Result<()> {
    let temp = session_dir.join(format!("{ENTRY_FILE}.tmp"));
    std::fs::write(&temp, serde_json::to_vec_pretty(entry)?)?;
    std::fs::rename(&temp, session_dir.join(ENTRY_FILE))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChooseError {
    #[error("no Session is running; start one with `tizen-sim run`")]
    NoSession,
    #[error("no running Session has the id `{0}`")]
    NotFound(String),
    #[error("several Sessions are running; choose one with --session:\n{}", list(.0))]
    Several(Vec<RegistryEntry>),
}

fn list(entries: &[RegistryEntry]) -> String {
    entries
        .iter()
        .map(|e| format!("  {}  {} on Tizen {}", e.id, e.app_id, e.profile))
        .collect::<Vec<_>>()
        .join("\n")
}

fn choose(entries: Vec<RegistryEntry>, id: Option<&str>) -> Result<RegistryEntry, ChooseError> {
    let candidates: Vec<RegistryEntry> = match id {
        Some(id) => {
            if let Some(exact) = entries.iter().find(|e| e.id == id) {
                return Ok(exact.clone());
            }
            let matching: Vec<_> = entries
                .into_iter()
                .filter(|e| e.id.starts_with(id))
                .collect();
            if matching.is_empty() {
                return Err(ChooseError::NotFound(id.to_string()));
            }
            matching
        }
        None => entries,
    };
    match <[RegistryEntry; 1]>::try_from(candidates) {
        Ok([only]) => Ok(only),
        Err(candidates) if candidates.is_empty() => Err(ChooseError::NoSession),
        Err(candidates) => Err(ChooseError::Several(candidates)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the Session refused the request: wrong or missing token")]
    Unauthorized,
    /// The Session answered with an error, e.g. an unknown Remote Key.
    #[error("{0}")]
    Session(String),
    #[error("could not reach the Session: {0}")]
    Unreachable(#[from] reqwest::Error),
    #[error("the Session sent an unreadable answer: {0}")]
    Invalid(String),
}

/// A client for a running Session's control endpoint.
pub struct SessionClient {
    endpoint: String,
    token: String,
    http: reqwest::Client,
}

impl SessionClient {
    pub fn new(endpoint: &str, token: &str) -> SessionClient {
        SessionClient {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            token: token.to_string(),
            // The endpoint is on this machine; never send the token through a proxy.
            http: reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .expect("a plain HTTP client"),
        }
    }

    pub async fn status(&self) -> Result<SessionStatus, ClientError> {
        json_body(
            self.send(self.request(reqwest::Method::GET, "status"))
                .await?,
        )
        .await
    }

    /// Presses a Remote Key, as `Session::press_key` does.
    pub async fn press_key(&self, key: &str) -> Result<KeyOutcome, ClientError> {
        let request = self
            .request(reqwest::Method::POST, "key")
            .json(&json!({ "key": key }));
        json_body(self.send(request).await?).await
    }

    pub async fn reload(&self) -> Result<(), ClientError> {
        self.send(self.request(reqwest::Method::POST, "reload"))
            .await?;
        Ok(())
    }

    /// A PNG of the App's screen, as `Session::screenshot` takes it.
    pub async fn screenshot(&self) -> Result<Vec<u8>, ClientError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "screenshot"))
            .await?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Evaluates a JavaScript expression in the App's page.
    pub async fn evaluate(&self, expression: &str) -> Result<Value, ClientError> {
        let request = self
            .request(reqwest::Method::POST, "evaluate")
            .json(&json!({ "expression": expression }));
        json_body(self.send(request).await?).await
    }

    /// Every event of the Session so far, then new ones until it exits.
    pub async fn events(&self) -> Result<ClientEvents, ClientError> {
        let response = self.send(self.http.get(self.url("events"))).await?;
        Ok(ClientEvents {
            response,
            buffer: vec![],
        })
    }

    /// A request that must be answered in time, unlike the event stream.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, self.url(path))
            .timeout(REQUEST_TIMEOUT)
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.endpoint)
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ClientError> {
        let response = request.bearer_auth(&self.token).send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        if !status.is_success() {
            let body: Value = response.json().await.unwrap_or_default();
            let message = body["error"].as_str().map(String::from);
            return Err(ClientError::Session(
                message.unwrap_or_else(|| format!("the Session answered {status}")),
            ));
        }
        Ok(response)
    }
}

async fn json_body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ClientError> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(|e| ClientError::Invalid(e.to_string()))
}

/// A running Session's event stream, one JSON event per line.
pub struct ClientEvents {
    response: reqwest::Response,
    buffer: Vec<u8>,
}

impl ClientEvents {
    /// The next event, or `None` once the Session has exited.
    pub async fn next(&mut self) -> Option<Result<SessionEvent, ClientError>> {
        loop {
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return Some(
                    serde_json::from_slice(&line).map_err(|e| ClientError::Invalid(e.to_string())),
                );
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => self.buffer.extend_from_slice(&chunk),
                Ok(None) => return None,
                Err(error) => return Some(Err(error.into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> RegistryEntry {
        RegistryEntry {
            id: id.into(),
            app_id: "Fixture001.Hello".into(),
            profile: "10.0".into(),
            pid: 1,
            endpoint: "http://127.0.0.1:1".into(),
            token: "t".into(),
            window: None,
        }
    }

    #[test]
    fn with_one_session_it_is_chosen() {
        assert_eq!(choose(vec![entry("ab12")], None), Ok(entry("ab12")));
    }

    #[test]
    fn with_none_running_nothing_is_chosen() {
        assert_eq!(choose(vec![], None), Err(ChooseError::NoSession));
        assert_eq!(
            choose(vec![], Some("ab")),
            Err(ChooseError::NotFound("ab".into()))
        );
    }

    #[test]
    fn with_several_one_must_be_named() {
        let running = vec![entry("ab12"), entry("cd34")];
        let err = choose(running.clone(), None).unwrap_err();
        assert_eq!(err, ChooseError::Several(running.clone()));
        let message = err.to_string();
        assert!(
            message.contains("--session") && message.contains("ab12") && message.contains("cd34"),
            "{message}"
        );

        assert_eq!(choose(running.clone(), Some("cd34")), Ok(entry("cd34")));
        assert_eq!(choose(running.clone(), Some("c")), Ok(entry("cd34")));
        assert_eq!(
            choose(running, Some("ef")),
            Err(ChooseError::NotFound("ef".into()))
        );
    }

    #[test]
    fn an_ambiguous_prefix_is_not_a_choice() {
        let running = vec![entry("ab12"), entry("ab34"), entry("cd56")];
        assert_eq!(
            choose(running, Some("ab")),
            Err(ChooseError::Several(vec![entry("ab12"), entry("ab34")]))
        );
    }

    #[test]
    fn entries_of_dead_owners_are_not_listed_and_are_removed() {
        let state = tempfile::tempdir().unwrap();
        let home = Home::at(state.path()).with_state_dir(state.path().to_path_buf());
        let sessions = state.path().join("sessions");
        let live = crate::session_dir::SessionDir::create(&sessions).unwrap();
        write_entry(live.path(), &entry(&live.id())).unwrap();
        // A killed owner's directory: its entry remains but its lock is free.
        let dead = sessions.join("dead");
        std::fs::create_dir_all(&dead).unwrap();
        crate::session_dir::write_old_lock(&dead);
        write_entry(&dead, &entry("dead")).unwrap();

        let listed = SessionRegistry::new(&home).list();

        assert_eq!(listed, vec![entry(&live.id())]);
        assert!(!dead.exists());
    }
}
