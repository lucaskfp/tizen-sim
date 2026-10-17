//! The tizen-sim GUI: a Tauri app that drives Sessions through
//! `tizen-sim-core`. It is a thin client; the Session's event stream is the
//! single source of everything it shows.

pub mod controller;
pub mod embed;

use controller::{Controller, ControllerOptions, GuiEvent, RunningSession, Screen};
use embed::{Embedder, ScreenArea};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, Runtime, State};
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::profile::VersionProfile;
use tizen_sim_core::session::KeyOutcome;

/// The event every Session event reaches the GUI's webview in.
pub const SESSION_EVENT: &str = "session-event";

/// The GUI's window; the only one with IPC (capabilities/main.json).
pub const MAIN_WINDOW: &str = "main";

/// The App Source the developer chose.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceChoice {
    /// A Project Folder, or a Package (`.wgt` file).
    Path { path: PathBuf },
    /// A bundler's Dev Server URL with the App's `config.xml`.
    DevServer { url: String, config: PathBuf },
}

impl SourceChoice {
    fn into_source(self) -> Result<AppSource, String> {
        Ok(match self {
            SourceChoice::Path { path } => AppSource::from_path(path),
            SourceChoice::DevServer { url, config } => AppSource::DevServerUrl {
                url: url
                    .parse()
                    .map_err(|e| format!("invalid dev server URL {url}: {e}"))?,
                config,
            },
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileInfo {
    pub version: String,
    pub milestone: u32,
    pub latest: bool,
}

#[tauri::command]
async fn open_app(source: SourceChoice, controller: State<'_, Controller>) -> Result<(), String> {
    controller.open(source.into_source()?, None).await;
    Ok(())
}

#[tauri::command]
async fn switch_profile(profile: String, controller: State<'_, Controller>) -> Result<(), String> {
    controller
        .switch_profile(&profile)
        .await
        .map_err(|e| e.to_string())
}

/// Starts the current App afresh, e.g. after its Engine crashed.
#[tauri::command]
async fn restart(controller: State<'_, Controller>) -> Result<(), String> {
    controller.restart().await.map_err(|e| e.to_string())
}

/// The Sessions other processes (the CLI) are running, which the GUI can adopt.
#[tauri::command]
fn running_sessions(controller: State<'_, Controller>) -> Vec<RunningSession> {
    controller.adoptable()
}

#[tauri::command]
async fn adopt(id: String, controller: State<'_, Controller>) -> Result<(), String> {
    controller.adopt(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn press_key(key: String, controller: State<'_, Controller>) -> Result<KeyOutcome, String> {
    controller.press_key(&key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn reload(controller: State<'_, Controller>) -> Result<(), String> {
    controller.reload().await.map_err(|e| e.to_string())
}

/// Saves a PNG of the App's screen to `path`, which the developer chose.
#[tauri::command]
async fn screenshot(path: PathBuf, controller: State<'_, Controller>) -> Result<(), String> {
    let png = controller.screenshot().await.map_err(|e| e.to_string())?;
    tokio::fs::write(&path, png)
        .await
        .map_err(|e| format!("could not save the screenshot to {}: {e}", path.display()))
}

#[tauri::command]
fn profiles() -> Vec<ProfileInfo> {
    let latest = &VersionProfile::latest().version;
    VersionProfile::all()
        .iter()
        .map(|p| ProfileInfo {
            version: p.version.clone(),
            milestone: p.milestone,
            latest: &p.version == latest,
        })
        .collect()
}

/// Version Profile `profile`'s device values, with those chosen in the GUI.
#[tauri::command]
fn device_values(
    profile: String,
    controller: State<'_, Controller>,
) -> Result<serde_json::Value, String> {
    controller
        .device_values(&profile)
        .map_err(|e| e.to_string())
}

/// Sets a device value the platform APIs report, restarting the App.
#[tauri::command]
async fn set_device_value(
    name: String,
    value: serde_json::Value,
    controller: State<'_, Controller>,
) -> Result<(), String> {
    controller
        .set_device_value(&name, value)
        .await
        .map_err(|e| e.to_string())
}

/// Whether the TV screen is shown inside the GUI (ADR-0006: on Windows).
#[tauri::command]
fn embeds_screen() -> bool {
    Embedder::SUPPORTED
}

/// Where the GUI's screen area is now, or `None` while it shows none. Not
/// `async`, so it runs on the main thread, which owns the GUI's window.
#[tauri::command]
fn set_screen_area<R: Runtime>(
    area: Option<ScreenArea>,
    window: tauri::Window<R>,
    screen: State<'_, Arc<GuiScreen>>,
) {
    let scale = window.scale_factor().unwrap_or(1.0);
    screen.embedder.set_area(area, scale);
}

/// The screen area of the GUI's window, which every Embedder call must run
/// on the main thread for.
pub struct GuiScreen {
    embedder: Arc<Embedder>,
    main_thread: std::thread::ThreadId,
    /// Runs a closure on the main thread.
    dispatch: Dispatch,
}

/// Posts a closure to the main thread; `false` if it can't be.
type Dispatch = Box<dyn Fn(Box<dyn FnOnce() + Send>) -> bool + Send + Sync>;

/// How long to wait for the main thread; it may be busy closing the GUI.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(2);

impl GuiScreen {
    fn on_main_thread<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Embedder) -> T + Send + 'static,
    ) -> Option<T> {
        if std::thread::current().id() == self.main_thread {
            return Some(f(&self.embedder));
        }
        let embedder = self.embedder.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let posted = (self.dispatch)(Box::new(move || {
            let _ = tx.send(f(&embedder));
        }));
        if !posted {
            return None;
        }
        rx.recv_timeout(MAIN_THREAD_TIMEOUT).ok()
    }
}

impl Screen for GuiScreen {
    fn embed(&self, window: u64, engine_pid: Option<u32>, owned: bool) -> bool {
        self.on_main_thread(move |e| e.embed(window, engine_pid, owned))
            .unwrap_or(false)
    }

    fn release(&self) {
        // Nothing to wait for the main thread for, which is busy, or even
        // blocked, while the GUI closes.
        if self.embedder.is_embedded() {
            self.on_main_thread(|e| e.release());
        }
    }
}

/// Sets up the GUI app on `builder`, which tests start on Tauri's mock runtime.
pub fn setup<R: Runtime>(
    builder: tauri::Builder<R>,
    options: ControllerOptions,
) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let screen = Arc::new(gui_screen(app.handle()));
            let controller = Controller::new(options, move |event: GuiEvent| {
                if let Err(error) = handle.emit_to(MAIN_WINDOW, SESSION_EVENT, event) {
                    tracing::warn!(%error, "could not send a Session event to the GUI");
                }
            })
            .with_screen(screen.clone());
            app.manage(controller);
            app.manage(screen);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_app,
            switch_profile,
            restart,
            running_sessions,
            adopt,
            press_key,
            reload,
            screenshot,
            profiles,
            embeds_screen,
            set_screen_area,
            device_values,
            set_device_value
        ])
}

/// The screen area of the main window. Called during setup, on the main thread.
fn gui_screen<R: Runtime>(app: &tauri::AppHandle<R>) -> GuiScreen {
    let parent = main_window_handle(app);
    let dispatcher = app.clone();
    GuiScreen {
        embedder: Arc::new(Embedder::new(parent)),
        main_thread: std::thread::current().id(),
        dispatch: Box::new(move |f| dispatcher.run_on_main_thread(f).is_ok()),
    }
}

#[cfg(windows)]
fn main_window_handle<R: Runtime>(app: &tauri::AppHandle<R>) -> isize {
    app.get_webview_window(MAIN_WINDOW)
        .and_then(|w| w.hwnd().ok())
        .map(|hwnd| hwnd.0 as isize)
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn main_window_handle<R: Runtime>(_app: &tauri::AppHandle<R>) -> isize {
    0
}

/// The GUI's configuration and capabilities, as built into the app.
pub fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let options = ControllerOptions {
        home: Home::from_env(),
        headless: false,
        engine_path: None,
    };
    // Only in the app itself: the updater needs the configuration it is
    // built with, which tests on the mock runtime don't have.
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    setup(builder, options)
        .build(context())
        .expect("could not start the GUI")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent { label, event, .. } if label == MAIN_WINDOW => {
                let screen = app.state::<Arc<GuiScreen>>();
                match event {
                    // An adopted Session's Engine window would close with the
                    // GUI's: give it back while the GUI's window is still up.
                    tauri::WindowEvent::CloseRequested { .. } => screen.release(),
                    tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        screen.embedder.set_scale(scale_factor);
                    }
                    _ => {}
                }
            }
            // Closing the GUI stops its own Session: the Engine and every
            // server. An adopted Session keeps running.
            tauri::RunEvent::Exit => {
                tauri::async_runtime::block_on(app.state::<Controller>().stop());
            }
            _ => {}
        })
}
