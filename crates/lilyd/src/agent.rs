use crate::groq;
use crate::history;
use crate::tokio_util_cancel::Token;
use crate::{AppState, MODEL, SERVICE_TIER};
use base64::Engine;
use lily_actions::{dispatch, tool_schemas, ToolOutcome};
use lily_core::protocol::Event;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

static BROWSER_SHOT_COUNTER: AtomicU32 = AtomicU32::new(0);

const SYSTEM_PROMPT: &str = r#"You are Lily, an autonomous AI computer-control agent running as a macOS user. The human owner is watching your actions through a terminal UI.

Your job is to accomplish whatever task the human asks you to do by taking actions on the Mac.

You have TWO kinds of "vision":
  • Pixel vision (screenshot) — only works when this user's session is foregrounded. May return "VISION_UNAVAILABLE" on backgrounded Fast-User-Switched sessions.
  • Structured vision (read_ui, get_text) — uses the macOS accessibility tree. Works ALWAYS, even on backgrounded sessions. Shows you every button, link, text field, and chunk of static text in any app — including web content inside Chrome/Safari, because Chromium maps the DOM into AX.

Default playbook (works regardless of session state):
1. `describe_screen` to orient — frontmost app, window title, open apps, browser URL.
2. **Anything web (Gmail, X, YouTube, GitHub, news, search, etc.) → use `browser_*` tools.** They run inside Chrome via the Lily extension, so they're not constrained by macOS perms or session state.

   How it works: state-changing browser tools (`browser_navigate`, `browser_click`, `browser_type`, `browser_back`, `browser_forward`, `browser_reload`, `browser_wait_for`, `browser_scroll`, `browser_switch_tab`) ALWAYS return `{url, title, hints}` in their response. `hints` is a numbered list of every visible interactive element on the page right now: `[{id, role, text, tag, href}, ...]`. You don't need to ask for it.

   So the natural flow is: `browser_navigate("https://gmail.com")` → response contains hints → pick the hint with text "Inbox" → `browser_click("hint:N")` → response contains updated hints → continue. `browser_type("hint:N", "search query", submit=true)` types and presses Enter. Hint ids are page-scoped; the latest tool response always has the fresh ones.

   Other tools: `browser_read_page` for full visible text, `browser_query(selector)` to find by CSS when hints aren't enough, `browser_screenshot` only if the user explicitly wants to see the tab.
3. For native macOS apps (Mail, Calendar, Messages, Music, Finder, Notes, Reminders): prefer `applescript` for direct app control — faster and more reliable than UI traversal.
4. For any other app: use `get_text` or `read_ui` to learn what's on the screen, then `click_element(query: "Inbox")` to click by AX-tree name.
5. For filesystem and web/curl tasks, prefer `shell`.

Only fall back to the native macOS `screenshot` tool if you specifically need pixel content from a non-Chrome app. If `screenshot` returns "VISION_UNAVAILABLE", do not retry it this turn.

Be efficient. Don't dump giant AX trees when get_text would do. Don't repeat failing actions; try a different approach. When the task is complete, ALWAYS call `done` with a 1-3 sentence summary.

Hard limits: 40 tool calls per request. No sudo. Don't interact with banking, password managers, or anything obviously sensitive without an explicit instruction from the human.

Today's date is provided by the system."#;

const MAX_STEPS: usize = 40;

