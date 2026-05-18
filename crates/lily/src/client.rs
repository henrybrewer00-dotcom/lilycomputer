use anyhow::{Context, Result};
use eventsource_stream::Eventsource;
use futures::stream::Stream;
use futures_util::StreamExt;
use lily_core::protocol::{
    CancelRequest, Event as LilyEvent, HealthResponse, RunRequest, RunResponse,
};
use reqwest::header::AUTHORIZATION;
use std::pin::Pin;

#[derive(Clone)]
pub struct DaemonClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(base: String, token: String) -> Self {
        Self {
            base,
            token,
            http: reqwest::Client::builder()
                .pool_idle_timeout(std::time::Duration::from_secs(60))
                .build()
                .expect("reqwest client"),
        }
    }

    fn auth(&self) -> String { format!("Bearer {}", self.token) }

    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(format!("{}/health", self.base))
            .header(AUTHORIZATION, self.auth())
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .context("connect")?;
        let h: HealthResponse = resp.json().await.context("decode health")?;
        Ok(h)
    }

    pub async fn run(&self, prompt: String) -> Result<RunResponse> {
        let req = RunRequest { prompt, session_id: None, reset: false };
        let resp = self
            .http
            .post(format!("{}/run", self.base))
            .header(AUTHORIZATION, self.auth())
            .json(&req)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("/run {status}: {text}");
        }
        let parsed: RunResponse = serde_json::from_str(&text)?;
        Ok(parsed)
    }

    pub async fn cancel(&self, session_id: String) -> Result<()> {
        let req = CancelRequest { session_id };
        let _ = self
            .http
            .post(format!("{}/cancel", self.base))
            .header(AUTHORIZATION, self.auth())
            .json(&req)
            .send()
            .await?;
        Ok(())
    }

    pub async fn reset(&self) -> Result<()> {
        let _ = self
            .http
            .post(format!("{}/reset", self.base))
            .header(AUTHORIZATION, self.auth())
            .send()
            .await?;
        Ok(())
    }

    pub async fn diagnose(&self) -> Result<String> {
        let resp = self
            .http
            .get(format!("{}/diagnose", self.base))
            .header(AUTHORIZATION, self.auth())
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        Ok(resp.text().await?)
    }

    pub async fn stream(&self) -> Result<Pin<Box<dyn Stream<Item = Result<LilyEvent>> + Send>>> {
        let resp = self
            .http
            .get(format!("{}/stream", self.base))
            .header(AUTHORIZATION, self.auth())
            .send()
            .await?;
        let byte_stream = resp.bytes_stream();
        let sse = byte_stream.eventsource();
        let mapped = sse.map(|ev| -> Result<LilyEvent> {
            let ev = ev.map_err(|e| anyhow::anyhow!("sse: {e}"))?;
            let parsed: LilyEvent = serde_json::from_str(&ev.data)
                .with_context(|| format!("decode sse event: {}", ev.data))?;
            Ok(parsed)
        });
        Ok(Box::pin(mapped))
    }
}
