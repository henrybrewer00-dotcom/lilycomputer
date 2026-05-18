use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub mod apple;
pub mod apps;
pub mod input;
pub mod screen;
pub mod shell;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub ok: bool,
    /// Human-readable summary to display in the TUI.
    pub summary: String,
    /// Full content sent back to the model (often the same as summary).
    pub content: String,
    /// If this tool produces an image, the PNG bytes go here (base64-encoded later).
    #[serde(skip)]
    pub image_png: Option<Vec<u8>>,
    /// If this tool wrote an image to disk for the client to display, the path.
    #[serde(default)]
    pub screenshot_path: Option<String>,
    pub elapsed_ms: u64,
    /// If true, the agent loop should terminate after this tool result.
    pub terminate: bool,
}

impl ToolOutcome {
    pub fn ok(summary: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            content: content.into(),
            image_png: None,
            screenshot_path: None,
            elapsed_ms: 0,
            terminate: false,
        }
    }
    pub fn err(summary: impl Into<String>) -> Self {
        let s: String = summary.into();
        Self {
            ok: false,
            summary: s.clone(),
            content: s,
            image_png: None,
            screenshot_path: None,
            elapsed_ms: 0,
            terminate: false,
        }
    }
    pub fn with_image(mut self, png: Vec<u8>) -> Self {
        self.image_png = Some(png);
        self
    }
    pub fn with_screenshot_path(mut self, path: impl Into<String>) -> Self {
        self.screenshot_path = Some(path.into());
        self
    }
    pub fn terminating(mut self) -> Self {
        self.terminate = true;
        self
    }
}

pub async fn dispatch(name: &str, args: &serde_json::Value) -> ToolOutcome {
    let start = Instant::now();
    let mut out = match dispatch_inner(name, args).await {
        Ok(o) => o,
        Err(e) => ToolOutcome::err(format!("{name} failed: {e}")),
    };
    out.elapsed_ms = start.elapsed().as_millis() as u64;
    out
}

async fn dispatch_inner(name: &str, args: &serde_json::Value) -> Result<ToolOutcome> {
    match name {
        "screenshot" => screen::take().await,
        "click" => input::click(args).await,
        "move_mouse" => input::move_mouse(args).await,
        "scroll" => input::scroll(args).await,
        "type_text" => input::type_text(args).await,
        "key_press" => input::key_press(args).await,
        "open_app" => apps::open_app(args).await,
        "applescript" => apple::run(args).await,
        "describe_screen" => apple::describe_screen(args).await,
        "read_ui" => apple::read_ui(args).await,
        "get_text" => apple::get_text(args).await,
        "click_element" => apple::click_element(args).await,
        "shell" => shell::run(args).await,
        "read_file" => shell::read_file(args).await,
        "list_dir" => shell::list_dir(args).await,
        "wait" => {
            let secs = args.get("seconds").and_then(|v| v.as_f64()).unwrap_or(0.0).clamp(0.0, 10.0);
            tokio::time::sleep(std::time::Duration::from_millis((secs * 1000.0) as u64)).await;
            Ok(ToolOutcome::ok(format!("waited {secs:.1}s"), format!("waited {secs:.1}s")))
        }
        "done" => {
            let s = args.get("summary").and_then(|v| v.as_str()).unwrap_or("done").to_owned();
            Ok(ToolOutcome::ok(s.clone(), s).terminating())
        }
        other => Ok(ToolOutcome::err(format!("unknown tool: {other}"))),
    }
}

