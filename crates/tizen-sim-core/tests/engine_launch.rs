//! Launching an Engine itself, below the Session.

mod support;

use tizen_sim_core::engine::{self, EngineSpec, EngineWindow, LaunchOptions};
use tizen_sim_core::home::Home;

/// Options for the test Version Profile's Engine, in its own data directory.
fn options(spec: &EngineSpec, user_data_dir: &std::path::Path) -> LaunchOptions {
    LaunchOptions {
        user_data_dir: user_data_dir.to_path_buf(),
        window: if spec.headless {
            EngineWindow::Headless
        } else {
            EngineWindow::OffScreen
        },
        user_agent: None,
        network_proxy: None,
        disable_web_security: false,
        sandbox: spec.sandbox,
        gpu_compositing: spec.gpu_compositing,
    }
}

fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A port picked as free can be taken by another Engine starting at the same
/// time; the Engine then has no debugging port, and the page found on it is
/// the other Engine's (#83, measured on M47 to M130). The launch moves on to
/// another port and finds its own page.
#[tokio::test]
async fn an_engine_moves_off_a_debugging_port_another_engine_took() {
    let platform = engine::current_platform().unwrap();
    let spec = EngineSpec::for_profile(&support::profile().version, platform).unwrap();
    let installed = Home::from_env()
        .engine_store()
        .ensure(spec, |_| {})
        .await
        .unwrap();
    let (first_dir, second_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let taken = unused_port();
    let first = engine::launch_with_ports(
        &installed.executable,
        &options(spec, first_dir.path()),
        [taken],
    )
    .await
    .unwrap();
    assert_eq!(first.debug_port, taken);

    let other = unused_port();
    let second = engine::launch_with_ports(
        &installed.executable,
        &options(spec, second_dir.path()),
        [taken, other],
    )
    .await
    .unwrap();

    assert_eq!(second.debug_port, other);
    assert_ne!(second.page_ws_url, first.page_ws_url);
    assert!(
        second.page_ws_url.contains(&format!(":{other}/")),
        "{}",
        second.page_ws_url
    );
}
