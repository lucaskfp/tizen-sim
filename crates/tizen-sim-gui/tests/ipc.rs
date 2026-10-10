//! The GUI app's IPC, on Tauri's mock runtime: no webview, real commands and
//! the real capabilities.

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tizen_sim_core::home::Home;
use tizen_sim_gui::controller::ControllerOptions;

fn app() -> tauri::App<MockRuntime> {
    let options = ControllerOptions {
        // Never touched: these tests start no Session.
        home: Home::at(std::env::temp_dir().join("tizen-sim-ipc-tests")),
        headless: true,
        engine_path: None,
    };
    tizen_sim_gui::setup(mock_builder(), options)
        .build(tizen_sim_gui::context())
        .unwrap()
}

fn window(app: &tauri::App<MockRuntime>, label: &str) -> tauri::WebviewWindow<MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .unwrap()
}

fn invoke(
    webview: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
) -> Result<serde_json::Value, serde_json::Value> {
    get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: webview.url().unwrap(),
            body: InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize().unwrap())
}

#[test]
fn the_main_window_lists_the_version_profiles() {
    let app = app();
    let main = window(&app, tizen_sim_gui::MAIN_WINDOW);

    let profiles = invoke(&main, "profiles").unwrap();

    let versions: Vec<&str> = profiles
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["version"].as_str().unwrap())
        .collect();
    assert!(versions.contains(&"10.0"), "{profiles}");
}

#[test]
fn only_the_main_window_has_ipc() {
    let app = app();
    let other = window(&app, "other");

    let refused = invoke(&other, "profiles").unwrap_err();

    assert!(refused.to_string().contains("not allowed"), "{refused}");
}
