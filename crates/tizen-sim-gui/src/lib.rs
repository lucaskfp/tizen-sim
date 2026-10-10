//! The tizen-sim GUI: a Tauri app that drives Sessions through
//! `tizen-sim-core`. It is a thin client; the Session's event stream is the
//! single source of everything it shows.

pub mod controller;

use controller::{Controller, ControllerOptions, GuiEvent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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

/// Sets up the GUI app on `builder`, which tests start on Tauri's mock runtime.
pub fn setup<R: Runtime>(
    builder: tauri::Builder<R>,
    options: ControllerOptions,
) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            app.manage(Controller::new(options, move |event: GuiEvent| {
                if let Err(error) = handle.emit_to(MAIN_WINDOW, SESSION_EVENT, event) {
                    tracing::warn!(%error, "could not send a Session event to the GUI");
                }
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_app,
            switch_profile,
            press_key,
            reload,
            screenshot,
            profiles
        ])
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
    setup(tauri::Builder::default(), options)
        .build(context())
        .expect("could not start the GUI")
        .run(|app, event| {
            // Closing the GUI stops its Session: the Engine and every server.
            if let tauri::RunEvent::Exit = event {
                tauri::async_runtime::block_on(app.state::<Controller>().stop());
            }
        });
}
