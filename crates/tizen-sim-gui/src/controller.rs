//! The GUI's Session control: one Session at a time, either started by the
//! GUI for an App Source or adopted from the Session Registry (a Session the
//! CLI started, ADR-0005). Every event of every Session goes to one sink, in
//! order, tagged with the Session it came from. Once a Session's App has
//! loaded, its Engine window goes on the GUI's screen (ADR-0006).

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::registry::{
    ChooseError, ClientError, ClientEvents, RegistryEntry, SessionClient, SessionRegistry,
};
use tizen_sim_core::remote_keys::KeyMap;
use tizen_sim_core::session::{
    Events, ExitReason, KeyOutcome, Lifecycle, Session, SessionConfig, SessionError, SessionEvent,
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
    /// A Session is shown from now on: one the GUI starts for `source`, or
    /// one it `adopted` from the Session Registry. Its events follow.
    SessionStarted {
        session: u64,
        source: String,
        adopted: bool,
    },
    /// An event of Session `session`.
    Session { session: u64, event: SessionEvent },
    /// Whether Session `session`'s Engine window is on the GUI's screen
    /// rather than in a window of its own, once its App has loaded.
    Screen { session: u64, embedded: bool },
}

/// The GUI's screen area, where Engine windows are embedded (ADR-0006).
pub trait Screen: Send + Sync {
    /// Puts Engine window `window` (an `HWND`) on the screen; `false` if it
    /// can't be, e.g. where embedding isn't supported or the window isn't
    /// `engine_pid`'s. `owned` says whether the GUI owns the Session.
    fn embed(&self, window: u64, engine_pid: Option<u32>, owned: bool) -> bool;
    /// Takes the embedded window, if any, off the screen; an adopted
    /// Session's becomes its own window again.
    fn release(&self);
}

/// Where Engine windows aren't embedded: they stay in their own windows.
struct NoScreen;

impl Screen for NoScreen {
    fn embed(&self, _window: u64, _engine_pid: Option<u32>, _owned: bool) -> bool {
        false
    }

    fn release(&self) {}
}

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error("no App is open")]
    NoSession,
    #[error("that Session is the one the GUI is showing")]
    AlreadyShown,
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Registry(#[from] ChooseError),
    #[error(transparent)]
    Client(#[from] ClientError),
}

/// A running Session the GUI can adopt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunningSession {
    pub id: String,
    pub app_id: String,
    pub profile: String,
    pub source: String,
}

type Sink = Arc<dyn Fn(GuiEvent) + Send + Sync>;

pub struct Controller {
    options: ControllerOptions,
    sink: Sink,
    screen: Arc<dyn Screen>,
    current: Mutex<Option<Current>>,
    /// The id the next Session shown gets.
    next_id: std::sync::atomic::AtomicU64,
}

/// The Session the GUI is showing.
struct Current {
    /// Where the App comes from, to start it again on another profile or
    /// after a crash.
    source: AppSource,
    /// The Version Profile it was started on, to restart it on the same
    /// one; `None` for the one last used for the App.
    profile: Option<String>,
    handle: Handle,
    /// Forwards the Session's events to the sink until it exits.
    forwarder: JoinHandle<()>,
}

/// How the GUI acts on the Session it shows. Shared, so acting on it doesn't
/// hold the Controller's lock: a stuck reload mustn't stop the developer
/// switching away.
#[derive(Clone)]
enum Handle {
    /// A Session the GUI started, and stops.
    Owned(Arc<Session>),
    /// Another process's Session, reached over its control endpoint.
    Adopted {
        registry_id: String,
        client: Arc<SessionClient>,
    },
}

