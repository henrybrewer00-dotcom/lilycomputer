use crate::ToolOutcome;
use anyhow::{Context, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::process::Command;

pub async fn run(args: &Value) -> Result<ToolOutcome> {
    let script = args
        .get("script")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if script.is_empty() {
        return Ok(ToolOutcome::err("applescript: empty script"));
    }
    match run_script(&script).await {
        Ok(stdout) => {
            let trimmed = trim_for_display(&stdout, 4_000);
            Ok(ToolOutcome::ok(
                format!("applescript ({} bytes out)", stdout.len()),
                if trimmed.is_empty() { "[no output]".into() } else { trimmed },
            ))
        }
        Err(e) => Ok(ToolOutcome::err(format!("applescript: {e}"))),
    }
}

/// Returns a textual description of "what's on screen" via AppleScript. Useful
/// as a vision-less fallback when screencapture is blocked (e.g. the worker
/// session is backgrounded via Fast User Switching).
pub async fn describe_screen(_args: &Value) -> Result<crate::ToolOutcome> {
    let script = r#"
        set output to ""
        try
            tell application "System Events"
                set frontProc to first application process whose frontmost is true
                set fmName to name of frontProc
                set output to output & "frontmost_app: " & fmName & linefeed
                try
                    set frontWin to front window of frontProc
                    set output to output & "frontmost_window: " & (name of frontWin) & linefeed
                    try
                        set sz to size of frontWin
                        set ps to position of frontWin
                        set output to output & "window_position: " & (item 1 of ps) & "," & (item 2 of ps) & linefeed
                        set output to output & "window_size: " & (item 1 of sz) & "x" & (item 2 of sz) & linefeed
                    end try
                end try
                set runningApps to name of every application process whose background only is false
                set output to output & "open_apps: "
                repeat with a in runningApps
                    set output to output & a & ", "
                end repeat
                set output to output & linefeed
            end tell
        on error errMsg
            set output to output & "error: " & errMsg & linefeed
        end try
        try
            tell application "Google Chrome"
                if (count of windows) > 0 then
                    set output to output & "chrome_url: " & (URL of active tab of front window) & linefeed
                    set output to output & "chrome_title: " & (title of active tab of front window) & linefeed
                end if
            end tell
        end try
        try
            tell application "Safari"
                if (count of windows) > 0 then
                    set output to output & "safari_url: " & (URL of current tab of front window) & linefeed
                    set output to output & "safari_title: " & (name of current tab of front window) & linefeed
                end if
            end tell
        end try
        return output
    "#;
    let text = run_script(script).await.unwrap_or_else(|e| format!("(describe failed: {e})"));
    let trimmed = trim_for_display(&text, 4_000);
    Ok(crate::ToolOutcome::ok(
        format!("describe_screen ({} bytes)", text.len()),
        if trimmed.is_empty() { "(no window state reported)".into() } else { trimmed },
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Accessibility-tree tools — Lily's text-based "vision".
//
// macOS keeps an AXUIElement tree alive for every running app, regardless of
// whether the user's session is foregrounded. System Events exposes the tree
// via AppleScript, so we can dump structured text from any app — including web
// content inside Chrome/Safari (Chromium maps the DOM into AX, so links and
// buttons in Gmail/etc. are addressable by name).
// ─────────────────────────────────────────────────────────────────────────────

fn proc_clause(args: &Value) -> String {
    match args.get("process").and_then(|v| v.as_str()) {
        Some(n) => format!(r#"first process whose name is "{}""#, n.replace('"', "\\\"")),
        None => "first process whose frontmost is true".to_string(),
    }
}

pub async fn read_ui(args: &Value) -> Result<crate::ToolOutcome> {
    let max_depth = args.get("max_depth").and_then(|v| v.as_i64()).unwrap_or(8).clamp(1, 20);
    let proc = proc_clause(args);

    let script = format!(r#"
on walk(el, depth, maxDepth)
    if depth > maxDepth then return ""
    set pad to ""
    repeat depth times
        set pad to pad & "  "
    end repeat
    tell application "System Events"
        set lineText to pad
        try
            set lineText to lineText & ((role of el) as text)
        end try
        try
            set t to (title of el) as text
            if t is not "" and t is not "missing value" then set lineText to lineText & "  " & t
        end try
        try
            set v to (value of el) as text
            if v is not "" and v is not "missing value" then
                if (count v) > 120 then set v to (text 1 thru 120 of v) & "..."
                set lineText to lineText & "  = " & v
            end if
        end try
        set output to lineText & linefeed
        try
            set kids to UI elements of el
            repeat with k in kids
                set output to output & my walk(k, depth + 1, maxDepth)
            end repeat
        end try
        return output
    end tell
end walk

tell application "System Events"
    set theProc to {proc}
    try
        set rootWin to front window of theProc
        return my walk(rootWin, 0, {max_depth})
    on error
        return "(no front window)"
    end try
end tell
"#);

    let text = run_script(&script).await?;
    let trimmed = trim_for_display(&text, 16_000);
    Ok(crate::ToolOutcome::ok(
        format!("read_ui ({} lines)", text.lines().count()),
        if trimmed.trim().is_empty() { "(empty UI tree)".into() } else { trimmed },
    ))
}

pub async fn get_text(args: &Value) -> Result<crate::ToolOutcome> {
    let max_depth = args.get("max_depth").and_then(|v| v.as_i64()).unwrap_or(12).clamp(1, 30);
    let proc = proc_clause(args);

    let script = format!(r#"
on flat(el, depth, maxDepth)
    if depth > maxDepth then return ""
    tell application "System Events"
        set output to ""
        try
            set t to (title of el) as text
            if t is not "" and t is not "missing value" then set output to output & t & linefeed
        end try
        try
            set v to (value of el) as text
            if v is not "" and v is not "missing value" then
                if (count v) > 400 then set v to (text 1 thru 400 of v) & "..."
                set output to output & v & linefeed
            end if
        end try
        try
            set kids to UI elements of el
            repeat with k in kids
                set output to output & my flat(k, depth + 1, maxDepth)
            end repeat
        end try
        return output
    end tell
end flat

tell application "System Events"
    set theProc to {proc}
    try
        return my flat(front window of theProc, 0, {max_depth})
    on error
        return "(no front window)"
    end try
end tell
"#);

    let text = run_script(&script).await?;
    let trimmed = trim_for_display(&text, 12_000);
    Ok(crate::ToolOutcome::ok(
        format!("get_text ({} lines)", text.lines().count()),
        if trimmed.trim().is_empty() { "(empty)".into() } else { trimmed },
    ))
}

pub async fn click_element(args: &Value) -> Result<crate::ToolOutcome> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if query.is_empty() {
        return Ok(crate::ToolOutcome::err("click_element: query required"));
    }
    let q = query.replace('"', "\\\"");
    let proc = proc_clause(args);
    let max_depth = args.get("max_depth").and_then(|v| v.as_i64()).unwrap_or(12).clamp(1, 30);

    let script = format!(r#"
on findInTree(el, q, depth, maxDepth)
    if depth > maxDepth then return missing value
    tell application "System Events"
        try
            set t to (title of el) as text
            if t contains q then return el
        end try
        try
            set v to (value of el) as text
            if v contains q then return el
        end try
        try
            set d to (description of el) as text
            if d contains q then return el
        end try
        try
            set kids to UI elements of el
            repeat with k in kids
                set found to my findInTree(k, q, depth + 1, maxDepth)
                if found is not missing value then return found
            end repeat
        end try
        return missing value
    end tell
end findInTree

tell application "System Events"
    set theProc to {proc}
    try
        set target to my findInTree(front window of theProc, "{q}", 0, {max_depth})
        if target is missing value then return "not found: {q}"
        try
            click target
        on error errMsg
            try
                perform action "AXPress" of target
            on error
                return "found but click failed: " & errMsg
            end try
        end try
        set lbl to ""
        try
            set lbl to (role of target) as text
        end try
        try
            set lbl to lbl & " " & ((title of target) as text)
        end try
        return "clicked: " & lbl
    on error errMsg
        return "error: " & errMsg
    end try
end tell
"#);

    let result = run_script(&script).await.unwrap_or_else(|e| format!("error: {e}"));
    let r = result.trim();
    if r.starts_with("clicked:") {
        Ok(crate::ToolOutcome::ok(
            format!("click_element(\"{}\")", query),
            r.to_string(),
        ))
    } else {
        Ok(crate::ToolOutcome::err(format!("click_element(\"{}\"): {}", query, r)))
    }
}

pub async fn run_script(script: &str) -> Result<String> {
    let fut = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(20), fut)
        .await
        .context("osascript timed out after 20s")??;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn trim_for_display(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}\n…[truncated {} bytes]", &s[..max], s.len() - max)
    }
}
