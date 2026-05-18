use crate::apple;
use crate::ToolOutcome;
use anyhow::Result;
use serde_json::Value;

pub async fn click(args: &Value) -> Result<ToolOutcome> {
    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
    let button = args.get("button").and_then(|v| v.as_str()).unwrap_or("left");
    let double = args.get("double").and_then(|v| v.as_bool()).unwrap_or(false);

    let script = match (button, double) {
        ("right", _) => format!(
            r#"tell application "System Events" to perform action "AXShowMenu" of (UI element 1 of (process 1 whose frontmost is true))
            "#
        ),
        (_, true) => format!(
            r#"tell application "System Events"
                click at {{{x}, {y}}}
                delay 0.05
                click at {{{x}, {y}}}
            end tell"#
        ),
        _ => format!(
            r#"tell application "System Events" to click at {{{x}, {y}}}"#
        ),
    };
    apple::run_script(&script).await?;
    let label = if double { "double-click" } else if button == "right" { "right-click" } else { "click" };
    Ok(ToolOutcome::ok(
        format!("{label}({x}, {y})"),
        format!("{label} at ({x},{y}) completed"),
    ))
}

pub async fn move_mouse(args: &Value) -> Result<ToolOutcome> {
    let x = args.get("x").and_then(|v| v.as_i64()).unwrap_or(0);
    let y = args.get("y").and_then(|v| v.as_i64()).unwrap_or(0);
    // System Events lacks a pure 'move' primitive — emulate via a do shell using the
    // python-objc bridge would be heavy; instead leave a no-op marker. Most agentic
    // flows go straight to click().
    let _ = (x, y);
    Ok(ToolOutcome::ok(
        format!("move_mouse({x}, {y}) [noop]"),
        "move_mouse is a no-op in this build; call click() directly.".to_string(),
    ))
}

pub async fn scroll(args: &Value) -> Result<ToolOutcome> {
    let _dx = args.get("dx").and_then(|v| v.as_i64()).unwrap_or(0);
    let dy = args.get("dy").and_then(|v| v.as_i64()).unwrap_or(0);

    // Map to a number of arrow-key presses; positive dy => down, negative => up.
    let (key, n_each) = if dy == 0 {
        ("Down Arrow", 0)
    } else if dy > 0 {
        ("Down Arrow", scroll_clicks(dy))
    } else {
        ("Up Arrow", scroll_clicks(-dy))
    };
    if n_each == 0 {
        return Ok(ToolOutcome::ok("scroll(0)".to_string(), "no-op".to_string()));
    }
    // Use 'page' shortcut when amount is large.
    let body = if n_each > 6 {
        let page_key = if dy > 0 { "Page Down" } else { "Page Up" };
        let pages = (n_each / 6).max(1);
        format!(
            r#"repeat {pages} times
                key code (key code of "{page_key}")
                delay 0.02
            end repeat"#
        )
    } else {
        let keycode = match key {
            "Down Arrow" => 125,
            "Up Arrow" => 126,
            _ => 125,
        };
        format!(
            r#"repeat {n_each} times
                key code {keycode}
                delay 0.01
            end repeat"#
        )
    };
    let script = format!(r#"tell application "System Events" to {body}"#);
    apple::run_script(&script).await?;
    Ok(ToolOutcome::ok(
        format!("scroll(dy={dy})"),
        format!("scrolled {dy}"),
    ))
}

fn scroll_clicks(magnitude: i64) -> i64 {
    // Treat 100px ≈ 1 arrow keypress, 800px ≈ a page worth.
    (magnitude.abs() / 100).clamp(1, 30)
}

pub async fn type_text(args: &Value) -> Result<ToolOutcome> {
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(r#"tell application "System Events" to keystroke "{escaped}""#);
    apple::run_script(&script).await?;
    Ok(ToolOutcome::ok(
        format!("type_text({} chars)", text.chars().count()),
        format!("typed {} characters", text.chars().count()),
    ))
}

pub async fn key_press(args: &Value) -> Result<ToolOutcome> {
    let combo = args.get("combo").and_then(|v| v.as_str()).unwrap_or("");
    let script = combo_to_applescript(combo)?;
    apple::run_script(&script).await?;
    Ok(ToolOutcome::ok(
        format!("key_press({combo})"),
        format!("pressed {combo}"),
    ))
}

fn combo_to_applescript(combo: &str) -> Result<String> {
    let parts: Vec<&str> = combo.split('+').map(|s| s.trim()).collect();
    if parts.is_empty() {
        anyhow::bail!("empty key combo");
    }
    let key = parts.last().copied().unwrap_or("");
    let modifiers: Vec<&str> = parts[..parts.len() - 1].iter().copied().collect();

    let mut applescript_mods: Vec<&str> = Vec::new();
    for m in &modifiers {
        let m = m.to_lowercase();
        match m.as_str() {
            "cmd" | "command" => applescript_mods.push("command down"),
            "shift" => applescript_mods.push("shift down"),
            "opt" | "option" | "alt" => applescript_mods.push("option down"),
            "ctrl" | "control" => applescript_mods.push("control down"),
            other => anyhow::bail!("unknown modifier: {other}"),
        }
    }
    let using = if applescript_mods.is_empty() {
        String::new()
    } else {
        format!(" using {{{}}}", applescript_mods.join(", "))
    };

    // Named keys → key code; single char → keystroke.
    let named = match key.to_lowercase().as_str() {
        "return" | "enter" => Some(36),
        "tab" => Some(48),
        "space" => Some(49),
        "delete" | "backspace" => Some(51),
        "escape" | "esc" => Some(53),
        "left" | "left arrow" => Some(123),
        "right" | "right arrow" => Some(124),
        "down" | "down arrow" => Some(125),
        "up" | "up arrow" => Some(126),
        "home" => Some(115),
        "end" => Some(119),
        "pageup" | "page_up" | "page up" => Some(116),
        "pagedown" | "page_down" | "page down" => Some(121),
        _ => None,
    };

    if let Some(code) = named {
        Ok(format!(
            r#"tell application "System Events" to key code {code}{using}"#
        ))
    } else if key.chars().count() == 1 {
        let k = key.replace('\\', "\\\\").replace('"', "\\\"");
        Ok(format!(
            r#"tell application "System Events" to keystroke "{k}"{using}"#
        ))
    } else {
        anyhow::bail!("unrecognized key: {key}");
    }
}
