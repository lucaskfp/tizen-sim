//! DevTools for Engines that take one debugging client per page.
//!
//! M47 (protocol 1.1) refuses a second connection to a page target while the
//! Session holds one (#38), so the Engine's bundled inspector can't attach
//! to the App on its own. The relay lets it in through the Session's
//! connection: the inspector connects to the relay, its commands go out on
//! the Session's connection under fresh ids, and every event comes back to
//! it.
//!
//! The Session already enabled `Runtime`, `Page` and `Console`, and enabling
//! a domain again replays nothing on M47 (measured), so the relay answers the
//! inspector's `enable` itself, replaying the execution contexts and console
//! messages it has seen, and keeps its `disable` from turning off what the
//! Session relies on.

use crate::cdp::{Cdp, CdpEvent};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// The domains the Session enables (see `session::startup`).
const SESSION_DOMAINS: [&str; 3] = ["Runtime", "Page", "Console"];

/// How many console messages are kept for an inspector that connects later.
const CONSOLE_HISTORY: usize = 1000;

/// What a newly connected inspector is told when it enables a domain.
#[derive(Default)]
struct Replay {
    /// `Runtime.executionContextCreated` params of the live contexts.
    contexts: Vec<Value>,
    /// `Console.messageAdded` params since the console was last cleared.
    console: VecDeque<Value>,
}

impl Replay {
    fn record(&mut self, (method, params): &CdpEvent) {
        match method.as_str() {
            "Runtime.executionContextCreated" => self.contexts.push(params.clone()),
            "Runtime.executionContextDestroyed" => {
                let id = &params["executionContextId"];
                self.contexts.retain(|c| &c["context"]["id"] != id);
            }
            "Runtime.executionContextsCleared" => self.contexts.clear(),
            "Console.messageAdded" => {
                if self.console.len() == CONSOLE_HISTORY {
                    self.console.pop_front();
                }
                self.console.push_back(params.clone());
            }
            "Console.messagesCleared" => self.console.clear(),
            _ => {}
        }
    }

    /// The events replaying a domain's state on `enable`.
    fn events(&self, domain: &str) -> Vec<(&'static str, Value)> {
        match domain {
            "Runtime" => self
                .contexts
                .iter()
                .map(|c| ("Runtime.executionContextCreated", c.clone()))
                .collect(),
            "Console" => self
                .console
                .iter()
                .map(|m| ("Console.messageAdded", m.clone()))
                .collect(),
            _ => vec![],
        }
    }
}

/// A running relay; it stops when dropped.
pub(crate) struct DevtoolsRelay {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DevtoolsRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl DevtoolsRelay {
    /// Starts relaying for the page `cdp` is connected to. Call it before the
    /// App loads, so the replay has every context and console message.
    pub(crate) async fn start(cdp: Arc<Cdp>) -> std::io::Result<DevtoolsRelay> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let replay = Arc::new(Mutex::new(Replay::default()));
        let mut events = cdp.subscribe();
        let recorder = replay.clone();
        let task = tokio::spawn(async move {
            let record = async {
                loop {
                    match events.recv().await {
                        Ok(event) => recorder.lock().unwrap().record(&event),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => return,
                    }
                }
            };
            let accept = async {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, cdp.clone(), replay.clone()));
                }
            };
            tokio::join!(record, accept);
        });
        Ok(DevtoolsRelay { addr, task })
    }

    /// The bundled inspector's address on the Engine's debugging port, with
    /// its WebSocket pointed at the relay.
    pub(crate) fn inspector_url(&self, debug_port: u16, page_ws_url: &str) -> String {
        let page = crate::engine::page_id(page_ws_url);
        format!(
            "http://127.0.0.1:{debug_port}/devtools/inspector.html?ws={}/devtools/page/{page}",
            self.addr
        )
    }
}

/// Serves one inspector connection until it closes.
async fn serve(stream: tokio::net::TcpStream, cdp: Arc<Cdp>, replay: Arc<Mutex<Replay>>) {
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut incoming) = socket.split();
    let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            if sink.send(Message::text(message.to_string())).await.is_err() {
                return;
            }
        }
    });
    let mut events = cdp.subscribe();
    let forward = outgoing.clone();
    let events_task = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok((method, params)) => {
                    let _ = forward.send(json!({ "method": method, "params": params }));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            }
        }
    });

    while let Some(Ok(message)) = incoming.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(request) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default().to_string();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let (domain, command) = method.split_once('.').unwrap_or((&method, ""));
        if SESSION_DOMAINS.contains(&domain) && matches!(command, "enable" | "disable") {
            if command == "enable" {
                for (event, params) in replay.lock().unwrap().events(domain) {
                    let _ = outgoing.send(json!({ "method": event, "params": params }));
                }
            }
            let _ = outgoing.send(json!({ "id": id, "result": {} }));
            continue;
        }
        let (cdp, outgoing) = (cdp.clone(), outgoing.clone());
        tokio::spawn(async move {
            let reply = match cdp.call_raw(&method, params).await {
                Ok(Ok(result)) => json!({ "id": id, "result": result }),
                Ok(Err(error)) => json!({ "id": id, "error": error }),
                Err(error) => {
                    json!({ "id": id, "error": { "code": -32000, "message": error.to_string() } })
                }
            };
            let _ = outgoing.send(reply);
        });
    }
    events_task.abort();
    writer.abort();
}

/// Whether the Engine lets a second client debug the page at `page_ws_url`.
pub(crate) async fn takes_another_client(page_ws_url: &str) -> bool {
    match tokio_tungstenite::connect_async(page_ws_url).await {
        Ok((mut socket, _)) => {
            let _ = socket.close(None).await;
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(method: &str, params: Value) -> CdpEvent {
        (method.to_string(), params)
    }

    #[test]
    fn replays_the_live_contexts() {
        let mut replay = Replay::default();
        replay.record(&event(
            "Runtime.executionContextCreated",
            json!({ "context": { "id": 1 } }),
        ));
        replay.record(&event(
            "Runtime.executionContextCreated",
            json!({ "context": { "id": 2 } }),
        ));
        replay.record(&event(
            "Runtime.executionContextDestroyed",
            json!({ "executionContextId": 1 }),
        ));
        let events = replay.events("Runtime");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["context"]["id"], 2);

        replay.record(&event("Runtime.executionContextsCleared", json!({})));
        assert!(replay.events("Runtime").is_empty());
    }

    #[test]
    fn replays_console_messages_since_the_last_clear() {
        let mut replay = Replay::default();
        replay.record(&event(
            "Console.messageAdded",
            json!({ "message": { "text": "a" } }),
        ));
        replay.record(&event("Console.messagesCleared", json!({})));
        replay.record(&event(
            "Console.messageAdded",
            json!({ "message": { "text": "b" } }),
        ));
        let events = replay.events("Console");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1["message"]["text"], "b");
        assert!(replay.events("Page").is_empty());
    }
}
