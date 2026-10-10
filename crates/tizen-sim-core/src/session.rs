//! A Session: one App running in one Engine under one Version Profile.
//!
//! `Session::start` returns immediately; startup (App loading, Engine
//! download, dev server, Engine launch) runs in the background and reports
//! through the Session's event stream.

use crate::app::{App, AppError, AppFiles, AppSource};
use crate::cdp::{Cdp, CdpError, CdpEvent};
use crate::dev_server::{BridgeMessage, DevServer, ServerSetup};
use crate::engine::{
    self, DownloadProgress, EngineSpec, EngineStore, LaunchOptions, RunningEngine,
};
use crate::home::Home;
use crate::network::{NetworkProxy, NetworkRequest, Verdict};
use crate::profile::VersionProfile;
use crate::remote_keys::{KeyMap, PhysicalKey, RemoteKey};
use crate::session_dir::SessionDir;
use crate::shim;
use crate::watcher::{Action, Batch, SourceWatcher};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Lenient Mode: allow calls a TV would refuse (undeclared privileges),
    /// still raising a Diagnostic for each.
    pub lenient: bool,
    /// Which keyboard keys act as which Remote Keys in the Engine window.
    pub key_map: KeyMap,
    /// Per-Session overrides of the Version Profile's device values (model,
    /// DUID, network settings, ...), by name. Unknown names fail the start.
    pub device: serde_json::Map<String, Value>,
    pub home: Home,
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    Lifecycle(Lifecycle),
    Console(ConsoleMessage),
    EngineDownload(DownloadProgress),
    Diagnostic(Diagnostic),
    /// A Remote Key press and what became of it.
    RemoteKey {
        key: String,
        outcome: KeyOutcome,
    },
    HotReload(HotReload),
    /// A request the App made over the network, with the Access Policy's
    /// verdict on it.
    Network(NetworkRequest),
}

/// The Session following edits to its Project Folder or Package, or to the
/// `config.xml` of a Dev Server URL App (the bundler reloads its own output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotReload {
    /// Files changed together; paths relative to the Project Folder,
    /// `/`-separated, or the Package's file name. One of the other events
    /// follows.
    FilesChanged { paths: Vec<String> },
    /// Only stylesheets changed, and they were swapped in place: the page
    /// keeps its state.
    StylesSwapped,
    /// The App was reloaded to pick up the changes.
    Reloaded,
    /// `config.xml` or the Package changed, so the App was re-read (a
    /// Package unpacked again) and started afresh with its new privileges,
    /// Access Policy and entry point.
    Restarted { app_id: String },
    /// The changes couldn't be applied; the App keeps running as it was.
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    Delivered,
    /// Not delivered, as a TV wouldn't; `reason` says why.
    Dropped {
        reason: String,
    },
}

/// A keyboard key going down, auto-repeating while held, or going up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Down,
    Repeat,
    Up,
}

/// Something the App did that would behave differently or fail on a real TV.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable identifier, e.g. `privilege-not-declared`.
    pub code: String,
    pub message: String,
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
    /// The App went to the background (`Application.hide()`).
    Hidden,
    /// The App came back to the foreground (`Session::resume`).
    Shown,
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
    /// The App ended itself (`tizen.application.getCurrentApplication().exit()`).
    AppExited,
    /// The Exit key was pressed or the Back key held, which makes a TV
    /// terminate the App.
    TerminatedByRemote,
}

/// A message in the App's console: written by the App, or by the Engine
/// about the App (a CSP violation, a failed resource).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleMessage {
    /// The level as the Engine reports it: the console method (`log`,
    /// `info`, `warning`, `error`, `debug`, ...) or, for the Engine's own
    /// messages, `verbose`, `info`, `warning` or `error`.
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
    #[error("unknown Remote Key `{0}`")]
    UnknownRemoteKey(String),
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
    /// Set once the App has loaded.
    loaded: OnceLock<Loaded>,
    /// Ends the running App with a reason; set once the Bridge is up.
    app_exit: OnceLock<mpsc::UnboundedSender<ExitReason>>,
    stop: Mutex<Option<oneshot::Sender<()>>>,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