pub async fn run(state: AppState, session_id: String, prompt: String, cancel: Token) {
    let tx = state.events.clone();
    let _ = tx.send(Event::Status { state: "thinking".into() });
    let _ = tx.send(Event::UserPrompt { text: prompt.clone() });

    let tools = tool_schemas();
    let mut messages: Vec<Value> = {
        let mut h = state.history.lock().await;
        if h.messages.is_empty() {
            h.messages.push(groq::text_message("system", SYSTEM_PROMPT));
        }
        h.messages.push(groq::text_message("user", &prompt));
        // Persist the user-turn immediately so a crash mid-run still keeps the prompt.
        h.save();
        h.messages.clone()
    };

    let mut total_prompt_tokens: u32 = 0;
    let mut total_completion_tokens: u32 = 0;

    for step in 0..MAX_STEPS {
        if cancel.is_cancelled() {
            let _ = tx.send(Event::Error { message: "cancelled by user".into() });
            let _ = tx.send(Event::Done { summary: "cancelled".into() });
            break;
        }

        let req = groq::ChatRequest {
            model: MODEL.to_string(),
            messages: messages.clone(),
            tools: tools.clone(),
            tool_choice: "auto".to_string(),
            temperature: 0.2,
            max_tokens: 1024,
            service_tier: SERVICE_TIER.to_string(),
            stream: false,
        };

        let resp = match groq::chat(&state.http, &state.groq_key, &req).await {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(Event::Error { message: format!("groq error: {e}") });
                let _ = tx.send(Event::Done { summary: "errored".into() });
                break;
            }
        };

        if let Some(u) = resp.usage {
            total_prompt_tokens += u.prompt_tokens;
            total_completion_tokens += u.completion_tokens;
            let _ = tx.send(Event::Tokens {
                prompt: total_prompt_tokens,
                completion: total_completion_tokens,
                total: total_prompt_tokens + total_completion_tokens,
            });
        }

        let Some(choice) = resp.choices.into_iter().next() else {
            let _ = tx.send(Event::Error { message: "groq returned no choices".into() });
            let _ = tx.send(Event::Done { summary: "no response".into() });
            break;
        };
        let msg = choice.message;

        // Emit any assistant text.
        if let Some(content) = &msg.content {
            if let Some(s) = content.as_str() {
                if !s.trim().is_empty() {
                    let _ = tx.send(Event::Assistant { text: s.to_string() });
                }
            }
        }

        // If no tool calls, we're done — model spoke its piece.
        let Some(tool_calls) = msg.tool_calls.clone() else {
            let summary = msg
                .content
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("(no summary)")
                .to_string();
            let _ = tx.send(Event::Done { summary });
            break;
        };
        if tool_calls.is_empty() {
            let summary = msg
                .content
                .as_ref()
                .and_then(|v| v.as_str())
                .unwrap_or("(no summary)")
                .to_string();
            let _ = tx.send(Event::Done { summary });
            break;
        }

        // Append the assistant message verbatim.
        messages.push(groq::assistant_message_to_json(&msg));

        // Execute each tool call sequentially, in order.
        let mut terminated = false;
        let mut final_summary: Option<String> = None;
        let mut image_followup: Option<(String, Vec<u8>)> = None;
        let _ = tx.send(Event::Status { state: "acting".into() });

        for tc in tool_calls {
            if cancel.is_cancelled() { terminated = true; break; }
            let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Object(Default::default()));
            let _ = tx.send(Event::ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                args: args.clone(),
            });

            let outcome = if let Some(sub) = tc.function.name.strip_prefix("browser_") {
                dispatch_browser(&state, sub, &args).await
            } else {
                dispatch(&tc.function.name, &args).await
            };

            let _ = tx.send(Event::ToolResult {
                id: tc.id.clone(),
                ok: outcome.ok,
                summary: outcome.summary.clone(),
                elapsed_ms: outcome.elapsed_ms,
            });

            if let Some(path) = outcome.screenshot_path.clone() {
                let idx = path
                    .rsplit_once("screen-")
                    .and_then(|(_, rest)| rest.split_once('.').map(|(n, _)| n))
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(0);
                let _ = tx.send(Event::Screenshot { path, index: idx });
            }

            // Append tool result message.
            messages.push(groq::tool_result_message(&tc.id, &outcome.content));

            // If this tool produced an image, queue a user-message follow-up with the image
            // (only one image per round to keep tokens manageable — use the LAST image).
            if let Some(png) = outcome.image_png {
                image_followup = Some((
                    format!("Latest screenshot from tool call '{}' is below. Coordinates here map 1:1 to click() args.", tc.function.name),
                    png,
                ));
            }

            if outcome.terminate {
                terminated = true;
                final_summary = Some(outcome.summary);
            }
        }

        if let Some((prefix, png)) = image_followup {
            messages.push(groq::image_user_message(&prefix, &png));
        }

        if terminated {
            let summary = final_summary.unwrap_or_else(|| "done".into());
            let _ = tx.send(Event::Done { summary });
            break;
        }

        let _ = tx.send(Event::Status { state: "thinking".into() });
        let _ = step;
    }

    // (browser dispatch helper lives below.)

    // Persist what we accumulated (without screenshots) for the next turn.
    {
        let stripped = history::strip_images(messages);
        let capped = history::cap(stripped);
        let mut h = state.history.lock().await;
        h.messages = capped;
        h.save();
    }

    // Clear current session pointer if this run still owns it.
    let mut slot = state.session.lock().await;
    if let Some(s) = slot.as_ref() {
        if s.id == session_id {
            *slot = None;
        }
    }
}

/// Route a browser_* tool to the Chrome extension via the WebSocket bridge.
async fn dispatch_browser(state: &AppState, sub: &str, args: &Value) -> ToolOutcome {
    let start = Instant::now();
    if !state.browser.is_connected().await {
        let mut o = ToolOutcome::err(format!(
            "browser_{sub}: no Chrome extension connected. Open Chrome on the assistant and install/enable the Lily extension (see extension/README.md)."
        ));
        o.elapsed_ms = start.elapsed().as_millis() as u64;
        return o;
    }
    let resp = match state.browser.send(sub, args.clone()).await {
        Ok(v) => v,
        Err(e) => {
            let mut o = ToolOutcome::err(format!("browser_{sub}: {e}"));
            o.elapsed_ms = start.elapsed().as_millis() as u64;
            return o;
        }
    };
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let summary = resp
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or(if ok { "done" } else { "(no summary)" })
        .to_string();
    let mut content_obj = resp.clone();
    if let Some(o) = content_obj.as_object_mut() {
        // Image goes into a separate field; don't bloat the model's context.
        o.remove("image");
        o.remove("id");
    }
    let content = serde_json::to_string(&content_obj).unwrap_or_else(|_| summary.clone());
    let mut out = if ok {
        ToolOutcome::ok(format!("browser_{sub} → {summary}"), content)
    } else {
        ToolOutcome::err(format!("browser_{sub}: {summary}"))
    };

    if sub == "screenshot" {
        if let Some(data_url) = resp.get("image").and_then(|v| v.as_str()) {
            if let Some(b64) = data_url.strip_prefix("data:image/png;base64,") {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    let idx = BROWSER_SHOT_COUNTER.fetch_add(1, Ordering::SeqCst) + 10_000;
                    let dir = std::path::PathBuf::from("/Users/Shared/lily/shots");
                    let _ = std::fs::create_dir_all(&dir);
                    let numbered = dir.join(format!("browser-{idx:04}.png"));
                    let latest = dir.join("latest.png");
                    let _ = std::fs::write(&numbered, &bytes);
                    let _ = std::fs::write(&latest, &bytes);
                    out = out.with_screenshot_path(numbered.to_string_lossy().into_owned());
                    out = out.with_image(bytes);
                }
            }
        }
    }
    out.elapsed_ms = start.elapsed().as_millis() as u64;
    out
}
