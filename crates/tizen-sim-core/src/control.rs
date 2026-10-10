//! A Session's control endpoint: its handle's operations and event stream
//! over local HTTP, for other terminals and the GUI (ADR-0005). Every request
//! must carry the Session's token as `Authorization: Bearer <token>`.
//!
//! - `GET /status`: the `SessionStatus`
//! - `POST /key` `{"key": "Back"}`: presses a Remote Key, answering its `KeyOutcome`
//! - `POST /reload`: reloads the App
//! - `GET /screenshot`: a PNG of the App's screen
//! - `POST /evaluate` `{"expression": "..."}`: the expression's value
//! - `GET /events`: every `SessionEvent` so far, then new ones until the
//!   Session exits, one JSON object per line
//!
//! Errors answer `{"error": "..."}`.
//!
//! These are what the CLI and the GUI's adoption need for now; the rest of
//! the handle (stop, resume, DevTools, ...) can follow when a client needs it.

use crate::session::{Lifecycle, Session, SessionError, SessionEvent};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;

pub struct ControlEndpoint {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

struct ControlState {
    session: Session,
    token: String,
}

impl ControlEndpoint {
    pub async fn start(session: Session, token: String) -> std::io::Result<ControlEndpoint> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(ControlState { session, token });
        let router = Router::new()
            .route("/status", get(status))
            .route("/key", post(press_key))
            .route("/reload", post(reload))
            .route("/screenshot", get(screenshot))
            .route("/evaluate", post(evaluate))
            .route("/events", get(events))
            .layer(middleware::from_fn_with_state(state.clone(), authorize))
            .with_state(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(ControlEndpoint { addr, task })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for ControlEndpoint {
    /// Stops taking connections. Requests already in progress finish, and
    /// event streams end with the Session's `Exited` event.
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn authorize(
    State(state): State<Arc<ControlState>>,
    request: Request,
    next: Next,
) -> Response {
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match given {
        Some(given) if same_secret(given, &state.token) => next.run(request).await,
        _ => error(StatusCode::UNAUTHORIZED, "wrong or missing token"),
    }
}

/// Compares in time independent of where the strings differ.
fn same_secret(given: &str, token: &str) -> bool {
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn error(status: StatusCode, message: impl ToString) -> Response {
    (status, Json(json!({ "error": message.to_string() }))).into_response()
}

fn session_error(err: SessionError) -> Response {
    let status = match err {
        SessionError::UnknownRemoteKey(_) => StatusCode::BAD_REQUEST,
        SessionError::NotLoaded | SessionError::Exited => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, err)
}

async fn status(State(state): State<Arc<ControlState>>) -> Response {
    Json(state.session.status()).into_response()
}

#[derive(Deserialize)]
struct KeyRequest {
    key: String,
}

async fn press_key(
    State(state): State<Arc<ControlState>>,
    Json(body): Json<KeyRequest>,
) -> Response {
    match state.session.press_key(&body.key).await {
        Ok(outcome) => Json(outcome).into_response(),
        Err(err) => session_error(err),
    }
}

async fn reload(State(state): State<Arc<ControlState>>) -> Response {
    match state.session.reload().await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => session_error(err),
    }
}

async fn screenshot(State(state): State<Arc<ControlState>>) -> Response {
    match state.session.screenshot().await {
        Ok(png) => ([(header::CONTENT_TYPE, "image/png")], png).into_response(),
        Err(err) => session_error(err),
    }
}

#[derive(Deserialize)]
struct EvaluateRequest {
    expression: String,
}

async fn evaluate(
    State(state): State<Arc<ControlState>>,
    Json(body): Json<EvaluateRequest>,
) -> Response {
    match state.session.evaluate(&body.expression).await {
        Ok(value) => Json(value).into_response(),
        Err(err) => session_error(err),
    }
}

async fn events(State(state): State<Arc<ControlState>>) -> Response {
    let events = state.session.events();
    let lines = futures_util::stream::unfold(Some(events), |events| async move {
        let mut events = events?;
        let event = events.next().await?;
        let last = matches!(event, SessionEvent::Lifecycle(Lifecycle::Exited { .. }));
        let mut line = serde_json::to_vec(&event).unwrap_or_else(|error| {
            tracing::warn!(%error, ?event, "could not send a Session event");
            vec![]
        });
        if !line.is_empty() {
            line.push(b'\n');
        }
        let rest = (!last).then_some(events);
        Some((Ok::<_, std::convert::Infallible>(line), rest))
    });
    (
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        Body::from_stream(lines),
    )
        .into_response()
}
