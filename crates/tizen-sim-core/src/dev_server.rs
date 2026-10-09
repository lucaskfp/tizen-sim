//! The Session's dev server: serves the App's files over local HTTP.

use std::net::SocketAddr;
use std::path::Path;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;

pub struct DevServer {
    addr: SocketAddr,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl DevServer {
    pub async fn start(root: &Path) -> std::io::Result<DevServer> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let router = axum::Router::new().fallback_service(ServeDir::new(root));
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        Ok(DevServer {
            addr,
            shutdown,
            task,
        })
    }

    /// The URL of a path inside the App, e.g. its entry point.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path.trim_start_matches('/'))
    }

    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}
