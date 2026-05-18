use crate::ToolOutcome;
use anyhow::Result;
use serde_json::Value;
use tokio::process::Command;

pub async fn open_app(args: &Value) -> Result<ToolOutcome> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if name.is_empty() {
        return Ok(ToolOutcome::err("open_app: name required"));
    }
    let out = Command::new("/usr/bin/open")
        .args(["-a", &name])
        .output()
        .await?;
    if !out.status.success() {
        return Ok(ToolOutcome::err(format!(
            "open_app('{name}') failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(ToolOutcome::ok(
        format!("open_app(\"{name}\")"),
        format!("opened {name}"),
    ))
}