impl Controller {
    pub fn new(
        options: ControllerOptions,
        sink: impl Fn(GuiEvent) + Send + Sync + 'static,
    ) -> Controller {
        Controller {
            options,
            sink: Arc::new(sink),
            screen: Arc::new(NoScreen),
            current: Mutex::new(None),
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Embeds Engine windows on `screen` once their App has loaded.
    pub fn with_screen(self, screen: Arc<dyn Screen>) -> Controller {
        Controller { screen, ..self }
    }

    /// Stops or lets go of the current Session, if any, and starts one for
    /// `source` on `profile` (when `None`: the last one used for the App, or
    /// the latest).
    pub async fn open(&self, source: AppSource, profile: Option<String>) {
        let mut current = self.current.lock().await;
        self.start(&mut current, source, profile).await;
    }

    /// Restarts the current App on another Version Profile.
    pub async fn switch_profile(&self, profile: &str) -> Result<(), ControllerError> {
        let mut current = self.current.lock().await;
        let source = current_source(&current)?;
        self.start(&mut current, source, Some(profile.to_string()))
            .await;
        Ok(())
    }

    /// Starts the current App afresh, e.g. after its Engine crashed. An
    /// adopted Session's App is started by the GUI, which then owns it.
    pub async fn restart(&self) -> Result<(), ControllerError> {
        let mut current = self.current.lock().await;
        let (source, profile) = {
            let shown = current.as_ref().ok_or(ControllerError::NoSession)?;
            (shown.source.clone(), shown.profile.clone())
        };
        self.start(&mut current, source, profile).await;
        Ok(())
    }

    /// Shows a running Session another process started, found in the
    /// Session Registry by its id (or a unique prefix of it): its events so
    /// far, then new ones, and its Engine window on the screen. It keeps
    /// running when the GUI lets go of it.
    pub async fn adopt(&self, id: &str) -> Result<(), ControllerError> {
        let entry = SessionRegistry::new(&self.options.home).choose(Some(id))?;
        let mut current = self.current.lock().await;
        if let Some(Current {
            handle: Handle::Owned(session),
            ..
        }) = current.as_ref()
            && entry.engine_pid.is_some()
            && session.engine_pid() == entry.engine_pid
        {
            return Err(ControllerError::AlreadyShown);
        }
        let client = Arc::new(entry.connect());
        let events = client.events().await?;
        if let Some(previous) = current.take() {
            self.end(previous).await;
        }
        let id = self.announce(&entry.source, true);
        let handle = Handle::Adopted {
            registry_id: entry.id.clone(),
            client,
        };
        *current = Some(Current {
            source: entry.source,
            profile: Some(entry.profile),
            forwarder: self.forward(EventSource::Adopted(events), handle.clone(), id),
            handle,
        });
        Ok(())
    }

    /// The running Sessions the GUI could adopt: those of other processes.
    pub fn adoptable(&self) -> Vec<RunningSession> {
        adoptable(
            SessionRegistry::new(&self.options.home).list(),
            std::process::id(),
        )
    }

    async fn start(
        &self,
        current: &mut Option<Current>,
        source: AppSource,
        profile: Option<String>,
    ) {
        if let Some(previous) = current.take() {
            self.end(previous).await;
        }
        let id = self.announce(&source, false);
        let session = Arc::new(Session::start(SessionConfig {
            source: source.clone(),
            profile: profile.clone(),
            engine_path: self.options.engine_path.clone(),
            engine_has_headless: None,
            headless: self.options.headless,
            lenient: false,
            key_map: KeyMap::default(),
            device: Default::default(),
            resolution: Default::default(),
            checkerboard: false,
            home: self.options.home.clone(),
        }));
        let handle = Handle::Owned(session.clone());
        *current = Some(Current {
            source,
            profile,
            forwarder: self.forward(EventSource::Owned(session.events()), handle.clone(), id),
            handle,
        });
    }

    /// Tells the sink a new Session is shown, and returns its id.
    fn announce(&self, source: &AppSource, adopted: bool) -> u64 {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (self.sink)(GuiEvent::SessionStarted {
            session: id,
            source: describe(source),
            adopted,
        });
        id
    }

    fn forward(&self, events: EventSource, handle: Handle, id: u64) -> JoinHandle<()> {
        tokio::spawn(forward(
            events,
            handle,
            id,
            self.sink.clone(),
            self.screen.clone(),
            self.options.home.clone(),
        ))
    }

    /// Stops the GUI's own Session, or lets go of an adopted one, and waits
    /// until all its events have reached the sink, so the next Session's
    /// events come after them.
    async fn end(&self, current: Current) {
        match current.handle {
            Handle::Owned(session) => {
                session.stop().await;
                let _ = current.forwarder.await;
            }
            Handle::Adopted { .. } => {
                current.forwarder.abort();
                let _ = current.forwarder.await;
            }
        }
        self.screen.release();
    }

    /// Presses a Remote Key in the current Session, as the on-screen remote
    /// does. A dropped key is also reported in the event stream.
    pub async fn press_key(&self, name: &str) -> Result<KeyOutcome, ControllerError> {
        self.handle().await?.press_key(name).await
    }

    /// Reloads the current App and waits for it to finish loading.
    pub async fn reload(&self) -> Result<(), ControllerError> {
        self.handle().await?.reload().await
    }

    /// A PNG of the current App's screen, at the TV's resolution.
    pub async fn screenshot(&self) -> Result<Vec<u8>, ControllerError> {
        self.handle().await?.screenshot().await
    }

    async fn handle(&self) -> Result<Handle, ControllerError> {
        let current = self.current.lock().await;
        let current = current.as_ref().ok_or(ControllerError::NoSession)?;
        Ok(current.handle.clone())
    }

    /// Stops the GUI's own Session, or lets go of an adopted one.
    pub async fn stop(&self) {
        if let Some(current) = self.current.lock().await.take() {
            self.end(current).await;
        }
    }
}

impl Handle {
    async fn press_key(&self, name: &str) -> Result<KeyOutcome, ControllerError> {
        Ok(match self {
            Handle::Owned(session) => session.press_key(name).await?,
            Handle::Adopted { client, .. } => client.press_key(name).await?,
        })
    }

    async fn reload(&self) -> Result<(), ControllerError> {
        match self {
            Handle::Owned(session) => session.reload().await?,
            Handle::Adopted { client, .. } => client.reload().await?,
        }
        Ok(())
    }

    async fn screenshot(&self) -> Result<Vec<u8>, ControllerError> {
        Ok(match self {
            Handle::Owned(session) => session.screenshot().await?,
            Handle::Adopted { client, .. } => client.screenshot().await?,
        })
    }
}

fn current_source(current: &Option<Current>) -> Result<AppSource, ControllerError> {
    Ok(current
        .as_ref()
        .ok_or(ControllerError::NoSession)?
        .source
        .clone())
}

/// The Sessions of processes other than `own_pid`.
fn adoptable(entries: Vec<RegistryEntry>, own_pid: u32) -> Vec<RunningSession> {
    entries
        .into_iter()
        .filter(|e| e.pid != own_pid)
        .map(|e| RunningSession {
            source: describe(&e.source),
            id: e.id,
            app_id: e.app_id,
            profile: e.profile,
        })
        .collect()
}

/// A Session's event stream, in-process or over its control endpoint.
enum EventSource {
    Owned(Events),
    Adopted(ClientEvents),
}

impl EventSource {
    async fn next(&mut self) -> Option<SessionEvent> {
        match self {
            EventSource::Owned(events) => events.next().await,
            EventSource::Adopted(events) => match events.next().await? {
                Ok(event) => Some(event),
                Err(error) => {
                    tracing::warn!(%error, "lost the adopted Session's event stream");
                    None
                }
            },
        }
    }
}

async fn forward(
    mut events: EventSource,
    handle: Handle,
    session: u64,
    sink: Sink,
    screen: Arc<dyn Screen>,
    home: Home,
) {
    loop {
        let Some(event) = events.next().await else {
            // An adopted Session's owner went away without a word, and its
            // Engine with it (the Engine dies with its owner).
            if matches!(handle, Handle::Adopted { .. }) {
                sink(GuiEvent::Session {
                    session,
                    event: SessionEvent::Lifecycle(Lifecycle::Exited {
                        reason: ExitReason::EngineCrashed,
                    }),
                });
                screen.release();
            }
            return;
        };
        let loaded = matches!(event, SessionEvent::Lifecycle(Lifecycle::AppLoaded { .. }));
        let exited = matches!(event, SessionEvent::Lifecycle(Lifecycle::Exited { .. }));
        sink(GuiEvent::Session { session, event });
        if loaded {
            let embedded = match engine_window(&handle, &home) {
                Some((window, pid)) => {
                    let owned = matches!(handle, Handle::Owned(_));
                    screen.embed(window, pid, owned)
                }
                None => false,
            };
            sink(GuiEvent::Screen { session, embedded });
        }
        if exited {
            screen.release();
            return;
        }
    }
}

/// The Session's Engine window and process, once its App has loaded.
fn engine_window(handle: &Handle, home: &Home) -> Option<(u64, Option<u32>)> {
    match handle {
        Handle::Owned(session) => Some((session.engine_window()?, session.engine_pid())),
        Handle::Adopted { registry_id, .. } => {
            // Recorded in the Session's entry before it reports the App loaded.
            let entry = SessionRegistry::new(home)
                .list()
                .into_iter()
                .find(|e| &e.id == registry_id)?;
            Some((entry.window?, entry.engine_pid))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, pid: u32) -> RegistryEntry {
        RegistryEntry {
            id: id.into(),
            app_id: "Fixture001.Hello".into(),
            source: AppSource::ProjectFolder("apps/hello".into()),
            profile: "10.0".into(),
            pid,
            endpoint: "http://127.0.0.1:1".into(),
            token: "t".into(),
            engine_pid: None,
            window: None,
        }
    }

    #[test]
    fn only_other_processes_sessions_can_be_adopted() {
        let running = vec![entry("mine", 10), entry("cli", 20)];

        let adoptable = adoptable(running, 10);

        assert_eq!(
            adoptable,
            [RunningSession {
                id: "cli".into(),
                app_id: "Fixture001.Hello".into(),
                profile: "10.0".into(),
                source: PathBuf::from("apps/hello").display().to_string(),
            }]
        );
    }
}
