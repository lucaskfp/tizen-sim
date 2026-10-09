//! A Session: one App running in one Engine under one Version Profile.
//!
//! `Session::start` returns immediately; startup (App loading, Engine
//! download, dev server, Engine launch) runs in the background and reports
//! through the Session's event stream.

use crate::app::{App, AppSource};
use crate::cdp::{Cdp, CdpError, CdpEvent};
use crate::dev_server::DevServer;
use crate::engine::{
    self, DownloadProgress, EngineSpec, EngineStore, LaunchOptions, RunningEngine,
};
use crate::home::Home;
use crate::profile::VersionProfile;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub struct SessionConfig {
    pub source: AppSource,
    /// Version Profile to use; the latest when `None`.
    pub profile: Option<String>,
    /// Run the Engine without a window (CI, automated tests).
    pub headless: bool,
    pub home: Home,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Lifecycle(Lifecycle),
    Console(ConsoleMessage),
    EngineDownload(DownloadProgress),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
    Starting {
        app_id: String,
        profile: String,
    },
    EngineReady {
        engine_version: String,
        debugger_url: String,
    },
    AppLoaded {
        url: String,
    },
    Failed {
        error: String,
    },
    Exited {
        reason: ExitReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// The Session was stopped by its owner.
    Stopped,
    /// The Engine went away on its own (window closed, crash).
    EngineClosed,
    /// Startup failed; a `Failed` event with the reason came just before.
    FailedToStart,
}

/// A message the App wrote to its console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleMessage {
    /// The console method's level as the Engine reports it: `log`, `info`,
    /// `warning`, `error`, `debug`, ...
    pub level: String,
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("{0}")]
    Failed(String),
    #[error("the Session has exited")]
    Exited,
    #[error("the App is not loaded yet")]
    NotLoaded,
    #[error(transparent)]
    Cdp(#[from] CdpError),
}

#[derive(Debug, Clone, PartialEq)]
enum Status {
    Starting,
    Loaded,
    Failed(String),
    Exited,
}

pub struct Session {
    inner: Arc<Inner>,
}

struct Inner {
    log: EventLog,
    status: watch::Sender<Status>,
    cdp: OnceLock<Arc<Cdp>>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    pub fn start(config: SessionConfig) -> Session {
        let (stop_tx, stop_rx) = oneshot::channel();
        let inner = Arc::new(Inner {
            log: EventLog::new(),
            status: watch::channel(Status::Starting).0,
            cdp: OnceLock::new(),
            stop: Mutex::new(Some(stop_tx)),
            task: tokio::sync::Mutex::new(None),
        });
        let task = tokio::spawn(run(config, inner.clone(), stop_rx));
        *inner.task.try_lock().expect("fresh mutex") = Some(task);
        Session { inner }
    }

    /// Every event of this Session so far, followed by new ones as they happen.
    pub fn events(&self) -> Events {
        self.inner.log.subscribe()
    }

    /// Resolves once the App has loaded, or with the reason it never will.
    pub async fn wait_until_loaded(&self) -> Result<(), SessionError> {
        let mut status = self.inner.status.subscribe();
        let status = status
            .wait_for(|s| *s != Status::Starting)
            .await
            .map_err(|_| SessionError::Exited)?
            .clone();
        match status {
            Status::Loaded => Ok(()),
            Status::Failed(error) => Err(SessionError::Failed(error)),
            Status::Exited | Status::Starting => Err(SessionError::Exited),
        }
    }

    /// Evaluates a JavaScript expression in the App's page.
    pub async fn evaluate(&self, expression: &str) -> Result<Value, SessionError> {
        let cdp = self.inner.cdp.get().ok_or(SessionError::NotLoaded)?;
        Ok(cdp.evaluate(expression).await?)
    }

