//! The GUI's Session control: one Session at a time, started for an App
//! Source and restarted to switch Version Profile. Every event of every
//! Session goes to one sink, in order, tagged with the Session it came from.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::remote_keys::KeyMap;
use tizen_sim_core::session::{
    KeyOutcome, Lifecycle, Session, SessionConfig, SessionError, SessionEvent,
};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub struct ControllerOptions {
    pub home: Home,
    /// Run Engines without a window (tests, CI).
    pub headless: bool,
    /// A Chromium executable to run instead of each Version Profile's Engine.
    pub engine_path: Option<PathBuf>,
}

/// What the GUI shows, in the order it happened.
#[derive(Debug, Clone, Serialize)]
pub enum GuiEvent {
    /// A new Session is starting for `source`; its events follow.
    SessionStarted { session: u64, source: String },
    /// An event of Session `session`.
    Session { session: u64, event: SessionEvent },
}

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error("no App is open")]
    NoSession,
    #[error(transparent)]
    Session(#[from] SessionError),
}

type Sink = Arc<dyn Fn(GuiEvent) + Send + Sync>;

pub struct Controller {
    options: ControllerOptions,
    sink: Sink,
    current: Mutex<Option<Current>>,
}

/// The Session the GUI is showing.
struct Current {
    id: u64,
    /// Where the App comes from, to start it again on another profile.
    source: AppSource,
    /// Shared, so acting on it doesn't hold the Controller's lock: a stuck
    /// reload mustn't stop the developer switching away.
    session: Arc<Session>,
    /// Forwards the Session's events to the sink until it exits.
    forwarder: JoinHandle<()>,
}

impl Controller {
    pub fn new(
        options: ControllerOptions,
        sink: impl Fn(GuiEvent) + Send + Sync + 'static,
    ) -> Controller {
        Controller {
            options,
            sink: Arc::new(sink),
            current: Mutex::new(None),
        }
    }

    /// Stops the current Session, if any, and starts one for `source` on
    /// `profile` (when `None`: the last one used for the App, or the latest).
    pub async fn open(&self, source: AppSource, profile: Option<String>) {
        let mut current = self.current.lock().await;
        self.restart(&mut current, source, profile).await;
    }

    /// Restarts the current App on another Version Profile.
    pub async fn switch_profile(&self, profile: &str) -> Result<(), ControllerError> {
        let mut current = self.current.lock().await;
        let source = current
            .as_ref()
            .ok_or(ControllerError::NoSession)?
            .source
            .clone();
        self.restart(&mut current, source, Some(profile.to_string()))
            .await;
        Ok(())
    }

    async fn restart(
        &self,
        current: &mut Option<Current>,
        source: AppSource,
        profile: Option<String>,
    ) {
        let id = match current.take() {
            Some(previous) => {
                let id = previous.id + 1;
                end(previous).await;
                id
            }
            None => 1,
        };
        (self.sink)(GuiEvent::SessionStarted {
            session: id,
            source: describe(&source),
        });
        let session = Session::start(SessionConfig {
            source: source.clone(),
            profile,
            engine_path: self.options.engine_path.clone(),
            headless: self.options.headless,
            lenient: false,
            key_map: KeyMap::default(),
            device: Default::default(),
            resolution: Default::default(),
            checkerboard: false,
            home: self.options.home.clone(),
        });
        let forwarder = tokio::spawn(forward(session.events(), id, self.sink.clone()));
        *current = Some(Current {
            id,
            source,
            session: Arc::new(session),
            forwarder,
        });
    }

    /// Presses a Remote Key in the current Session, as the on-screen remote
    /// does. A dropped key is also reported in the event stream.
    pub async fn press_key(&self, name: &str) -> Result<KeyOutcome, ControllerError> {
        Ok(self.session().await?.press_key(name).await?)
    }

    /// Reloads the current App and waits for it to finish loading.
    pub async fn reload(&self) -> Result<(), ControllerError> {
        Ok(self.session().await?.reload().await?)
    }

    /// A PNG of the current App's screen, at the TV's resolution.
    pub async fn screenshot(&self) -> Result<Vec<u8>, ControllerError> {
        Ok(self.session().await?.screenshot().await?)
    }

    async fn session(&self) -> Result<Arc<Session>, ControllerError> {
        let current = self.current.lock().await;
        let current = current.as_ref().ok_or(ControllerError::NoSession)?;
        Ok(current.session.clone())
    }

    /// Stops the current Session, if any.
    pub async fn stop(&self) {
        if let Some(current) = self.current.lock().await.take() {
            end(current).await;
        }
    }
}

/// Stops a Session and waits until all its events have reached the sink, so
/// the next Session's events come after them.
async fn end(current: Current) {
    current.session.stop().await;
    let _ = current.forwarder.await;
}

async fn forward(mut events: tizen_sim_core::session::Events, session: u64, sink: Sink) {
    while let Some(event) = events.next().await {
        let exited = matches!(event, SessionEvent::Lifecycle(Lifecycle::Exited { .. }));
        sink(GuiEvent::Session { session, event });
        if exited {
            break;
        }
    }
}

/// The App Source as the developer would name it.
fn describe(source: &AppSource) -> String {
    match source {
        AppSource::ProjectFolder(path) | AppSource::Package(path) => path.display().to_string(),
        AppSource::DevServerUrl { url, .. } => url.to_string(),
    }
}