/// What a Session can act on once its App has loaded.
struct Loaded {
    cdp: Arc<Cdp>,
    profile: &'static VersionProfile,
    lenient: bool,
    /// Whether the App keeps running while hidden (background support).
    /// Re-read when a hot reload restarts the App.
    runs_in_background: AtomicBool,
    /// Whether the App is in the background.
    hidden: AtomicBool,
}

impl Session {
    pub fn start(config: SessionConfig) -> Session {
        let (stop_tx, stop_rx) = oneshot::channel();
        let inner = Arc::new(Inner {
            log: EventLog::new(),
            status: watch::channel(Status::Starting).0,
            loaded: OnceLock::new(),
            app_exit: OnceLock::new(),
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
        let cdp = &self.inner.loaded.get().ok_or(SessionError::NotLoaded)?.cdp;
        Ok(cdp.evaluate(expression).await?)
    }

    /// Presses a Remote Key, as on the TV remote: delivered if it's a
    /// Mandatory Key or the App registered it, otherwise dropped (or, in
    /// Lenient Mode, delivered with a Diagnostic). The outcome is reported as
    /// a `RemoteKey` event.
    pub async fn press_key(&self, name: &str) -> Result<(), SessionError> {
        self.inner.press_key(name).await
    }

    /// Presses or releases a key on the desktop keyboard in the Engine window,
    /// exactly as if the developer typed it, so the keyboard mapping applies.
    pub async fn physical_key(
        &self,
        key: PhysicalKey,
        action: KeyAction,
    ) -> Result<(), SessionError> {
        let loaded = self.inner.loaded.get().ok_or(SessionError::NotLoaded)?;
        let (event_type, repeat) = match action {
            KeyAction::Down => ("rawKeyDown", false),
            KeyAction::Repeat => ("rawKeyDown", true),
            KeyAction::Up => ("keyUp", false),
        };
        dispatch_key(
            &loaded.cdp,
            event_type,
            key.0,
            key.dom_key().as_deref(),
            repeat,
        )
        .await
    }

    /// Brings a hidden App back to the foreground, as returning to it on a TV
    /// does: execution resumes and `visibilitychange` fires. Does nothing if
    /// the App isn't hidden.
    pub async fn resume(&self) -> Result<(), SessionError> {
        self.inner.resume().await
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
    network: NetworkProxy,
    cdp: Arc<Cdp>,
    /// Fires when the App asks to exit.
    app_exit: mpsc::UnboundedReceiver<ExitReason>,
    /// Fires each time the page finishes loading.
    page_loads: mpsc::UnboundedReceiver<()>,
    /// The running App, whose id a restart may not change. It owns an
    /// unpacked Package's files, so they last until it is replaced.
    app: App,
    /// Changes to the Project Folder or Package; `None` if it can't be
    /// watched.
    watcher: Option<SourceWatcher>,
    /// Where the Package is unpacked. Declared last, so it is removed after
    /// everything using its files.
    session_dir: SessionDir,
}

async fn run(config: SessionConfig, inner: Arc<Inner>, mut stop: oneshot::Receiver<()>) {
    let started = tokio::select! {
        result = startup(&config, &inner) => result,
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

    let reason = loop {
        tokio::select! {
            _ = &mut stop => {
                close_engine(&mut running).await;
                break ExitReason::Stopped;
            }
            Some(reason) = running.app_exit.recv() => {
                inner.hide_before_exit().await;
                close_engine(&mut running).await;
                break reason;
            }
            _ = running.engine.child.wait() => break ExitReason::EngineClosed,
            Some(batch) = next_batch(&mut running.watcher) => {
                inner.hot_reload(&config, &mut running, batch).await;
            }
        }
    };
    stop_server(running.server).await;
    running.network.stop().await;
    inner.finish(reason);
}

impl Inner {
    async fn press_key(&self, name: &str) -> Result<(), SessionError> {
        let key = RemoteKey::find(name)
            .ok_or_else(|| SessionError::UnknownRemoteKey(name.to_string()))?;
        let loaded = self.loaded.get().ok_or(SessionError::NotLoaded)?;
        let drop_key = |reason: String| {
            self.log.publish(SessionEvent::RemoteKey {
                key: name.to_string(),
                outcome: KeyOutcome::Dropped { reason },
            });
            Ok(())
        };

        if !loaded.profile.supports_key(key) {
            return drop_key(format!(
                "{name} does not exist on Tizen {} TVs",
                loaded.profile.version
            ));
        }
        // The platform closes the App and shows the TV channel screen; the App
        // never gets the key, registered or not (Samsung's development
        // checklist, common.html CO-US-04 and CO-GE-07).
        if key.name == EXIT_KEY {
            drop_key("the platform terminates the App instead".into())?;
            self.terminate(ExitReason::TerminatedByRemote);
            return Ok(());
        }
        if !key.mandatory {
            // The page holds the registrations (see the tvinputdevice Shim).
            let registered = loaded
                .cdp
                .evaluate(&format!(
                    "window.__TIZEN_SIM__.keys.isRegistered({})",
                    Value::String(name.to_string())
                ))
                .await?;
            if registered != Value::Bool(true) {
                let reason =
                    format!("{name} is not registered (call tizen.tvinputdevice.registerKey)");
                if !loaded.lenient {
                    return drop_key(reason);
                }
                self.log.publish(SessionEvent::Diagnostic(Diagnostic {
                    code: "key-not-registered".into(),
                    message: format!(
                        "{reason}; delivered anyway in Lenient Mode, a TV would drop it"
                    ),
                }));
            }
        }

        let dom_key = key.dom_key.as_deref();
        dispatch_key(&loaded.cdp, "rawKeyDown", key.code, dom_key, false).await?;
        dispatch_key(&loaded.cdp, "keyUp", key.code, dom_key, false).await?;
        self.log.publish(SessionEvent::RemoteKey {
            key: name.to_string(),
            outcome: KeyOutcome::Delivered,
        });
        Ok(())
    }

    /// Sends the App to the background (TV guides/fundamentals/multitasking.html;
    /// application.html#Application::hide). Minimizing the Engine window makes
    /// `document.hidden` true and fires `visibilitychange`; then, unless the
    /// App has background support, freezing the page pauses JavaScript and
    /// rendering, as on a TV (TD guides/concepts/web-runtime.md).
    async fn hide(&self) -> Result<(), SessionError> {
        let loaded = self.loaded.get().ok_or(SessionError::NotLoaded)?;
        if !loaded.go_to_background().await {
            return Ok(());
        }
        if !loaded.runs_in_background.load(Ordering::SeqCst) {
            // Old Engines lack the call; there the App only looks hidden.
            if let Err(error) = loaded
                .cdp
                .call("Page.setWebLifecycleState", json!({ "state": "frozen" }))
                .await
            {
                tracing::warn!(%error, "could not pause the hidden App");
            }
        }
        self.log.publish(SessionEvent::Lifecycle(Lifecycle::Hidden));
        Ok(())
    }

    async fn resume(&self) -> Result<(), SessionError> {
        let loaded = self.loaded.get().ok_or(SessionError::NotLoaded)?;
        if !loaded.hidden.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        if !loaded.runs_in_background.load(Ordering::SeqCst) {
            let _ = loaded
                .cdp
                .call("Page.setWebLifecycleState", json!({ "state": "active" }))
                .await;
        }
        set_window_state(&loaded.cdp, "normal").await;
        self.log.publish(SessionEvent::Lifecycle(Lifecycle::Shown));
        Ok(())
    }

    /// `visibilitychange` fires when the App exits (TV
    /// guides/fundamentals/multitasking.html), so hide a visible App and give
    /// its handlers the chance to run before the Engine closes.
    async fn hide_before_exit(&self) {
        let Some(loaded) = self.loaded.get() else {
            return;
        };
        if !loaded.go_to_background().await {
            return;
        }
        // The event has been dispatched once the page reports itself hidden.
        for _ in 0..20 {
            match tokio::time::timeout(
                Duration::from_millis(500),
                loaded.cdp.evaluate("document.hidden"),
            )
            .await
            {
                Ok(Ok(Value::Bool(true))) | Ok(Err(_)) | Err(_) => return,
                Ok(Ok(_)) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }

    /// Applies a batch of Project Folder or Package changes to the running App.
    async fn hot_reload(&self, config: &SessionConfig, running: &mut Running, batch: Batch) {
        self.publish_hot_reload(HotReload::FilesChanged {
            paths: batch.paths.clone(),
        });
        // A hidden App may be frozen, and a frozen page doesn't load or
        // restyle; the developer wants to see the change anyway.
        if let Err(error) = self.resume().await {
            tracing::warn!(%error, "could not bring the App back before a hot reload");
        }
        // A new Package may hold any change, config.xml included; a Dev
        // Server URL App's only watched file is its config.xml.
        let action = match config.source {
            AppSource::ProjectFolder(_) => batch.action(),
            AppSource::Package(_) | AppSource::DevServerUrl { .. } => Action::Restart,
        };
        let result = match action {
            Action::SwapStyles => self.swap_styles(running).await,
            Action::Reload => self.reload(running).await,
            Action::Restart => self.restart(config, running).await,
        };
        if let Err(error) = result {
            tracing::warn!(%error, "hot reload failed");
            self.publish_hot_reload(HotReload::Failed { error });
        }
    }

    /// Re-fetches every stylesheet the page links, so focus and navigation
    /// state survive. Reloads instead if the page links none, since the
    /// change must then reach it some other way (an `@import` in a `<style>`,
    /// a frame).
    async fn swap_styles(&self, running: &mut Running) -> Result<(), String> {
        let swapped = running
            .cdp
            .evaluate(SWAP_STYLESHEETS)
            .await
            .map_err(|e| format!("could not swap the stylesheets: {e}"))?;
        if swapped.as_u64().unwrap_or(0) == 0 {
            return self.reload(running).await;
        }
        self.publish_hot_reload(HotReload::StylesSwapped);
        Ok(())
    }

    async fn reload(&self, running: &mut Running) -> Result<(), String> {
        load_page(running, "Page.reload", json!({ "ignoreCache": true })).await?;
        self.publish_hot_reload(HotReload::Reloaded);
        Ok(())
    }

    /// Re-reads the App (unpacking a Package again) and starts it afresh in
    /// the same Engine. This approximates reinstalling and relaunching the
    /// App, keeping its Engine data; it is a simulator shortcut, not
    /// documented TV behaviour. If the new App can't be loaded, the old one
    /// keeps running from its files.
    async fn restart(&self, config: &SessionConfig, running: &mut Running) -> Result<(), String> {
        let loaded = self.loaded.get().ok_or("the App is not loaded yet")?;
        let app = load_settled(&config.source, running.session_dir.path())
            .await
            .map_err(|e| e.to_string())?;
        app.check_installable_on(loaded.profile)
            .map_err(|e| e.to_string())?;
        // The Engine's data belongs to the App id, so another App would
        // inherit this one's storage.
        if app.id() != running.app.id() {
            return Err(format!(
                "the application id changed from {} to {}; restart the Session to run the new App",
                running.app.id(),
                app.id()
            ));
        }
        running.server.set_files(app.files());
        running.server.set_access_policy(app.access_policy());
        running.network.set_policy(app.access_policy().clone());
        let device = loaded
            .profile
            .device_values(&config.device)
            .map_err(|e| e.to_string())?;
        running.server.set_shim_data(shim::session_data(
            &app,
            loaded.profile,
            &device,
            loaded.lenient,
            &config.key_map,
        ));
        loaded
            .runs_in_background
            .store(app.runs_in_background(), Ordering::SeqCst);
        let url = running.server.url(&app.entry_path());
        // The previous Package's files go once the new App has loaded.
        let previous = std::mem::replace(&mut running.app, app);
        let navigated = load_page(running, "Page.navigate", json!({ "url": url })).await;
        drop(previous);
        navigated?;
        self.publish_hot_reload(HotReload::Restarted {
            app_id: running.app.id().to_string(),
        });
        Ok(())
    }

    fn publish_hot_reload(&self, event: HotReload) {
        self.log.publish(SessionEvent::HotReload(event));
    }

    /// Ends the running App, as the platform does.
    fn terminate(&self, reason: ExitReason) {
        if let Some(app_exit) = self.app_exit.get() {
            let _ = app_exit.send(reason);
        }
    }

    fn finish(&self, reason: ExitReason) {
        self.log
            .publish(SessionEvent::Lifecycle(Lifecycle::Exited { reason }));
        self.status.send_replace(Status::Exited);
    }
}

async fn startup(config: &SessionConfig, inner: &Arc<Inner>) -> Result<Running, String> {
    let session_dir = SessionDir::create(&config.home.state_dir().join("sessions"))
        .map_err(|e| format!("could not create the Session directory: {e}"))?;
    let app =
        App::load_unpacking_in(&config.source, session_dir.path()).map_err(|e| e.to_string())?;
    let profile = match &config.profile {
        Some(version) => VersionProfile::find(version).map_err(|e| e.to_string())?,
        None => VersionProfile::latest(),
    };
    app.check_installable_on(profile)
        .map_err(|e| e.to_string())?;
    let device = profile
        .device_values(&config.device)
        .map_err(|e| e.to_string())?;
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
    if let AppFiles::Bundler(bundler) = app.files() {
        check_reachable(bundler).await?;
    }

    let (bridge_tx, bridge_rx) = mpsc::unbounded_channel();
    let (app_exit_tx, app_exit) = mpsc::unbounded_channel();
    let _ = inner.app_exit.set(app_exit_tx);
    tokio::spawn(handle_bridge(bridge_rx, inner.clone()));
    let setup = ServerSetup {
        data: shim::session_data(&app, profile, &device, config.lenient, &config.key_map),
        token: bridge_token()?,
        profile,
        access_policy: app.access_policy().clone(),
    };
    let server = DevServer::start(app.files(), setup, bridge_tx)
        .await
        .map_err(|e| format!("could not start the dev server: {e}"))?;
    let (network_tx, network_rx) = mpsc::unbounded_channel();
    tokio::spawn(handle_network(network_rx, inner.clone()));
    let network = NetworkProxy::start(app.access_policy().clone(), server.addr(), network_tx)
        .await
        .map_err(|e| format!("could not start the network proxy: {e}"))?;
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
            network_proxy: Some(network.addr()),
            // Packaged Apps aren't held to browser CORS on a TV (measured on
            // a Tizen 5.0 TV, ADR-0003).
            disable_web_security: true,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let (cdp, events) = Cdp::connect(&engine.page_ws_url)
        .await
        .map_err(|e| e.to_string())?;
    let cdp = Arc::new(cdp);
    let (page_loads_tx, page_loads) = mpsc::unbounded_channel();
    tokio::spawn(forward_events(events, inner.clone(), page_loads_tx));
    // Attach before navigating so no console output from the first script is lost.
    cdp.call("Runtime.enable", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    cdp.call("Page.enable", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    // The Engine's own console messages, such as CSP violations.
    cdp.call("Log.enable", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    inner
        .log
        .publish(SessionEvent::Lifecycle(Lifecycle::EngineReady {
            engine_version: engine.engine_version.clone(),
            debugger_url: engine.page_ws_url.clone(),
        }));

    // Watch before the first load, so no edit made once the App is up is missed.
    let watcher = match &config.source {
        AppSource::ProjectFolder(root) => SourceWatcher::start(root),
        AppSource::Package(wgt) => SourceWatcher::start_file(wgt),
        AppSource::DevServerUrl { config, .. } => SourceWatcher::start_file(config),
    }
    .inspect_err(
        |error| tracing::warn!(%error, "could not watch the App Source; hot reload is off"),
    )
    .ok();

    let url = server.url(&app.entry_path());
    let runs_in_background = app.runs_in_background();
    let mut running = Running {
        engine,
        server,
        network,
        cdp: cdp.clone(),
        app_exit,
        page_loads,
        app,
        watcher,
        session_dir,
    };
    running.network.set_logging(true);
    load_page(&mut running, "Page.navigate", json!({ "url": url }))
        .await
        .map_err(|e| format!("could not load the App at {url}: {e}"))?;

    let _ = inner.loaded.set(Loaded {
        cdp,
        profile,
        lenient: config.lenient,
        runs_in_background: AtomicBool::new(runs_in_background),
        hidden: AtomicBool::new(false),
    });
    inner
        .log
        .publish(SessionEvent::Lifecycle(Lifecycle::AppLoaded { url }));
    inner.status.send_replace(Status::Loaded);
    Ok(running)
}

/// Loads a page with a `Page.navigate` or `Page.reload` call and waits for it
/// to finish loading, then focuses it.
async fn load_page(running: &mut Running, method: &str, params: Value) -> Result<(), String> {
    // Forget loads that finished before this one started.
    while running.page_loads.try_recv().is_ok() {}
    running
        .cdp
        .call(method, params)
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(30), running.page_loads.recv())
        .await
        .map_err(|_| "the App did not finish loading within 30s".to_string())?;

    // Key events only reach a focused page (#24 prototype). Older Engines
    // lack Page.bringToFront, so its failure is ignored.
    let _ = running.cdp.call("Page.bringToFront", json!({})).await;
    let _ = running.cdp.evaluate("window.focus(); 0").await;
    Ok(())
}

/// Fails unless the developer's bundler answers at the Dev Server URL, so a
/// dev server that isn't running is named instead of loading an error page.
async fn check_reachable(bundler: &url::Url) -> Result<(), String> {
    // The client the proxy uses, so both reach the bundler the same way.
    let request = crate::proxy::client().get(bundler.clone()).send();
    match tokio::time::timeout(Duration::from_secs(10), request).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(format!(
            "could not reach the dev server at {bundler}; is it running? ({error})"
        )),
        Err(_) => Err(format!(
            "the dev server at {bundler} did not answer within 10s"
        )),
    }
}

/// How often, and how far apart, a Package that can't be read or unpacked
/// is tried again: a build may still be writing it when its change is seen.
const PACKAGE_RETRIES: u32 = 4;
const PACKAGE_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Loads the App, retrying a Package that isn't fully written yet.
async fn load_settled(source: &AppSource, unpack_in: &std::path::Path) -> Result<App, AppError> {
    let mut retries = 0;
    loop {
        match App::load_unpacking_in(source, unpack_in) {
            Err(AppError::PackageCorrupt { .. } | AppError::PackageUnreadable { .. })
                if retries < PACKAGE_RETRIES =>
            {
                retries += 1;
                tokio::time::sleep(PACKAGE_RETRY_DELAY).await;
            }
            result => return result,
        }
    }
}

async fn next_batch(watcher: &mut Option<SourceWatcher>) -> Option<Batch> {
    match watcher {
        Some(watcher) => watcher.next().await,
        None => std::future::pending().await,
    }
}

/// Replaces each same-origin `<link rel="stylesheet">` with a copy that
/// re-fetches it, removing the old one once the new one has loaded so the
/// page never shows unstyled. Evaluates to how many it replaced. ES5, for
/// old Engines.
const SWAP_STYLESHEETS: &str = r#"(function () {
  var links = document.querySelectorAll('link[rel~="stylesheet"]');
  var parser = document.createElement('a');
  var stamp = 'tizen-sim-reload=' + Date.now();
  var swapped = 0;
  for (var i = 0; i < links.length; i++) {
    var link = links[i];
    parser.href = link.href;
    if (!link.parentNode || parser.host !== location.host) continue;
    var search = parser.search.replace(/[?&]tizen-sim-reload=\d+/, '').replace(/^&/, '?');
    var fresh = link.cloneNode(false);
    fresh.href = parser.protocol + '//' + parser.host + parser.pathname +
      (search ? search + '&' : '?') + stamp + parser.hash;
    var done = (function (old) {
      return function () { if (old.parentNode) old.parentNode.removeChild(old); };
    })(link);
    fresh.onload = done;
    fresh.onerror = done;
    link.parentNode.insertBefore(fresh, link.nextSibling);
    swapped++;
  }
  return swapped;
})()"#;

/// Acts on what the Shims send over the Bridge.
async fn handle_bridge(mut messages: mpsc::UnboundedReceiver<BridgeMessage>, inner: Arc<Inner>) {
    while let Some(message) = messages.recv().await {
        match message.method.as_str() {
            "diagnostic" => {
                let text = |key: &str| message.params[key].as_str().unwrap_or_default().to_string();
                let diagnostic = Diagnostic {
                    code: text("code"),
                    message: text("message"),
                };
                tracing::warn!(code = %diagnostic.code, "{}", diagnostic.message);
                inner.log.publish(SessionEvent::Diagnostic(diagnostic));
            }
            "application.exit" => inner.terminate(ExitReason::AppExited),
            "application.hide" => {
                if let Err(error) = inner.hide().await {
                    tracing::warn!(%error, "could not hide the App");
                }
            }
            // The Back key was held: the platform terminates the App.
            "application.terminate" => inner.terminate(ExitReason::TerminatedByRemote),
            // A keyboard key the Shim mapped to a Remote Key.
            "keys.press" => {
                let key = message.params["key"].as_str().unwrap_or_default();
                if let Err(error) = inner.press_key(key).await {
                    tracing::warn!(%error, key, "could not press the mapped Remote Key");
                    inner.log.publish(SessionEvent::RemoteKey {
                        key: key.to_string(),
                        outcome: KeyOutcome::Dropped {
                            reason: format!("could not deliver it: {error}"),
                        },
                    });
                }
            }
            // A keyboard key the Shim dropped because the App hasn't registered it.
            "keys.dropped" => {
                let text = |key: &str| message.params[key].as_str().unwrap_or_default().to_string();
                inner.log.publish(SessionEvent::RemoteKey {
                    key: text("key"),
                    outcome: KeyOutcome::Dropped {
                        reason: text("reason"),
                    },
                });
            }
            other => tracing::debug!(method = other, "unknown Bridge message"),
        }
    }
}

/// Publishes the Network Log, raising a Diagnostic the first time each
/// missing `<access>` element blocks a request.
async fn handle_network(mut requests: mpsc::UnboundedReceiver<NetworkRequest>, inner: Arc<Inner>) {
    let mut diagnosed = HashSet::new();
    while let Some(request) = requests.recv().await {
        let blocked = match &request.verdict {
            Verdict::Blocked { missing_access } if diagnosed.insert(missing_access.clone()) => {
                Some(Diagnostic {
                    code: "access-not-declared".into(),
                    message: format!(
                        "{} {} was blocked: in WARP mode a TV only lets an App reach what the \
                         <access> elements in its config.xml grant. Add {missing_access}, \
                         with subdomains=\"true\" to cover its subdomains too.",
                        request.method, request.url
                    ),
                })
            }
            _ => None,
        };
        inner.log.publish(SessionEvent::Network(request));
        if let Some(diagnostic) = blocked {
            tracing::warn!(code = %diagnostic.code, "{}", diagnostic.message);
            inner.log.publish(SessionEvent::Diagnostic(diagnostic));
        }
    }
}

/// A fresh secret for the Bridge, so other local processes can't impersonate the page.
fn bridge_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| format!("could not generate a Bridge token: {e}"))?;
    Ok(hex::encode(bytes))
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
            "Log.entryAdded" => {
                let entry = &params["entry"];
                inner.log.publish(SessionEvent::Console(ConsoleMessage {
                    level: entry["level"].as_str().unwrap_or("info").to_string(),
                    text: entry["text"].as_str().unwrap_or_default().to_string(),
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

impl Loaded {
    /// Marks the App hidden and minimizes the Engine window, which makes
    /// `document.hidden` true and fires `visibilitychange`. False if it was
    /// already hidden.
    async fn go_to_background(&self) -> bool {
        if self.hidden.swap(true, Ordering::SeqCst) {
            return false;
        }
        set_window_state(&self.cdp, "minimized").await;
        true
    }
}

/// Minimizes or restores the Engine window. Engines without the Browser
/// domain keep their window as it is.
async fn set_window_state(cdp: &Cdp, state: &str) {
    let window = match cdp.call("Browser.getWindowForTarget", json!({})).await {
        Ok(window) => window["windowId"].clone(),
        Err(error) => {
            tracing::warn!(%error, "could not find the Engine window");
            return;
        }
    };
    if let Err(error) = cdp
        .call(
            "Browser.setWindowBounds",
            json!({ "windowId": window, "bounds": { "windowState": state } }),
        )
        .await
    {
        tracing::warn!(%error, state, "could not change the Engine window state");
    }
}

async fn close_engine(running: &mut Running) {
    running.network.set_logging(false);
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

/// The Remote Key that makes the platform close the App.
const EXIT_KEY: &str = "Exit";

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

/// Sends one key event to the page as trusted input.
async fn dispatch_key(
    cdp: &Cdp,
    event_type: &str,
    key_code: u16,
    dom_key: Option<&str>,
    auto_repeat: bool,
) -> Result<(), SessionError> {
    let mut params = json!({
        "type": event_type,
        "windowsVirtualKeyCode": key_code,
        "nativeVirtualKeyCode": key_code,
        "autoRepeat": auto_repeat,
    });
    if let Some(dom_key) = dom_key {
        params["key"] = Value::String(dom_key.to_string());
    }
    cdp.call("Input.dispatchKeyEvent", params).await?;
    Ok(())
}