    /// Stops the Session: closes the Engine and every server. Returns once
    /// everything has shut down.
    pub async fn stop(&self) {
        if let Some(stop) = self.inner.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.inner.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

/// A running Session's resources, torn down together.
struct Running {
    engine: RunningEngine,
    server: DevServer,
    cdp: Arc<Cdp>,
}

async fn run(config: SessionConfig, inner: Arc<Inner>, mut stop: oneshot::Receiver<()>) {
    let started = tokio::select! {
        result = startup(config, &inner) => result,
        _ = &mut stop => {
            // Dropping the startup future kills a half-started Engine and server.
            inner.finish(ExitReason::Stopped);
            return;
        }
    };
    let mut running = match started {
        Ok(running) => running,
        Err(error) => {
            tracing::error!(%error, "Session failed to start");
            inner
                .log
                .publish(SessionEvent::Lifecycle(Lifecycle::Failed {
                    error: error.clone(),
                }));
            inner.status.send_replace(Status::Failed(error));
            inner
                .log
                .publish(SessionEvent::Lifecycle(Lifecycle::Exited {
                    reason: ExitReason::FailedToStart,
                }));
            return;
        }
    };

    let reason = tokio::select! {
        _ = &mut stop => {
            close_engine(&mut running).await;
            ExitReason::Stopped
        }
        _ = running.engine.child.wait() => ExitReason::EngineClosed,
    };
    stop_server(running.server).await;
    inner.finish(reason);
}

impl Inner {
    fn finish(&self, reason: ExitReason) {
        self.log
            .publish(SessionEvent::Lifecycle(Lifecycle::Exited { reason }));
        self.status.send_replace(Status::Exited);
    }
}

async fn startup(config: SessionConfig, inner: &Arc<Inner>) -> Result<Running, String> {
    let app = App::load(&config.source).map_err(|e| e.to_string())?;
    let profile = match &config.profile {
        Some(version) => VersionProfile::find(version).map_err(|e| e.to_string())?,
        None => VersionProfile::latest(),
    };
    inner
        .log
        .publish(SessionEvent::Lifecycle(Lifecycle::Starting {
            app_id: app.id().to_string(),
            profile: profile.version.clone(),
        }));

    let platform = engine::current_platform().map_err(|e| e.to_string())?;
    let spec = EngineSpec::for_profile(&profile.version, platform).map_err(|e| e.to_string())?;
    let installed = EngineStore::new(config.home.engines_dir().to_path_buf())
        .ensure(spec, |p| inner.log.publish(SessionEvent::EngineDownload(p)))
        .await
        .map_err(|e| e.to_string())?;

    let server = DevServer::start(app.root())
        .await
        .map_err(|e| format!("could not start the dev server: {e}"))?;
    let user_data_dir = config
        .home
        .state_dir()
        .join("engine-data")
        .join(sanitize(app.id()))
        .join(&profile.version);
    let engine = engine::launch(
        &installed.executable,
        &LaunchOptions {
            user_data_dir,
            headless: config.headless,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let (cdp, events) = Cdp::connect(&engine.page_ws_url)
        .await
        .map_err(|e| e.to_string())?;
    let cdp = Arc::new(cdp);
    let (loaded_tx, mut loaded_rx) = mpsc::unbounded_channel();
    tokio::spawn(forward_events(events, inner.clone(), loaded_tx));
    // Attach before navigating so no console output from the first script is lost.
    cdp.call("Runtime.enable", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    cdp.call("Page.enable", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    inner
        .log
        .publish(SessionEvent::Lifecycle(Lifecycle::EngineReady {
            engine_version: engine.engine_version.clone(),
            debugger_url: engine.page_ws_url.clone(),
        }));

    let url = server.url(app.entry_point());
    cdp.call("Page.navigate", json!({ "url": url }))
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(30), loaded_rx.recv())
        .await
        .map_err(|_| format!("the App at {url} did not finish loading within 30s"))?;

    let _ = inner.cdp.set(cdp.clone());
    inner
        .log
        .publish(SessionEvent::Lifecycle(Lifecycle::AppLoaded { url }));
    inner.status.send_replace(Status::Loaded);
    Ok(Running {
        engine,
        server,
        cdp,
    })
}

async fn forward_events(
    mut events: mpsc::UnboundedReceiver<CdpEvent>,
    inner: Arc<Inner>,
    loaded: mpsc::UnboundedSender<()>,
) {
    while let Some((method, params)) = events.recv().await {
        match method.as_str() {
            "Runtime.consoleAPICalled" => {
                inner.log.publish(SessionEvent::Console(ConsoleMessage {
                    level: params["type"].as_str().unwrap_or("log").to_string(),
                    text: console_text(&params["args"]),
                }));
            }
            "Page.loadEventFired" => {
                let _ = loaded.send(());
            }
            _ => {}
        }
    }
}

/// Renders console arguments the way DevTools' console line would.
fn console_text(args: &Value) -> String {
    args.as_array()
        .map(|args| {
            args.iter()
                .map(|arg| match &arg["value"] {
                    Value::String(s) => s.clone(),
                    Value::Null => arg["description"]
                        .as_str()
                        .or_else(|| arg["type"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

async fn close_engine(running: &mut Running) {
    // Ask politely so the Engine flushes its profile, then make sure.
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        running.cdp.call("Browser.close", json!({})),
    )
    .await;
    if tokio::time::timeout(Duration::from_secs(5), running.engine.child.wait())
        .await
        .is_err()
    {
        let _ = running.engine.child.kill().await;
    }
}

async fn stop_server(server: DevServer) {
    let _ = tokio::time::timeout(Duration::from_secs(5), server.stop()).await;
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

const HISTORY_LIMIT: usize = 10_000;

/// Session events with replay: late subscribers first get everything so far.
struct EventLog {
    history: Mutex<VecDeque<SessionEvent>>,
    live: broadcast::Sender<SessionEvent>,
}

impl EventLog {
    fn new() -> EventLog {
        EventLog {
            history: Mutex::new(VecDeque::new()),
            live: broadcast::channel(1024).0,
        }
    }

    fn publish(&self, event: SessionEvent) {
        let mut history = self.history.lock().unwrap();
        if history.len() == HISTORY_LIMIT {
            history.pop_front();
        }
        history.push_back(event.clone());
        let _ = self.live.send(event);
    }

    fn subscribe(&self) -> Events {
        let history = self.history.lock().unwrap();
        Events {
            backlog: history.clone(),
            live: self.live.subscribe(),
        }
    }
}

/// A subscription to a Session's events.
pub struct Events {
    backlog: VecDeque<SessionEvent>,
    live: broadcast::Receiver<SessionEvent>,
}

impl Events {
    /// The next event, or `None` once the Session is gone.
    pub async fn next(&mut self) -> Option<SessionEvent> {
        if let Some(event) = self.backlog.pop_front() {
            return Some(event);
        }
        loop {
            match self.live.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}