/// JSON-Schema fragments describing every tool to the LLM.
pub fn tool_schemas() -> serde_json::Value {
    serde_json::json!([
        {"type":"function","function":{
            "name":"screenshot",
            "description":"Capture the current macOS screen and attach it to the conversation. Use this BEFORE any visual action to confirm what's on screen. NOTE: on Tahoe, this fails on backgrounded Fast-User-Switched sessions — prefer the browser_* tools for web work, or the AX-tree tools (read_ui, get_text, click_element) for native apps.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"click",
            "description":"Click at absolute screen coordinates. Coordinates are in points (not pixels) — the same units used in the screenshot you most recently took.",
            "parameters":{"type":"object","properties":{
                "x":{"type":"integer"},
                "y":{"type":"integer"},
                "button":{"type":"string","enum":["left","right"],"default":"left"},
                "double":{"type":"boolean","default":false}
            },"required":["x","y"]}
        }},
        {"type":"function","function":{
            "name":"move_mouse",
            "description":"Move the mouse cursor without clicking.",
            "parameters":{"type":"object","properties":{"x":{"type":"integer"},"y":{"type":"integer"}},"required":["x","y"]}
        }},
        {"type":"function","function":{
            "name":"scroll",
            "description":"Scroll the wheel by dx/dy lines. Positive dy scrolls down (page goes up), negative dy scrolls up.",
            "parameters":{"type":"object","properties":{
                "dx":{"type":"integer","default":0},
                "dy":{"type":"integer"}
            },"required":["dy"]}
        }},
        {"type":"function","function":{
            "name":"type_text",
            "description":"Type literal text into the focused control. Does NOT press enter.",
            "parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}
        }},
        {"type":"function","function":{
            "name":"key_press",
            "description":"Press a key or chord. Examples: 'return', 'escape', 'tab', 'cmd+t', 'cmd+shift+n', 'down', 'up'.",
            "parameters":{"type":"object","properties":{"combo":{"type":"string"}},"required":["combo"]}
        }},
        {"type":"function","function":{
            "name":"open_app",
            "description":"Open / focus a macOS application by name. Examples: 'Google Chrome', 'Finder', 'Mail', 'Messages', 'Safari'.",
            "parameters":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}
        }},
        {"type":"function","function":{
            "name":"applescript",
            "description":"Run an AppleScript snippet via osascript. Prefer this over click/type when the target app is scriptable (Mail, Safari, Finder, Messages, Calendar, Music, etc.). Returns the script's stdout.",
            "parameters":{"type":"object","properties":{"script":{"type":"string"}},"required":["script"]}
        }},
        {"type":"function","function":{
            "name":"describe_screen",
            "description":"Quick summary of what's on screen: frontmost app + window title, list of open apps, current Chrome/Safari URL & tab title. Use this once at the start of a turn to orient yourself.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"read_ui",
            "description":"Dump the accessibility (AX) tree of the frontmost window (or a named process) as indented structured text. Each line shows role + title + value. Works regardless of session state (no display compositor needed). This is your structured 'vision' — use it instead of screenshot when you need to know what's on the screen of a backgrounded session, or when you need precise element names to click. For Chrome/Safari, this includes the web page's DOM mapped to AX roles.",
            "parameters":{"type":"object","properties":{
                "process":{"type":"string","description":"Optional: process name (e.g. 'Google Chrome', 'Mail'). Defaults to frontmost."},
                "max_depth":{"type":"integer","minimum":1,"maximum":20,"default":8}
            },"required":[]}
        }},
        {"type":"function","function":{
            "name":"get_text",
            "description":"Flat dump of all readable text in the frontmost window (titles + values, no structure). Cheaper and easier-to-read than read_ui when you just want to know 'what does the screen say'.",
            "parameters":{"type":"object","properties":{
                "process":{"type":"string"},
                "max_depth":{"type":"integer","minimum":1,"maximum":30,"default":12}
            },"required":[]}
        }},
        {"type":"function","function":{
            "name":"click_element",
            "description":"Find a UI element by substring match against its title, value, or description (in the AX tree of the frontmost or named process) and click it. Use this INSTEAD of click(x,y) on backgrounded sessions, or whenever you have a stable name (e.g. 'Inbox', 'Send', 'Compose'). Recursive — finds elements anywhere in the window's AX subtree.",
            "parameters":{"type":"object","properties":{
                "query":{"type":"string","description":"Substring to match against title/value/description"},
                "process":{"type":"string"},
                "max_depth":{"type":"integer","minimum":1,"maximum":30,"default":12}
            },"required":["query"]}
        }},
        {"type":"function","function":{
            "name":"shell",
            "description":"Run a bash command (cwd=$HOME, 30s timeout). Rejects sudo and some destructive patterns. Returns combined stdout+stderr (truncated to 16KB).",
            "parameters":{"type":"object","properties":{
                "cmd":{"type":"string"},
                "timeout_s":{"type":"integer","default":30,"minimum":1,"maximum":120}
            },"required":["cmd"]}
        }},
        {"type":"function","function":{
            "name":"read_file",
            "description":"Read a UTF-8 file from the assistant's filesystem (truncated to 200KB).",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}
        }},
        {"type":"function","function":{
            "name":"list_dir",
            "description":"List a directory's contents with sizes and modification times.",
            "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}
        }},
        {"type":"function","function":{
            "name":"wait",
            "description":"Sleep briefly (0–10 seconds) to allow UI animations or page loads to complete.",
            "parameters":{"type":"object","properties":{"seconds":{"type":"number","minimum":0,"maximum":10}},"required":["seconds"]}
        }},
        {"type":"function","function":{
            "name":"done",
            "description":"Signal task completion. ALWAYS call this when finished, with a 1-3 sentence summary of what was accomplished and any findings.",
            "parameters":{"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"]}
        }},

        // ───── Chrome extension bridge tools ─────────────────────────────
        // These call into the Lily Chrome extension via a WebSocket bridge,
        // which renders/captures pages inside Chrome's own process. They
        // work regardless of macOS session state (no display compositor or
        // Screen Recording perm needed) and are STRONGLY PREFERRED for any
        // task involving a web page.
        {"type":"function","function":{
            "name":"browser_screenshot",
            "description":"Capture a PNG of the active Chrome tab via chrome.tabs.captureVisibleTab. Works on backgrounded sessions. Prefer this over screenshot() for web work.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_navigate",
            "description":"Navigate the active Chrome tab to a URL (waits for load). Pass new_tab:true to open in a new tab. RESPONSE INCLUDES `hints` — a numbered list of every clickable element on the loaded page, ready for browser_click(\"hint:N\").",
            "parameters":{"type":"object","properties":{
                "url":{"type":"string"},
                "new_tab":{"type":"boolean","default":false}
            },"required":["url"]}
        }},
        {"type":"function","function":{
            "name":"browser_hints",
            "description":"Return a numbered list of every visible interactive element on the active tab. You usually don't need to call this — browser_navigate / browser_click / browser_wait_for and similar tools auto-attach `hints` to their response. Use this only if you need a fresh list without changing anything.",
            "parameters":{"type":"object","properties":{
                "max":{"type":"integer","default":200,"minimum":1,"maximum":500}
            },"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_click",
            "description":"Click an element. Pass 'hint:N' (use an N from the most recent response's `hints` array — strongly preferred), 'text:<substring>' (case-insensitive visible-text match), or a raw CSS selector. RESPONSE INCLUDES updated `hints` for the page state after the click.",
            "parameters":{"type":"object","properties":{
                "selector":{"type":"string","description":"'hint:N' | 'text:<substring>' | <css selector>"}
            },"required":["selector"]}
        }},
        {"type":"function","function":{
            "name":"browser_wait_for",
            "description":"Block until a CSS selector resolves to a visible element, or timeout. Use after page actions that trigger AJAX/SPA updates. RESPONSE INCLUDES fresh `hints`.",
            "parameters":{"type":"object","properties":{
                "selector":{"type":"string"},
                "timeout_ms":{"type":"integer","default":5000,"minimum":100,"maximum":30000}
            },"required":["selector"]}
        }},
        {"type":"function","function":{
            "name":"browser_back",
            "description":"History back in the active tab.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_forward",
            "description":"History forward in the active tab.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_reload",
            "description":"Reload the active tab.",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_switch_tab",
            "description":"Activate a tab by its id (from browser_tabs).",
            "parameters":{"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}
        }},
        {"type":"function","function":{
            "name":"browser_type",
            "description":"Type text into an input/textarea/contenteditable. Pass 'hint:N' from the latest response, or a CSS selector. submit:true also presses Enter. Dispatches proper input events so React/Vue controlled inputs pick up the change. RESPONSE INCLUDES fresh `hints`.",
            "parameters":{"type":"object","properties":{
                "selector":{"type":"string"},
                "text":{"type":"string"},
                "submit":{"type":"boolean","default":false}
            },"required":["selector","text"]}
        }},
        {"type":"function","function":{
            "name":"browser_read_page",
            "description":"Return the active tab's URL, title, and the first ~16KB of visible body text. Use this to read content before deciding what to click.",
            "parameters":{"type":"object","properties":{
                "max_chars":{"type":"integer","default":16000,"minimum":500,"maximum":40000}
            },"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_query",
            "description":"Return up to N elements matching a CSS selector in the active tab — useful for finding the right thing to click. Returns text, href, tag, and a stable selector for each match.",
            "parameters":{"type":"object","properties":{
                "selector":{"type":"string"},
                "limit":{"type":"integer","default":20,"minimum":1,"maximum":100}
            },"required":["selector"]}
        }},
        {"type":"function","function":{
            "name":"browser_tabs",
            "description":"List open Chrome tabs (id, title, url, active flag).",
            "parameters":{"type":"object","properties":{},"required":[]}
        }},
        {"type":"function","function":{
            "name":"browser_scroll",
            "description":"Scroll the active tab. Positive dy scrolls down. Pass {to:'top'|'bottom'} to jump.",
            "parameters":{"type":"object","properties":{
                "dy":{"type":"integer","default":600},
                "to":{"type":"string","enum":["top","bottom"]}
            },"required":[]}
        }}
    ])
}
