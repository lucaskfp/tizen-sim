//! A minimal Chrome DevTools Protocol client for one page target.
//!
//! Only the handful of methods every pinned Engine supports are used; see
//! ADR-0002 and the #24 prototype findings.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    #[error("could not connect to the Engine's debugging endpoint: {0}")]
    Connect(String),
    #[error("the Engine's debugging connection closed")]
    Closed,
    #[error("{method} failed: {message}")]
    Protocol { method: String, message: String },
}

/// A protocol event: method name and params.
pub type CdpEvent = (String, Value);

/// A reply: the result, or the protocol's error object.
type Reply = Result<Value, Value>;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Reply>>>>;

pub struct Cdp {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
    /// Every event, for other clients sharing this connection (see
    /// `devtools_relay`).
    shared_events: broadcast::Sender<CdpEvent>,
}

impl Cdp {
    /// Connects to a page target. Events are delivered on the returned receiver.
    pub async fn connect(
        ws_url: &str,
    ) -> Result<(Cdp, mpsc::UnboundedReceiver<CdpEvent>), CdpError> {
        let (socket, _) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| CdpError::Connect(e.to_string()))?;
        let (mut sink, mut stream) = socket.split();
        let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<Message>();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let pending: Pending = Arc::default();
        let shared_events = broadcast::channel(4096).0;

        tokio::spawn(async move {
            while let Some(msg) = outgoing_rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let reader_pending = pending.clone();
        let reader_shared = shared_events.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                let Message::Text(text) = msg else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if let Some(id) = value["id"].as_u64() {
                    if let Some(reply) = reader_pending.lock().await.remove(&id) {
                        let result = match value.get("error") {
                            Some(error) => Err(error.clone()),
                            None => Ok(value["result"].clone()),
                        };
                        let _ = reply.send(result);
                    }
                } else if let Some(method) = value["method"].as_str() {
                    let event = (method.to_string(), value["params"].clone());
                    let _ = reader_shared.send(event.clone());
                    let _ = events_tx.send(event);
                }
            }
            // Connection gone: fail everything still waiting.
            reader_pending.lock().await.clear();
        });

        Ok((
            Cdp {
                outgoing,
                pending,
                next_id: AtomicU64::new(1),
                shared_events,
            },
            events_rx,
        ))
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, CdpError> {
        self.call_raw(method, params)
            .await?
            .map_err(|error| CdpError::Protocol {
                method: method.to_string(),
                message: error["message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_string(),
            })
    }

    /// Like `call`, with a protocol error as the Engine sent it.
    pub async fn call_raw(&self, method: &str, params: Value) -> Result<Reply, CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write(id, method, params)?;
        rx.await.map_err(|_| CdpError::Closed)
    }

    /// Every event from now on, alongside the receiver `connect` returned.
    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.shared_events.subscribe()
    }

    /// Sends a command without waiting for its reply, which is dropped.
    pub fn send(&self, method: &str, params: Value) -> Result<(), CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.write(id, method, params)
    }

    fn write(&self, id: u64, method: &str, params: Value) -> Result<(), CdpError> {
        let request = json!({ "id": id, "method": method, "params": params }).to_string();
        self.outgoing
            .send(Message::text(request))
            .map_err(|_| CdpError::Closed)
    }

    /// Evaluates an expression in the page and returns its value as JSON.
    pub async fn evaluate(&self, expression: &str) -> Result<Value, CdpError> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true }),
            )
            .await?;
        if let Some(details) = result.get("exceptionDetails") {
            let message = details["exception"]["description"]
                .as_str()
                .or_else(|| details["text"].as_str())
                .unwrap_or("exception thrown")
                .to_string();
            return Err(CdpError::Protocol {
                method: "Runtime.evaluate".into(),
                message,
            });
        }
        Ok(result["result"]["value"].clone())
    }
}
