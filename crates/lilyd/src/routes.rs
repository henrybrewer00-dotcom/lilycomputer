use crate::{agent, tokio_util_cancel::Token, AppState, MODEL, Session};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::{stream::Stream, SinkExt, StreamExt};
use lily_core::protocol::{
    CancelRequest, Event as LilyEvent, HealthResponse, RunRequest, RunResponse,
};
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/run", post(run))
        .route("/cancel", post(cancel))
        .route("/reset", post(reset))
        .route("/diagnose", get(diagnose))
        .route("/stream", get(stream))
        .route("/ws/chrome", get(ws_chrome))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_token,
        ))
        .with_state(state)
}

async fn diagnose(State(state): State<AppState>) -> impl IntoResponse {
    let report = crate::run_diagnostic_checks_with(Some(&state.browser)).await;
    Json(report)
}

async fn ws_chrome(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_chrome_ws(socket, state))
}

async fn handle_chrome_ws(socket: WebSocket, state: AppState) {
    use tokio::sync::mpsc;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    state.browser.attach(tx).await;
    tracing::info!("chrome extension attached");

    let (mut ws_tx, mut ws_rx) = socket.split();

    // Forward outgoing daemon→ext frames.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Read incoming ext→daemon frames.
    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Text(t)) => {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    state.browser.deliver_response(v).await;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    send_task.abort();
    state.browser.detach().await;
    tracing::info!("chrome extension detached");
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let resp = HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_s: state.started.elapsed().as_secs(),
        model: MODEL.to_string(),
    };
    Json(resp)
}

async fn run(
    State(state): State<AppState>,
    Json(body): Json<RunRequest>,
) -> Result<Json<RunResponse>, (StatusCode, String)> {
    // Cancel any in-flight session.
    {
        let mut slot = state.session.lock().await;
        if let Some(prev) = slot.take() {
            prev.cancel.cancel();
        }
    }

    if body.reset {
        state.history.lock().await.reset();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let cancel = Token::new();

    {
        let mut slot = state.session.lock().await;
        *slot = Some(Session {
            id: session_id.clone(),
            cancel: cancel.clone(),
        });
    }

    let s = state.clone();
    let id = session_id.clone();
    let prompt = body.prompt.clone();
    tokio::spawn(async move {
        agent::run(s, id, prompt, cancel).await;
    });

    Ok(Json(RunResponse { session_id }))
}

async fn reset(State(state): State<AppState>) -> impl IntoResponse {
    state.history.lock().await.reset();
    (StatusCode::OK, "memory cleared")
}

async fn cancel(
    State(state): State<AppState>,
    Json(body): Json<CancelRequest>,
) -> impl IntoResponse {
    let mut slot = state.session.lock().await;
    if let Some(s) = slot.as_ref() {
        if s.id == body.session_id {
            s.cancel.cancel();
            *slot = None;
            return (StatusCode::OK, "cancelled");
        }
    }
    (StatusCode::NOT_FOUND, "no matching session")
}

#[derive(Debug, Deserialize)]
struct StreamQuery {
    #[serde(default)]
    _session_id: Option<String>,
}

async fn stream(
    State(state): State<AppState>,
    Query(_q): Query<StreamQuery>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = state.events.subscribe();
    let s = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let payload = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                    let sse = SseEvent::default().data(payload);
                    return Some((Ok::<_, Infallible>(sse), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    // Type-erase to satisfy `impl Stream` return; tag last LilyEvent so it isn't unused.
    let _ = std::marker::PhantomData::<LilyEvent>::default();
    let s = s.boxed();
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
