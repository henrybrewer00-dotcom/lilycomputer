use crate::ToolOutcome;
use anyhow::Result;
use serde_json::Value;
use std::time::Duration;
use tokio::process::Command;

const MAX_OUTPUT: usize = 16 * 1024;
const MAX_FILE: usize = 200 * 1024;

pub async fn run(args: &Value) -> Result<ToolOutcome> {
    let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    if cmd.trim().is_empty() {
        return Ok(ToolOutcome::err("shell: empty command"));
    }
    if is_blocked(&cmd) {
        return Ok(ToolOutcome::err(format!(
            "shell: command blocked by safety policy ({cmd:?})"
        )));
    }
    let timeout_s = args
        .get("timeout_s")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .clamp(1, 120);

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let fut = Command::new("/bin/zsh")
        .args(["-l", "-c", &cmd])
        .current_dir(&home)
        .output();
    let out = match tokio::time::timeout(Duration::from_secs(timeout_s), fut).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return Ok(ToolOutcome::err(format!("shell spawn: {e}"))),
        Err(_) => return Ok(ToolOutcome::err(format!("shell: timed out after {timeout_s}s"))),
    };
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&out.stdout));
    if !out.stderr.is_empty() {
        combined.push_str("\n[stderr]\n");
        combined.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    let trimmed = if combined.len() > MAX_OUTPUT {
        format!(
            "{}\n…[truncated {} bytes]",
            &combined[..MAX_OUTPUT],
            combined.len() - MAX_OUTPUT
        )
    } else {
        combined
    };
    let code = out.status.code().unwrap_or(-1);
    let summary = format!(
        "shell({:?}) → exit {}",
        cmd.chars().take(48).collect::<String>(),
        code
    );
    let content = format!("exit {code}\n{trimmed}");
    if out.status.success() {
        Ok(ToolOutcome::ok(summary, content))
    } else {
        Ok(ToolOutcome::err(format!("{summary}\n{content}")))
    }
}

pub async fn read_file(args: &Value) -> Result<ToolOutcome> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    if path.is_empty() {
        return Ok(ToolOutcome::err("read_file: path required"));
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let total = bytes.len();
            let slice = if total > MAX_FILE { &bytes[..MAX_FILE] } else { &bytes[..] };
            let body = String::from_utf8_lossy(slice).into_owned();
            let mut content = body;
            if total > MAX_FILE {
                content.push_str(&format!("\n…[truncated {} bytes]", total - MAX_FILE));
            }
            Ok(ToolOutcome::ok(
                format!("read_file({path}) — {total} bytes"),
                content,
            ))
        }
        Err(e) => Ok(ToolOutcome::err(format!("read_file({path}): {e}"))),
    }
}

pub async fn list_dir(args: &Value) -> Result<ToolOutcome> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned();
    if path.is_empty() {
        return Ok(ToolOutcome::err("list_dir: path required"));
    }
    let cmd = format!(
        "/bin/ls -lAhrt -G --time-style=long-iso {} 2>/dev/null || /bin/ls -lAhrt {}",
        shell_escape(&path),
        shell_escape(&path)
    );
    let out = Command::new("/bin/zsh")
        .args(["-c", &cmd])
        .output()
        .await?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Ok(ToolOutcome::err(format!(
            "list_dir({path}): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(ToolOutcome::ok(format!("list_dir({path})"), text))
}

fn shell_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' { out.push_str(r"'\''"); } else { out.push(c); }
    }
    out.push('\'');
    out
}

fn is_blocked(cmd: &str) -> bool {
    let c = cmd.trim();
    let lower = c.to_lowercase();
    if lower.starts_with("sudo ") || lower.contains(" sudo ") {
        return true;
    }
    // Catastrophic deletes.
    if lower.contains("rm -rf /") || lower.contains("rm -rf ~") || lower.contains("rm -rf $home") {
        return true;
    }
    // Fork bombs.
    if c.contains(":(){:|:&};:") {
        return true;
    }
    // Disk format.
    if lower.contains("diskutil eraseDisk".to_lowercase().as_str())
        || lower.contains("mkfs")
        || lower.contains("dd if=")
    {
        return true;
    }
    false
}
