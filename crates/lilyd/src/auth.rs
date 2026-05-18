use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::AppState;

pub async fn require_token(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let path = req.uri().path();
    // Some routes are open: /health is a sanity probe, /ws/chrome is the
    // browser-extension bridge (only reachable on loopback, so practically
    // restricted to processes on this machine).
    if path == "/health" || path == "/ws/chrome" {
        return Ok(next.run(req).await);
    }

    let provided = header.strip_prefix("Bearer ").unwrap_or("");
    if !constant_time_eq(provided.as_bytes(), state.token.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut x: u8 = 0;
    for (x1, x2) in a.iter().zip(b.iter()) {
        x |= x1 ^ x2;
    }
    x == 0
}
