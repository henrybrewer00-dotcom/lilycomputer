//! Bridge between the agent loop and a connected Chrome extension.
//!
//! The extension's service worker connects to `/ws/chrome`; the daemon
//! tracks the latest connection in `BrowserBridge` and uses it to send
//! command frames (`{id, cmd, args}`) and await response frames keyed
//! by `id`.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};

#[derive(Clone, Default)]
pub struct BrowserBridge {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// Channel into the currently-connected WS task (None if no extension).
    tx_to_ext: Option<mpsc::UnboundedSender<String>>,
    /// Waiters keyed by command id.
    pending: HashMap<String, oneshot::Sender<Value>>,
}

impl BrowserBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn is_connected(&self) -> bool {
        self.inner.lock().await.tx_to_ext.is_some()
    }

    /// Called by the WS handler when an extension connects.
    pub async fn attach(&self, tx: mpsc::UnboundedSender<String>) {
        let mut inner = self.inner.lock().await;
        inner.tx_to_ext = Some(tx);
    }

    pub async fn detach(&self) {
        let mut inner = self.inner.lock().await;
        inner.tx_to_ext = None;
        // Fail any pending waiters.
        for (_, w) in inner.pending.drain() {
            let _ = w.send(json!({"ok": false, "summary": "extension disconnected mid-call"}));
        }
    }

    /// Called by the WS handler on each incoming message.
    pub async fn deliver_response(&self, v: Value) {
        let Some(id) = v.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()) else { return; };
        let mut inner = self.inner.lock().await;
        if let Some(waiter) = inner.pending.remove(&id) {
            let _ = waiter.send(v);
        }
    }

    /// Send a command to the extension and await its response.
    pub async fn send(&self, cmd: &str, args: Value) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        let payload = json!({ "id": &id, "cmd": cmd, "args": args });

        let rx = {
            let mut inner = self.inner.lock().await;
            let Some(tx) = inner.tx_to_ext.clone() else {
                return Err(anyhow!("no Chrome extension is connected — open Chrome on the assistant and verify the Lily extension is loaded and enabled"));
            };
            let (tx_resp, rx_resp) = oneshot::channel();
            inner.pending.insert(id.clone(), tx_resp);
            tx.send(payload.to_string())
                .map_err(|e| anyhow!("ws send to extension: {e}"))?;
            rx_resp
        };

        match tokio::time::timeout(Duration::from_secs(45), rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err(anyhow!("extension response channel closed")),
            Err(_) => {
                self.inner.lock().await.pending.remove(&id);
                Err(anyhow!("extension timed out after 45s"))
            }
        }
    }
}
